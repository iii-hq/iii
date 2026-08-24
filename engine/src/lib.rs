// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

pub mod bin_resolve;
pub mod builtins;
pub mod condition;
pub mod config;
pub mod engine;
pub mod function;
pub mod invocation;
mod legacy_worker_functions;
pub mod logging;
pub mod protocol;
pub mod services;
pub mod telemetry;
pub mod trigger;
pub mod trigger_formats;
pub(crate) mod update_ops;
pub mod worker_connections;

pub mod workers {
    pub(crate) mod bridge;
    pub mod config;
    pub mod config_rewrite;
    pub mod configuration;
    pub mod engine_fn;
    pub mod external;
    pub mod http_functions;
    pub mod observability;
    /// Compatibility API for downstream custom queue adapters. The legacy
    /// `iii-queue` worker is no longer registered by the engine.
    pub mod queue;
    pub mod redis;
    pub mod registry;
    pub mod reload;
    pub mod secure_temp;
    pub mod stream;
    pub mod telemetry;
    pub mod traits;
    pub mod worker;
}

pub use workers::{config::EngineBuilder, queue::QueueAdapter};

// todo: create a prelude module for commonly used traits and types
