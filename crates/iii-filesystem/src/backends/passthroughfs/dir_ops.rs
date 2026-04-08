//! Directory operations: opendir, readdir, readdirplus, releasedir.
//!
//! ## Memory Strategy: Tracked Leak with Destroy Reclamation
//!
//! `DynFileSystem::readdir` returns `Vec<DirEntry<'static>>` where names are `&'static [u8]`.
//! Since the trait requires `'static` lifetimes, we cannot return borrowed data. Instead, we
//! collect all entry names into a single contiguous `Vec<u8>`, leak it once per readdir call,
//! and slice `&'static [u8]` references from it. Leaked pointers are tracked in
//! `PassthroughFs::leaked_readdir_bufs` and reclaimed in `destroy()` when the filesystem
//! shuts down, preventing unbounded memory growth for long-lived VMs.

use std::{
    io,
    os::fd::{AsRawFd, FromRawFd},
    sync::{Arc, RwLock, atomic::Ordering},
};

use super::{LeakedBufPtr, PassthroughFs, inode};
use crate::{
    Context, DirEntry, Entry, OpenOptions,
    backends::shared::{handle_table::HandleData, init_binary, platform},
};

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Open a directory and return a handle.
pub(crate) fn do_opendir(
    fs: &PassthroughFs,
    _ctx: Context,
    ino: u64,
    _flags: u32,
) -> io::Result<(Option<u64>, OpenOptions)> {
    let fd = inode::open_inode_fd(fs, ino, libc::O_RDONLY | libc::O_DIRECTORY)?;
    let file = unsafe { std::fs::File::from_raw_fd(fd) };

    let handle = fs.next_handle.fetch_add(1, Ordering::Relaxed);
    let data = Arc::new(HandleData {
        file: RwLock::new(file),
    });

    fs.handles.insert(handle, data);
    Ok((Some(handle), fs.cache_dir_options()))
}

/// Read directory entries.
///
/// On Linux, uses raw `getdents64` syscall with buffer sized by the FUSE
/// `size` parameter (clamped to 1KB--64KB).
///
/// Names are collected into a single contiguous buffer that is leaked once
/// per call and tracked for reclamation in destroy().
pub(crate) fn do_readdir(
    fs: &PassthroughFs,
    _ctx: Context,
    ino: u64,
    handle: u64,
    size: u32,
    offset: u64,
) -> io::Result<Vec<DirEntry<'static>>> {
    let data = fs.handles.get(&handle).ok_or_else(platform::ebadf)?;
    // Write lock: lseek in read_dir_entries modifies fd seek position.
    #[allow(clippy::readonly_write_lock)]
    let f = data.file.write().unwrap();
    let fd = f.as_raw_fd();

    let mut entries = read_dir_entries(fs, fd, offset, size)?;

    // Inject init.krun into root directory listing.
    if ino == 1 {
        inject_init_entry(&mut entries);
    }

    Ok(entries)
}

/// Read directory entries with attributes (readdirplus).
///
/// Each entry gets a full `do_lookup` to obtain the Entry with stat data.
/// For init.krun, returns the synthetic init entry.
pub(crate) fn do_readdirplus(
    fs: &PassthroughFs,
    ctx: Context,
    ino: u64,
    handle: u64,
    size: u32,
    offset: u64,
) -> io::Result<Vec<(DirEntry<'static>, Entry)>> {
    let dir_entries = do_readdir(fs, ctx, ino, handle, size, offset)?;
    let mut result = Vec::with_capacity(dir_entries.len());

    for de in dir_entries {
        let name_bytes = de.name;
        // Skip . and .. -- the kernel handles these itself.
        if name_bytes == b"." || name_bytes == b".." {
            continue;
        }

        // For init.krun, return the synthetic entry (only when init is embedded).
        if init_binary::has_init() && name_bytes == init_binary::INIT_FILENAME {
            let entry = init_binary::init_entry(fs.cfg.entry_timeout, fs.cfg.attr_timeout);
            result.push((de, entry));
            continue;
        }

        // Look up the entry to get full attributes.
        let name_cstr = match std::ffi::CString::new(name_bytes.to_vec()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        match inode::do_lookup(fs, ino, &name_cstr) {
            Ok(entry) => {
                // Correct d_type from the lookup's stat (free: no extra syscalls).
                let mut de = de;
                let file_type = platform::mode_file_type(entry.attr.st_mode);
                de.type_ = platform::dirent_type_from_mode(file_type);
                result.push((de, entry));
            }
            Err(_) => continue,
        }
    }

    Ok(result)
}

/// Release an open directory handle.
pub(crate) fn do_releasedir(
    fs: &PassthroughFs,
    _ctx: Context,
    _ino: u64,
    _flags: u32,
    handle: u64,
) -> io::Result<()> {
    fs.handles.remove(&handle);
    Ok(())
}

//--------------------------------------------------------------------------------------------------
// Functions: Helpers
//--------------------------------------------------------------------------------------------------

/// Inject the init.krun entry into a directory listing if not already present.
/// Only injects when the init binary is embedded; otherwise the real file on disk
/// appears naturally in readdir results.
fn inject_init_entry(entries: &mut Vec<DirEntry<'static>>) {
    if !init_binary::has_init() {
        return;
    }

    let already_present = entries.iter().any(|e| e.name == init_binary::INIT_FILENAME);

    if !already_present {
        let next_offset = entries.last().map(|e| e.offset + 1).unwrap_or(1);
        let name: &'static [u8] = init_binary::INIT_FILENAME;
        entries.push(DirEntry {
            ino: init_binary::INIT_INODE,
            offset: next_offset,
            type_: platform::DIRENT_REG,
            name,
        });
    }
}

/// Read directory entries using `getdents64` with FUSE size as buffer hint.
///
/// Names are collected into a single contiguous buffer, leaked once, tracked
/// in `PassthroughFs::leaked_readdir_bufs`, and sliced into `&'static [u8]` references.
#[cfg(target_os = "linux")]
fn read_dir_entries(
    fs: &PassthroughFs,
    fd: i32,
    offset: u64,
    size: u32,
) -> io::Result<Vec<DirEntry<'static>>> {
    // Seek to the requested offset.
    if offset > 0 {
        let ret = unsafe { libc::lseek64(fd, offset as i64, libc::SEEK_SET) };
        if ret < 0 {
            return Err(platform::linux_error(io::Error::last_os_error()));
        }
    }

    // Use FUSE size as a hint for the getdents buffer.
    let buf_size = (size as usize).clamp(1024, 65536);
    let mut buf = vec![0u8; buf_size];

    // Collect raw entry data and names into a contiguous buffer.
    let mut raw_entries: Vec<(u64, u64, u8, usize, usize)> = Vec::new();
    let mut names_buf: Vec<u8> = Vec::new();

    loop {
        let nread = unsafe { libc::syscall(libc::SYS_getdents64, fd, buf.as_mut_ptr(), buf.len()) };

        if nread < 0 {
            return Err(platform::linux_error(io::Error::last_os_error()));
        }
        if nread == 0 {
            break;
        }

        let mut pos = 0usize;
        while pos < nread as usize {
            // SAFETY: getdents64 returns properly aligned linux_dirent64 structs.
            let d_ino = u64::from_ne_bytes(buf[pos..pos + 8].try_into().unwrap());
            let d_off = u64::from_ne_bytes(buf[pos + 8..pos + 16].try_into().unwrap());
            let d_reclen = u16::from_ne_bytes(buf[pos + 16..pos + 18].try_into().unwrap());
            let d_type = buf[pos + 18];

            // Name starts at offset 19, null-terminated.
            let name_start = pos + 19;
            let name_end = pos + d_reclen as usize;
            let name_slice = &buf[name_start..name_end];
            let name_len = name_slice
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(name_slice.len());
            let name_bytes = &name_slice[..name_len];

            let name_offset = names_buf.len();
            names_buf.extend_from_slice(name_bytes);

            raw_entries.push((d_ino, d_off, d_type, name_offset, name_len));

            pos += d_reclen as usize;
        }
    }

    if raw_entries.is_empty() {
        return Ok(Vec::new());
    }

    // Leak one contiguous buffer for all names and track for reclamation in destroy().
    let boxed = names_buf.into_boxed_slice();
    let len = boxed.len();
    let leaked: &'static mut [u8] = Box::leak(boxed);
    let ptr = leaked.as_mut_ptr();
    fs.leaked_readdir_bufs
        .lock()
        .unwrap()
        .push((LeakedBufPtr(ptr), len));

    let entries = raw_entries
        .into_iter()
        .map(|(ino, off, typ, start, len)| DirEntry {
            ino,
            offset: off,
            type_: typ as u32,
            name: &leaked[start..start + len],
        })
        .collect();

    Ok(entries)
}

/// Read directory entries from a file descriptor using readdir on macOS.
///
/// Names are collected into a single contiguous buffer, leaked once, and tracked
/// in `PassthroughFs::leaked_readdir_bufs` for reclamation in `destroy()`.
///
/// macOS `seekdir`/`telldir` cookies are NOT portable across different
/// `fdopendir` sessions: after one session reads to EOF the shared fd position
/// is at the end, and a new `fdopendir(dup(fd))` starts there. `seekdir` with
/// a cookie from the old session silently fails.
///
/// To work around this, we:
///  1. `lseek(fd, 0)` to rewind the underlying fd before dup/fdopendir.
///  2. Use a sequential entry index (1-based) as `d_off` instead of `telldir`.
///  3. When `offset > 0`, skip that many entries from the start.
#[cfg(target_os = "macos")]
fn read_dir_entries(
    fs: &PassthroughFs,
    fd: i32,
    offset: u64,
    _size: u32,
) -> io::Result<Vec<DirEntry<'static>>> {
    // Rewind the directory fd so dup inherits position 0.
    let ret = unsafe { libc::lseek(fd, 0, libc::SEEK_SET) };
    if ret < 0 {
        return Err(platform::linux_error(io::Error::last_os_error()));
    }

    let dup_fd = unsafe { libc::dup(fd) };
    if dup_fd < 0 {
        return Err(platform::linux_error(io::Error::last_os_error()));
    }

    let dirp = unsafe { libc::fdopendir(dup_fd) };
    if dirp.is_null() {
        unsafe { libc::close(dup_fd) };
        return Err(platform::linux_error(io::Error::last_os_error()));
    }

    let mut raw_entries: Vec<(u64, u64, u32, usize, usize)> = Vec::new();
    let mut names_buf: Vec<u8> = Vec::new();
    let mut entry_index: u64 = 0;

    loop {
        unsafe { *libc::__error() = 0 };

        let ent = unsafe { libc::readdir(dirp) };
        if ent.is_null() {
            let errno = unsafe { *libc::__error() };
            if errno != 0 {
                unsafe { libc::closedir(dirp) };
                return Err(platform::linux_error(io::Error::from_raw_os_error(errno)));
            }
            break; // EOF
        }

        entry_index += 1;

        // Skip entries before the requested offset.
        if entry_index <= offset {
            continue;
        }

        let d = unsafe { &*ent };
        let name_len = d.d_namlen as usize;

        let name_bytes =
            unsafe { std::slice::from_raw_parts(d.d_name.as_ptr() as *const u8, name_len) };

        let name_offset = names_buf.len();
        names_buf.extend_from_slice(name_bytes);

        raw_entries.push((
            d.d_ino,
            entry_index, // sequential 1-based index as d_off
            d.d_type as u32,
            name_offset,
            name_len,
        ));
    }

    unsafe { libc::closedir(dirp) };

    if raw_entries.is_empty() {
        return Ok(Vec::new());
    }

    // Leak one contiguous buffer for all names and track for reclamation in destroy().
    let boxed = names_buf.into_boxed_slice();
    let len = boxed.len();
    let leaked: &'static mut [u8] = Box::leak(boxed);
    let ptr = leaked.as_mut_ptr();
    fs.leaked_readdir_bufs
        .lock()
        .unwrap()
        .push((LeakedBufPtr(ptr), len));

    let entries = raw_entries
        .into_iter()
        .map(|(ino, off, typ, start, len)| DirEntry {
            ino,
            offset: off,
            type_: typ,
            name: &leaked[start..start + len],
        })
        .collect();

    Ok(entries)
}
