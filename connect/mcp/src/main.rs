mod handler;
mod prompts;
mod transport;
mod worker_manager;

use std::sync::Arc;

use clap::Parser;
use iii_sdk::{InitOptions, register_worker};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(Parser, Debug)]
#[command(name = "iii-mcp")]
#[command(version)]
#[command(about = "MCP protocol worker for iii-engine")]
struct Args {
    #[arg(long, short = 'e', default_value = "ws://localhost:49134")]
    engine_url: String,

    #[arg(long, short = 'd')]
    debug: bool,

    #[arg(long, help = "Skip stdio, run as HTTP-only (POST /mcp on engine port)")]
    no_stdio: bool,

    #[arg(
        long,
        help = "Expose all functions as tools (ignore mcp.expose metadata)"
    )]
    expose_all: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let filter = if args.debug {
        EnvFilter::new("iii_mcp=debug,iii_sdk=debug")
    } else {
        EnvFilter::new("iii_mcp=info,iii_sdk=warn")
    };

    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(filter)
        .init();

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Starting iii-mcp");

    let iii = register_worker(&args.engine_url, InitOptions::default());

    handler::register_http(&iii, args.expose_all);

    if args.no_stdio {
        tracing::info!("MCP HTTP-only mode. POST /mcp on engine port. Ctrl+C to stop.");
        tokio::signal::ctrl_c().await?;
    } else {
        let h = Arc::new(handler::McpHandler::new(
            iii,
            args.engine_url,
            args.expose_all,
        ));
        transport::run_stdio(h).await?;
    }

    Ok(())
}
