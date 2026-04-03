//! Firmware and init binary management for iii-worker.
//!
//! The VMM (msb_krun) is compiled directly into the iii-worker binary.
//! libkrunfw (the guest kernel firmware) and iii-init (the guest init process)
//! are resolved via a three-stage chain:
//!
//! 1. **Local resolution** -- env var, `~/.iii/lib/`, adjacent to binary, system paths
//! 2. **Embedded bytes** -- compile-time features (`embed-libkrunfw`, `embed-init`)
//! 3. **Runtime download** -- fetched from GitHub release assets on first use
//!
//! For CI/release builds, both are embedded via `--features embed-init,embed-libkrunfw`.
//! For local development, `cargo build -p iii-worker --release` works without any
//! features -- the binaries are downloaded automatically on first use.

pub mod constants;
pub mod download;
pub mod libkrunfw_bytes;
pub mod resolve;
pub mod symlinks;
