//! Library facade for `iii-init`, the PID 1 init binary for iii microVM workers.
//!
//! All modules are Linux-only since the init binary runs inside a Linux microVM.
//! This library target exists so integration tests can import and verify real
//! crate types and functions instead of reimplementing them.

#[cfg(target_os = "linux")]
pub mod error;
#[cfg(target_os = "linux")]
pub mod mount;
#[cfg(target_os = "linux")]
pub mod root_pivot;

// Pure parsers — compiled on every platform so they can be unit-tested
// on the build host (iii-init itself is a Linux-guest binary).
#[cfg(target_os = "linux")]
pub mod network;
pub mod parse;
#[cfg(target_os = "linux")]
pub mod rlimit;
// Platform-agnostic: just an in-memory registry plus channels, no
// Linux syscalls. Keeping it unconditional makes unit tests on
// macOS developer machines work without cross-compiling.
pub mod child_exits;
#[cfg(target_os = "linux")]
pub mod shell_dispatcher;
#[cfg(target_os = "linux")]
pub mod supervisor;
// Filesystem operation handlers. Linux-only because the module calls
// std::os::unix::fs APIs and references shell_dispatcher internals.
// Keeping it behind cfg(linux) also keeps macOS unit test builds clean.
#[cfg(target_os = "linux")]
pub mod fs_handler;

#[cfg(target_os = "linux")]
pub use error::InitError;
