use clap::Parser;
use iii::{EngineBuilder, logging};

#[derive(Parser, Debug)]
#[command(name = "engine", about = "Process communication engine")]
struct Args {
    #[arg(short, long, default_value = "config.yaml")]
    config: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logging::init_tracing();
    let args = Args::parse();

    EngineBuilder::new()
        .config_file_or_default(&args.config)?
        .address("127.0.0.1:49134")
        .build()
        .await?
        .serve()
        .await
}
