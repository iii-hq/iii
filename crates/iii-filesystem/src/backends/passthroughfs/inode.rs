//! Inode management: lookup, forget, and reference counting.
//!
//! ## Lookup Strategy
//!
//! Linux lookup uses a "collapse" optimization: open -> statx(AT_EMPTY_PATH) -> done,
//! yielding 2 syscalls instead of fstatat + open (without xattr, one less than microsandbox).
//! The stat is taken on the *opened* fd, eliminating TOCTOU between stat and open.
//!
//! macOS lookup uses fstatat -> inode table check -> register, with a separate fd open
//! via `/.vol/dev/ino` for stat access (since macOS doesn't store per-inode O_PATH fds).
//!
//! ## Procfd Reopen
//!
//! `open_inode_fd` reopens tracked inodes for I/O via `/proc/self/fd/N`.
//! Procfd entries are themselves symlinks on Linux, so reopening them must not
//! add `O_NOFOLLOW` or the kernel will fail with `ELOOP`. Instead, the pinned
//! inode is `fstat`'d first and real host symlinks are rejected before reopen.

use std::{
    ffi::CStr,
    io,
    os::fd::AsRawFd,
    sync::{Arc, atomic::Ordering},
};

use super::PassthroughFs;
use crate::{
    Entry,
    backends::shared::{
        inode_table::{InodeAltKey, InodeData, MultikeyBTreeMap},
        platform,
    },
    stat64,
};


//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Owned-or-borrowed fd for inode operations.
///
/// On Linux, borrows the O_PATH fd from InodeData (no close on drop).
/// On macOS, may own a temporary fd opened via `/.vol/` (closed on drop).
pub(crate) struct InodeFd {
    fd: i32,
    #[cfg(target_os = "macos")]
    owned: bool,
}

impl InodeFd {
    pub(crate) fn raw(&self) -> i32 {
        self.fd
    }
}

impl Drop for InodeFd {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if self.owned && self.fd >= 0 {
            unsafe { libc::close(self.fd) };
        }
    }
}

/// Linux guest open flag constants.
///
/// The guest kernel sends Linux flag values over virtio-fs. On Linux hosts these
/// match `libc` constants, but on macOS the numeric values differ (e.g. Linux
/// `O_TRUNC` 0x200 = macOS `O_CREAT` 0x200). This module defines the Linux
/// values so we can translate them to host values on macOS.
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
mod linux_flags {
    pub const O_APPEND: i32 = 0x400;
    pub const O_CREAT: i32 = 0x40;
    pub const O_TRUNC: i32 = 0x200;
    pub const O_EXCL: i32 = 0x80;
    pub const O_NOFOLLOW: i32 = 0x20000;
    pub const O_NONBLOCK: i32 = 0x800;
    pub const O_CLOEXEC: i32 = 0x80000;
    pub const O_DIRECTORY: i32 = 0x10000;
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod linux_flags {
    pub const O_APPEND: i32 = 0x400;
    pub const O_CREAT: i32 = 0x40;
    pub const O_TRUNC: i32 = 0x200;
    pub const O_EXCL: i32 = 0x80;
    pub const O_NOFOLLOW: i32 = 0x8000;
    pub const O_NONBLOCK: i32 = 0x800;
    pub const O_CLOEXEC: i32 = 0x80000;
    pub const O_DIRECTORY: i32 = 0x4000;
}

#[cfg(all(
    target_os = "macos",
    not(any(target_arch = "x86_64", target_arch = "aarch64"))
))]
compile_error!("unsupported macOS architecture for Linux open-flag translation");

#[cfg(target_os = "macos")]
const O_RESOLVE_BENEATH: i32 = 0x0000_1000;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Translate Linux guest open flags to host open flags.
///
/// On Linux this is a no-op (flags match). On macOS, maps Linux numeric values
/// to the corresponding macOS `libc` constants. Without this translation,
/// Linux `O_TRUNC` (0x200) becomes macOS `O_CREAT` (0x200), and Linux
/// `O_APPEND` (0x400) becomes macOS `O_TRUNC` (0x400).
#[cfg(target_os = "linux")]
pub(crate) fn translate_open_flags(flags: i32) -> i32 {
    flags
}

#[cfg(target_os = "macos")]
pub(crate) fn translate_open_flags(linux_flags_val: i32) -> i32 {
    // Access mode (O_RDONLY=0, O_WRONLY=1, O_RDWR=2) -- same on both platforms.
    let mut flags = linux_flags_val & 0b11;
    if linux_flags_val & linux_flags::O_APPEND != 0 {
        flags |= libc::O_APPEND;
    }
    if linux_flags_val & linux_flags::O_CREAT != 0 {
        flags |= libc::O_CREAT;
    }
    if linux_flags_val & linux_flags::O_TRUNC != 0 {
        flags |= libc::O_TRUNC;
    }
    if linux_flags_val & linux_flags::O_EXCL != 0 {
        flags |= libc::O_EXCL;
    }
    if linux_flags_val & linux_flags::O_NOFOLLOW != 0 {
        flags |= libc::O_NOFOLLOW;
    }
    if linux_flags_val & linux_flags::O_NONBLOCK != 0 {
        flags |= libc::O_NONBLOCK;
    }
    if linux_flags_val & linux_flags::O_CLOEXEC != 0 {
        flags |= libc::O_CLOEXEC;
    }
    if linux_flags_val & linux_flags::O_DIRECTORY != 0 {
        flags |= libc::O_DIRECTORY;
    }
    flags
}

#[cfg(target_os = "macos")]
pub(crate) fn store_unlinked_fd(data: &InodeData, fd: i32) {
    let previous = data.unlinked_fd.swap(fd as i64, Ordering::AcqRel);
    if previous >= 0 {
        unsafe { libc::close(previous as i32) };
    }
}

#[cfg(target_os = "macos")]
fn is_unsupported_macos_open_flag(err: &io::Error) -> bool {
    const ECAPMODE: i32 = 107;
    matches!(
        err.raw_os_error(),
        Some(libc::EINVAL | libc::ENOTSUP | libc::EOPNOTSUPP | ECAPMODE | libc::ELOOP)
    )
}

#[cfg(target_os = "macos")]
fn open_macos_path_hardened(path: *const libc::c_char, flags: i32) -> io::Result<i32> {
    let attempts = [
        (
            (flags | libc::O_CLOEXEC | libc::O_NOFOLLOW_ANY | O_RESOLVE_BENEATH) & !libc::O_EXLOCK,
            true,
        ),
        (
            (flags | libc::O_CLOEXEC | libc::O_NOFOLLOW_ANY) & !libc::O_EXLOCK,
            false,
        ),
        (
            (flags | libc::O_CLOEXEC | libc::O_NOFOLLOW) & !libc::O_EXLOCK,
            false,
        ),
    ];

    let mut last_err: Option<io::Error> = None;
    for (attempt, allow_access_denied_fallback) in attempts {
        let fd = unsafe { libc::open(path, attempt) };
        if fd >= 0 {
            return Ok(fd);
        }

        let err = io::Error::last_os_error();
        let fallback_ok = is_unsupported_macos_open_flag(&err)
            || (allow_access_denied_fallback
                && matches!(err.raw_os_error(), Some(libc::EACCES | libc::EPERM)));
        if !fallback_ok {
            return Err(platform::linux_error(err));
        }
        last_err = Some(err);
    }

    Err(platform::linux_error(
        last_err.unwrap_or_else(io::Error::last_os_error),
    ))
}

#[cfg(target_os = "macos")]
pub(crate) fn vol_path(dev: u64, ino: u64) -> std::ffi::CString {
    use std::ffi::CString;

    CString::new(format!("/.vol/{dev}/{ino}"))
        .expect("formatted /.vol path never contains interior nul")
}

/// Look up a child name in a parent directory and return an [`Entry`].
///
/// If the inode is already in the table (matched by host identity), its
/// refcount is incremented and the existing inode number is returned.
/// Otherwise a new inode is allocated.
pub(crate) fn do_lookup(fs: &PassthroughFs, parent: u64, name: &CStr) -> io::Result<Entry> {
    crate::backends::shared::name_validation::validate_name(name)?;

    let parent_fd = get_inode_fd(fs, parent)?;

    #[cfg(target_os = "linux")]
    return do_lookup_linux(fs, parent_fd.raw(), name);

    #[cfg(target_os = "macos")]
    return do_lookup_macos(fs, parent_fd.raw(), name);
}

/// Linux lookup: open -> statx(AT_EMPTY_PATH) (2 syscalls, no xattr).
///
/// This is more efficient than the fstatat + statx + open path (4 syscalls),
/// and also more correct: the stat is on the *opened* fd, eliminating TOCTOU
/// between stat and open.
///
/// The open uses `RESOLVE_BENEATH` (Linux 5.6+) for kernel-enforced containment,
/// which atomically blocks `..` traversal, absolute symlinks, and handles concurrent
/// rename races. Falls back to `openat(O_NOFOLLOW)` on older kernels.
#[cfg(target_os = "linux")]
fn do_lookup_linux(fs: &PassthroughFs, parent_fd: i32, name: &CStr) -> io::Result<Entry> {
    use std::os::fd::FromRawFd;

    // Syscall 1: Open with RESOLVE_BENEATH containment.
    let fd = platform::open_beneath(
        parent_fd,
        name.as_ptr(),
        libc::O_PATH | libc::O_NOFOLLOW,
        fs.has_openat2.load(Ordering::Relaxed),
    );
    if fd < 0 {
        return Err(platform::linux_error(io::Error::last_os_error()));
    }

    // Syscall 2: statx with AT_EMPTY_PATH on the opened fd.
    // Gets stat data + mnt_id in one call.
    let mut stx: libc::statx = unsafe { std::mem::zeroed() };
    let ret = unsafe {
        libc::statx(
            fd,
            c"".as_ptr(),
            libc::AT_EMPTY_PATH | libc::AT_SYMLINK_NOFOLLOW | libc::AT_STATX_SYNC_AS_STAT,
            libc::STATX_BASIC_STATS | libc::STATX_MNT_ID,
            &mut stx,
        )
    };
    if ret < 0 {
        let err = platform::linux_error(io::Error::last_os_error());
        unsafe { libc::close(fd) };
        return Err(err);
    }

    let st = platform::statx_to_stat64(&stx);
    let mnt_id = stx.stx_mnt_id;
    let alt_key = InodeAltKey::new(st.st_ino, st.st_dev, mnt_id);

    // Fast path: most lookups hit an already-tracked inode and only need a
    // refcount bump. We still recheck under the write lock below before
    // inserting to close the concurrent registration race.
    {
        let inodes = fs.inodes.read().unwrap();
        if let Some(data) = inodes.get_alt(&alt_key) {
            data.refcount.fetch_add(1, Ordering::Acquire);
            // Close the fd -- we already have one for this inode.
            unsafe { libc::close(fd) };
            return Ok(Entry {
                inode: data.inode,
                generation: 0,
                attr: st,
                attr_flags: 0,
                attr_timeout: fs.cfg.attr_timeout,
                entry_timeout: fs.cfg.entry_timeout,
            });
        }
    }

    // New inode candidate -- take ownership of the fd while we race-proof registration.
    let file = unsafe { std::fs::File::from_raw_fd(fd) };

    // Recheck under the write lock so concurrent lookups cannot register two
    // synthetic inode numbers for the same host identity.
    let mut inodes = fs.inodes.write().unwrap();
    if let Some(data) = inodes.get_alt(&alt_key) {
        data.refcount.fetch_add(1, Ordering::Acquire);
        return Ok(Entry {
            inode: data.inode,
            generation: 0,
            attr: st,
            attr_flags: 0,
            attr_timeout: fs.cfg.attr_timeout,
            entry_timeout: fs.cfg.entry_timeout,
        });
    }

    let inode_num = fs.next_inode.fetch_add(1, Ordering::Relaxed);
    let data = Arc::new(InodeData {
        inode: inode_num,
        ino: st.st_ino,
        dev: st.st_dev,
        refcount: std::sync::atomic::AtomicU64::new(1),
        file,
        mnt_id,
    });
    inodes.insert(inode_num, alt_key, data);

    Ok(Entry {
        inode: inode_num,
        generation: 0,
        attr: st,
        attr_flags: 0,
        attr_timeout: fs.cfg.attr_timeout,
        entry_timeout: fs.cfg.entry_timeout,
    })
}

/// macOS lookup: fstatat -> check table -> register.
///
/// Uses raw stat data directly (no xattr patching).
#[cfg(target_os = "macos")]
fn do_lookup_macos(fs: &PassthroughFs, parent_fd: i32, name: &CStr) -> io::Result<Entry> {
    let st = platform::fstatat_nofollow(parent_fd, name)?;
    let alt_key = InodeAltKey::new(platform::stat_ino(&st), platform::stat_dev(&st));

    // Fast path: most lookups hit an already-tracked inode and only need a
    // refcount bump. We still recheck under the write lock below before
    // inserting to close the concurrent registration race.
    {
        let inodes = fs.inodes.read().unwrap();
        if let Some(data) = inodes.get_alt(&alt_key) {
            data.refcount.fetch_add(1, Ordering::Acquire);
            return Ok(Entry {
                inode: data.inode,
                generation: 0,
                attr: st,
                attr_flags: 0,
                attr_timeout: fs.cfg.attr_timeout,
                entry_timeout: fs.cfg.entry_timeout,
            });
        }
    }

    // Recheck under the write lock so concurrent lookups cannot register two
    // synthetic inode numbers for the same host identity.
    let mut inodes = fs.inodes.write().unwrap();
    if let Some(data) = inodes.get_alt(&alt_key) {
        data.refcount.fetch_add(1, Ordering::Acquire);
        return Ok(Entry {
            inode: data.inode,
            generation: 0,
            attr: st,
            attr_flags: 0,
            attr_timeout: fs.cfg.attr_timeout,
            entry_timeout: fs.cfg.entry_timeout,
        });
    }

    let inode_num = fs.next_inode.fetch_add(1, Ordering::Relaxed);
    let data = Arc::new(InodeData {
        inode: inode_num,
        ino: platform::stat_ino(&st),
        dev: platform::stat_dev(&st),
        refcount: std::sync::atomic::AtomicU64::new(1),
        #[cfg(target_os = "macos")]
        unlinked_fd: std::sync::atomic::AtomicI64::new(-1),
    });
    inodes.insert(inode_num, alt_key, data);

    Ok(Entry {
        inode: inode_num,
        generation: 0,
        attr: st,
        attr_flags: 0,
        attr_timeout: fs.cfg.attr_timeout,
        entry_timeout: fs.cfg.entry_timeout,
    })
}

/// Decrement the reference count for an inode. Remove it from the table
/// when the count reaches zero.
pub(crate) fn forget_one(fs: &PassthroughFs, inode: u64, count: u64) {
    let mut inodes = fs.inodes.write().unwrap();
    forget_one_locked(&mut inodes, inode, count);
}

/// Decrement the reference count under an already-held write lock.
///
/// Used by [`super::PassthroughFs::batch_forget`] to process all entries
/// under a single lock acquisition (O(1) lock ops vs O(n) for per-entry locking).
///
/// Uses a CAS loop to handle the race where a concurrent `lookup` may increment
/// the refcount between our load and compare_exchange. `saturating_sub` prevents
/// underflow if the kernel sends a forget count larger than the current refcount.
pub(crate) fn forget_one_locked(
    inodes: &mut MultikeyBTreeMap<u64, InodeAltKey, Arc<InodeData>>,
    inode: u64,
    count: u64,
) {
    if let Some(data) = inodes.get(&inode) {
        loop {
            let old = data.refcount.load(Ordering::Relaxed);
            let new = old.saturating_sub(count);
            if data
                .refcount
                .compare_exchange(old, new, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                if new == 0 {
                    // Close the unlinked fd if one was preserved.
                    #[cfg(target_os = "macos")]
                    {
                        let ufd = data.unlinked_fd.load(Ordering::Acquire);
                        if ufd >= 0 {
                            unsafe { libc::close(ufd as i32) };
                        }
                    }
                    inodes.remove(&inode);
                }
                break;
            }
        }
    }
}

/// Get an fd for an inode suitable for `*at()` syscalls.
///
/// On Linux, returns the borrowed O_PATH fd from InodeData (no close on drop).
/// On macOS, opens a temporary fd via `/.vol/<dev>/<ino>` (closed on drop).
/// Root inode (1) always borrows the stored root fd.
pub(crate) fn get_inode_fd(fs: &PassthroughFs, inode: u64) -> io::Result<InodeFd> {
    // Root inode uses the stored root fd.
    if inode == 1 {
        let inodes = fs.inodes.read().unwrap();
        if inodes.get(&inode).is_none() {
            return Err(platform::ebadf());
        }
        drop(inodes);

        return Ok(InodeFd {
            fd: fs.root_fd.as_raw_fd(),
            #[cfg(target_os = "macos")]
            owned: false,
        });
    }

    let inodes = fs.inodes.read().unwrap();
    let data = inodes.get(&inode).ok_or_else(platform::ebadf)?;

    #[cfg(target_os = "linux")]
    {
        Ok(InodeFd {
            fd: data.file.as_raw_fd(),
        })
    }

    #[cfg(target_os = "macos")]
    {
        // Try unlinked_fd first -- /.vol/ path is invalid after unlink.
        let ufd = data.unlinked_fd.load(Ordering::Acquire);
        if ufd >= 0 {
            let fd = unsafe { libc::fcntl(ufd as i32, libc::F_DUPFD_CLOEXEC, 0) };
            if fd >= 0 {
                return Ok(InodeFd { fd, owned: true });
            }
        }

        let fd = open_vol_fd(data.dev, data.ino)?;
        Ok(InodeFd { fd, owned: true })
    }
}

/// Open a temporary fd via `/.vol/<dev>/<ino>` on macOS.
///
/// Tries `O_RDONLY | O_DIRECTORY` first (most callers need a parent directory fd),
/// then falls back to plain `O_RDONLY` for non-directory inodes, then
/// `O_SYMLINK` for symlink inodes (which O_NOFOLLOW/O_NOFOLLOW_ANY reject).
#[cfg(target_os = "macos")]
fn open_vol_fd(dev: u64, ino: u64) -> io::Result<i32> {
    let path = vol_path(dev, ino);

    // Try directory open first (most callers want a parent fd).
    if let Ok(fd) = open_macos_path_hardened(path.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY) {
        return Ok(fd);
    }

    // Fall back to regular open.
    if let Ok(fd) = open_macos_path_hardened(path.as_ptr(), libc::O_RDONLY) {
        return Ok(fd);
    }

    // Last resort: open symlinks without following via O_SYMLINK.
    if let Ok(fd) = open_macos_path_hardened(path.as_ptr(), libc::O_SYMLINK) {
        return Ok(fd);
    }

    Err(platform::linux_error(io::Error::last_os_error()))
}

/// Open a file for I/O by inode. Returns a real file descriptor (not O_PATH).
///
/// On Linux, uses `openat(proc_self_fd, "N", flags)` to reopen the tracked
/// procfd entry. Adding `O_NOFOLLOW` here would make every procfd reopen fail
/// with `ELOOP`, because `/proc/self/fd/N` is itself a symlink. Real host
/// symlinks are rejected before reopen so we never follow them through procfd.
pub(crate) fn open_inode_fd(fs: &PassthroughFs, inode: u64, flags: i32) -> io::Result<i32> {
    #[cfg(target_os = "linux")]
    {
        let inode_fd = get_inode_fd(fs, inode)?;
        let st = platform::fstat(inode_fd.raw())?;
        if st.st_mode & libc::S_IFMT == libc::S_IFLNK {
            return Err(platform::eloop());
        }
        let mut buf = [0u8; 20];
        let fd_str = format_fd_cstr(inode_fd.raw(), &mut buf);
        let reopen_flags = (flags & !libc::O_NOFOLLOW) | libc::O_CLOEXEC;
        let fd = unsafe { libc::openat(fs.proc_self_fd.as_raw_fd(), fd_str, reopen_flags) };
        if fd < 0 {
            return Err(platform::linux_error(io::Error::last_os_error()));
        }
        Ok(fd)
    }

    #[cfg(target_os = "macos")]
    {
        let inodes = fs.inodes.read().unwrap();
        let data = inodes.get(&inode).ok_or_else(platform::ebadf)?;

        // If the file was unlinked, dup the preserved fd instead of using /.vol/ path.
        let ufd = data.unlinked_fd.load(Ordering::Acquire);
        if ufd >= 0 {
            let fd = unsafe { libc::fcntl(ufd as i32, libc::F_DUPFD_CLOEXEC, 0) };
            if fd >= 0 {
                return Ok(fd);
            }
        }

        let path = vol_path(data.dev, data.ino);
        open_macos_path_hardened(path.as_ptr(), flags)
    }
}

/// Format a file descriptor number as a null-terminated C string into a stack buffer.
///
/// Avoids the heap allocation of `format!("/proc/self/fd/{fd}")` on the hot
/// reopen path. A 20-byte stack buffer is sufficient for any i32 fd number
/// plus null terminator.
#[cfg(target_os = "linux")]
fn format_fd_cstr(fd: i32, buf: &mut [u8; 20]) -> *const libc::c_char {
    use std::io::Write;
    let mut cursor = std::io::Cursor::new(&mut buf[..]);
    write!(cursor, "{}\0", fd).unwrap();
    buf.as_ptr() as *const libc::c_char
}

/// Stat an inode (raw stat data, no xattr patching).
pub(crate) fn stat_inode(
    fs: &PassthroughFs,
    inode: u64,
    handle: Option<u64>,
) -> io::Result<stat64> {
    use crate::backends::shared::init_binary;

    // Init binary has a synthetic stat (only when init is embedded).
    if init_binary::has_init() && inode == init_binary::INIT_INODE {
        return Ok(init_binary::init_stat());
    }

    // If a handle is provided, fstat the handle's fd directly.
    if let Some(h) = handle {
        let handles = fs.handles.read().unwrap();
        if let Some(hdata) = handles.get(&h) {
            let file = hdata.file.read().unwrap();
            return platform::fstat(file.as_raw_fd());
        }
    }

    #[cfg(target_os = "linux")]
    {
        let fd = get_inode_fd(fs, inode)?;
        platform::fstat(fd.raw())
    }

    #[cfg(target_os = "macos")]
    {
        let inodes = fs.inodes.read().unwrap();
        let data = inodes.get(&inode).ok_or_else(platform::ebadf)?;

        // Try unlinked_fd first -- /.vol/ path is invalid after unlink.
        let ufd = data.unlinked_fd.load(Ordering::Acquire);
        if ufd >= 0 {
            return platform::fstat(ufd as i32);
        }

        if let Ok(fd) = open_vol_fd(data.dev, data.ino) {
            let result = platform::fstat(fd);
            unsafe { libc::close(fd) };
            return result;
        }

        let path = vol_path(data.dev, data.ino);
        let mut st = unsafe { std::mem::zeroed::<stat64>() };
        let ret = unsafe { libc::lstat(path.as_ptr(), &mut st) };
        if ret < 0 {
            return Err(platform::linux_error(io::Error::last_os_error()));
        }
        Ok(st)
    }
}
