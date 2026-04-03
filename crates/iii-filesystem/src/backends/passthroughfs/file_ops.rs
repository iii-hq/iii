//! File I/O operations: open, read, write, flush, release.
//!
//! ## I/O Path
//!
//! Read and write use the `ZeroCopyWriter`/`ZeroCopyReader` traits from msb_krun, which
//! bridge FUSE transport buffers directly to file I/O via `preadv64`/`pwritev64`. These take
//! an explicit offset and do NOT modify the fd seek position, so `HandleData.file` only needs
//! a `RwLock` read lock for I/O -- the write lock is reserved for `lseek`, `fsync`, `ftruncate`.
//!
//! ## Writeback Cache
//!
//! When writeback caching is negotiated, the kernel may read from write-only files for cache
//! coherency. `do_open` adjusts `O_WRONLY` -> `O_RDWR` and strips `O_APPEND` (which races with
//! the kernel's cached view of the file).

use std::{
    io,
    os::fd::{AsRawFd, FromRawFd},
    sync::{Arc, RwLock, atomic::Ordering},
};

use super::{PassthroughFs, inode};
use crate::{
    Context, OpenOptions, ZeroCopyReader, ZeroCopyWriter,
    backends::shared::{handle_table::HandleData, init_binary, platform},
};

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Open a file and return a handle.
///
/// Init binary (inode 2) returns the reserved handle 0 without opening any fd.
/// For regular files, opens via `open_inode_fd` and allocates a new handle.
pub(crate) fn do_open(
    fs: &PassthroughFs,
    _ctx: Context,
    ino: u64,
    _kill_priv: bool,
    flags: u32,
) -> io::Result<(Option<u64>, OpenOptions)> {
    if init_binary::has_init() && ino == init_binary::INIT_INODE {
        return Ok((Some(init_binary::INIT_HANDLE), OpenOptions::empty()));
    }

    let mut open_flags = inode::translate_open_flags(flags as i32);

    // Writeback cache: kernel may issue reads on O_WRONLY fds for cache coherency,
    // so widen to O_RDWR. Strip O_APPEND because it races with the kernel's cached
    // write position.
    if fs.writeback.load(Ordering::Relaxed) {
        if open_flags & libc::O_WRONLY != 0 {
            open_flags = (open_flags & !libc::O_WRONLY) | libc::O_RDWR;
        }
        open_flags &= !libc::O_APPEND;
    }

    // open_inode_fd adds O_CLOEXEC itself and rejects real host symlinks.
    let fd = inode::open_inode_fd(fs, ino, open_flags)?;
    let file = unsafe { std::fs::File::from_raw_fd(fd) };

    let handle = fs.next_handle.fetch_add(1, Ordering::Relaxed);
    let data = Arc::new(HandleData {
        file: RwLock::new(file),
    });

    fs.handles.write().unwrap().insert(handle, data);
    Ok((Some(handle), fs.cache_open_options()))
}

/// Read data from a file.
///
/// Init binary reads are served from the pre-created init file via zero-copy.
/// Regular file reads use the read lock on HandleData (preadv does not modify seek).
pub(crate) fn do_read(
    fs: &PassthroughFs,
    _ctx: Context,
    ino: u64,
    handle: u64,
    w: &mut dyn ZeroCopyWriter,
    size: u32,
    offset: u64,
) -> io::Result<usize> {
    // Virtual init.krun binary.
    if init_binary::has_init()
        && handle == init_binary::INIT_HANDLE
        && ino == init_binary::INIT_INODE
    {
        return init_binary::read_init(w, &fs.init_file, size, offset);
    }

    let handles = fs.handles.read().unwrap();
    let data = handles.get(&handle).ok_or_else(platform::ebadf)?;
    let f = data.file.read().unwrap();
    w.write_from(&f, size as usize, offset)
}

/// Write data to a file.
///
/// Init binary is read-only; writes return EPERM.
/// Regular file writes use the read lock on HandleData (pwritev does not modify seek).
///
/// When `kill_priv` is true (HANDLE_KILLPRIV_V2), clears SUID/SGID bits via fchmod
/// after a successful write -- no xattr override (per D-02).
#[allow(clippy::too_many_arguments)]
pub(crate) fn do_write(
    fs: &PassthroughFs,
    _ctx: Context,
    ino: u64,
    handle: u64,
    r: &mut dyn ZeroCopyReader,
    size: u32,
    offset: u64,
    kill_priv: bool,
) -> io::Result<usize> {
    if init_binary::has_init()
        && handle == init_binary::INIT_HANDLE
        && ino == init_binary::INIT_INODE
    {
        return Err(platform::eperm());
    }

    let handles = fs.handles.read().unwrap();
    let data = handles.get(&handle).ok_or_else(platform::ebadf)?;
    let f = data.file.read().unwrap();
    let written = r.read_to(&f, size as usize, offset)?;

    // Clear SUID/SGID after write when writeback cache is active and kill_priv requested.
    if kill_priv && fs.writeback.load(Ordering::Relaxed) {
        let fd = f.as_raw_fd();
        let st = platform::fstat(fd)?;
        let mode = platform::mode_u32(st.st_mode);
        if mode & (platform::MODE_SETUID | platform::MODE_SETGID) != 0 {
            let new_mode = mode & !(platform::MODE_SETUID | platform::MODE_SETGID);
            // Use fchmod directly (no xattr per D-02).
            let ret = unsafe { libc::fchmod(fd, new_mode as libc::mode_t) };
            if ret < 0 {
                // Best-effort: don't fail the write for kill_priv failure.
                let _ = platform::linux_error(io::Error::last_os_error());
            }
        }
    }

    Ok(written)
}

/// Flush pending data for a file handle.
///
/// Emulates POSIX close semantics by duplicating and closing the fd.
/// Called on every guest `close()` (may fire multiple times if the fd was `dup`'d).
pub(crate) fn do_flush(fs: &PassthroughFs, _ctx: Context, ino: u64, handle: u64) -> io::Result<()> {
    if init_binary::has_init()
        && handle == init_binary::INIT_HANDLE
        && ino == init_binary::INIT_INODE
    {
        return Ok(());
    }

    let handles = fs.handles.read().unwrap();
    let data = handles.get(&handle).ok_or_else(platform::ebadf)?;
    let f = data.file.read().unwrap();

    let newfd = unsafe { libc::dup(f.as_raw_fd()) };
    if newfd < 0 {
        return Err(platform::linux_error(io::Error::last_os_error()));
    }
    let ret = unsafe { libc::close(newfd) };
    if ret < 0 {
        return Err(platform::linux_error(io::Error::last_os_error()));
    }
    Ok(())
}

/// Release an open file handle.
///
/// Removes the handle from the table. The `HandleData` drop closes the fd.
pub(crate) fn do_release(
    fs: &PassthroughFs,
    _ctx: Context,
    ino: u64,
    handle: u64,
) -> io::Result<()> {
    if init_binary::has_init()
        && handle == init_binary::INIT_HANDLE
        && ino == init_binary::INIT_INODE
    {
        return Ok(());
    }

    fs.handles.write().unwrap().remove(&handle);
    Ok(())
}
