mod engine;
mod function;
mod invocation;
mod logging;

mod pending_invocations;
mod protocol;
mod services;
mod trigger;
mod workers;
mod modules {
    pub mod adapter_registry;
    pub mod config;
    pub mod configurable;
    pub mod core_module;
    pub mod cron;
    pub mod event;
    pub mod observability;
    pub mod rest_api;
    pub mod streams;
}

use crate::modules::config::EngineBuilder;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logging::init_tracing();

    EngineBuilder::new()
        .config_file_or_default("config.yaml")?
        .address("127.0.0.1:49134")
        .build()
        .await?
        .serve()
        .await
}
