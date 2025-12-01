use ::engine::EngineBuilder;

mod engine;
mod function;
mod invocation;
mod logging;

mod pending_invocations;
mod protocol;
mod services;
mod trigger;
mod workers;

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
