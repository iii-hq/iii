//! Firmware management for libkrunfw.
//!
//! The VMM (msb_krun) is compiled directly into the iii binary.
//! libkrunfw (the guest kernel firmware) is embedded at compile time
//! via the `embed-libkrunfw` feature and extracted to `~/.iii/lib/`
//! on first use. Runtime resolution also checks system install paths
//! and environment variables.

pub mod constants;
pub mod download;
pub mod libkrunfw_bytes;
pub mod resolve;
pub mod symlinks;
