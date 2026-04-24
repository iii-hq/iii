// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Library facade for `iii-worker`, the iii managed worker runtime.
//!
//! Exposes CLI types so integration tests can verify the real argument
//! definitions instead of maintaining duplicate struct copies.

pub mod cli;

pub use cli::app::{AddArgs, Cli, Commands, DEFAULT_PORT, ExecArgs, WatchSourceArgs};
pub use cli::vm_boot::VmBootArgs;

// Test-time env/HOME/CWD serialization: unified with `cli::test_support::TEST_HOME_LOCK`
// so every HOME-mutating test (`TEST_ENV_LOCK` callers) is mutually exclusive with
// every HOME-reading test (`test_support::lock_home()` callers). Keeping two distinct
// mutexes silently let HOME get rewritten under tests that only read it via
// `dirs::home_dir()`, producing intermittent pidfile-path mismatches.
#[cfg(test)]
pub(crate) use crate::cli::test_support::TEST_HOME_LOCK as TEST_ENV_LOCK;
