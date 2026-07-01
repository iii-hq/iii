//! Minimal EROFS reader for extracting file contents from our own images.
//!
//! Only supports the subset of EROFS that our writer produces:
//! - Extended inodes (64 bytes)
//! - Uncompressed data (FLAT_PLAIN or FLAT_INLINE)
//! - Sorted directory entries (binary search)
//! - No shared xattrs, no compression, no chunks

use std::collections::HashSet;
use std::io::Read;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::{ffi::OsString, fs::File, io, path::PathBuf};

use super::format::{
    EROFS_BLKSIZ, EROFS_DIRENT_SIZE, EROFS_INODE_EXTENDED_SIZE, EROFS_INODE_FLAT_INLINE,
    EROFS_INODE_FLAT_PLAIN, EROFS_NULL_ADDR, EROFS_SUPER_OFFSET, EROFS_XATTR_IBODY_HEADER_SIZE,
    EROFS_XATTR_INDEX_SECURITY, EROFS_XATTR_INDEX_TRUSTED, EROFS_XATTR_INDEX_USER, S_IFBLK,
    S_IFCHR, S_IFDIR, S_IFIFO, S_IFLNK, S_IFMT, S_IFREG, S_IFSOCK, erofs_xattr_align,
};
use crate::cli::erofs::tree::{InodeMetadata, Xattr};

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// A handle to an open EROFS image for reading.
pub struct ErofsReader {
    file: File,
    meta_blkaddr: u32,
    root_nid: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErofsEntryKind {
    RegularFile,
    Directory,
    Symlink,
    CharDevice,
    BlockDevice,
    Fifo,
    Socket,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErofsEntryInfo {
    pub kind: ErofsEntryKind,
    pub opaque: bool,
    pub whiteout: bool,
}

/// A filesystem entry discovered while walking an EROFS image.
#[derive(Clone)]
pub struct ErofsTreeEntry {
    /// Path relative to the image root.
    pub path: PathBuf,
    /// Stable EROFS inode identifier.
    pub nid: u32,
    /// Entry kind.
    pub kind: ErofsEntryKind,
    /// POSIX inode metadata.
    pub metadata: InodeMetadata,
    /// Inline xattrs stored on the inode.
    pub xattrs: Vec<Xattr>,
    /// File or symlink data size.
    pub size: u64,
    /// Device major/minor for device nodes.
    pub rdev: Option<(u32, u32)>,
}

/// Streaming reader for a regular file stored inside an EROFS image.
pub struct ErofsFileDataReader {
    file: File,
    segments: Vec<(u64, u64)>,
    segment_index: usize,
    segment_offset: u64,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ErofsInodeDebugInfo {
    pub nid: u32,
    pub nlink: u32,
    pub size: u64,
    pub data_layout: u8,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl ErofsReader {
    /// Open an EROFS image by parsing the superblock.
    pub fn new(file: File) -> io::Result<Self> {
        let mut sb = [0u8; 128];
        read_exact_at(&file, EROFS_SUPER_OFFSET, &mut sb)?;

        let magic = u32::from_le_bytes([sb[0], sb[1], sb[2], sb[3]]);
        if magic != 0xE0F5_E1E2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("bad EROFS magic: {magic:#x}"),
            ));
        }

        let root_nid = u16::from_le_bytes([sb[0x0E], sb[0x0F]]) as u32;
        let meta_blkaddr = u32::from_le_bytes([sb[0x28], sb[0x29], sb[0x2A], sb[0x2B]]);

        Ok(Self {
            file,
            meta_blkaddr,
            root_nid,
        })
    }

    /// Read a file by path from the EROFS image. Returns the file data.
    pub fn read_file(&mut self, path: &str) -> io::Result<Vec<u8>> {
        let target_inode = self.lookup_path(path)?;
        if (target_inode.mode & S_IFMT) != S_IFREG {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "target is not a regular file",
            ));
        }
        self.read_inode_data(&target_inode)
    }

    /// Read a symlink target by path from the EROFS image.
    pub fn read_link(&mut self, path: &str) -> io::Result<Vec<u8>> {
        let target_inode = self.lookup_path(path)?;
        if (target_inode.mode & S_IFMT) != S_IFLNK {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "target is not a symlink",
            ));
        }
        self.read_inode_data(&target_inode)
    }

    pub fn entry_info(&mut self, path: &str) -> io::Result<ErofsEntryInfo> {
        let inode = self.lookup_path(path)?;
        let kind = inode_kind(&inode)?;
        let opaque = if kind == ErofsEntryKind::Directory {
            self.inode_is_opaque(&inode)?
        } else {
            false
        };
        let whiteout = kind == ErofsEntryKind::CharDevice && inode.rdev == 0;

        Ok(ErofsEntryInfo {
            kind,
            opaque,
            whiteout,
        })
    }

    /// Walk all entries in the image in stable path order.
    pub fn walk(&mut self) -> io::Result<Vec<ErofsTreeEntry>> {
        let root = self.read_inode(self.root_nid)?;
        let mut entries = Vec::new();
        let mut visited = HashSet::new();
        self.walk_dir(&root, PathBuf::new(), &mut entries, &mut visited)?;
        Ok(entries)
    }

    /// Walk all entries in stable path order, invoking a callback for each entry.
    pub fn walk_entries<E, F>(&mut self, mut visit: F) -> Result<(), E>
    where
        E: From<io::Error>,
        F: FnMut(&mut Self, ErofsTreeEntry) -> Result<(), E>,
    {
        let root = self.read_inode(self.root_nid)?;
        let mut visited = HashSet::new();
        self.walk_dir_entries(&root, PathBuf::new(), &mut visited, &mut visit)
    }

    /// Create a streaming reader for a regular file inode by NID.
    pub fn file_data_reader(&mut self, nid: u32) -> io::Result<ErofsFileDataReader> {
        let inode = self.read_inode(nid)?;
        if (inode.mode & S_IFMT) != S_IFREG {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "target is not a regular file",
            ));
        }

        Ok(ErofsFileDataReader {
            file: self.file.try_clone()?,
            segments: self.inode_data_segments(&inode)?,
            segment_index: 0,
            segment_offset: 0,
        })
    }

    /// Read a symlink target by NID.
    pub fn read_link_by_nid(&mut self, nid: u32) -> io::Result<Vec<u8>> {
        let inode = self.read_inode(nid)?;
        if (inode.mode & S_IFMT) != S_IFLNK {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "target is not a symlink",
            ));
        }
        self.read_inode_data(&inode)
    }

    #[cfg(test)]
    pub(crate) fn inode_debug_info(&mut self, path: &str) -> io::Result<ErofsInodeDebugInfo> {
        let inode = self.lookup_path(path)?;
        Ok(ErofsInodeDebugInfo {
            nid: inode.nid,
            nlink: inode.nlink,
            size: inode.size,
            data_layout: inode.data_layout,
        })
    }

    fn inode_offset(&self, nid: u32) -> u64 {
        (self.meta_blkaddr as u64) * (EROFS_BLKSIZ as u64) + (nid as u64) * 32
    }

    fn read_inode(&mut self, nid: u32) -> io::Result<InodeInfo> {
        let offset = self.inode_offset(nid);

        let mut buf = [0u8; EROFS_INODE_EXTENDED_SIZE as usize];
        read_exact_at(&self.file, offset, &mut buf)?;

        let i_format = u16::from_le_bytes([buf[0], buf[1]]);
        let i_xattr_icount = u16::from_le_bytes([buf[2], buf[3]]);
        let mode = u16::from_le_bytes([buf[4], buf[5]]);
        let size = u64::from_le_bytes([
            buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15],
        ]);
        let i_u = u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]);
        let nlink = u32::from_le_bytes([buf[44], buf[45], buf[46], buf[47]]);
        let uid = u32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]);
        let gid = u32::from_le_bytes([buf[28], buf[29], buf[30], buf[31]]);
        let mtime = u64::from_le_bytes([
            buf[32], buf[33], buf[34], buf[35], buf[36], buf[37], buf[38], buf[39],
        ]);
        let mtime_nsec = u32::from_le_bytes([buf[40], buf[41], buf[42], buf[43]]);

        let data_layout = ((i_format >> 1) & 0x07) as u8;

        // Compute xattr ibody size to know where inline data starts.
        // Formula from EROFS spec: ibody = 12-byte header + (i_xattr_icount - 1) * 4 bytes.
        // The "- 1" accounts for the header occupying the first count unit.
        let xattr_ibody_size = if i_xattr_icount == 0 {
            0u32
        } else {
            12 + ((i_xattr_icount as u32) - 1) * 4
        };

        Ok(InodeInfo {
            nid,
            mode,
            size,
            nlink,
            uid,
            gid,
            mtime,
            mtime_nsec,
            data_layout,
            startblk_lo: i_u,
            rdev: i_u,
            xattr_ibody_size,
        })
    }

    fn lookup_path(&mut self, path: &str) -> io::Result<InodeInfo> {
        let components: Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|c| !c.is_empty())
            .collect();

        if components.is_empty() {
            if path == "/" {
                return self.read_inode(self.root_nid);
            }
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty path"));
        }

        let mut current_nid = self.root_nid;
        for (i, component) in components.iter().enumerate() {
            let inode = self.read_inode(current_nid)?;
            let mode_type = inode.mode & S_IFMT;

            if mode_type != S_IFDIR {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("not a directory at component '{component}'"),
                ));
            }

            let target_nid = self.lookup_in_dir(&inode, component)?;
            if i + 1 == components.len() {
                return self.read_inode(target_nid);
            }

            current_nid = target_nid;
        }

        Err(io::Error::new(io::ErrorKind::NotFound, "path not found"))
    }

    /// Look up a named entry in a directory inode's data.
    ///
    /// EROFS directory data is organized as self-contained blocks. Each block
    /// starts with a packed array of 12-byte dirent headers, followed by the
    /// concatenated name strings. The first dirent's `nameoff` field divided
    /// by 12 gives the number of dirents in that block (the kernel uses this
    /// same trick). Name lengths are derived from consecutive `nameoff`
    /// values; the last entry's name extends to the end of valid data.
    fn lookup_in_dir(&mut self, dir_inode: &InodeInfo, name: &str) -> io::Result<u32> {
        let blksiz = EROFS_BLKSIZ as usize;
        let target = name.as_bytes();
        let block_count = self.checked_inode_data_len(dir_inode)?.div_ceil(blksiz);
        let mut left = 0usize;
        let mut right = block_count;

        while left < right {
            let mid = (left + right) / 2;
            let block = self.read_inode_data_block(dir_inode, mid)?;
            let dirent_count = dir_block_dirent_count(&block)?;
            let first_name = dirent_name(&block, 0, dirent_count)?;
            let last_name = dirent_name(&block, dirent_count - 1, dirent_count)?;

            if target < first_name {
                right = mid;
                continue;
            }

            if target > last_name {
                left = mid + 1;
                continue;
            }

            return lookup_in_dir_block(&block, dirent_count, target)?.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("entry '{name}' not found in directory"),
                )
            });
        }

        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("entry '{name}' not found in directory"),
        ))
    }

    fn walk_dir(
        &mut self,
        dir_inode: &InodeInfo,
        dir_path: PathBuf,
        entries: &mut Vec<ErofsTreeEntry>,
        visited: &mut HashSet<u32>,
    ) -> io::Result<()> {
        if !visited.insert(dir_inode.nid) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "cycle detected while walking EROFS directory tree",
            ));
        }

        self.visit_dir_entries::<io::Error, _>(dir_inode, &mut |reader, name, nid| {
            if name.as_bytes() == b"." || name.as_bytes() == b".." {
                return Ok(());
            }

            let path = dir_path.join(&name);
            let inode = reader.read_inode(nid)?;
            let entry = reader.tree_entry(path.clone(), &inode)?;
            let is_dir = entry.kind == ErofsEntryKind::Directory;
            entries.push(entry);

            if is_dir {
                reader.walk_dir(&inode, path, entries, visited)?;
            }
            Ok(())
        })?;

        Ok(())
    }

    fn walk_dir_entries<E, F>(
        &mut self,
        dir_inode: &InodeInfo,
        dir_path: PathBuf,
        visited: &mut HashSet<u32>,
        visit: &mut F,
    ) -> Result<(), E>
    where
        E: From<io::Error>,
        F: FnMut(&mut Self, ErofsTreeEntry) -> Result<(), E>,
    {
        if !visited.insert(dir_inode.nid) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "cycle detected while walking EROFS directory tree",
            )
            .into());
        }

        self.visit_dir_entries::<E, _>(dir_inode, &mut |reader, name, nid| {
            if name.as_bytes() == b"." || name.as_bytes() == b".." {
                return Ok(());
            }

            let path = dir_path.join(&name);
            let inode = reader.read_inode(nid)?;
            let entry = reader.tree_entry(path.clone(), &inode)?;
            let is_dir = entry.kind == ErofsEntryKind::Directory;
            visit(reader, entry)?;

            if is_dir {
                reader.walk_dir_entries(&inode, path, visited, visit)?;
            }
            Ok(())
        })?;

        Ok(())
    }

    fn visit_dir_entries<E, F>(&mut self, dir_inode: &InodeInfo, visit: &mut F) -> Result<(), E>
    where
        E: From<io::Error>,
        F: FnMut(&mut Self, OsString, u32) -> Result<(), E>,
    {
        if (dir_inode.mode & S_IFMT) != S_IFDIR {
            return Err(
                io::Error::new(io::ErrorKind::InvalidInput, "target is not a directory").into(),
            );
        }

        let blksiz = EROFS_BLKSIZ as usize;
        let block_count = self.checked_inode_data_len(dir_inode)?.div_ceil(blksiz);

        for block_index in 0..block_count {
            let block = self.read_inode_data_block(dir_inode, block_index)?;
            if block.is_empty() {
                continue;
            }
            let dirent_count = dir_block_dirent_count(&block)?;
            for idx in 0..dirent_count {
                let name = dirent_name(&block, idx, dirent_count)?;
                if name.is_empty() {
                    continue;
                }
                visit(
                    self,
                    OsString::from_vec(name.to_vec()),
                    dirent_nid(&block, idx)?,
                )?;
            }
        }

        Ok(())
    }

    fn tree_entry(&mut self, path: PathBuf, inode: &InodeInfo) -> io::Result<ErofsTreeEntry> {
        let kind = inode_kind(inode)?;
        let rdev = if matches!(
            kind,
            ErofsEntryKind::CharDevice | ErofsEntryKind::BlockDevice
        ) {
            Some(decode_dev(inode.rdev))
        } else {
            None
        };

        Ok(ErofsTreeEntry {
            path,
            nid: inode.nid,
            kind,
            metadata: inode.metadata(),
            xattrs: self
                .read_inode_xattrs(inode)?
                .into_iter()
                .map(|(name, value)| Xattr { name, value })
                .collect(),
            size: inode.size,
            rdev,
        })
    }

    fn read_inode_data(&mut self, inode: &InodeInfo) -> io::Result<Vec<u8>> {
        let size = self.checked_inode_data_len(inode)?;
        if size == 0 {
            return Ok(Vec::new());
        }

        let blksiz = EROFS_BLKSIZ as usize;

        match inode.data_layout {
            EROFS_INODE_FLAT_PLAIN => {
                if inode.startblk_lo == EROFS_NULL_ADDR {
                    return Ok(Vec::new());
                }
                let data_offset = (inode.startblk_lo as u64) * (EROFS_BLKSIZ as u64);
                let mut data = vec![0u8; size];
                read_exact_at(&self.file, data_offset, &mut data)?;
                Ok(data)
            }
            EROFS_INODE_FLAT_INLINE => {
                let full_blocks = size / blksiz;
                let tail_size = size % blksiz;
                let mut data = Vec::with_capacity(size);

                // Read full blocks from data area.
                if full_blocks > 0 && inode.startblk_lo != EROFS_NULL_ADDR {
                    let data_offset = (inode.startblk_lo as u64) * (EROFS_BLKSIZ as u64);
                    let mut block_data = vec![0u8; full_blocks * blksiz];
                    read_exact_at(&self.file, data_offset, &mut block_data)?;
                    data.extend_from_slice(&block_data);
                }

                // Read inline tail from after inode metadata.
                if tail_size > 0 {
                    let inline_offset = self.inode_offset(inode.nid)
                        + EROFS_INODE_EXTENDED_SIZE as u64
                        + inode.xattr_ibody_size as u64;
                    let mut tail = vec![0u8; tail_size];
                    read_exact_at(&self.file, inline_offset, &mut tail)?;
                    data.extend_from_slice(&tail);
                }

                Ok(data)
            }
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("unsupported data layout: {}", inode.data_layout),
            )),
        }
    }

    fn read_inode_data_block(&self, inode: &InodeInfo, block_index: usize) -> io::Result<Vec<u8>> {
        let blksiz = EROFS_BLKSIZ as usize;
        let size = self.checked_inode_data_len(inode)?;
        let start = block_index.checked_mul(blksiz).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "directory block overflow")
        })?;
        if start >= size {
            return Ok(Vec::new());
        }

        let remaining = size - start;
        let len = remaining.min(blksiz);
        self.read_inode_data_range(inode, start as u64, len)
    }

    fn read_inode_data_range(
        &self,
        inode: &InodeInfo,
        start: u64,
        len: usize,
    ) -> io::Result<Vec<u8>> {
        let size = self.checked_inode_data_len(inode)? as u64;
        let end = start
            .checked_add(len as u64)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "inode range overflow"))?;
        if end > size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "inode data range exceeds inode size",
            ));
        }

        let segments = self.inode_data_segments(inode)?;
        let mut data = vec![0u8; len];
        let mut copied = 0usize;
        let mut logical_start = 0u64;

        for (file_offset, segment_len) in segments {
            let logical_end = logical_start.checked_add(segment_len).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "inode segment range overflow")
            })?;
            let overlap_start = start.max(logical_start);
            let overlap_end = end.min(logical_end);

            if overlap_start < overlap_end {
                let dst_start = (overlap_start - start) as usize;
                let read_len = (overlap_end - overlap_start) as usize;
                let source_offset = file_offset
                    .checked_add(overlap_start - logical_start)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "inode file offset overflow")
                    })?;
                read_exact_at(
                    &self.file,
                    source_offset,
                    &mut data[dst_start..dst_start + read_len],
                )?;
                copied += read_len;
            }

            logical_start = logical_end;
            if logical_start >= end {
                break;
            }
        }

        if copied != len {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "inode data range is not fully backed",
            ));
        }

        Ok(data)
    }

    fn checked_inode_data_len(&self, inode: &InodeInfo) -> io::Result<usize> {
        let file_len = self.file.metadata()?.len();
        if inode.size > file_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "inode data size exceeds EROFS image size",
            ));
        }

        usize::try_from(inode.size).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "inode data size does not fit in memory",
            )
        })
    }

    fn inode_data_segments(&self, inode: &InodeInfo) -> io::Result<Vec<(u64, u64)>> {
        let size = inode.size;
        if size == 0 {
            return Ok(Vec::new());
        }

        let blksiz = EROFS_BLKSIZ as u64;
        match inode.data_layout {
            EROFS_INODE_FLAT_PLAIN => {
                if inode.startblk_lo == EROFS_NULL_ADDR {
                    Ok(Vec::new())
                } else {
                    Ok(vec![((inode.startblk_lo as u64) * blksiz, size)])
                }
            }
            EROFS_INODE_FLAT_INLINE => {
                let full_blocks = size / blksiz;
                let tail_size = size % blksiz;
                let mut segments = Vec::new();
                if full_blocks > 0 && inode.startblk_lo != EROFS_NULL_ADDR {
                    segments.push(((inode.startblk_lo as u64) * blksiz, full_blocks * blksiz));
                }
                if tail_size > 0 {
                    segments.push((
                        self.inode_offset(inode.nid)
                            + EROFS_INODE_EXTENDED_SIZE as u64
                            + inode.xattr_ibody_size as u64,
                        tail_size,
                    ));
                }
                Ok(segments)
            }
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("unsupported data layout: {}", inode.data_layout),
            )),
        }
    }

    fn inode_is_opaque(&mut self, inode: &InodeInfo) -> io::Result<bool> {
        for (name, value) in self.read_inode_xattrs(inode)? {
            if name == b"trusted.overlay.opaque" && value == b"y" {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn read_inode_xattrs(&mut self, inode: &InodeInfo) -> io::Result<Vec<(Vec<u8>, Vec<u8>)>> {
        if inode.xattr_ibody_size == 0 {
            return Ok(Vec::new());
        }

        let total = inode.xattr_ibody_size as usize;
        if total < EROFS_XATTR_IBODY_HEADER_SIZE as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "xattr ibody smaller than header",
            ));
        }

        let mut offset = self.inode_offset(inode.nid)
            + EROFS_INODE_EXTENDED_SIZE as u64
            + EROFS_XATTR_IBODY_HEADER_SIZE as u64;
        let mut remaining = total - EROFS_XATTR_IBODY_HEADER_SIZE as usize;
        let mut xattrs = Vec::new();

        while remaining > 0 {
            if remaining < 4 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "truncated xattr entry header",
                ));
            }

            let mut entry = [0u8; 4];
            read_exact_at(&self.file, offset, &mut entry)?;

            let name_len = entry[0] as usize;
            let name_index = entry[1];
            let value_len = u16::from_le_bytes([entry[2], entry[3]]) as usize;
            let entry_size = 4 + name_len + value_len;
            let aligned_size = erofs_xattr_align(entry_size);

            if aligned_size > remaining {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "xattr entry exceeds ibody size",
                ));
            }

            let mut suffix = vec![0u8; name_len];
            read_exact_at(&self.file, offset + 4, &mut suffix)?;
            let mut value = vec![0u8; value_len];
            read_exact_at(&self.file, offset + 4 + name_len as u64, &mut value)?;

            let name = match name_index {
                EROFS_XATTR_INDEX_USER => [b"user.".as_slice(), suffix.as_slice()].concat(),
                EROFS_XATTR_INDEX_TRUSTED => [b"trusted.".as_slice(), suffix.as_slice()].concat(),
                EROFS_XATTR_INDEX_SECURITY => [b"security.".as_slice(), suffix.as_slice()].concat(),
                other => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("unsupported xattr name index: {other}"),
                    ));
                }
            };

            xattrs.push((name, value));
            offset += aligned_size as u64;
            remaining -= aligned_size;
        }

        Ok(xattrs)
    }
}

//--------------------------------------------------------------------------------------------------
// Types: Internal
//--------------------------------------------------------------------------------------------------

struct InodeInfo {
    nid: u32,
    mode: u16,
    size: u64,
    #[allow(dead_code)]
    nlink: u32,
    uid: u32,
    gid: u32,
    mtime: u64,
    mtime_nsec: u32,
    data_layout: u8,
    startblk_lo: u32,
    rdev: u32,
    xattr_ibody_size: u32,
}

impl InodeInfo {
    fn metadata(&self) -> InodeMetadata {
        InodeMetadata {
            uid: self.uid,
            gid: self.gid,
            mode: self.mode,
            mtime: self.mtime,
            mtime_nsec: self.mtime_nsec,
        }
    }
}

impl ErofsTreeEntry {
    /// Return true if this directory carries the overlay opaque marker.
    pub fn is_opaque(&self) -> bool {
        self.xattrs
            .iter()
            .any(|x| x.name == b"trusted.overlay.opaque" && x.value == b"y")
    }
}

impl Read for ErofsFileDataReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        while self.segment_index < self.segments.len() {
            let (offset, len) = self.segments[self.segment_index];
            if self.segment_offset >= len {
                self.segment_index += 1;
                self.segment_offset = 0;
                continue;
            }

            let remaining = (len - self.segment_offset) as usize;
            let to_read = remaining.min(buf.len());
            let read = self
                .file
                .read_at(&mut buf[..to_read], offset + self.segment_offset)?;
            self.segment_offset += read as u64;
            return Ok(read);
        }

        Ok(0)
    }
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

fn read_exact_at(file: &File, offset: u64, mut buf: &mut [u8]) -> io::Result<()> {
    let mut current_offset = offset;
    while !buf.is_empty() {
        let read = file.read_at(buf, current_offset)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "unexpected EOF",
            ));
        }
        current_offset += read as u64;
        buf = &mut buf[read..];
    }

    Ok(())
}

fn dir_block_dirent_count(block: &[u8]) -> io::Result<usize> {
    if block.len() < EROFS_DIRENT_SIZE as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "directory block smaller than one dirent",
        ));
    }

    let first_nameoff = u16::from_le_bytes([block[8], block[9]]) as usize;
    let dirent_size = EROFS_DIRENT_SIZE as usize;
    if first_nameoff < dirent_size
        || !first_nameoff.is_multiple_of(dirent_size)
        || first_nameoff > block.len()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid first dirent name offset",
        ));
    }

    Ok(first_nameoff / dirent_size)
}

fn dirent_name(block: &[u8], idx: usize, dirent_count: usize) -> io::Result<&[u8]> {
    let dirent_size = EROFS_DIRENT_SIZE as usize;
    let dirent_off = idx
        .checked_mul(dirent_size)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "dirent offset overflow"))?;

    if idx >= dirent_count || dirent_off + dirent_size > block.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "dirent index out of bounds",
        ));
    }

    let nameoff = u16::from_le_bytes([block[dirent_off + 8], block[dirent_off + 9]]) as usize;
    let mut name_end = if idx + 1 < dirent_count {
        let next_off = dirent_off + dirent_size;
        u16::from_le_bytes([block[next_off + 8], block[next_off + 9]]) as usize
    } else {
        block.len()
    };

    if nameoff > name_end || name_end > block.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "dirent name range out of bounds",
        ));
    }

    while name_end > nameoff && block[name_end - 1] == 0 {
        name_end -= 1;
    }

    Ok(&block[nameoff..name_end])
}

fn dirent_nid(block: &[u8], idx: usize) -> io::Result<u32> {
    let dirent_size = EROFS_DIRENT_SIZE as usize;
    let dirent_off = idx
        .checked_mul(dirent_size)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "dirent offset overflow"))?;
    if dirent_off + dirent_size > block.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "dirent NID out of bounds",
        ));
    }

    let nid = u64::from_le_bytes([
        block[dirent_off],
        block[dirent_off + 1],
        block[dirent_off + 2],
        block[dirent_off + 3],
        block[dirent_off + 4],
        block[dirent_off + 5],
        block[dirent_off + 6],
        block[dirent_off + 7],
    ]);
    u32::try_from(nid)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "dirent NID overflow"))
}

fn lookup_in_dir_block(
    block: &[u8],
    dirent_count: usize,
    target: &[u8],
) -> io::Result<Option<u32>> {
    let mut left = 0usize;
    let mut right = dirent_count;

    while left < right {
        let mid = (left + right) / 2;
        match target.cmp(dirent_name(block, mid, dirent_count)?) {
            std::cmp::Ordering::Less => right = mid,
            std::cmp::Ordering::Greater => left = mid + 1,
            std::cmp::Ordering::Equal => return dirent_nid(block, mid).map(Some),
        }
    }

    Ok(None)
}

fn inode_kind(inode: &InodeInfo) -> io::Result<ErofsEntryKind> {
    match inode.mode & S_IFMT {
        S_IFREG => Ok(ErofsEntryKind::RegularFile),
        S_IFDIR => Ok(ErofsEntryKind::Directory),
        S_IFLNK => Ok(ErofsEntryKind::Symlink),
        S_IFCHR => Ok(ErofsEntryKind::CharDevice),
        S_IFBLK => Ok(ErofsEntryKind::BlockDevice),
        S_IFIFO => Ok(ErofsEntryKind::Fifo),
        S_IFSOCK => Ok(ErofsEntryKind::Socket),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported inode mode type: {other:#o}"),
        )),
    }
}

fn decode_dev(encoded: u32) -> (u32, u32) {
    let major = (encoded >> 8) & 0x0000_0fff;
    let minor = (encoded & 0x0000_00ff) | ((encoded >> 12) & 0xffff_ff00);
    (major, minor)
}

/// Read a file from an EROFS image file on disk.
pub fn read_file_from_erofs(image_path: &Path, file_path: &str) -> io::Result<Vec<u8>> {
    let file = std::fs::File::open(image_path)?;
    let mut reader = ErofsReader::new(file)?;
    reader.read_file(file_path)
}

pub fn entry_info_from_erofs(image_path: &Path, file_path: &str) -> io::Result<ErofsEntryInfo> {
    let file = std::fs::File::open(image_path)?;
    let mut reader = ErofsReader::new(file)?;
    reader.entry_info(file_path)
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::{fs::File, io, path::PathBuf};

    use tempfile::tempdir;

    use super::ErofsReader;
    use crate::cli::erofs::tree::{
        FileData, FileTree, InodeMetadata, RegularFileId, RegularFileNode, TreeNode,
    };
    use crate::cli::erofs::write_erofs;

    fn make_regular_file(data: &[u8]) -> TreeNode {
        make_regular_file_with_id(data, RegularFileId::new())
    }

    fn make_regular_file_with_id(data: &[u8], id: RegularFileId) -> TreeNode {
        TreeNode::RegularFile(RegularFileNode {
            id,
            metadata: InodeMetadata::default(),
            xattrs: Vec::new(),
            data: FileData::Memory(data.to_vec()),
            nlink: 1,
        })
    }

    #[test]
    fn lookup_path_resolves_large_multi_block_directory() {
        let mut tree = FileTree::new();
        for i in 0..5000 {
            let path = format!("dir/file-{i:04}.txt");
            tree.insert(path.as_bytes(), make_regular_file(b"x"))
                .expect("insert file");
        }

        let output_dir = tempdir().expect("tempdir");
        let output = output_dir.path().join("large-dir.erofs");
        write_erofs(&tree, &output).expect("write erofs");

        let file = File::open(&output).expect("open erofs");
        let mut reader = ErofsReader::new(file).expect("reader");

        assert_eq!(reader.read_file("/dir/file-0000.txt").expect("first"), b"x");
        assert_eq!(
            reader.read_file("/dir/file-2500.txt").expect("middle"),
            b"x"
        );
        assert_eq!(reader.read_file("/dir/file-4999.txt").expect("last"), b"x");

        let err = reader
            .entry_info("/dir/file-9999.txt")
            .expect_err("missing entry should fail");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn hardlinked_regular_files_share_inode_and_data_blocks() {
        let mut tree = FileTree::new();
        let file_id = RegularFileId::new();

        tree.insert(b"alpha", make_regular_file_with_id(b"shared", file_id))
            .expect("insert alpha");
        tree.insert(b"beta", make_regular_file_with_id(b"shared", file_id))
            .expect("insert beta");

        let output_dir = tempdir().expect("tempdir");
        let output = output_dir.path().join("hardlinks.erofs");
        let data_map = write_erofs(&tree, &output).expect("write erofs");
        let alpha_path = PathBuf::from("alpha");
        let beta_path = PathBuf::from("beta");

        assert_eq!(
            data_map
                .file_blocks
                .get(&alpha_path)
                .copied()
                .expect("alpha data map"),
            data_map
                .file_blocks
                .get(&beta_path)
                .copied()
                .expect("beta data map")
        );

        let file = File::open(&output).expect("open erofs");
        let mut reader = ErofsReader::new(file).expect("reader");
        let alpha = reader.inode_debug_info("/alpha").expect("alpha inode");
        let beta = reader.inode_debug_info("/beta").expect("beta inode");

        assert_eq!(alpha.nid, beta.nid);
        assert_eq!(alpha.nlink, 2);
        assert_eq!(beta.nlink, 2);
        assert_eq!(alpha.size, b"shared".len() as u64);
        assert_eq!(reader.read_file("/alpha").expect("read alpha"), b"shared");
        assert_eq!(reader.read_file("/beta").expect("read beta"), b"shared");
    }
}
