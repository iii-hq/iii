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
pub mod network;
#[cfg(target_os = "linux")]
pub mod rlimit;
#[cfg(target_os = "linux")]
pub mod supervisor;

#[cfg(target_os = "linux")]
pub use error::InitError;
