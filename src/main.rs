use engine::{EngineBuilder, logging};

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
