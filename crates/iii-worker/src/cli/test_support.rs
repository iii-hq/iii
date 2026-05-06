// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Test-only helpers shared across `cli/` test modules.
//!
//! Anything in here is gated on `#[cfg(test)]` at the module
//! declaration site (`cli/mod.rs`) so it never ships in production
//! binaries.
//!
//! The central primitive is [`TEST_HOME_LOCK`], a process-wide mutex
//! that every test mutating or reading `HOME` (or `CWD`, which on
//! macOS is derived from `HOME` in several of our resolution paths)
//! must take before entering its body. Without it, `cargo test
//! -- --test-threads=N` lets one test override `HOME` to a long
//! `/var/folders/...` tempdir while another concurrently resolves
//! `dirs::home_dir()` — the concurrent reader then derives a path
//! that blows past macOS's 104-byte `SUN_LEN` limit for Unix socket
//! addresses and the test fails with a misleading bind error.
//!
//! Use [`lock_home`] rather than taking the mutex directly: it
//! transparently recovers from poisoned state (a prior test
//! panicking while holding the lock leaves the mutex poisoned; that
//! is not a correctness problem for our callers, since the lock
//! protects an `()` and is only there to serialize env mutations).
//!
//! Any new test that reads or mutates `HOME` must call `lock_home()`
//! for the whole body — missing the call reintroduces the race.

/// Process-global lock serializing tests that read or mutate
/// `HOME` / `CWD`. See the module docstring for the full rationale.
pub(crate) static TEST_HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquire [`TEST_HOME_LOCK`], transparently recovering from a
/// poisoned mutex. Tests should bind the returned guard to `_g` for
/// the whole body so the lock is released on scope exit.
pub(crate) fn lock_home() -> std::sync::MutexGuard<'static, ()> {
    TEST_HOME_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
