// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Filesystem-operations dispatcher for the in-VM supervisor.
//!
//! Runs native Rust handlers against the guest rootfs — no child
//! processes, no shell-outs. Called from `shell_dispatcher.rs` when a
//! `ShellMessage::FsRequest` frame arrives on a fresh session.
//!
//! Write and read streams are driven by follow-up `FsChunk` / `FsEnd`
//! frames on the same correlation id; the dispatcher hands us the
//! per-session inbound channel so we can pull them synchronously via
//! `std::sync::mpsc::Receiver`.
//!
//! **Sync-only invariant.** This module contains no `tokio`, no `async
//! fn`, no `AsyncRead`/`AsyncWrite`. All I/O uses `std::fs` and
//! `std::io`. This matches the broader `iii-init` invariant and keeps
//! the binary cross-compilable to linux-musl without a tokio runtime.

pub mod ops;
pub mod streaming;

#[cfg(test)]
mod tests;

use iii_shell_proto::FsResult;

/// One-shot ops reply with `Ok(FsResult)` or `Err(FsError)`.
pub type FsCallResult = Result<FsResult, FsError>;

/// Error payload returned by each handler. `code` is the S21x wire
/// string, matching `SandboxError::code().as_str()` on the worker
/// side so the trigger response serializes identically.
#[derive(Debug, Clone)]
pub struct FsError {
    pub code: &'static str,
    pub message: String,
}

impl FsError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Translate a `std::io::Error` to the best-fitting S21x code.
    /// Mirrors `SandboxError::from_io` on the worker side.
    pub fn from_io(path: &str, err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::NotFound => Self::new("S211", format!("path not found: {path}")),
            std::io::ErrorKind::AlreadyExists => {
                Self::new("S213", format!("path already exists: {path}"))
            }
            std::io::ErrorKind::PermissionDenied => Self::new("S215", format!("{path}: {err}")),
            _ => Self::new("S216", format!("{path}: {err}")),
        }
    }
}

/// Parse an octal mode string like `"0755"` into a raw `u32` permission
/// bit set. Returns `Err(FsError { code: "S210", .. })` if the string
/// is not valid octal.
pub fn parse_mode(mode: &str) -> Result<u32, FsError> {
    // Strip leading "0" prefix (e.g. "0755" -> "755"), but keep "0" as "0".
    let stripped = mode.trim_start_matches('0');
    let s = if stripped.is_empty() { "0" } else { stripped };
    u32::from_str_radix(s, 8)
        .map_err(|_| FsError::new("S210", format!("invalid octal mode: {mode}")))
}
