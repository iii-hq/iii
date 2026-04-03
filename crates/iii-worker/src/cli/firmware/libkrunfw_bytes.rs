//! Embedded libkrunfw library bytes.
//!
//! When built with `--features embed-libkrunfw` and the pre-built firmware
//! library is available for the target platform, `LIBKRUNFW_BYTES` contains
//! the full dylib/so. Otherwise it is empty, and the runtime falls back to
//! system-installed libkrunfw.

#[cfg(has_libkrunfw)]
pub const LIBKRUNFW_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/libkrunfw"));

#[cfg(not(has_libkrunfw))]
pub const LIBKRUNFW_BYTES: &[u8] = &[];
