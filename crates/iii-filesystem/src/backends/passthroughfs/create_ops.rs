//! Creation operations: create, mkdir, symlink, link.
//!
//! ## Creation Pattern (Simplified from microsandbox)
//!
//! All create-type operations follow: validate name -> host syscall -> do_lookup.
//! Unlike microsandbox, iii does not use xattr stat overrides (per D-02), so files
//! are created directly with the requested permissions instead of at 0o600 with
//! xattr-stored permissions.

use std::{
    ffi::CStr,
    io,
    os::fd::FromRawFd,
    sync::{atomic::Ordering, Arc, RwLock},
};

use super::{inode, PassthroughFs};
use crate::{
    backends::shared::{handle_table::HandleData, init_binary, name_validation, platform},
    Context, Entry, Extensions, OpenOptions,
};


//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Create and open a regular file.
///
/// Creates the file with the requested permissions directly (no xattr override per D-02).
/// Protects init.krun from being overwritten.
#[allow(clippy::too_many_arguments)]
pub(crate) fn do_create(
    fs: &PassthroughFs,
    _ctx: Context,
    parent: u64,
    name: &CStr,
    mode: u32,
    _kill_priv: bool,
    flags: u32,
    umask: u32,
    _extensions: Extensions,
) -> io::Result<(Entry, Option<u64>, OpenOptions)> {
    name_validation::validate_name(name)?;

    // Protect init.krun from being overwritten.
    if parent == 1 && init_binary::is_init_name(name.to_bytes()) {
        return Err(platform::eexist());
    }

    let parent_fd = inode::get_inode_fd(fs, parent)?;

    // Apply umask to get effective permissions.
    let file_mode = mode & !umask & 0o7777;

    let mut open_flags = inode::translate_open_flags(flags as i32);
    open_flags |= libc::O_CREAT | libc::O_CLOEXEC | libc::O_NOFOLLOW;

    // Create with the requested permissions directly (no xattr per D-02).
    let fd = unsafe {
        libc::openat(
            parent_fd.raw(),
            name.as_ptr(),
            open_flags,
            file_mode as libc::c_uint,
        )
    };
    if fd < 0 {
        return Err(platform::linux_error(io::Error::last_os_error()));
    }

    // Close the creation fd, then do a proper lookup.
    unsafe { libc::close(fd) };

    let entry = inode::do_lookup(fs, parent, name)?;

    // Reopen for the handle -- strip O_CREAT since the file already exists.
    // open_inode_fd adds O_CLOEXEC itself and rejects real host symlinks.
    let open_fd = inode::open_inode_fd(fs, entry.inode, open_flags & !libc::O_CREAT)?;
    let file = unsafe { std::fs::File::from_raw_fd(open_fd) };

    let handle = fs.next_handle.fetch_add(1, Ordering::Relaxed);
    let data = Arc::new(HandleData {
        file: RwLock::new(file),
    });
    fs.handles.write().unwrap().insert(handle, data);

    Ok((entry, Some(handle), fs.cache_open_options()))
}

/// Create a directory.
///
/// Creates with the requested permissions directly (no xattr per D-02).
/// Protects init.krun name from being used as a directory name.
pub(crate) fn do_mkdir(
    fs: &PassthroughFs,
    _ctx: Context,
    parent: u64,
    name: &CStr,
    mode: u32,
    umask: u32,
    _extensions: Extensions,
) -> io::Result<Entry> {
    name_validation::validate_name(name)?;

    // Protect init.krun from being used as a directory name.
    if parent == 1 && init_binary::is_init_name(name.to_bytes()) {
        return Err(platform::eexist());
    }

    let parent_fd = inode::get_inode_fd(fs, parent)?;
    let dir_mode = mode & !umask & 0o7777;

    let ret = unsafe { libc::mkdirat(parent_fd.raw(), name.as_ptr(), dir_mode as libc::mode_t) };
    if ret < 0 {
        return Err(platform::linux_error(io::Error::last_os_error()));
    }

    inode::do_lookup(fs, parent, name)
}

/// Create a symbolic link.
///
/// Creates a symlink `name` in `parent` pointing to `linkname`.
/// Protects init.krun from being used as a symlink name.
pub(crate) fn do_symlink(
    fs: &PassthroughFs,
    _ctx: Context,
    linkname: &CStr,
    parent: u64,
    name: &CStr,
    _extensions: Extensions,
) -> io::Result<Entry> {
    name_validation::validate_name(name)?;

    if parent == 1 && init_binary::is_init_name(name.to_bytes()) {
        return Err(platform::eexist());
    }

    let parent_fd = inode::get_inode_fd(fs, parent)?;

    let ret = unsafe { libc::symlinkat(linkname.as_ptr(), parent_fd.raw(), name.as_ptr()) };
    if ret < 0 {
        return Err(platform::linux_error(io::Error::last_os_error()));
    }

    inode::do_lookup(fs, parent, name)
}

/// Create a hard link.
///
/// Creates a new directory entry `newname` in `newparent` that points to the
/// same host inode as `inode`. Protects init.krun from being linked.
pub(crate) fn do_link(
    fs: &PassthroughFs,
    _ctx: Context,
    inode_num: u64,
    newparent: u64,
    newname: &CStr,
) -> io::Result<Entry> {
    name_validation::validate_name(newname)?;

    if inode_num == init_binary::INIT_INODE {
        return Err(platform::eperm());
    }
    if newparent == 1 && init_binary::is_init_name(newname.to_bytes()) {
        return Err(platform::eexist());
    }

    let newparent_fd = inode::get_inode_fd(fs, newparent)?;

    #[cfg(target_os = "linux")]
    {
        let mut buf = [0u8; 32];
        use std::io::Write;
        let mut cursor = std::io::Cursor::new(&mut buf[..]);
        write!(cursor, "/proc/self/fd/{}\0", inode_fd.raw()).unwrap();
        let path_ptr = buf.as_ptr() as *const libc::c_char;

        let ret = unsafe {
            libc::linkat(
                libc::AT_FDCWD,
                path_ptr,
                newparent_fd.raw(),
                newname.as_ptr(),
                libc::AT_SYMLINK_FOLLOW,
            )
        };
        if ret < 0 {
            return Err(platform::linux_error(io::Error::last_os_error()));
        }
    }

    #[cfg(target_os = "macos")]
    {
        let inodes = fs.inodes.read().unwrap();
        let data = inodes
            .get(&inode_num)
            .ok_or_else(platform::ebadf)?;
        let path = inode::vol_path(data.dev, data.ino);
        drop(inodes);

        let ret = unsafe {
            libc::linkat(
                libc::AT_FDCWD,
                path.as_ptr(),
                newparent_fd.raw(),
                newname.as_ptr(),
                0,
            )
        };
        if ret < 0 {
            return Err(platform::linux_error(io::Error::last_os_error()));
        }
    }

    inode::do_lookup(fs, newparent, newname)
}
