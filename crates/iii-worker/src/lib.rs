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

pub use cli::app::{AddArgs, Cli, Commands, DEFAULT_PORT};
pub use cli::vm_boot::VmBootArgs;
