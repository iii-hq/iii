// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

pub mod bin_resolve;
pub mod builtins;
pub mod config;
pub(crate) mod deprecation;
pub mod engine;
pub mod event_formats;
pub mod function;
pub mod invocation;
pub mod logging;
pub mod memory;
mod missing_function_hints;
pub mod observer;
pub mod protocol;
pub mod services;
pub mod telemetry;
pub mod timestamps;
pub(crate) mod update_ops;
pub mod worker_connections;

pub mod workers {
    pub(crate) mod bridge;
    pub mod config;
    pub mod config_rewrite;
    pub mod configuration;
    pub mod engine_fn;
    pub mod http_functions;
    pub mod observability;
    pub mod redis;
    pub mod registry;
    pub mod reload;
    pub mod sandbox;
    pub mod stream;
    pub mod telemetry;
    pub mod traits;
    pub mod worker;
}

pub use workers::config::EngineBuilder;

// todo: create a prelude module for commonly used traits and types

// Exercise the actual CLI state and Windows ZIP reader without linking the
// product binary (these modules otherwise belong only to main.rs).
#[cfg(test)]
#[expect(
    dead_code,
    reason = "only the focused CLI modules are included in library tests"
)]
mod cli {
    mod download;
    mod error;
    mod github;
    mod platform;
    mod registry;
    mod state;
}
