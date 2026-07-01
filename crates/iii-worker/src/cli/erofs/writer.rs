use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::cli::erofs::crc32c;
use crate::cli::erofs::tree::{
    DirectoryNode, FileTree, InodeMetadata, RegularFileId, TreeNode, Xattr,
};

use super::format::{
    self, EROFS_BLKSIZ, EROFS_BLKSIZ_BITS, EROFS_DIRENT_SIZE, EROFS_FEATURE_COMPAT_SB_CHKSUM,
    EROFS_INODE_EXTENDED_SIZE, EROFS_INODE_FLAT_INLINE, EROFS_INODE_FLAT_PLAIN, EROFS_ISLOT_SIZE,
    EROFS_NULL_ADDR, EROFS_SUPER_MAGIC, EROFS_SUPER_OFFSET, EROFS_SUPERBLOCK_SIZE,
    EROFS_XATTR_IBODY_HEADER_SIZE, dirent_file_type, erofs_xattr_align, mode_type_bits,
    new_encode_dev, xattr_prefix_index,
};

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// Stack-allocated zero buffer for padding writes (avoids heap allocation per pad).
static ZEROS: [u8; 4096] = [0u8; 4096];

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

#[derive(Debug)]
pub enum ErofsError {
    Io(io::Error),
    NidOverflow,
    UnsupportedXattrPrefix,
}

/// Data layout map returned by [`write_erofs()`].
///
/// Records where each file's data blocks were placed in the output image,
/// consumed by the fsmeta writer to build chunk-based inodes.
#[derive(Debug, Clone)]
pub struct ErofsDataMap {
    /// For each file path: `(start_block, size_bytes)` within the output image.
    pub file_blocks: HashMap<PathBuf, (u32, u64)>,
    /// Total block count of the output image.
    pub total_blocks: u32,
}

#[allow(dead_code)]
struct InodePlan {
    nid: u32,
    data_layout: u8,
    data_block_start: u32,
    data_block_count: u32,
    inline_tail_size: u32,
    xattr_ibody_size: u32,
    total_inode_size: u32,
    slots: u32,
    dir_data: Option<Vec<u8>>,
    parent_nid: u32,
}

#[allow(dead_code)]
struct LayoutState {
    plans: Vec<InodePlan>,
    regular_file_plans: HashMap<RegularFileId, usize>,
    regular_file_link_counts: HashMap<RegularFileId, u32>,
    current_meta_offset: u64,
    current_data_block: u32,
    meta_blkaddr: u32,
    root_nid: u32,
    inode_count: u64,
}

pub(super) struct CursorTrackingWriter<'a, W> {
    inner: &'a mut W,
    cursor: &'a mut u64,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl LayoutState {
    fn new() -> Self {
        Self {
            plans: Vec::new(),
            regular_file_plans: HashMap::new(),
            regular_file_link_counts: HashMap::new(),
            current_meta_offset: EROFS_BLKSIZ as u64,
            current_data_block: 0,
            meta_blkaddr: 1,
            root_nid: 0,
            inode_count: 0,
        }
    }
}

impl<'a, W> CursorTrackingWriter<'a, W> {
    pub(super) fn new(inner: &'a mut W, cursor: &'a mut u64) -> Self {
        Self { inner, cursor }
    }
}

//--------------------------------------------------------------------------------------------------
// Trait Implementations
//--------------------------------------------------------------------------------------------------

impl std::fmt::Display for ErofsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErofsError::Io(e) => write!(f, "I/O error: {e}"),
            ErofsError::NidOverflow => write!(f, "root NID exceeds u16::MAX"),
            ErofsError::UnsupportedXattrPrefix => write!(f, "unsupported xattr prefix"),
        }
    }
}

impl std::error::Error for ErofsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ErofsError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for ErofsError {
    fn from(e: io::Error) -> Self {
        ErofsError::Io(e)
    }
}

impl<W: Write> Write for CursorTrackingWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buf)?;
        *self.cursor += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

pub fn write_erofs(tree: &FileTree, output: &Path) -> Result<ErofsDataMap, ErofsError> {
    let mut file = BufWriter::new(std::fs::File::create(output)?);
    let mut state = LayoutState::new();

    // Phase 1+2: Plan layout and assign NIDs.
    plan_directory(&tree.root, 0, &mut state, true)?;
    state.root_nid = state.plans[0].nid;

    // Phase 3: Write data blocks and collect data map.
    let regular_file_paths = state
        .regular_file_link_counts
        .values()
        .map(|count| *count as usize)
        .sum();
    let mut data_map = HashMap::with_capacity(regular_file_paths);
    write_data_blocks(&mut file, &state, tree, &mut data_map)?;

    // Phase 4: Write metadata.
    write_metadata(&mut file, &state, tree)?;

    // Phase 5: Write superblock.
    write_superblock(&mut file, &state)?;

    // Flush buffered writes, then pad the file to a whole number of 4096-byte
    // EROFS blocks. Also ensure 512-byte sector alignment for virtio-blk.
    file.flush()?;

    // stream_position() may not reflect the true file length after seeks,
    // so seek to end to get the actual length.
    let current_len = file.seek(SeekFrom::End(0))?;
    let block_aligned = align_to_block(current_len);
    // Also align to 512-byte sectors for virtio-blk.
    let sector_aligned = block_aligned.div_ceil(512) * 512;
    let target_len = sector_aligned.max(block_aligned);

    if target_len > current_len {
        file.seek(SeekFrom::Start(target_len - 1))?;
        file.write_all(&[0u8])?;
        file.flush()?;
    }

    // Compute total blocks from the final file size.
    let final_len = file.seek(SeekFrom::End(0))?;
    let total_blocks = (final_len / EROFS_BLKSIZ as u64) as u32;

    Ok(ErofsDataMap {
        file_blocks: data_map,
        total_blocks,
    })
}

pub(super) fn compute_xattr_ibody_size(xattrs: &[Xattr]) -> Result<u32, ErofsError> {
    if xattrs.is_empty() {
        return Ok(0);
    }

    let mut size = EROFS_XATTR_IBODY_HEADER_SIZE as usize;
    for xattr in xattrs {
        let (_, suffix) =
            xattr_prefix_index(&xattr.name).ok_or(ErofsError::UnsupportedXattrPrefix)?;
        // erofs_xattr_entry (4 bytes) + suffix name + value, aligned to 4
        let entry_size = 4 + suffix.len() + xattr.value.len();
        size += erofs_xattr_align(entry_size);
    }

    Ok(size as u32)
}

pub(super) fn compute_xattr_icount(xattr_ibody_size: u32) -> u16 {
    if xattr_ibody_size == 0 {
        0
    } else {
        ((xattr_ibody_size - EROFS_XATTR_IBODY_HEADER_SIZE) / 4 + 1) as u16
    }
}

/// Layout decision result for an inode's data storage strategy.
pub(super) struct DataLayoutDecision {
    pub(super) layout: u8,
    pub(super) inline_tail_size: u32,
    pub(super) block_count: u32,
    pub(super) block_start: u32,
}

/// Decide between FLAT_PLAIN and FLAT_INLINE for an inode's data.
///
/// FLAT_INLINE stores the tail (< block_size remainder) immediately after
/// the inode metadata, saving a data block. Falls back to FLAT_PLAIN
/// (with a padded last block) if the tail doesn't fit in the current
/// metadata block alongside the inode.
///
/// `allow_inline` must be false for regular files — the fsmeta references
/// each full block via a chunk index by block address, so the tail must
/// live in a real data block (padded) rather than being inlined in the
/// per-layer EROFS metadata area, which fsmeta cannot reach.
pub(super) fn decide_data_layout(
    data_size: u64,
    inode_fixed_size: u32,
    meta_offset: u64,
    current_data_block: &mut u32,
    allow_inline: bool,
) -> DataLayoutDecision {
    let blksiz = EROFS_BLKSIZ as u64;
    let tail_size = data_size % blksiz;
    let full_blocks = data_size / blksiz;

    if data_size == 0 {
        DataLayoutDecision {
            layout: EROFS_INODE_FLAT_PLAIN,
            inline_tail_size: 0,
            block_count: 0,
            block_start: EROFS_NULL_ADDR,
        }
    } else if tail_size == 0 {
        let start = *current_data_block;
        *current_data_block += full_blocks as u32;
        DataLayoutDecision {
            layout: EROFS_INODE_FLAT_PLAIN,
            inline_tail_size: 0,
            block_count: full_blocks as u32,
            block_start: start,
        }
    } else if allow_inline {
        let inode_pos_in_block = meta_offset % blksiz;
        let remaining_in_block = blksiz - inode_pos_in_block;
        let needed = inode_fixed_size as u64 + tail_size;

        if needed <= remaining_in_block {
            let start = if full_blocks > 0 {
                let s = *current_data_block;
                *current_data_block += full_blocks as u32;
                s
            } else {
                EROFS_NULL_ADDR
            };
            DataLayoutDecision {
                layout: EROFS_INODE_FLAT_INLINE,
                inline_tail_size: tail_size as u32,
                block_count: full_blocks as u32,
                block_start: start,
            }
        } else {
            let start = *current_data_block;
            *current_data_block += (full_blocks + 1) as u32;
            DataLayoutDecision {
                layout: EROFS_INODE_FLAT_PLAIN,
                inline_tail_size: 0,
                block_count: (full_blocks + 1) as u32,
                block_start: start,
            }
        }
    } else {
        let start = *current_data_block;
        *current_data_block += (full_blocks + 1) as u32;
        DataLayoutDecision {
            layout: EROFS_INODE_FLAT_PLAIN,
            inline_tail_size: 0,
            block_count: (full_blocks + 1) as u32,
            block_start: start,
        }
    }
}

pub(super) fn compute_dir_data_size(dir: &DirectoryNode) -> u32 {
    // Total entries = 2 (. and ..) + number of children
    let entry_count = 2 + dir.entries.len();

    // Collect all names to determine block packing
    let mut names: Vec<&[u8]> = Vec::with_capacity(entry_count);
    names.push(b".");
    names.push(b"..");
    for name in dir.entries.keys() {
        names.push(name.as_bytes());
    }
    // EROFS requires dirents sorted by name in byte order.
    names.sort();

    // Pack entries into blocks. Each block is EROFS_BLKSIZ bytes.
    // Block layout: dirents first, then names.
    let blksiz = EROFS_BLKSIZ as usize;
    let mut total_size = 0usize;
    let mut idx = 0;

    while idx < names.len() {
        // Figure out how many entries fit in this block
        let mut block_entries = 0;
        let mut dirent_area = 0usize;
        let mut name_area = 0usize;

        for name in &names[idx..] {
            let new_dirent_area = (block_entries + 1) * EROFS_DIRENT_SIZE as usize;
            let new_name_area = name_area + name.len();
            if new_dirent_area + new_name_area > blksiz {
                break;
            }
            dirent_area = new_dirent_area;
            name_area = new_name_area;
            block_entries += 1;
        }

        if block_entries == 0 {
            // Single entry that's too big shouldn't happen for reasonable names
            block_entries = 1;
            name_area = names[idx].len();
            dirent_area = EROFS_DIRENT_SIZE as usize;
        }

        let used = dirent_area + name_area;
        // Last block: size is the used portion (not padded to block boundary for inline)
        // But for data blocks, we pad. We'll track the actual used size.
        // For sizing purposes, non-last blocks are full blocks.
        if idx + block_entries < names.len() {
            total_size += blksiz;
        } else {
            total_size += used;
        }

        idx += block_entries;
    }

    total_size as u32
}

/// Serialize directory entries into EROFS directory data blocks.
///
/// EROFS directory blocks are self-contained: each block packs 12-byte
/// dirent headers at the start followed by the concatenated name strings.
/// `dirent[0].nameoff / 12` tells the kernel how many entries are in the
/// block, so the first nameoff must equal the total dirent header area.
///
/// Entries are sorted alphabetically by name (the kernel binary-searches
/// within each block). `.` and `..` are always the first two entries.
///
/// The last block may be shorter than 4096 bytes — it will be stored
/// inline after the inode if the layout planner chose FLAT_INLINE.
pub(super) fn serialize_dir_blocks(
    dir: &DirectoryNode,
    own_nid: u32,
    parent_nid: u32,
    child_nids: &BTreeMap<OsString, u32>,
) -> Result<Vec<u8>, ErofsError> {
    struct DirEntryInfo {
        name: Vec<u8>,
        nid: u64,
        file_type: u8,
    }

    let mut entries: Vec<DirEntryInfo> = Vec::new();

    entries.push(DirEntryInfo {
        name: b".".to_vec(),
        nid: own_nid as u64,
        file_type: format::EROFS_FT_DIR,
    });
    entries.push(DirEntryInfo {
        name: b"..".to_vec(),
        nid: parent_nid as u64,
        file_type: format::EROFS_FT_DIR,
    });

    for (name, child) in &dir.entries {
        let nid = *child_nids.get(name).expect("child NID not found") as u64;
        entries.push(DirEntryInfo {
            name: name.as_bytes().to_vec(),
            nid,
            file_type: dirent_file_type(child),
        });
    }

    // EROFS requires dirents sorted by name in byte order (memcmp).
    entries.sort_by(|a, b| a.name.cmp(&b.name));

    let blksiz = EROFS_BLKSIZ as usize;
    let mut result = Vec::new();
    let mut idx = 0;

    while idx < entries.len() {
        // Determine how many entries fit in this block
        let mut block_entries = 0usize;
        let mut name_total = 0usize;

        for entry in &entries[idx..] {
            let new_dirent_area = (block_entries + 1) * EROFS_DIRENT_SIZE as usize;
            let new_name_total = name_total + entry.name.len();
            if new_dirent_area + new_name_total > blksiz {
                break;
            }
            name_total += entry.name.len();
            block_entries += 1;
        }

        if block_entries == 0 {
            block_entries = 1;
            name_total = entries[idx].name.len();
        }

        let dirent_area_size = block_entries * EROFS_DIRENT_SIZE as usize;
        let is_last_block = idx + block_entries >= entries.len();

        // Build this block
        let mut block = vec![
            0u8;
            if is_last_block {
                dirent_area_size + name_total
            } else {
                blksiz
            }
        ];

        // Write dirents
        let mut name_offset = dirent_area_size;
        for i in 0..block_entries {
            let e = &entries[idx + i];
            let dirent_off = i * EROFS_DIRENT_SIZE as usize;

            // nid: u64 at offset 0
            block[dirent_off..dirent_off + 8].copy_from_slice(&e.nid.to_le_bytes());
            // nameoff: u16 at offset 8
            block[dirent_off + 8..dirent_off + 10]
                .copy_from_slice(&(name_offset as u16).to_le_bytes());
            // file_type: u8 at offset 10
            block[dirent_off + 10] = e.file_type;
            // reserved: u8 at offset 11
            block[dirent_off + 11] = 0;

            // Write name
            block[name_offset..name_offset + e.name.len()].copy_from_slice(&e.name);
            name_offset += e.name.len();
        }

        result.extend_from_slice(&block);
        idx += block_entries;
    }

    Ok(result)
}

pub(super) fn node_data_size(node: &TreeNode) -> u64 {
    match node {
        TreeNode::RegularFile(f) => f.data.len() as u64,
        TreeNode::Symlink(s) => s.target.len() as u64,
        _ => 0,
    }
}

pub(super) fn node_xattrs(node: &TreeNode) -> &[Xattr] {
    match node {
        TreeNode::RegularFile(f) => &f.xattrs,
        TreeNode::Directory(d) => &d.xattrs,
        _ => &[],
    }
}

pub(super) fn node_metadata(node: &TreeNode) -> &InodeMetadata {
    match node {
        TreeNode::RegularFile(f) => &f.metadata,
        TreeNode::Directory(d) => &d.metadata,
        TreeNode::Symlink(s) => &s.metadata,
        TreeNode::CharDevice(d) => &d.metadata,
        TreeNode::BlockDevice(d) => &d.metadata,
        TreeNode::Fifo(m) => m,
        TreeNode::Socket(m) => m,
    }
}

pub(super) fn node_nlink(
    node: &TreeNode,
    regular_file_link_counts: &HashMap<RegularFileId, u32>,
) -> u32 {
    match node {
        TreeNode::RegularFile(f) => regular_file_link_counts
            .get(&f.id)
            .copied()
            .unwrap_or(f.nlink.max(1)),
        TreeNode::Directory(d) => {
            // nlink for directory = 2 + number of child directories
            let child_dirs = d
                .entries
                .values()
                .filter(|c| matches!(c, TreeNode::Directory(_)))
                .count();
            2 + child_dirs as u32
        }
        _ => 1,
    }
}

/// Recursive function that plans the layout for a directory and all its descendants.
/// Assigns NIDs, computes data layouts, and tracks data block assignments.
fn plan_directory(
    dir: &DirectoryNode,
    parent_nid: u32,
    state: &mut LayoutState,
    is_root: bool,
) -> Result<u32, ErofsError> {
    let blksiz = EROFS_BLKSIZ as u64;

    // Reserve an index for this directory's plan
    let dir_plan_idx = state.plans.len();
    state.plans.push(InodePlan {
        nid: 0,
        data_layout: 0,
        data_block_start: 0,
        data_block_count: 0,
        inline_tail_size: 0,
        xattr_ibody_size: 0,
        total_inode_size: 0,
        slots: 0,
        dir_data: None,
        parent_nid,
    });
    state.inode_count += 1;

    // Compute xattr ibody size for the directory
    let xattr_ibody_size = compute_xattr_ibody_size(&dir.xattrs)?;

    // Compute directory data size
    let dir_data_size = compute_dir_data_size(dir) as u64;

    // Determine data layout for this directory
    let inode_fixed_size = EROFS_INODE_EXTENDED_SIZE + xattr_ibody_size;

    // Assign NID for this directory
    let meta_base = state.meta_blkaddr as u64 * blksiz;
    let nid_offset = state.current_meta_offset - meta_base;
    if !nid_offset.is_multiple_of(EROFS_ISLOT_SIZE as u64) {
        // Align to slot boundary
        let aligned = nid_offset.div_ceil(EROFS_ISLOT_SIZE as u64) * EROFS_ISLOT_SIZE as u64;
        state.current_meta_offset = meta_base + aligned;
    }

    let nid_offset = state.current_meta_offset - meta_base;
    let nid = (nid_offset / EROFS_ISLOT_SIZE as u64) as u32;

    let d = decide_data_layout(
        dir_data_size,
        inode_fixed_size,
        state.current_meta_offset,
        &mut state.current_data_block,
        true, // directories: fsmeta builds its own, layer dir data is unreferenced
    );
    let (data_layout, inline_tail_size, data_block_count, data_block_start) =
        (d.layout, d.inline_tail_size, d.block_count, d.block_start);

    let total_inode_size = inode_fixed_size + inline_tail_size;
    let slots = total_inode_size.div_ceil(EROFS_ISLOT_SIZE);

    state.current_meta_offset += (slots * EROFS_ISLOT_SIZE) as u64;

    // Update the plan
    state.plans[dir_plan_idx] = InodePlan {
        nid,
        data_layout,
        data_block_start,
        data_block_count,
        inline_tail_size,
        xattr_ibody_size,
        total_inode_size,
        slots,
        dir_data: None,
        parent_nid: if is_root { nid } else { parent_nid },
    };

    let dir_nid = nid;

    // Now plan children (depth-first)
    let mut child_nids: BTreeMap<OsString, u32> = BTreeMap::new();

    for (name, child) in &dir.entries {
        let child_nid = match child {
            TreeNode::Directory(child_dir) => plan_directory(child_dir, dir_nid, state, false)?,
            TreeNode::RegularFile(file) => plan_regular_file(child, file.id, state)?,
            _ => plan_leaf_node(child, state)?,
        };
        child_nids.insert(name.clone(), child_nid);
    }

    // Now serialize directory data with real NIDs
    let dir_data = serialize_dir_blocks(
        dir,
        dir_nid,
        state.plans[dir_plan_idx].parent_nid,
        &child_nids,
    )?;
    state.plans[dir_plan_idx].dir_data = Some(dir_data);

    Ok(dir_nid)
}

fn plan_regular_file(
    node: &TreeNode,
    file_id: RegularFileId,
    state: &mut LayoutState,
) -> Result<u32, ErofsError> {
    *state.regular_file_link_counts.entry(file_id).or_insert(0) += 1;

    if let Some(plan_idx) = state.regular_file_plans.get(&file_id) {
        return Ok(state.plans[*plan_idx].nid);
    }

    let plan_idx = state.plans.len();
    let nid = plan_leaf_node(node, state)?;
    state.regular_file_plans.insert(file_id, plan_idx);
    Ok(nid)
}

fn plan_leaf_node(node: &TreeNode, state: &mut LayoutState) -> Result<u32, ErofsError> {
    let blksiz = EROFS_BLKSIZ as u64;

    let plan_idx = state.plans.len();
    state.plans.push(InodePlan {
        nid: 0,
        data_layout: 0,
        data_block_start: 0,
        data_block_count: 0,
        inline_tail_size: 0,
        xattr_ibody_size: 0,
        total_inode_size: 0,
        slots: 0,
        dir_data: None,
        parent_nid: 0,
    });
    state.inode_count += 1;

    let xattrs = node_xattrs(node);
    let xattr_ibody_size = compute_xattr_ibody_size(xattrs)?;
    let data_size = node_data_size(node);
    let inode_fixed_size = EROFS_INODE_EXTENDED_SIZE + xattr_ibody_size;

    // Assign NID
    let meta_base = state.meta_blkaddr as u64 * blksiz;
    let nid_offset = state.current_meta_offset - meta_base;
    let aligned_offset = nid_offset.div_ceil(EROFS_ISLOT_SIZE as u64) * EROFS_ISLOT_SIZE as u64;
    state.current_meta_offset = meta_base + aligned_offset;

    let nid = (aligned_offset / EROFS_ISLOT_SIZE as u64) as u32;

    // Regular files must use FLAT_PLAIN so every block is addressable by
    // fsmeta chunk indexes. Symlinks/devices/fifos/sockets can still inline
    // since fsmeta reads their content from the tree directly.
    let allow_inline = !matches!(node, TreeNode::RegularFile(_));
    let d = decide_data_layout(
        data_size,
        inode_fixed_size,
        state.current_meta_offset,
        &mut state.current_data_block,
        allow_inline,
    );
    let (data_layout, inline_tail_size, data_block_count, data_block_start) =
        (d.layout, d.inline_tail_size, d.block_count, d.block_start);

    let total_inode_size = inode_fixed_size + inline_tail_size;
    let slots = total_inode_size.div_ceil(EROFS_ISLOT_SIZE);

    state.current_meta_offset += (slots * EROFS_ISLOT_SIZE) as u64;

    state.plans[plan_idx] = InodePlan {
        nid,
        data_layout,
        data_block_start,
        data_block_count,
        inline_tail_size,
        xattr_ibody_size,
        total_inode_size,
        slots,
        dir_data: None,
        parent_nid: 0,
    };

    Ok(nid)
}

pub(super) fn write_zero_padding_to(
    file: &mut impl Write,
    cursor: &mut u64,
    target: u64,
) -> Result<(), ErofsError> {
    if target < *cursor {
        return Err(ErofsError::Io(io::Error::other(
            "EROFS layout cursor moved backwards",
        )));
    }

    while *cursor < target {
        let remaining = (target - *cursor) as usize;
        let to_write = remaining.min(ZEROS.len());
        file.write_all(&ZEROS[..to_write])?;
        *cursor += to_write as u64;
    }

    Ok(())
}

fn write_data_blocks(
    file: &mut (impl Write + Seek),
    state: &LayoutState,
    tree: &FileTree,
    data_map: &mut HashMap<PathBuf, (u32, u64)>,
) -> Result<(), ErofsError> {
    // Compute where data area starts: after the metadata area
    // The metadata area ends at current_meta_offset, rounded up to block boundary
    let meta_end = align_to_block(state.current_meta_offset);
    let data_area_start = meta_end;

    // We need to figure out the data block base. The data blocks are numbered
    // starting from the block after metadata.
    // Actually, data_block_start in each plan is an absolute block number.
    // We need to determine: what block number does the data area start at?
    let data_start_block = (data_area_start / EROFS_BLKSIZ as u64) as u32;

    // Wait - we assigned current_data_block starting from 0 during planning,
    // but we need them to be absolute block numbers. Let's fix this.
    // Actually, looking at the plan again: data_block_start should be absolute.
    // During planning, we started current_data_block at 0, which means they're
    // relative to the data area. We need to add the data_start_block offset.

    file.seek(SeekFrom::Start(data_area_start))?;
    let mut data_cursor = data_area_start;

    let current_path = PathBuf::new();
    write_data_for_tree(
        file,
        state,
        &tree.root,
        data_start_block,
        &mut 0,
        &current_path,
        data_map,
        &mut data_cursor,
    )?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_data_for_tree(
    file: &mut (impl Write + Seek),
    state: &LayoutState,
    dir: &DirectoryNode,
    data_start_block: u32,
    plan_idx: &mut usize,
    current_path: &Path,
    data_map: &mut HashMap<PathBuf, (u32, u64)>,
    data_cursor: &mut u64,
) -> Result<(), ErofsError> {
    let blksiz = EROFS_BLKSIZ as u64;
    let plan = &state.plans[*plan_idx];
    *plan_idx += 1;

    // Write directory data blocks (non-inline portion)
    if let Some(ref dir_data) = plan.dir_data
        && plan.data_block_count > 0
    {
        let abs_block = data_start_block + plan.data_block_start;
        let offset = abs_block as u64 * blksiz;
        write_zero_padding_to(file, data_cursor, offset)?;
        let mut tracked = CursorTrackingWriter::new(file, data_cursor);

        let full_block_bytes = plan.data_block_count as usize * EROFS_BLKSIZ as usize;
        let data_to_write = &dir_data[..std::cmp::min(full_block_bytes, dir_data.len())];
        tracked.write_all(data_to_write)?;

        // Pad remaining space in last full block if needed
        if data_to_write.len() < full_block_bytes {
            let pad = full_block_bytes - data_to_write.len();
            tracked.write_all(&ZEROS[..pad])?;
        }
    }

    // Recurse into children in BTreeMap order
    for (name, child) in &dir.entries {
        let child_path = current_path.join(name);
        match child {
            TreeNode::Directory(child_dir) => {
                write_data_for_tree(
                    file,
                    state,
                    child_dir,
                    data_start_block,
                    plan_idx,
                    &child_path,
                    data_map,
                    data_cursor,
                )?;
            }
            TreeNode::RegularFile(f) => {
                let child_plan_idx = *state
                    .regular_file_plans
                    .get(&f.id)
                    .expect("regular file plan missing");
                let child_plan = &state.plans[child_plan_idx];
                let first_visit = child_plan_idx == *plan_idx;
                if first_visit {
                    *plan_idx += 1;
                }

                // Record block address for this file in the data map.
                if child_plan.data_block_start != EROFS_NULL_ADDR {
                    let abs_block = data_start_block + child_plan.data_block_start;
                    data_map.insert(child_path, (abs_block, f.data.len() as u64));
                } else {
                    // Empty file — record with NULL_ADDR.
                    data_map.insert(child_path, (EROFS_NULL_ADDR, 0));
                }

                if first_visit && child_plan.data_block_count > 0 {
                    let abs_block = data_start_block + child_plan.data_block_start;
                    let offset = abs_block as u64 * blksiz;
                    write_zero_padding_to(file, data_cursor, offset)?;
                    let mut tracked = CursorTrackingWriter::new(file, data_cursor);

                    let full_block_bytes =
                        child_plan.data_block_count as usize * EROFS_BLKSIZ as usize;
                    let data_end = if child_plan.data_layout == EROFS_INODE_FLAT_INLINE {
                        full_block_bytes
                    } else {
                        std::cmp::min(f.data.len(), full_block_bytes)
                    };

                    f.data.write_range(0, data_end, &mut tracked)?;

                    if child_plan.data_layout == EROFS_INODE_FLAT_PLAIN
                        && data_end < full_block_bytes
                    {
                        let pad = full_block_bytes - data_end;
                        tracked.write_all(&ZEROS[..pad])?;
                    }
                }
            }
            TreeNode::Symlink(s) => {
                let child_plan = &state.plans[*plan_idx];
                *plan_idx += 1;

                if child_plan.data_block_count > 0 {
                    let abs_block = data_start_block + child_plan.data_block_start;
                    let offset = abs_block as u64 * blksiz;
                    write_zero_padding_to(file, data_cursor, offset)?;
                    let mut tracked = CursorTrackingWriter::new(file, data_cursor);

                    let full_block_bytes =
                        child_plan.data_block_count as usize * EROFS_BLKSIZ as usize;
                    let data_end = if child_plan.data_layout == EROFS_INODE_FLAT_INLINE {
                        full_block_bytes
                    } else {
                        std::cmp::min(s.target.len(), full_block_bytes)
                    };

                    tracked.write_all(&s.target[..data_end])?;

                    if child_plan.data_layout == EROFS_INODE_FLAT_PLAIN
                        && data_end < full_block_bytes
                    {
                        let pad = full_block_bytes - data_end;
                        tracked.write_all(&ZEROS[..pad])?;
                    }
                }
            }
            _ => {
                // CharDevice, BlockDevice, Fifo, Socket have no data
                *plan_idx += 1;
            }
        }
    }

    Ok(())
}

fn write_metadata(
    file: &mut (impl Write + Seek),
    state: &LayoutState,
    tree: &FileTree,
) -> Result<(), ErofsError> {
    let meta_end = align_to_block(state.current_meta_offset);
    let data_start_block = (meta_end / EROFS_BLKSIZ as u64) as u32;
    let mut meta_cursor = state.meta_blkaddr as u64 * EROFS_BLKSIZ as u64;

    file.seek(SeekFrom::Start(meta_cursor))?;

    write_metadata_for_tree(
        file,
        state,
        &TreeNode::Directory(clone_dir_shell(&tree.root)),
        &tree.root,
        data_start_block,
        &mut 0,
        &mut meta_cursor,
    )?;

    Ok(())
}

pub(super) fn clone_dir_shell(dir: &DirectoryNode) -> DirectoryNode {
    DirectoryNode {
        metadata: InodeMetadata {
            uid: dir.metadata.uid,
            gid: dir.metadata.gid,
            mode: dir.metadata.mode,
            mtime: dir.metadata.mtime,
            mtime_nsec: dir.metadata.mtime_nsec,
        },
        xattrs: dir
            .xattrs
            .iter()
            .map(|x| Xattr {
                name: x.name.clone(),
                value: x.value.clone(),
            })
            .collect(),
        entries: BTreeMap::new(),
    }
}

fn write_metadata_for_tree(
    file: &mut (impl Write + Seek),
    state: &LayoutState,
    node: &TreeNode,
    real_dir: &DirectoryNode,
    data_start_block: u32,
    plan_idx: &mut usize,
    meta_cursor: &mut u64,
) -> Result<(), ErofsError> {
    let blksiz = EROFS_BLKSIZ as u64;
    let meta_base = state.meta_blkaddr as u64 * blksiz;
    let plan = &state.plans[*plan_idx];
    *plan_idx += 1;

    // Compute the byte offset of this inode
    let inode_offset = meta_base + plan.nid as u64 * EROFS_ISLOT_SIZE as u64;

    write_zero_padding_to(file, meta_cursor, inode_offset)?;

    // Build the 64-byte extended inode
    let mut inode = [0u8; 64];

    // i_format: bit 0 = 1 (extended), datalayout in bits 1..
    let i_format: u16 = 1 | ((plan.data_layout as u16) << 1);
    inode[0..2].copy_from_slice(&i_format.to_le_bytes());

    // i_xattr_icount
    let i_xattr_icount = compute_xattr_icount(plan.xattr_ibody_size);
    inode[2..4].copy_from_slice(&i_xattr_icount.to_le_bytes());

    // i_mode
    let mode_bits = mode_type_bits(node);
    let meta = node_metadata(node);
    let i_mode = mode_bits | meta.mode;
    inode[4..6].copy_from_slice(&i_mode.to_le_bytes());

    // i_nb = 0
    inode[6..8].copy_from_slice(&0u16.to_le_bytes());

    // i_size
    let i_size: u64 = match node {
        TreeNode::Directory(_) => {
            if let Some(ref dd) = plan.dir_data {
                dd.len() as u64
            } else {
                0
            }
        }
        _ => node_data_size(node),
    };
    inode[8..16].copy_from_slice(&i_size.to_le_bytes());

    // i_u (startblk_lo or rdev)
    let i_u: u32 = match node {
        TreeNode::CharDevice(d) | TreeNode::BlockDevice(d) => new_encode_dev(d.major, d.minor),
        TreeNode::Fifo(_) | TreeNode::Socket(_) => 0,
        _ => {
            if plan.data_block_start == EROFS_NULL_ADDR {
                EROFS_NULL_ADDR
            } else {
                data_start_block + plan.data_block_start
            }
        }
    };
    inode[16..20].copy_from_slice(&i_u.to_le_bytes());

    // i_ino (use NID as inode number)
    inode[20..24].copy_from_slice(&plan.nid.to_le_bytes());

    // i_uid
    inode[24..28].copy_from_slice(&meta.uid.to_le_bytes());

    // i_gid
    inode[28..32].copy_from_slice(&meta.gid.to_le_bytes());

    // i_mtime
    inode[32..40].copy_from_slice(&meta.mtime.to_le_bytes());

    // i_mtime_nsec
    inode[40..44].copy_from_slice(&meta.mtime_nsec.to_le_bytes());

    // i_nlink
    let nlink = node_nlink(node, &state.regular_file_link_counts);
    inode[44..48].copy_from_slice(&nlink.to_le_bytes());

    // reserved[16] already zeroed

    {
        let mut tracked = CursorTrackingWriter::new(file, meta_cursor);
        tracked.write_all(&inode)?;

        // Write xattr ibody if present
        let xattrs = node_xattrs(node);
        if plan.xattr_ibody_size > 0 {
            write_xattr_ibody(&mut tracked, xattrs)?;
        }

        // Write inline tail data
        if plan.inline_tail_size > 0 {
            match node {
                TreeNode::Directory(_) => {
                    if let Some(ref dir_data) = plan.dir_data {
                        let full_block_bytes =
                            plan.data_block_count as usize * EROFS_BLKSIZ as usize;
                        let tail = &dir_data[full_block_bytes..];
                        tracked.write_all(tail)?;
                    }
                }
                TreeNode::RegularFile(f) => {
                    let full_block_bytes = plan.data_block_count as usize * EROFS_BLKSIZ as usize;
                    let tail_len = f.data.len() - full_block_bytes;
                    f.data
                        .write_range(full_block_bytes, tail_len, &mut tracked)?;
                }
                TreeNode::Symlink(s) => {
                    let full_block_bytes = plan.data_block_count as usize * EROFS_BLKSIZ as usize;
                    let tail = &s.target[full_block_bytes..];
                    tracked.write_all(tail)?;
                }
                _ => {}
            }
        }
    }

    // Recurse into children for directories
    if let TreeNode::Directory(_) = node {
        for child in real_dir.entries.values() {
            match child {
                TreeNode::Directory(child_dir) => {
                    write_metadata_for_tree(
                        file,
                        state,
                        child,
                        child_dir,
                        data_start_block,
                        plan_idx,
                        meta_cursor,
                    )?;
                }
                _ => {
                    write_metadata_for_leaf(
                        file,
                        state,
                        child,
                        data_start_block,
                        plan_idx,
                        meta_cursor,
                    )?;
                }
            }
        }
    }

    Ok(())
}

fn write_metadata_for_leaf(
    file: &mut (impl Write + Seek),
    state: &LayoutState,
    node: &TreeNode,
    data_start_block: u32,
    plan_idx: &mut usize,
    meta_cursor: &mut u64,
) -> Result<(), ErofsError> {
    let blksiz = EROFS_BLKSIZ as u64;
    let meta_base = state.meta_blkaddr as u64 * blksiz;
    let (plan, first_regular_visit) = match node {
        TreeNode::RegularFile(file) => {
            let child_plan_idx = *state
                .regular_file_plans
                .get(&file.id)
                .expect("regular file plan missing");
            let first_visit = child_plan_idx == *plan_idx;
            if first_visit {
                *plan_idx += 1;
            }
            (&state.plans[child_plan_idx], first_visit)
        }
        _ => {
            let plan = &state.plans[*plan_idx];
            *plan_idx += 1;
            (plan, true)
        }
    };

    if !first_regular_visit {
        return Ok(());
    }

    let inode_offset = meta_base + plan.nid as u64 * EROFS_ISLOT_SIZE as u64;
    write_zero_padding_to(file, meta_cursor, inode_offset)?;

    let mut inode = [0u8; 64];

    let i_format: u16 = 1 | ((plan.data_layout as u16) << 1);
    inode[0..2].copy_from_slice(&i_format.to_le_bytes());

    let i_xattr_icount = compute_xattr_icount(plan.xattr_ibody_size);
    inode[2..4].copy_from_slice(&i_xattr_icount.to_le_bytes());

    let mode_bits = mode_type_bits(node);
    let meta = node_metadata(node);
    let i_mode = mode_bits | meta.mode;
    inode[4..6].copy_from_slice(&i_mode.to_le_bytes());

    inode[6..8].copy_from_slice(&0u16.to_le_bytes());

    let i_size = node_data_size(node);
    inode[8..16].copy_from_slice(&i_size.to_le_bytes());

    let i_u: u32 = match node {
        TreeNode::CharDevice(d) | TreeNode::BlockDevice(d) => new_encode_dev(d.major, d.minor),
        TreeNode::Fifo(_) | TreeNode::Socket(_) => 0,
        _ => {
            if plan.data_block_start == EROFS_NULL_ADDR {
                EROFS_NULL_ADDR
            } else {
                data_start_block + plan.data_block_start
            }
        }
    };
    inode[16..20].copy_from_slice(&i_u.to_le_bytes());

    inode[20..24].copy_from_slice(&plan.nid.to_le_bytes());
    inode[24..28].copy_from_slice(&meta.uid.to_le_bytes());
    inode[28..32].copy_from_slice(&meta.gid.to_le_bytes());
    inode[32..40].copy_from_slice(&meta.mtime.to_le_bytes());
    inode[40..44].copy_from_slice(&meta.mtime_nsec.to_le_bytes());

    let nlink = node_nlink(node, &state.regular_file_link_counts);
    inode[44..48].copy_from_slice(&nlink.to_le_bytes());

    {
        let mut tracked = CursorTrackingWriter::new(file, meta_cursor);
        tracked.write_all(&inode)?;

        let xattrs = node_xattrs(node);
        if plan.xattr_ibody_size > 0 {
            write_xattr_ibody(&mut tracked, xattrs)?;
        }

        if plan.inline_tail_size > 0 {
            match node {
                TreeNode::RegularFile(f) => {
                    let full_block_bytes = plan.data_block_count as usize * EROFS_BLKSIZ as usize;
                    let tail_len = f.data.len() - full_block_bytes;
                    f.data
                        .write_range(full_block_bytes, tail_len, &mut tracked)?;
                }
                TreeNode::Symlink(s) => {
                    let full_block_bytes = plan.data_block_count as usize * EROFS_BLKSIZ as usize;
                    let tail = &s.target[full_block_bytes..];
                    tracked.write_all(tail)?;
                }
                _ => {}
            }
        }
    }

    Ok(())
}

pub(super) fn write_xattr_ibody(file: &mut impl Write, xattrs: &[Xattr]) -> Result<(), ErofsError> {
    // Write the ibody header (12 bytes)
    // h_name_filter: u32 = 0, h_shared_count: u8 = 0, reserved: [u8; 7] = 0
    let header = [0u8; EROFS_XATTR_IBODY_HEADER_SIZE as usize];
    file.write_all(&header)?;

    for xattr in xattrs {
        let (prefix_idx, suffix) =
            xattr_prefix_index(&xattr.name).ok_or(ErofsError::UnsupportedXattrPrefix)?;

        // erofs_xattr_entry: 4 bytes
        let mut entry = [0u8; 4];
        entry[0] = suffix.len() as u8; // e_name_len
        entry[1] = prefix_idx; // e_name_index
        entry[2..4].copy_from_slice(&(xattr.value.len() as u16).to_le_bytes()); // e_value_size
        file.write_all(&entry)?;

        // Write suffix name
        file.write_all(suffix)?;

        // Write value
        file.write_all(&xattr.value)?;

        // Pad to 4-byte alignment
        let entry_size = 4 + suffix.len() + xattr.value.len();
        let aligned = erofs_xattr_align(entry_size);
        let pad = aligned - entry_size;
        if pad > 0 {
            file.write_all(&ZEROS[..pad])?;
        }
    }

    Ok(())
}

fn write_superblock(file: &mut (impl Write + Seek), state: &LayoutState) -> Result<(), ErofsError> {
    let blksiz = EROFS_BLKSIZ as u64;

    // First, ensure the file has at least the first block zeroed (boot sector area)
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&[0u8; EROFS_SUPER_OFFSET as usize])?;

    // Compute total blocks
    let meta_end = align_to_block(state.current_meta_offset);
    let data_start_block = (meta_end / blksiz) as u32;
    let total_blocks = data_start_block + state.current_data_block;

    // Build superblock (128 bytes)
    let mut sb = [0u8; EROFS_SUPERBLOCK_SIZE as usize];

    // magic
    sb[0x00..0x04].copy_from_slice(&EROFS_SUPER_MAGIC.to_le_bytes());

    // checksum (zeroed for now, computed below)
    sb[0x04..0x08].copy_from_slice(&0u32.to_le_bytes());

    // feature_compat (SB_CHKSUM)
    sb[0x08..0x0C].copy_from_slice(&EROFS_FEATURE_COMPAT_SB_CHKSUM.to_le_bytes());

    // blkszbits
    sb[0x0C] = EROFS_BLKSIZ_BITS;

    // sb_extslots
    sb[0x0D] = 0;

    // rootnid_2b (16-bit field — image is unmountable if NID exceeds u16::MAX).
    if state.root_nid > u16::MAX as u32 {
        return Err(ErofsError::NidOverflow);
    }
    sb[0x0E..0x10].copy_from_slice(&(state.root_nid as u16).to_le_bytes());

    // inos
    sb[0x10..0x18].copy_from_slice(&state.inode_count.to_le_bytes());

    // epoch (base timestamp = 0)
    sb[0x18..0x20].copy_from_slice(&0u64.to_le_bytes());

    // fixed_nsec
    sb[0x20..0x24].copy_from_slice(&0u32.to_le_bytes());

    // blocks_lo
    sb[0x24..0x28].copy_from_slice(&total_blocks.to_le_bytes());

    // meta_blkaddr
    sb[0x28..0x2C].copy_from_slice(&state.meta_blkaddr.to_le_bytes());

    // xattr_blkaddr
    sb[0x2C..0x30].copy_from_slice(&0u32.to_le_bytes());

    // uuid (16 bytes) - use zeros for now (deterministic)
    // sb[0x30..0x40] already zeroed

    // volume_name (16 bytes) - zeros
    // sb[0x40..0x50] already zeroed

    // feature_incompat
    sb[0x50..0x54].copy_from_slice(&0u32.to_le_bytes());

    // dirblkbits at offset 0x5A
    sb[0x5A] = 0;

    // Now compute CRC32C over the entire superblock block with checksum field zeroed
    // The superblock block starts at offset 0 and is EROFS_BLKSIZ bytes.
    // But the superblock starts at EROFS_SUPER_OFFSET within that block.
    // CRC is over bytes from EROFS_SUPER_OFFSET to end of the superblock block.

    // Build the full block
    let mut block = vec![0u8; EROFS_BLKSIZ as usize];
    block
        [EROFS_SUPER_OFFSET as usize..EROFS_SUPER_OFFSET as usize + EROFS_SUPERBLOCK_SIZE as usize]
        .copy_from_slice(&sb);

    // CRC32C of the range [EROFS_SUPER_OFFSET .. EROFS_BLKSIZ].
    // EROFS uses raw CRC32C (seed ~0, no final XOR) — call crc32c_raw directly.
    let crc_data = &block[EROFS_SUPER_OFFSET as usize..EROFS_BLKSIZ as usize];
    let checksum = crc32c::crc32c_raw(0xFFFF_FFFF, crc_data);

    // Set checksum in the superblock
    sb[0x04..0x08].copy_from_slice(&checksum.to_le_bytes());

    // Write the superblock at EROFS_SUPER_OFFSET
    file.seek(SeekFrom::Start(EROFS_SUPER_OFFSET))?;
    file.write_all(&sb)?;

    // Pad the rest of block 0 with zeros (up to EROFS_BLKSIZ)
    let remaining = EROFS_BLKSIZ as u64 - EROFS_SUPER_OFFSET - EROFS_SUPERBLOCK_SIZE as u64;
    file.write_all(&vec![0u8; remaining as usize])?;

    Ok(())
}

fn align_to_block(offset: u64) -> u64 {
    let blksiz = EROFS_BLKSIZ as u64;
    offset.div_ceil(blksiz) * blksiz
}
