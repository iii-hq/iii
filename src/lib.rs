// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

pub mod builtins;
pub mod engine;
pub mod function;
pub mod invocation;
pub mod logging;
pub mod protocol;
pub mod services;
pub mod telemetry;
pub mod trigger;
pub mod workers;

pub mod modules {
    pub mod bridge_client;
    pub mod config;
    pub mod cron;
    pub mod kv_server;
    pub mod module;
    pub mod observability;
    pub mod pubsub;
    pub mod queue;
    pub mod redis;
    pub mod registry;
    pub mod rest_api;
    pub mod shell;
    pub mod state;
    pub mod streams;
    pub mod worker;
}

// Re-export commonly used types
pub use modules::{config::EngineBuilder, queue::QueueAdapter};

// todo: create a prelude module for commonly used traits and types
