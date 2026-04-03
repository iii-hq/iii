//! Builder API for constructing a PassthroughFs instance.
//!
//! ```ignore
//! PassthroughFs::builder()
//!     .root_dir("./rootfs")
//!     .entry_timeout(Duration::from_secs(5))
//!     .build()?
//! ```

use std::{
    collections::BTreeMap,
    fs::File,
    io,
    os::fd::FromRawFd,
    path::PathBuf,
    sync::{
        RwLock,
        atomic::{AtomicBool, AtomicU64},
    },
    time::Duration,
};

use super::{CachePolicy, PassthroughFs};
use crate::backends::shared::{init_binary, inode_table::MultikeyBTreeMap, platform};

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Builder for constructing a [`PassthroughFs`] instance.
pub struct PassthroughFsBuilder {
    root_dir: Option<PathBuf>,
    entry_timeout: Duration,
    attr_timeout: Duration,
    cache_policy: CachePolicy,
    writeback: bool,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl PassthroughFsBuilder {
    /// Create a new builder with default settings.
    pub(crate) fn new() -> Self {
        Self {
            root_dir: None,
            entry_timeout: Duration::from_secs(5),
            attr_timeout: Duration::from_secs(5),
            cache_policy: CachePolicy::Auto,
            writeback: false,
        }
    }

    /// Set the host directory to expose.
    pub fn root_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.root_dir = Some(path.into());
        self
    }

    /// Set the FUSE entry cache timeout.
    pub fn entry_timeout(mut self, timeout: Duration) -> Self {
        self.entry_timeout = timeout;
        self
    }

    /// Set the FUSE attribute cache timeout.
    pub fn attr_timeout(mut self, timeout: Duration) -> Self {
        self.attr_timeout = timeout;
        self
    }

    /// Set the cache policy.
    pub fn cache_policy(mut self, policy: CachePolicy) -> Self {
        self.cache_policy = policy;
        self
    }

    /// Enable or disable writeback caching.
    pub fn writeback(mut self, enabled: bool) -> Self {
        self.writeback = enabled;
        self
    }

    /// Build the PassthroughFs instance.
    pub fn build(self) -> io::Result<PassthroughFs> {
        let root_dir = self
            .root_dir
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "root_dir not set"))?;

        // Open the root directory.
        let root_path =
            std::ffi::CString::new(root_dir.to_str().ok_or_else(platform::einval)?.as_bytes())
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

        let cfg = super::PassthroughConfig {
            root_dir,
            entry_timeout: self.entry_timeout,
            attr_timeout: self.attr_timeout,
            cache_policy: self.cache_policy,
            writeback: self.writeback,
        };

        // When init is embedded: inode 2 = init, handle 0 = init handle.
        // When init is NOT embedded: inode 2 and handle 0 are available for real files.
        let (start_inode, start_handle) = if init_binary::has_init() {
            (3u64, 1u64) // 1=root, 2=init (reserved)
        } else {
            (2u64, 0u64) // 1=root, no reserved init inode
        };

        Ok(PassthroughFs {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_without_root_dir_returns_error() {
        let result = PassthroughFsBuilder::new().build();
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("root_dir not set"));
    }

    #[test]
    fn build_with_nonexistent_root_dir_returns_error() {
        let result = PassthroughFsBuilder::new()
            .root_dir("/nonexistent_path_that_should_not_exist_12345")
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn build_with_valid_root_dir_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let result = PassthroughFsBuilder::new().root_dir(dir.path()).build();
        assert!(result.is_ok());
    }

    #[test]
    fn builder_default_timeouts() {
        let builder = PassthroughFsBuilder::new();
        assert_eq!(builder.entry_timeout, Duration::from_secs(5));
        assert_eq!(builder.attr_timeout, Duration::from_secs(5));
    }

    #[test]
    fn builder_custom_timeouts() {
        let builder = PassthroughFsBuilder::new()
            .entry_timeout(Duration::from_secs(10))
            .attr_timeout(Duration::from_secs(20));
        assert_eq!(builder.entry_timeout, Duration::from_secs(10));
        assert_eq!(builder.attr_timeout, Duration::from_secs(20));
    }

    #[test]
    fn builder_cache_policy() {
        let builder = PassthroughFsBuilder::new().cache_policy(CachePolicy::Always);
        assert_eq!(builder.cache_policy, CachePolicy::Always);
    }

    #[test]
    fn builder_writeback() {
        let builder = PassthroughFsBuilder::new().writeback(true);
        assert!(builder.writeback);
    }

    #[test]
    fn builder_default_cache_policy_is_auto() {
        let builder = PassthroughFsBuilder::new();
        assert_eq!(builder.cache_policy, CachePolicy::Auto);
    }

    #[test]
    fn builder_default_writeback_is_false() {
        let builder = PassthroughFsBuilder::new();
        assert!(!builder.writeback);
    }
}
