use clap::Parser;
use iii::{EngineBuilder, logging};

#[derive(Parser, Debug)]
#[command(name = "engine", about = "Process communication engine")]
struct Args {
    #[arg(short, long, default_value = "config.yaml")]
    config: String,
    #[arg(short = 'v', long)]
    version: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    logging::init_log(&args.config);

    EngineBuilder::new()
        .config_file_or_default(&args.config)?
        .address("0.0.0.0:49134")
        .build()
        .await?
        .serve()
        .await?;
    Ok(())
}
