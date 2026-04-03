//! Passthrough filesystem backend.
//!
//! Exposes a single host directory to the guest VM via virtio-fs, with
//! init.krun injection and name validation.

pub(crate) mod builder;
mod create_ops;
mod dir_ops;
mod file_ops;
pub(crate) mod inode;
mod metadata;
mod remove_ops;
mod special;

use std::{
    collections::BTreeMap,
    ffi::CStr,
    fs::File,
    io,
    os::fd::{AsRawFd, FromRawFd},
    path::PathBuf,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use crate::{
    Context, DirEntry, DynFileSystem, Entry, Extensions, FsOptions, OpenOptions, SetattrValid,
    ZeroCopyReader, ZeroCopyWriter,
    backends::shared::{
        handle_table::HandleData,
        init_binary,
        inode_table::{InodeAltKey, InodeData, MultikeyBTreeMap},
        platform,
    },
    stat64, statvfs64,
};

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Cache policy for the passthrough filesystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePolicy {
    /// Never cache -- every access goes to the host filesystem.
    Never,
    /// Let the kernel decide (default).
    Auto,
    /// Aggressively cache -- assume the host filesystem is static.
    Always,
}

/// Configuration for the passthrough filesystem backend.
#[derive(Debug, Clone)]
pub struct PassthroughConfig {
    /// Path to the root directory on the host.
    pub root_dir: PathBuf,

    /// FUSE entry cache timeout.
    pub entry_timeout: Duration,

    /// FUSE attribute cache timeout.
    pub attr_timeout: Duration,

    /// Cache policy.
    pub cache_policy: CachePolicy,

    /// Whether to enable writeback caching.
    pub writeback: bool,
}

/// Passthrough filesystem backend.
///
/// Implements [`DynFileSystem`] by mapping guest filesystem operations to
/// the host filesystem, with init binary injection at inode 2.
pub struct PassthroughFs {
    /// Configuration.
    pub(crate) cfg: PassthroughConfig,

    /// Open file descriptor for the root directory.
    pub(crate) root_fd: File,

    /// Inode table with dual-key lookup (FUSE inode + host identity).
    pub(crate) inodes: RwLock<MultikeyBTreeMap<u64, InodeAltKey, Arc<InodeData>>>,

    /// Next FUSE inode number to allocate (starts at 3, after root=1 and init=2).
    pub(crate) next_inode: AtomicU64,

    /// Open file handle table.
    pub(crate) handles: RwLock<BTreeMap<u64, Arc<HandleData>>>,

    /// Next file handle number to allocate (starts at 1, after init_handle=0).
    pub(crate) next_handle: AtomicU64,

    /// Whether writeback caching is negotiated.
    pub(crate) writeback: AtomicBool,

    /// File containing the init binary bytes (memfd on Linux, tmpfile on macOS).
    pub(crate) init_file: File,

    /// Whether `openat2` with `RESOLVE_BENEATH` is available (Linux 5.6+).
    #[cfg(target_os = "linux")]
    pub(crate) has_openat2: AtomicBool,

    /// Open fd to /proc/self/fd (Linux only).
    ///
    /// Used by `open_inode_fd` to reopen tracked inodes via procfd handles
    /// after first rejecting real host symlinks on the pinned inode.
    #[cfg(target_os = "linux")]
    pub(crate) proc_self_fd: File,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl PassthroughFs {
    /// Create a builder for constructing a `PassthroughFs` instance.
    pub fn builder() -> builder::PassthroughFsBuilder {
        builder::PassthroughFsBuilder::new()
    }

    /// Create a new passthrough filesystem backend.
    ///
    /// Opens the root directory and prepares init binary injection.
    pub fn new(cfg: PassthroughConfig) -> io::Result<Self> {
        // Open the root directory.
        let root_path = std::ffi::CString::new(
            cfg.root_dir
                .to_str()
                .ok_or_else(platform::einval)?
                .as_bytes(),
        )
        .map_err(|_| platform::einval())?;

        let root_fd_raw = unsafe {
            libc::open(
                root_path.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY,
            )
        };
        if root_fd_raw < 0 {
            return Err(platform::linux_error(io::Error::last_os_error()));
        }
        let root_fd = unsafe { File::from_raw_fd(root_fd_raw) };

        // Create the init binary file.
        let init_file = init_binary::create_init_file()?;

        // Probe openat2 / RESOLVE_BENEATH availability (Linux 5.6+).
        #[cfg(target_os = "linux")]
        let has_openat2 = AtomicBool::new(platform::probe_openat2());

        // Open /proc/self/fd on Linux for efficient path resolution.
        #[cfg(target_os = "linux")]
        let proc_self_fd = {
            let path = std::ffi::CString::new("/proc/self/fd").unwrap();
            let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
            if fd < 0 {
                return Err(platform::linux_error(io::Error::last_os_error()));
            }
            unsafe { File::from_raw_fd(fd) }
        };

        let (start_inode, start_handle) = if init_binary::has_init() {
            (3u64, 1u64)
        } else {
            (2u64, 0u64)
        };

        Ok(Self {
            cfg,
            root_fd,
            inodes: RwLock::new(MultikeyBTreeMap::new()),
            next_inode: AtomicU64::new(start_inode),
            handles: RwLock::new(BTreeMap::new()),
            next_handle: AtomicU64::new(start_handle),
            writeback: AtomicBool::new(false),
            init_file,
            #[cfg(target_os = "linux")]
            has_openat2,
            #[cfg(target_os = "linux")]
            proc_self_fd,
        })
    }
}

impl PassthroughFs {
    /// Register root inode (inode 1) in the inode table.
    ///
    /// Called during `init()`. The guest kernel sends GETATTR on the root inode
    /// immediately after FUSE_INIT, so the root must be in the table before any
    /// other FUSE operations are processed.
    fn register_root_inode(&self) -> io::Result<()> {
        let root_fd = self.root_fd.as_raw_fd();

        #[cfg(target_os = "linux")]
        let (st, mnt_id) = {
            let mut stx: libc::statx = unsafe { std::mem::zeroed() };
            let ret = unsafe {
                libc::statx(
                    root_fd,
                    c"".as_ptr(),
                    libc::AT_EMPTY_PATH | libc::AT_SYMLINK_NOFOLLOW | libc::AT_STATX_SYNC_AS_STAT,
                    libc::STATX_BASIC_STATS | libc::STATX_MNT_ID,
                    &mut stx,
                )
            };
            if ret < 0 {
                return Err(platform::linux_error(io::Error::last_os_error()));
            }
            (platform::statx_to_stat64(&stx), stx.stx_mnt_id)
        };

        #[cfg(target_os = "macos")]
        let st = platform::fstat(root_fd)?;

        #[cfg(target_os = "linux")]
        let alt_key = InodeAltKey::new(st.st_ino, st.st_dev, mnt_id);

        #[cfg(target_os = "macos")]
        let alt_key = InodeAltKey::new(platform::stat_ino(&st), platform::stat_dev(&st));

        let data = Arc::new(InodeData {
            inode: 1, // ROOT_ID
            ino: platform::stat_ino(&st),
            dev: platform::stat_dev(&st),
            refcount: AtomicU64::new(2), // libfuse convention: root gets refcount 2
            #[cfg(target_os = "linux")]
            file: {
                // Dup the root fd so InodeData owns its own copy.
                let fd = unsafe { libc::fcntl(root_fd, libc::F_DUPFD_CLOEXEC, 0) };
                if fd < 0 {
                    return Err(platform::linux_error(io::Error::last_os_error()));
                }
                unsafe { std::fs::File::from_raw_fd(fd) }
            },
            #[cfg(target_os = "linux")]
            mnt_id,
            #[cfg(target_os = "macos")]
            unlinked_fd: std::sync::atomic::AtomicI64::new(-1),
        });

        let mut inodes = self.inodes.write().unwrap();
        inodes.insert(1, alt_key, data);

        Ok(())
    }

    /// Get the `OpenOptions` for file opens based on cache policy.
    pub(crate) fn cache_open_options(&self) -> OpenOptions {
        match self.cfg.cache_policy {
            CachePolicy::Never => OpenOptions::DIRECT_IO,
            CachePolicy::Auto => OpenOptions::empty(),
            CachePolicy::Always => OpenOptions::KEEP_CACHE,
        }
    }

    /// Get the `OpenOptions` for directory opens based on cache policy.
    pub(crate) fn cache_dir_options(&self) -> OpenOptions {
        match self.cfg.cache_policy {
            CachePolicy::Never => OpenOptions::DIRECT_IO,
            CachePolicy::Auto => OpenOptions::empty(),
            CachePolicy::Always => OpenOptions::CACHE_DIR,
        }
    }
}

impl Default for PassthroughConfig {
    fn default() -> Self {
        Self {
            root_dir: PathBuf::new(),
            entry_timeout: Duration::from_secs(5),
            attr_timeout: Duration::from_secs(5),
            cache_policy: CachePolicy::Auto,
            writeback: false,
        }
    }
}

//--------------------------------------------------------------------------------------------------
// Trait Implementations
//--------------------------------------------------------------------------------------------------

impl DynFileSystem for PassthroughFs {
    fn init(&self, capable: FsOptions) -> io::Result<FsOptions> {
        // Register root inode (inode 1) in the inode table.
        // The guest kernel issues GETATTR on the root inode immediately after FUSE_INIT.
        // Without this entry, stat_inode(1) fails and the guest cannot resolve any paths.
        self.register_root_inode()?;

        let mut opts = FsOptions::empty();

        // DONT_MASK: we handle umask ourselves in create/mkdir/mknod.
        if capable.contains(FsOptions::DONT_MASK) {
            opts |= FsOptions::DONT_MASK;
        }
        if capable.contains(FsOptions::BIG_WRITES) {
            opts |= FsOptions::BIG_WRITES;
        }
        if capable.contains(FsOptions::ASYNC_READ) {
            opts |= FsOptions::ASYNC_READ;
        }
        if capable.contains(FsOptions::PARALLEL_DIROPS) {
            opts |= FsOptions::PARALLEL_DIROPS;
        }
        if capable.contains(FsOptions::MAX_PAGES) {
            opts |= FsOptions::MAX_PAGES;
        }
        if capable.contains(FsOptions::HANDLE_KILLPRIV_V2) {
            opts |= FsOptions::HANDLE_KILLPRIV_V2;
        }
        // READDIRPLUS_AUTO: let kernel decide when to use readdirplus vs plain readdir.
        // readdirplus returns attrs with entries, saving per-entry getattr calls.
        if capable.contains(FsOptions::DO_READDIRPLUS) {
            opts |= FsOptions::DO_READDIRPLUS | FsOptions::READDIRPLUS_AUTO;
        }

        // Enable writeback cache if requested and supported.
        if self.cfg.writeback && capable.contains(FsOptions::WRITEBACK_CACHE) {
            opts |= FsOptions::WRITEBACK_CACHE;
            self.writeback.store(true, Ordering::Relaxed);
        }

        // Clear umask so the client can set all mode bits.
        unsafe { libc::umask(0o000) };

        Ok(opts)
    }

    fn destroy(&self) {
        self.handles.write().unwrap().clear();
        self.inodes.write().unwrap().clear();
    }

    fn lookup(&self, _ctx: Context, parent: u64, name: &CStr) -> io::Result<Entry> {
        // Handle init.krun lookup in root directory (only when init is embedded).
        if init_binary::has_init() && parent == 1 && init_binary::is_init_name(name.to_bytes()) {
            return Ok(init_binary::init_entry(
                self.cfg.entry_timeout,
                self.cfg.attr_timeout,
            ));
        }
        inode::do_lookup(self, parent, name)
    }

    fn forget(&self, _ctx: Context, ino: u64, count: u64) {
        if init_binary::has_init() && ino == init_binary::INIT_INODE {
            return;
        }
        inode::forget_one(self, ino, count);
    }

    fn batch_forget(&self, _ctx: Context, requests: Vec<(u64, u64)>) {
        // Single lock acquisition for all entries (O(1) instead of O(n) locks).
        // batch_forget is called with hundreds of entries after directory traversals.
        let mut inodes = self.inodes.write().unwrap();
        for (ino, count) in requests {
            if init_binary::has_init() && ino == init_binary::INIT_INODE {
                continue;
            }
            inode::forget_one_locked(&mut inodes, ino, count);
        }
    }

    fn getattr(
        &self,
        ctx: Context,
        ino: u64,
        handle: Option<u64>,
    ) -> io::Result<(stat64, Duration)> {
        metadata::do_getattr(self, ctx, ino, handle)
    }

    fn setattr(
        &self,
        ctx: Context,
        ino: u64,
        attr: stat64,
        handle: Option<u64>,
        valid: SetattrValid,
    ) -> io::Result<(stat64, Duration)> {
        metadata::do_setattr(self, ctx, ino, attr, handle, valid)
    }

    fn mkdir(
        &self,
        ctx: Context,
        parent: u64,
        name: &CStr,
        mode: u32,
        umask: u32,
        extensions: Extensions,
    ) -> io::Result<Entry> {
        create_ops::do_mkdir(self, ctx, parent, name, mode, umask, extensions)
    }

    fn unlink(&self, ctx: Context, parent: u64, name: &CStr) -> io::Result<()> {
        remove_ops::do_unlink(self, ctx, parent, name)
    }

    fn rmdir(&self, ctx: Context, parent: u64, name: &CStr) -> io::Result<()> {
        remove_ops::do_rmdir(self, ctx, parent, name)
    }

    fn rename(
        &self,
        ctx: Context,
        olddir: u64,
        oldname: &CStr,
        newdir: u64,
        newname: &CStr,
        flags: u32,
    ) -> io::Result<()> {
        remove_ops::do_rename(self, ctx, olddir, oldname, newdir, newname, flags)
    }

    fn open(
        &self,
        ctx: Context,
        ino: u64,
        kill_priv: bool,
        flags: u32,
    ) -> io::Result<(Option<u64>, OpenOptions)> {
        file_ops::do_open(self, ctx, ino, kill_priv, flags)
    }

    #[allow(clippy::too_many_arguments)]
    fn create(
        &self,
        ctx: Context,
        parent: u64,
        name: &CStr,
        mode: u32,
        kill_priv: bool,
        flags: u32,
        umask: u32,
        extensions: Extensions,
    ) -> io::Result<(Entry, Option<u64>, OpenOptions)> {
        create_ops::do_create(
            self, ctx, parent, name, mode, kill_priv, flags, umask, extensions,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn read(
        &self,
        ctx: Context,
        ino: u64,
        handle: u64,
        w: &mut dyn ZeroCopyWriter,
        size: u32,
        offset: u64,
        _lock_owner: Option<u64>,
        _flags: u32,
    ) -> io::Result<usize> {
        file_ops::do_read(self, ctx, ino, handle, w, size, offset)
    }

    #[allow(clippy::too_many_arguments)]
    fn write(
        &self,
        ctx: Context,
        ino: u64,
        handle: u64,
        r: &mut dyn ZeroCopyReader,
        size: u32,
        offset: u64,
        _lock_owner: Option<u64>,
        _delayed_write: bool,
        kill_priv: bool,
        _flags: u32,
    ) -> io::Result<usize> {
        file_ops::do_write(self, ctx, ino, handle, r, size, offset, kill_priv)
    }

    fn flush(&self, ctx: Context, ino: u64, handle: u64, _lock_owner: u64) -> io::Result<()> {
        file_ops::do_flush(self, ctx, ino, handle)
    }

    fn fsync(&self, ctx: Context, ino: u64, datasync: bool, handle: u64) -> io::Result<()> {
        special::do_fsync(self, ctx, ino, datasync, handle)
    }

    #[allow(clippy::too_many_arguments)]
    fn release(
        &self,
        ctx: Context,
        ino: u64,
        _flags: u32,
        handle: u64,
        _flush: bool,
        _flock_release: bool,
        _lock_owner: Option<u64>,
    ) -> io::Result<()> {
        file_ops::do_release(self, ctx, ino, handle)
    }

    fn statfs(&self, ctx: Context, ino: u64) -> io::Result<statvfs64> {
        special::do_statfs(self, ctx, ino)
    }

    fn opendir(
        &self,
        ctx: Context,
        ino: u64,
        flags: u32,
    ) -> io::Result<(Option<u64>, OpenOptions)> {
        dir_ops::do_opendir(self, ctx, ino, flags)
    }

    fn readdir(
        &self,
        ctx: Context,
        ino: u64,
        handle: u64,
        size: u32,
        offset: u64,
    ) -> io::Result<Vec<DirEntry<'static>>> {
        dir_ops::do_readdir(self, ctx, ino, handle, size, offset)
    }

    fn readdirplus(
        &self,
        ctx: Context,
        ino: u64,
        handle: u64,
        size: u32,
        offset: u64,
    ) -> io::Result<Vec<(DirEntry<'static>, Entry)>> {
        dir_ops::do_readdirplus(self, ctx, ino, handle, size, offset)
    }

    fn fsyncdir(&self, ctx: Context, ino: u64, datasync: bool, handle: u64) -> io::Result<()> {
        special::do_fsyncdir(self, ctx, ino, datasync, handle)
    }

    fn releasedir(&self, ctx: Context, ino: u64, flags: u32, handle: u64) -> io::Result<()> {
        dir_ops::do_releasedir(self, ctx, ino, flags, handle)
    }

    fn access(&self, ctx: Context, ino: u64, mask: u32) -> io::Result<()> {
        metadata::do_access(self, ctx, ino, mask)
    }

    fn readlink(&self, _ctx: Context, ino: u64) -> io::Result<Vec<u8>> {
        if init_binary::has_init() && ino == init_binary::INIT_INODE {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }

        #[cfg(target_os = "linux")]
        {
            let inode_fd = inode::get_inode_fd(self, ino)?;
            platform::readlink_fd(inode_fd.raw())
        }

        #[cfg(target_os = "macos")]
        {
            let inodes = self.inodes.read().unwrap();
            let data = inodes.get(&ino).ok_or_else(platform::ebadf)?;
            let path = inode::vol_path(data.dev, data.ino);
            drop(inodes);
            let mut buf = vec![0u8; libc::PATH_MAX as usize];
            let len = unsafe {
                libc::readlink(
                    path.as_ptr(),
                    buf.as_mut_ptr() as *mut libc::c_char,
                    buf.len(),
                )
            };
            if len < 0 {
                Err(platform::linux_error(io::Error::last_os_error()))
            } else {
                buf.truncate(len as usize);
                Ok(buf)
            }
        }
    }

    fn symlink(
        &self,
        ctx: Context,
        linkname: &CStr,
        parent: u64,
        name: &CStr,
        extensions: Extensions,
    ) -> io::Result<Entry> {
        create_ops::do_symlink(self, ctx, linkname, parent, name, extensions)
    }

    fn link(&self, ctx: Context, inode: u64, newparent: u64, newname: &CStr) -> io::Result<Entry> {
        create_ops::do_link(self, ctx, inode, newparent, newname)
    }

    // Skipped in v1 (D-11): mknod, fallocate, lseek,
    // xattr ops, copyfilerange -- all use the default ENOSYS from the trait.
}

//--------------------------------------------------------------------------------------------------
// Re-Exports
//--------------------------------------------------------------------------------------------------

pub use builder::PassthroughFsBuilder;
