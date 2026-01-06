pub mod engine;
pub mod function;
pub mod invocation;
pub mod logging;
pub mod protocol;
pub mod services;
pub mod trigger;
pub mod workers;

pub mod modules {
    pub mod config;
    pub mod core_module;
    pub mod cron;
    pub mod devtools;
    pub mod event;
    pub mod management;
    pub mod observability;
    pub mod redis;
    pub mod registry;
    pub mod rest_api;
    pub mod shell;
    pub mod streams;
}

// Re-export commonly used types
pub use modules::{config::EngineBuilder, event::EventAdapter};

// todo: create a prelude module for commonly used traits and types
