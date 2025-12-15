use clap::Parser;
use engine::{EngineBuilder, logging};

#[derive(Parser, Debug)]
#[command(name = "engine", about = "Process communication engine")]
struct Args {
    #[arg(short, long, default_value = "config.yaml")]
    config: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    logging::init_log(&args.config);

    EngineBuilder::new()
        .config_file_or_default(&args.config)?
        .address("127.0.0.1:49134")
        .build()
        .await?
        .serve()
        .await
}
