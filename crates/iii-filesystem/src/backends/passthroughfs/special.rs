//! Special operations: fsync, fsyncdir, statfs.
//!
//! ## fsync
//!
//! Uses `fdatasync` on Linux when `datasync` is true (metadata not needed),
//! plain `fsync` otherwise. On macOS, always uses `fsync` since `fdatasync`
//! is not available.

use std::{io, os::fd::AsRawFd};

use super::PassthroughFs;
use crate::{
    backends::shared::{init_binary, platform},
    statvfs64, Context,
};

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Synchronize file contents.
pub(crate) fn do_fsync(
    fs: &PassthroughFs,
    _ctx: Context,
    ino: u64,
    datasync: bool,
    handle: u64,
) -> io::Result<()> {
    if init_binary::has_init() && handle == init_binary::INIT_HANDLE && ino == init_binary::INIT_INODE {
        return Ok(());
    }

    let handles = fs.handles.read().unwrap();
    let data = handles.get(&handle).ok_or_else(platform::ebadf)?;
    // Write lock: fsync/fdatasync modify fd state.
    #[allow(clippy::readonly_write_lock)]
    let f = data.file.write().unwrap();
    let fd = f.as_raw_fd();

    #[cfg(target_os = "linux")]
    let ret = if datasync {
        unsafe { libc::fdatasync(fd) }
    } else {
        unsafe { libc::fsync(fd) }
    };

    #[cfg(target_os = "macos")]
    let ret = {
        let _ = datasync;
        unsafe { libc::fsync(fd) }
    };

    if ret < 0 {
        return Err(platform::linux_error(io::Error::last_os_error()));
    }
    Ok(())
}

/// Synchronize directory contents.
pub(crate) fn do_fsyncdir(
    fs: &PassthroughFs,
    ctx: Context,
    ino: u64,
    datasync: bool,
    handle: u64,
) -> io::Result<()> {
    do_fsync(fs, ctx, ino, datasync, handle)
}

/// Get filesystem statistics.
pub(crate) fn do_statfs(fs: &PassthroughFs, _ctx: Context, ino: u64) -> io::Result<statvfs64> {
    // Keep InodeFd guard alive so the fd isn't closed before fstatvfs uses it.
    let inode_fd;
    let fd = if (init_binary::has_init() && ino == init_binary::INIT_INODE) || ino == 1 {
        fs.root_fd.as_raw_fd()
    } else {
        inode_fd = super::inode::get_inode_fd(fs, ino)?;
        inode_fd.raw()
    };

    #[cfg(target_os = "linux")]
    {
        let mut st = unsafe { std::mem::zeroed::<statvfs64>() };
        let ret = unsafe { libc::fstatvfs64(fd, &mut st) };
        if ret < 0 {
            return Err(platform::linux_error(io::Error::last_os_error()));
        }
        Ok(st)
    }

    #[cfg(target_os = "macos")]
    {
        let mut st = unsafe { std::mem::zeroed::<statvfs64>() };
        let ret = unsafe { libc::fstatvfs(fd, &mut st) };
        if ret < 0 {
            return Err(platform::linux_error(io::Error::last_os_error()));
        }
        Ok(st)
    }
}
