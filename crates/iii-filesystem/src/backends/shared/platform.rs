//! Platform abstractions for filesystem backends.
//!
//! Provides errno translation (macOS -> Linux), stat wrappers, and error helpers.
//!
//! ## Errno Translation
//!
//! The FUSE protocol always expects Linux errno values. On Linux, errors pass through
//! unchanged. On macOS, BSD errno values are mapped to their Linux equivalents via
//! `linux_error()`. All filesystem operations must wrap OS errors with `linux_error()`
//! before returning them through the FUSE interface.
//!
//! ## RESOLVE_BENEATH
//!
//! `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS)` (Linux 5.6+)
//! provides kernel-enforced path containment that blocks `..` traversal, symlink traversal,
//! procfs-style magic links, and concurrent rename races atomically. Availability is probed
//! at init time and cached in `PassthroughFs::has_openat2`. Falls back to `openat(O_NOFOLLOW)`
//! on older kernels.

use std::{io, os::fd::RawFd};

use crate::{SetattrValid, stat64};

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

const LINUX_EPERM: i32 = 1;
const LINUX_ENOENT: i32 = 2;
const LINUX_ESRCH: i32 = 3;
const LINUX_EINTR: i32 = 4;
const LINUX_EIO: i32 = 5;
const LINUX_ENXIO: i32 = 6;
const LINUX_ENOEXEC: i32 = 8;
const LINUX_EBADF: i32 = 9;
const LINUX_ECHILD: i32 = 10;
const LINUX_EAGAIN: i32 = 11;
const LINUX_ENOMEM: i32 = 12;
const LINUX_EACCES: i32 = 13;
const LINUX_EFAULT: i32 = 14;
const LINUX_ENOTBLK: i32 = 15;
const LINUX_EBUSY: i32 = 16;
const LINUX_EEXIST: i32 = 17;
const LINUX_EXDEV: i32 = 18;
const LINUX_ENODEV: i32 = 19;
const LINUX_ENOTDIR: i32 = 20;
const LINUX_EISDIR: i32 = 21;
const LINUX_EINVAL: i32 = 22;
const LINUX_ENFILE: i32 = 23;
const LINUX_EMFILE: i32 = 24;
const LINUX_ENOTTY: i32 = 25;
const LINUX_ETXTBSY: i32 = 26;
const LINUX_EFBIG: i32 = 27;
const LINUX_ENOSPC: i32 = 28;
const LINUX_ESPIPE: i32 = 29;
const LINUX_EROFS: i32 = 30;
const LINUX_EMLINK: i32 = 31;
const LINUX_EPIPE: i32 = 32;
const LINUX_EDOM: i32 = 33;
const LINUX_ERANGE: i32 = 34;
const LINUX_EDEADLK: i32 = 35;
const LINUX_ENAMETOOLONG: i32 = 36;
const LINUX_ENOLCK: i32 = 37;
pub(crate) const LINUX_ENOSYS: i32 = 38;
const LINUX_ENOTEMPTY: i32 = 39;
const LINUX_ELOOP: i32 = 40;
const LINUX_ENOMSG: i32 = 42;
const LINUX_EIDRM: i32 = 43;
const LINUX_ENOSTR: i32 = 60;
pub(crate) const LINUX_ENODATA: i32 = 61;
const LINUX_ETIME: i32 = 62;
const LINUX_ENOSR: i32 = 63;
const LINUX_EREMOTE: i32 = 66;
const LINUX_ENOLINK: i32 = 67;
const LINUX_EPROTO: i32 = 71;
const LINUX_EMULTIHOP: i32 = 72;
const LINUX_EBADMSG: i32 = 74;
const LINUX_EOVERFLOW: i32 = 75;
const LINUX_EILSEQ: i32 = 84;
const LINUX_EUSERS: i32 = 87;
const LINUX_ENOTSOCK: i32 = 88;
const LINUX_EDESTADDRREQ: i32 = 89;
const LINUX_EMSGSIZE: i32 = 90;
const LINUX_EPROTOTYPE: i32 = 91;
const LINUX_ENOPROTOOPT: i32 = 92;
const LINUX_EPROTONOSUPPORT: i32 = 93;
const LINUX_ESOCKTNOSUPPORT: i32 = 94;
const LINUX_EOPNOTSUPP: i32 = 95;
const LINUX_EPFNOSUPPORT: i32 = 96;
const LINUX_EAFNOSUPPORT: i32 = 97;
const LINUX_EADDRINUSE: i32 = 98;
const LINUX_EADDRNOTAVAIL: i32 = 99;
const LINUX_ENETDOWN: i32 = 100;
const LINUX_ENETUNREACH: i32 = 101;
const LINUX_ENETRESET: i32 = 102;
const LINUX_ECONNABORTED: i32 = 103;
const LINUX_ECONNRESET: i32 = 104;
const LINUX_ENOBUFS: i32 = 105;
const LINUX_EISCONN: i32 = 106;
const LINUX_ENOTCONN: i32 = 107;
const LINUX_ESHUTDOWN: i32 = 108;
const LINUX_ETOOMANYREFS: i32 = 109;
const LINUX_ETIMEDOUT: i32 = 110;
const LINUX_ECONNREFUSED: i32 = 111;
const LINUX_EHOSTDOWN: i32 = 112;
const LINUX_EHOSTUNREACH: i32 = 113;
const LINUX_EALREADY: i32 = 114;
const LINUX_EINPROGRESS: i32 = 115;
const LINUX_ESTALE: i32 = 116;
const LINUX_EDQUOT: i32 = 122;
const LINUX_ECANCELED: i32 = 125;
const LINUX_EOWNERDEAD: i32 = 130;
const LINUX_ENOTRECOVERABLE: i32 = 131;

#[cfg(target_os = "linux")]
pub(crate) const MODE_TYPE_MASK: u32 = libc::S_IFMT;
#[cfg(target_os = "macos")]
pub(crate) const MODE_TYPE_MASK: u32 = libc::S_IFMT as u32;

#[cfg(target_os = "linux")]
pub(crate) const MODE_REG: u32 = libc::S_IFREG;
#[cfg(target_os = "macos")]
pub(crate) const MODE_REG: u32 = libc::S_IFREG as u32;

#[cfg(target_os = "linux")]
pub(crate) const MODE_DIR: u32 = libc::S_IFDIR;
#[cfg(target_os = "macos")]
pub(crate) const MODE_DIR: u32 = libc::S_IFDIR as u32;

#[cfg(target_os = "linux")]
pub(crate) const MODE_LNK: u32 = libc::S_IFLNK;
#[cfg(target_os = "macos")]
pub(crate) const MODE_LNK: u32 = libc::S_IFLNK as u32;

#[cfg(target_os = "linux")]
pub(crate) const MODE_CHR: u32 = libc::S_IFCHR;
#[cfg(target_os = "macos")]
pub(crate) const MODE_CHR: u32 = libc::S_IFCHR as u32;

#[cfg(target_os = "linux")]
pub(crate) const MODE_BLK: u32 = libc::S_IFBLK;
#[cfg(target_os = "macos")]
pub(crate) const MODE_BLK: u32 = libc::S_IFBLK as u32;

#[cfg(target_os = "linux")]
pub(crate) const MODE_FIFO: u32 = libc::S_IFIFO;
#[cfg(target_os = "macos")]
pub(crate) const MODE_FIFO: u32 = libc::S_IFIFO as u32;

#[cfg(target_os = "linux")]
pub(crate) const MODE_SOCK: u32 = libc::S_IFSOCK;
#[cfg(target_os = "macos")]
pub(crate) const MODE_SOCK: u32 = libc::S_IFSOCK as u32;

#[cfg(target_os = "linux")]
pub(crate) const MODE_SETUID: u32 = libc::S_ISUID;
#[cfg(target_os = "macos")]
pub(crate) const MODE_SETUID: u32 = libc::S_ISUID as u32;

#[cfg(target_os = "linux")]
pub(crate) const MODE_SETGID: u32 = libc::S_ISGID;
#[cfg(target_os = "macos")]
pub(crate) const MODE_SETGID: u32 = libc::S_ISGID as u32;

pub(crate) const DIRENT_REG: u32 = libc::DT_REG as u32;

pub(crate) const DIRENT_DIR: u32 = libc::DT_DIR as u32;

pub(crate) const DIRENT_LNK: u32 = libc::DT_LNK as u32;

pub(crate) const DIRENT_CHR: u32 = libc::DT_CHR as u32;

pub(crate) const DIRENT_BLK: u32 = libc::DT_BLK as u32;

pub(crate) const DIRENT_FIFO: u32 = libc::DT_FIFO as u32;

pub(crate) const DIRENT_SOCK: u32 = libc::DT_SOCK as u32;

pub(crate) const ACCESS_F_OK: u32 = libc::F_OK as u32;

pub(crate) const ACCESS_R_OK: u32 = libc::R_OK as u32;

pub(crate) const ACCESS_W_OK: u32 = libc::W_OK as u32;

pub(crate) const ACCESS_X_OK: u32 = libc::X_OK as u32;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Build a utimens-compatible timespec array from a FUSE setattr request.
pub(crate) fn build_timespecs(attr: stat64, valid: SetattrValid) -> [libc::timespec; 2] {
    let mut times = [libc::timespec {
        tv_sec: 0,
        tv_nsec: libc::UTIME_OMIT,
    }; 2];

    if valid.contains(SetattrValid::ATIME) {
        if valid.contains(SetattrValid::ATIME_NOW) {
            times[0].tv_nsec = libc::UTIME_NOW;
        } else {
            times[0].tv_sec = attr.st_atime;
            times[0].tv_nsec = attr.st_atime_nsec;
        }
    }

    if valid.contains(SetattrValid::MTIME) {
        if valid.contains(SetattrValid::MTIME_NOW) {
            times[1].tv_nsec = libc::UTIME_NOW;
        } else {
            times[1].tv_sec = attr.st_mtime;
            times[1].tv_nsec = attr.st_mtime_nsec;
        }
    }

    times
}

/// Translate a native OS error to a Linux errno value.
///
/// On Linux this is an identity function. On macOS, BSD errno values are
/// mapped to their Linux equivalents, since the FUSE protocol always
/// expects Linux errno values.
#[cfg(target_os = "linux")]
pub(crate) fn linux_error(error: io::Error) -> io::Error {
    error
}

/// Translate a native OS error to a Linux errno value.
#[cfg(target_os = "macos")]
pub(crate) fn linux_error(error: io::Error) -> io::Error {
    io::Error::from_raw_os_error(linux_errno_raw(error.raw_os_error().unwrap_or(libc::EIO)))
}

/// Map a native errno to its Linux equivalent.
#[cfg(target_os = "macos")]
fn linux_errno_raw(errno: i32) -> i32 {
    match errno {
        libc::EPERM => LINUX_EPERM,
        libc::ENOENT => LINUX_ENOENT,
        libc::ESRCH => LINUX_ESRCH,
        libc::EINTR => LINUX_EINTR,
        libc::EIO => LINUX_EIO,
        libc::ENXIO => LINUX_ENXIO,
        libc::ENOEXEC => LINUX_ENOEXEC,
        libc::EBADF => LINUX_EBADF,
        libc::ECHILD => LINUX_ECHILD,
        libc::EDEADLK => LINUX_EDEADLK,
        libc::ENOMEM => LINUX_ENOMEM,
        libc::EACCES => LINUX_EACCES,
        libc::EFAULT => LINUX_EFAULT,
        libc::ENOTBLK => LINUX_ENOTBLK,
        libc::EBUSY => LINUX_EBUSY,
        libc::EEXIST => LINUX_EEXIST,
        libc::EXDEV => LINUX_EXDEV,
        libc::ENODEV => LINUX_ENODEV,
        libc::ENOTDIR => LINUX_ENOTDIR,
        libc::EISDIR => LINUX_EISDIR,
        libc::EINVAL => LINUX_EINVAL,
        libc::ENFILE => LINUX_ENFILE,
        libc::EMFILE => LINUX_EMFILE,
        libc::ENOTTY => LINUX_ENOTTY,
        libc::ETXTBSY => LINUX_ETXTBSY,
        libc::EFBIG => LINUX_EFBIG,
        libc::ENOSPC => LINUX_ENOSPC,
        libc::ESPIPE => LINUX_ESPIPE,
        libc::EROFS => LINUX_EROFS,
        libc::EMLINK => LINUX_EMLINK,
        libc::EPIPE => LINUX_EPIPE,
        libc::EDOM => LINUX_EDOM,
        libc::EAGAIN => LINUX_EAGAIN,
        libc::EINPROGRESS => LINUX_EINPROGRESS,
        libc::EALREADY => LINUX_EALREADY,
        libc::ENOTSOCK => LINUX_ENOTSOCK,
        libc::EDESTADDRREQ => LINUX_EDESTADDRREQ,
        libc::EMSGSIZE => LINUX_EMSGSIZE,
        libc::EPROTOTYPE => LINUX_EPROTOTYPE,
        libc::ENOPROTOOPT => LINUX_ENOPROTOOPT,
        libc::EPROTONOSUPPORT => LINUX_EPROTONOSUPPORT,
        libc::ESOCKTNOSUPPORT => LINUX_ESOCKTNOSUPPORT,
        libc::EPFNOSUPPORT => LINUX_EPFNOSUPPORT,
        libc::EAFNOSUPPORT => LINUX_EAFNOSUPPORT,
        libc::EADDRINUSE => LINUX_EADDRINUSE,
        libc::EADDRNOTAVAIL => LINUX_EADDRNOTAVAIL,
        libc::ENETDOWN => LINUX_ENETDOWN,
        libc::ENETUNREACH => LINUX_ENETUNREACH,
        libc::ENETRESET => LINUX_ENETRESET,
        libc::ECONNABORTED => LINUX_ECONNABORTED,
        libc::ECONNRESET => LINUX_ECONNRESET,
        libc::ENOBUFS => LINUX_ENOBUFS,
        libc::EISCONN => LINUX_EISCONN,
        libc::ENOTCONN => LINUX_ENOTCONN,
        libc::ESHUTDOWN => LINUX_ESHUTDOWN,
        libc::ETOOMANYREFS => LINUX_ETOOMANYREFS,
        libc::ETIMEDOUT => LINUX_ETIMEDOUT,
        libc::ECONNREFUSED => LINUX_ECONNREFUSED,
        libc::ELOOP => LINUX_ELOOP,
        libc::ENAMETOOLONG => LINUX_ENAMETOOLONG,
        libc::EHOSTDOWN => LINUX_EHOSTDOWN,
        libc::EHOSTUNREACH => LINUX_EHOSTUNREACH,
        libc::ENOTEMPTY => LINUX_ENOTEMPTY,
        libc::EUSERS => LINUX_EUSERS,
        libc::EDQUOT => LINUX_EDQUOT,
        libc::ESTALE => LINUX_ESTALE,
        libc::EREMOTE => LINUX_EREMOTE,
        libc::ENOLCK => LINUX_ENOLCK,
        libc::ENOSYS => LINUX_ENOSYS,
        libc::EOVERFLOW => LINUX_EOVERFLOW,
        libc::ECANCELED => LINUX_ECANCELED,
        libc::EIDRM => LINUX_EIDRM,
        libc::ENOMSG => LINUX_ENOMSG,
        libc::EILSEQ => LINUX_EILSEQ,
        libc::ENOATTR => LINUX_ENODATA,
        libc::EBADMSG => LINUX_EBADMSG,
        libc::EMULTIHOP => LINUX_EMULTIHOP,
        libc::ENODATA => LINUX_ENODATA,
        libc::ENOLINK => LINUX_ENOLINK,
        libc::ENOSR => LINUX_ENOSR,
        libc::ENOSTR => LINUX_ENOSTR,
        libc::EPROTO => LINUX_EPROTO,
        libc::ETIME => LINUX_ETIME,
        libc::EOPNOTSUPP => LINUX_EOPNOTSUPP,
        libc::ENOTRECOVERABLE => LINUX_ENOTRECOVERABLE,
        libc::EOWNERDEAD => LINUX_EOWNERDEAD,
        _ => LINUX_EIO,
    }
}

/// Create an `io::Error` with Linux `EIO`.
pub(crate) fn eio() -> io::Error {
    io::Error::from_raw_os_error(LINUX_EIO)
}

/// Create an `io::Error` with Linux `EBADF`.
pub(crate) fn ebadf() -> io::Error {
    io::Error::from_raw_os_error(LINUX_EBADF)
}

/// Create an `io::Error` with Linux `EINVAL`.
pub(crate) fn einval() -> io::Error {
    io::Error::from_raw_os_error(LINUX_EINVAL)
}

/// Create an `io::Error` with Linux `EACCES`.
pub(crate) fn eacces() -> io::Error {
    io::Error::from_raw_os_error(LINUX_EACCES)
}

/// Create an `io::Error` with Linux `EPERM`.
pub(crate) fn eperm() -> io::Error {
    io::Error::from_raw_os_error(LINUX_EPERM)
}

/// Create an `io::Error` with Linux `ENOSYS`.
pub(crate) fn enosys() -> io::Error {
    io::Error::from_raw_os_error(LINUX_ENOSYS)
}

/// Create an `io::Error` with Linux `ENOENT`.
pub(crate) fn enoent() -> io::Error {
    io::Error::from_raw_os_error(LINUX_ENOENT)
}

/// Create an `io::Error` with Linux `ENODATA`.
pub(crate) fn enodata() -> io::Error {
    io::Error::from_raw_os_error(LINUX_ENODATA)
}

/// Create an `io::Error` with Linux `EISDIR`.
pub(crate) fn eisdir() -> io::Error {
    io::Error::from_raw_os_error(LINUX_EISDIR)
}

/// Create an `io::Error` with Linux `ENOTDIR`.
pub(crate) fn enotdir() -> io::Error {
    io::Error::from_raw_os_error(LINUX_ENOTDIR)
}

/// Create an `io::Error` with Linux `ENOTEMPTY`.
pub(crate) fn enotempty() -> io::Error {
    io::Error::from_raw_os_error(LINUX_ENOTEMPTY)
}

/// Create an `io::Error` with Linux `ELOOP`.
pub(crate) fn eloop() -> io::Error {
    io::Error::from_raw_os_error(LINUX_ELOOP)
}

/// Create an `io::Error` with Linux `ENAMETOOLONG`.
pub(crate) fn enametoolong() -> io::Error {
    io::Error::from_raw_os_error(LINUX_ENAMETOOLONG)
}

/// Create an `io::Error` with Linux `EEXIST`.
pub(crate) fn eexist() -> io::Error {
    io::Error::from_raw_os_error(LINUX_EEXIST)
}

/// Create an `io::Error` with Linux `ENOSPC`.
pub(crate) fn enospc() -> io::Error {
    io::Error::from_raw_os_error(LINUX_ENOSPC)
}

/// Create an `io::Error` with Linux `EFBIG`.
pub(crate) fn efbig() -> io::Error {
    io::Error::from_raw_os_error(LINUX_EFBIG)
}

/// Create an `io::Error` with Linux `EOPNOTSUPP`.
pub(crate) fn eopnotsupp() -> io::Error {
    io::Error::from_raw_os_error(LINUX_EOPNOTSUPP)
}

/// Create an `io::Error` with Linux `ENODEV`.
pub(crate) fn enodev() -> io::Error {
    io::Error::from_raw_os_error(LINUX_ENODEV)
}

/// Create an `io::Error` with Linux `ENXIO`.
pub(crate) fn enxio() -> io::Error {
    io::Error::from_raw_os_error(LINUX_ENXIO)
}

/// Create an `io::Error` with Linux `ERANGE`.
pub(crate) fn erange() -> io::Error {
    io::Error::from_raw_os_error(LINUX_ERANGE)
}

/// Create an `io::Error` with Linux `EROFS`.
pub(crate) fn erofs() -> io::Error {
    io::Error::from_raw_os_error(LINUX_EROFS)
}

/// Check if an error is ENOENT.
///
/// ENOENT is 2 on both Linux and macOS, so a single check suffices
/// regardless of whether `linux_error()` was applied.
pub(crate) fn is_enoent(err: &io::Error) -> bool {
    err.raw_os_error() == Some(2)
}

/// Call `fstat` on a raw file descriptor and return a `stat64`.
pub(crate) fn fstat(fd: RawFd) -> io::Result<stat64> {
    let mut st = unsafe { std::mem::zeroed::<stat64>() };

    #[cfg(target_os = "linux")]
    let ret = unsafe { libc::fstat64(fd, &mut st) };

    #[cfg(target_os = "macos")]
    let ret = unsafe { libc::fstat(fd, &mut st) };

    if ret < 0 {
        Err(linux_error(io::Error::last_os_error()))
    } else {
        Ok(st)
    }
}

/// Normalize a mode value to `u32` across platforms.
#[cfg(target_os = "linux")]
pub(crate) fn mode_u32(mode: libc::mode_t) -> u32 {
    mode
}

/// Normalize a mode value to `u32` across platforms.
#[cfg(target_os = "macos")]
pub(crate) fn mode_u32(mode: libc::mode_t) -> u32 {
    mode as u32
}

/// Extract the file type bits from a mode value.
pub(crate) fn mode_file_type(mode: libc::mode_t) -> u32 {
    mode_u32(mode) & MODE_TYPE_MASK
}

/// Convert a file type bitmask to a dirent type value.
pub(crate) fn dirent_type_from_mode(file_type: u32) -> u32 {
    match file_type {
        MODE_LNK => DIRENT_LNK,
        MODE_DIR => DIRENT_DIR,
        MODE_CHR => DIRENT_CHR,
        MODE_BLK => DIRENT_BLK,
        MODE_FIFO => DIRENT_FIFO,
        MODE_SOCK => DIRENT_SOCK,
        _ => DIRENT_REG,
    }
}

/// Normalize `st_ino` to `u64` across platforms.
pub(crate) fn stat_ino(st: &stat64) -> u64 {
    st.st_ino
}

/// Normalize `st_dev` to `u64` across platforms.
#[cfg(target_os = "linux")]
pub(crate) fn stat_dev(st: &stat64) -> u64 {
    st.st_dev
}

/// Normalize `st_dev` to `u64` across platforms.
#[cfg(target_os = "macos")]
pub(crate) fn stat_dev(st: &stat64) -> u64 {
    st.st_dev as u64
}

/// Read the target of a symlink opened by file descriptor (Linux only).
///
/// This uses `readlinkat(fd, "", ...)` so the kernel reads the symlink target
/// referenced by the already-pinned fd itself. Using `/proc/self/fd/N` here
/// would instead expose the procfs magic-link target and leak a host path.
#[cfg(target_os = "linux")]
pub(crate) fn readlink_fd(fd: RawFd) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; libc::PATH_MAX as usize];
    let len = unsafe {
        libc::readlinkat(
            fd,
            c"".as_ptr(),
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
        )
    };
    if len < 0 {
        Err(linux_error(io::Error::last_os_error()))
    } else {
        buf.truncate(len as usize);
        Ok(buf)
    }
}

/// Struct for the `openat2` syscall (Linux 5.6+).
#[cfg(target_os = "linux")]
#[repr(C)]
pub(crate) struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}

/// `RESOLVE_BENEATH` flag -- prevent path resolution from escaping the directory tree.
#[cfg(target_os = "linux")]
pub(crate) const RESOLVE_BENEATH: u64 = 0x08;

/// `RESOLVE_NO_SYMLINKS` flag -- reject all symlink traversal during resolution.
#[cfg(target_os = "linux")]
pub(crate) const RESOLVE_NO_SYMLINKS: u64 = 0x04;

/// `RESOLVE_NO_MAGICLINKS` flag -- reject procfs-style magic links.
#[cfg(target_os = "linux")]
pub(crate) const RESOLVE_NO_MAGICLINKS: u64 = 0x02;

#[cfg(target_os = "linux")]
const OPENAT2_RESOLVE_FLAGS: u64 = RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS;

/// Syscall number for `openat2` (same on x86_64 and aarch64).
#[cfg(target_os = "linux")]
const SYS_OPENAT2: libc::c_long = 437;

/// Probe whether the `openat2` syscall is available (Linux 5.6+).
///
/// Attempts a minimal openat2 call on the current directory. Returns `true`
/// if the syscall succeeds or returns any error other than `ENOSYS`.
#[cfg(target_os = "linux")]
pub(crate) fn probe_openat2() -> bool {
    let how = OpenHow {
        flags: libc::O_CLOEXEC as u64 | libc::O_PATH as u64,
        mode: 0,
        resolve: OPENAT2_RESOLVE_FLAGS,
    };
    let ret = unsafe {
        libc::syscall(
            SYS_OPENAT2,
            libc::AT_FDCWD,
            c".".as_ptr(),
            &how as *const OpenHow,
            std::mem::size_of::<OpenHow>(),
        )
    };
    if ret >= 0 {
        unsafe { libc::close(ret as i32) };
        true
    } else {
        !matches!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::ENOSYS | libc::EINVAL)
        )
    }
}

/// Open a file relative to a directory with Linux openat2 containment if available.
///
/// Falls back to regular `openat` if `openat2` is not available.
#[cfg(target_os = "linux")]
pub(crate) fn open_beneath(
    dirfd: RawFd,
    name: *const libc::c_char,
    flags: i32,
    use_openat2: bool,
) -> RawFd {
    if use_openat2 {
        let how = OpenHow {
            flags: (flags | libc::O_CLOEXEC) as u64,
            mode: 0,
            resolve: OPENAT2_RESOLVE_FLAGS,
        };
        let ret = unsafe {
            libc::syscall(
                SYS_OPENAT2,
                dirfd,
                name,
                &how as *const OpenHow,
                std::mem::size_of::<OpenHow>(),
            ) as i32
        };
        if ret >= 0 || io::Error::last_os_error().raw_os_error() != Some(libc::ENOSYS) {
            return ret;
        }
        // ENOSYS fallthrough to regular openat.
    }
    unsafe { libc::openat(dirfd, name, flags | libc::O_CLOEXEC) }
}

/// Convert a `libc::statx` struct to a `stat64` struct (Linux only).
///
/// Used in the lookup collapse optimization where `statx` with `AT_EMPTY_PATH`
/// provides both stat data and `mnt_id` in a single syscall.
#[cfg(target_os = "linux")]
pub(crate) fn statx_to_stat64(stx: &libc::statx) -> stat64 {
    let mut st: stat64 = unsafe { std::mem::zeroed() };
    st.st_dev = makedev(stx.stx_dev_major, stx.stx_dev_minor);
    st.st_ino = stx.stx_ino;
    st.st_nlink = stx.stx_nlink as _;
    st.st_mode = stx.stx_mode as _;
    st.st_uid = stx.stx_uid;
    st.st_gid = stx.stx_gid;
    st.st_rdev = makedev(stx.stx_rdev_major, stx.stx_rdev_minor);
    st.st_size = stx.stx_size as _;
    st.st_blksize = stx.stx_blksize as _;
    st.st_blocks = stx.stx_blocks as _;
    st.st_atime = stx.stx_atime.tv_sec;
    st.st_atime_nsec = stx.stx_atime.tv_nsec as _;
    st.st_mtime = stx.stx_mtime.tv_sec;
    st.st_mtime_nsec = stx.stx_mtime.tv_nsec as _;
    st.st_ctime = stx.stx_ctime.tv_sec;
    st.st_ctime_nsec = stx.stx_ctime.tv_nsec as _;
    st
}

/// Compute a `dev_t` from major and minor numbers (Linux glibc formula).
#[cfg(target_os = "linux")]
fn makedev(major: u32, minor: u32) -> u64 {
    ((major as u64 & 0xfffff000) << 32)
        | ((major as u64 & 0x00000fff) << 8)
        | ((minor as u64 & 0xffffff00) << 12)
        | (minor as u64 & 0x000000ff)
}

/// Call `fstatat` (no follow) on a name relative to a directory fd.
pub(crate) fn fstatat_nofollow(dirfd: RawFd, name: &std::ffi::CStr) -> io::Result<stat64> {
    let mut st = unsafe { std::mem::zeroed::<stat64>() };

    #[cfg(target_os = "linux")]
    let ret = unsafe { libc::fstatat64(dirfd, name.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) };

    #[cfg(target_os = "macos")]
    let ret = unsafe { libc::fstatat(dirfd, name.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) };

    if ret < 0 {
        Err(linux_error(io::Error::last_os_error()))
    } else {
        Ok(st)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eio_returns_correct_errno() {
        let err = eio();
        assert_eq!(err.raw_os_error(), Some(LINUX_EIO));
    }

    #[test]
    fn ebadf_returns_correct_errno() {
        let err = ebadf();
        assert_eq!(err.raw_os_error(), Some(LINUX_EBADF));
    }

    #[test]
    fn einval_returns_correct_errno() {
        let err = einval();
        assert_eq!(err.raw_os_error(), Some(LINUX_EINVAL));
    }

    #[test]
    fn eacces_returns_correct_errno() {
        let err = eacces();
        assert_eq!(err.raw_os_error(), Some(LINUX_EACCES));
    }

    #[test]
    fn eperm_returns_correct_errno() {
        let err = eperm();
        assert_eq!(err.raw_os_error(), Some(LINUX_EPERM));
    }

    #[test]
    fn enosys_returns_correct_errno() {
        let err = enosys();
        assert_eq!(err.raw_os_error(), Some(LINUX_ENOSYS));
    }

    #[test]
    fn enoent_returns_correct_errno() {
        let err = enoent();
        assert_eq!(err.raw_os_error(), Some(LINUX_ENOENT));
    }

    #[test]
    fn eloop_returns_correct_errno() {
        let err = eloop();
        assert_eq!(err.raw_os_error(), Some(LINUX_ELOOP));
    }

    #[test]
    fn is_enoent_true_for_enoent() {
        let err = enoent();
        assert!(is_enoent(&err));
    }

    #[test]
    fn is_enoent_false_for_eio() {
        let err = eio();
        assert!(!is_enoent(&err));
    }

    #[test]
    fn dirent_type_from_mode_reg() {
        assert_eq!(dirent_type_from_mode(MODE_REG), DIRENT_REG);
    }

    #[test]
    fn dirent_type_from_mode_dir() {
        assert_eq!(dirent_type_from_mode(MODE_DIR), DIRENT_DIR);
    }

    #[test]
    fn dirent_type_from_mode_lnk() {
        assert_eq!(dirent_type_from_mode(MODE_LNK), DIRENT_LNK);
    }

    #[test]
    fn dirent_type_from_mode_chr() {
        assert_eq!(dirent_type_from_mode(MODE_CHR), DIRENT_CHR);
    }

    #[test]
    fn dirent_type_from_mode_blk() {
        assert_eq!(dirent_type_from_mode(MODE_BLK), DIRENT_BLK);
    }

    #[test]
    fn dirent_type_from_mode_fifo() {
        assert_eq!(dirent_type_from_mode(MODE_FIFO), DIRENT_FIFO);
    }

    #[test]
    fn dirent_type_from_mode_sock() {
        assert_eq!(dirent_type_from_mode(MODE_SOCK), DIRENT_SOCK);
    }

    #[test]
    fn dirent_type_from_mode_unknown_defaults_to_reg() {
        assert_eq!(dirent_type_from_mode(0xFFFF), DIRENT_REG);
    }

    #[test]
    fn mode_file_type_extracts_type_bits() {
        let mode = MODE_REG | 0o755;
        assert_eq!(mode_file_type(mode as libc::mode_t), MODE_REG);
    }

    #[test]
    fn fstat_on_valid_fd() {
        // Use a real fd (stdout)
        let st = fstat(1);
        assert!(st.is_ok());
    }

    #[test]
    fn fstat_on_invalid_fd() {
        let st = fstat(-1);
        assert!(st.is_err());
    }

    #[test]
    fn build_timespecs_omit_both() {
        let attr = unsafe { std::mem::zeroed::<stat64>() };
        let valid = SetattrValid::empty();
        let times = build_timespecs(attr, valid);
        assert_eq!(times[0].tv_nsec, libc::UTIME_OMIT);
        assert_eq!(times[1].tv_nsec, libc::UTIME_OMIT);
    }

    #[test]
    fn build_timespecs_atime_now() {
        let attr = unsafe { std::mem::zeroed::<stat64>() };
        let valid = SetattrValid::ATIME | SetattrValid::ATIME_NOW;
        let times = build_timespecs(attr, valid);
        assert_eq!(times[0].tv_nsec, libc::UTIME_NOW);
        assert_eq!(times[1].tv_nsec, libc::UTIME_OMIT);
    }

    #[test]
    fn build_timespecs_mtime_now() {
        let attr = unsafe { std::mem::zeroed::<stat64>() };
        let valid = SetattrValid::MTIME | SetattrValid::MTIME_NOW;
        let times = build_timespecs(attr, valid);
        assert_eq!(times[0].tv_nsec, libc::UTIME_OMIT);
        assert_eq!(times[1].tv_nsec, libc::UTIME_NOW);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_error_is_identity_on_linux() {
        let err = io::Error::from_raw_os_error(libc::ENOENT);
        let mapped = linux_error(err);
        assert_eq!(mapped.raw_os_error(), Some(libc::ENOENT));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn linux_error_maps_macos_enoent() {
        let err = io::Error::from_raw_os_error(libc::ENOENT);
        let mapped = linux_error(err);
        assert_eq!(mapped.raw_os_error(), Some(LINUX_ENOENT));
    }
}
