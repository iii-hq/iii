pub mod engine;
pub mod function;
pub mod invocation;
pub mod logging;
pub mod pending_invocations;
pub mod protocol;
pub mod services;
pub mod trigger;
pub mod workers;

pub mod modules {
    pub mod config;
    pub mod core_module;
    pub mod cron;
    pub mod event;
    pub mod observability;
    pub mod rest_api;
    pub mod shell;
    pub mod streams;
}

// Re-export commonly used types
pub use modules::{config::EngineBuilder, cron::CronSchedulerAdapter, event::EventAdapter};

// todo: create a prelude module for commonly used traits and types
