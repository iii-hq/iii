// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! `sandbox::fs::*` worker triggers — ten operations exposed as III functions.
//!
//! Each operation is implemented in its own sub-module following the same
//! pattern as `sandbox_daemon::exec`: typed request/response structs, a
//! pure `handle_*` function testable with a `FakeRunner`, and a
//! `register(iii, registry, runner)` function wired up by `register_all`.

pub mod adapter;
pub mod chmod;
pub mod grep;
pub mod ls;
pub mod mkdir;
pub mod mv;
pub mod read;
pub mod rm;
pub mod sed;
pub mod stat;
pub mod write;

pub use adapter::{FsRunner, IiiShellFsRunner};

use std::sync::Arc;

use crate::sandbox_daemon::registry::SandboxRegistry;

/// Register all ten `sandbox::fs::*` triggers with the III engine.
pub fn register_all(iii: &iii_sdk::III, registry: Arc<SandboxRegistry>, runner: Arc<dyn FsRunner>) {
    ls::register(iii, registry.clone(), runner.clone());
    stat::register(iii, registry.clone(), runner.clone());
    mkdir::register(iii, registry.clone(), runner.clone());
    rm::register(iii, registry.clone(), runner.clone());
    chmod::register(iii, registry.clone(), runner.clone());
    mv::register(iii, registry.clone(), runner.clone());
    grep::register(iii, registry.clone(), runner.clone());
    sed::register(iii, registry.clone(), runner.clone());
    write::register(iii, registry.clone(), runner.clone());
    read::register(iii, registry.clone(), runner.clone());
}
