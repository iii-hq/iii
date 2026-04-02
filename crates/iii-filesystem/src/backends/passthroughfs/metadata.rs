//! Metadata operations: getattr, setattr, access.
//!
//! ## No Stat Virtualization (D-02)
//!
//! Unlike microsandbox, iii does not use xattr-based stat overrides. All stat results
//! return raw host data directly. UID/GID changes use real `fchown`, mode changes use
//! real `fchmod`. If `fchown` fails with EPERM (host process lacks CAP_CHOWN), the
//! error is silently swallowed per RESEARCH.md guidance: "no-op that returns success".

use std::{io, os::fd::AsRawFd, time::Duration};

use super::{inode, PassthroughFs};
use crate::{
    backends::shared::{init_binary, platform},
    stat64, Context, SetattrValid,
};

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Get attributes for an inode.
///
/// Returns raw stat data (no xattr patching per D-02).
pub(crate) fn do_getattr(
    fs: &PassthroughFs,
    _ctx: Context,
    ino: u64,
    handle: Option<u64>,
) -> io::Result<(stat64, Duration)> {
    let st = inode::stat_inode(fs, ino, handle)?;
    Ok((st, fs.cfg.attr_timeout))
}

/// Set attributes on an inode.
///
/// Processes each SetattrValid flag: SIZE via ftruncate, MODE via fchmod,
/// UID/GID via fchown (silently succeeds on EPERM), timestamps via futimens.
/// No xattr/stat_override (per D-02).
pub(crate) fn do_setattr(
    fs: &PassthroughFs,
    _ctx: Context,
    ino: u64,
    attr: stat64,
    handle: Option<u64>,
    valid: SetattrValid,
) -> io::Result<(stat64, Duration)> {
    if ino == init_binary::INIT_INODE {
        return Err(platform::eperm());
    }

    // Open with O_RDWR when truncation is needed, O_RDONLY otherwise.
    // ftruncate(2) requires write permission on the fd.
    let open_flags = if valid.contains(SetattrValid::SIZE) {
        libc::O_RDWR
    } else {
        libc::O_RDONLY
    };

    // Use handle fd if available, otherwise open from inode.
    let fd = if let Some(h) = handle {
        let handles = fs.handles.read().unwrap();
        if let Some(hdata) = handles.get(&h) {
            hdata.file.read().unwrap().as_raw_fd()
        } else {
            inode::open_inode_fd(fs, ino, open_flags)?
        }
    } else {
        inode::open_inode_fd(fs, ino, open_flags)?
    };

    // Track whether we own the fd (and need to close it).
    let owns_fd = handle.is_none()
        || fs
            .handles
            .read()
            .unwrap()
            .get(&handle.unwrap_or(u64::MAX))
            .is_none();

    // Handle size changes via ftruncate.
    if valid.contains(SetattrValid::SIZE) {
        #[cfg(target_os = "linux")]
        let ret = unsafe { libc::ftruncate64(fd, attr.st_size) };
        #[cfg(target_os = "macos")]
        let ret = unsafe { libc::ftruncate(fd, attr.st_size) };

        if ret < 0 {
            if owns_fd {
                unsafe { libc::close(fd) };
            }
            return Err(platform::linux_error(io::Error::last_os_error()));
        }
    }

    // Handle mode changes via fchmod (no xattr per D-02).
    if valid.contains(SetattrValid::MODE) {
        let new_mode = platform::mode_u32(attr.st_mode) & !platform::MODE_TYPE_MASK;
        let ret = unsafe { libc::fchmod(fd, new_mode as libc::mode_t) };
        if ret < 0 {
            if owns_fd {
                unsafe { libc::close(fd) };
            }
            return Err(platform::linux_error(io::Error::last_os_error()));
        }
    }

    // Handle UID/GID changes via fchown.
    // If fchown fails with EPERM, silently succeed (host lacks CAP_CHOWN).
    if valid.intersects(SetattrValid::UID | SetattrValid::GID) {
        let uid = if valid.contains(SetattrValid::UID) {
            attr.st_uid
        } else {
            u32::MAX // -1 = no change
        };
        let gid = if valid.contains(SetattrValid::GID) {
            attr.st_gid
        } else {
            u32::MAX // -1 = no change
        };
        let ret = unsafe { libc::fchown(fd, uid, gid) };
        if ret < 0 {
            let err = io::Error::last_os_error();
            // Silently succeed on EPERM (per RESEARCH.md: "no-op that returns success").
            if err.raw_os_error() != Some(libc::EPERM) {
                if owns_fd {
                    unsafe { libc::close(fd) };
                }
                return Err(platform::linux_error(err));
            }
        }
    }

    // Handle timestamp changes via futimens.
    if valid.intersects(SetattrValid::ATIME | SetattrValid::MTIME) {
        let times = platform::build_timespecs(attr, valid);
        let ret = unsafe { libc::futimens(fd, times.as_ptr()) };
        if ret < 0 {
            if owns_fd {
                unsafe { libc::close(fd) };
            }
            return Err(platform::linux_error(io::Error::last_os_error()));
        }
    }

    if owns_fd {
        unsafe { libc::close(fd) };
    }

    // Return updated attributes.
    let st = inode::stat_inode(fs, ino, None)?;
    Ok((st, fs.cfg.attr_timeout))
}

/// Check file access permissions.
///
/// For init.krun, checks against 0o755 (r-x for all, no write).
/// For other inodes, uses `stat_inode` to check real host permissions.
pub(crate) fn do_access(fs: &PassthroughFs, ctx: Context, ino: u64, mask: u32) -> io::Result<()> {
    if ino == init_binary::INIT_INODE {
        // init.krun is mode 0o755: readable and executable by all, not writable.
        if mask == platform::ACCESS_F_OK {
            return Ok(());
        }
        if mask & platform::ACCESS_W_OK != 0 {
            return Err(platform::eacces());
        }
        return Ok(());
    }

    let st = inode::stat_inode(fs, ino, None)?;

    // F_OK: just check existence.
    if mask == platform::ACCESS_F_OK {
        return Ok(());
    }

    let st_mode = platform::mode_u32(st.st_mode);

    // Root (uid 0) bypasses read/write checks.
    if ctx.uid == 0 {
        if mask & platform::ACCESS_X_OK != 0 && st_mode & 0o111 == 0 {
            return Err(platform::eacces());
        }
        return Ok(());
    }

    let bits = if st.st_uid == ctx.uid {
        (st_mode >> 6) & 0o7
    } else if st.st_gid == ctx.gid {
        (st_mode >> 3) & 0o7
    } else {
        st_mode & 0o7
    };

    if mask & platform::ACCESS_R_OK != 0 && bits & 0o4 == 0 {
        return Err(platform::eacces());
    }
    if mask & platform::ACCESS_W_OK != 0 && bits & 0o2 == 0 {
        return Err(platform::eacces());
    }
    if mask & platform::ACCESS_X_OK != 0 && bits & 0o1 == 0 {
        return Err(platform::eacces());
    }

    Ok(())
}
