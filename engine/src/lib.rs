// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

pub mod builtins;
pub mod condition;
pub mod config;
pub mod engine;
pub mod function;
pub mod invocation;
pub mod logging;
pub mod protocol;
pub mod services;
pub mod telemetry;
pub mod trigger;
pub mod trigger_formats;
pub(crate) mod update_ops;
pub mod worker_connections;

pub mod workers {
    pub mod bridge_client;
    pub mod config;
    pub mod cron;
    pub mod engine_fn;
    pub mod external;
    pub mod http_functions;
    pub mod observability;
    pub mod pubsub;
    pub mod queue;
    pub mod redis;
    pub mod registry;
    pub mod registry_worker;
    pub mod reload;
    pub mod rest_api;
    pub mod secure_temp;
    pub mod shell;
    pub mod state;
    pub mod stream;
    pub mod telemetry;
    pub mod traits;
    pub mod worker;
}

pub use workers::{config::EngineBuilder, queue::QueueAdapter};

// todo: create a prelude module for commonly used traits and types
