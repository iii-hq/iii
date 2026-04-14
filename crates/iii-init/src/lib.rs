//! Library facade for `iii-init`, the PID 1 init binary for iii microVM workers.
//!
//! All modules are Linux-only since the init binary runs inside a Linux microVM.
//! This library target exists so integration tests can import and verify real
//! crate types and functions instead of reimplementing them.

#[cfg(target_os = "linux")]
pub mod error;
#[cfg(target_os = "linux")]
pub mod mount;

// Pure parsers — compiled on every platform so they can be unit-tested
// on the build host (iii-init itself is a Linux-guest binary).
#[cfg(target_os = "linux")]
pub mod network;
pub mod parse;
#[cfg(target_os = "linux")]
pub mod rlimit;
#[cfg(target_os = "linux")]
pub mod supervisor;

#[cfg(target_os = "linux")]
pub use error::InitError;
