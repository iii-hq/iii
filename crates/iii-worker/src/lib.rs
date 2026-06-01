// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Library facade for `iii-worker`, the iii managed worker runtime.
//!
//! Exposes CLI types so integration tests can verify the real argument
//! definitions instead of maintaining duplicate struct copies.

pub mod cli;
pub mod core;
pub mod sandbox_daemon;

pub use cli::app::{
    AddArgs, Cli, Commands, DEFAULT_PORT, ExecArgs, SandboxDaemonArgs, WatchSourceArgs,
};
pub use cli::vm_boot::VmBootArgs;

// Re-export under the old name so HOME-mutating tests serialize against
// the SAME mutex HOME-reading tests use. Two separate mutexes caused
// intermittent pidfile-path mismatches.
#[cfg(test)]
pub(crate) use crate::cli::test_support::TEST_HOME_LOCK as TEST_ENV_LOCK;
