mod handler;
mod types;

use clap::Parser;
use iii_sdk::{InitOptions, register_worker};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(Parser, Debug)]
#[command(name = "iii-a2a")]
#[command(version)]
#[command(about = "A2A protocol worker for iii-engine")]
struct Args {
    #[arg(long, short = 'e', default_value = "ws://localhost:49134")]
    engine_url: String,

    #[arg(long, short = 'd')]
    debug: bool,

    #[arg(
        long,
        help = "Expose all functions as skills (ignore a2a.expose metadata)"
    )]
    expose_all: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let filter = if args.debug {
        EnvFilter::new("iii_a2a=debug,iii_sdk=debug")
    } else {
        EnvFilter::new("iii_a2a=info,iii_sdk=warn")
    };

    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(filter)
        .init();

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Starting iii-a2a");

    let iii = register_worker(&args.engine_url, InitOptions::default());

    handler::register(&iii, args.expose_all);

    tracing::info!("A2A endpoints registered on engine port. Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;

    Ok(())
}
