//! Embedded init binary bytes.
//!
//! When built with `--features embed-init` and the iii-init binary is available,
//! `INIT_BYTES` contains the full binary. Otherwise it is empty, and VMs
//! fall back to the default libkrun init.

/// The embedded init binary. Empty when `embed-init` feature is off or binary unavailable.
#[cfg(has_init_binary)]
pub const INIT_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/iii-init"));

#[cfg(not(has_init_binary))]
pub const INIT_BYTES: &[u8] = &[];
