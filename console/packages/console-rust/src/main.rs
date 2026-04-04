use anyhow::Result;
use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

mod bridge;
mod proxy;
mod server;

#[derive(Parser, Debug)]
#[command(name = "iii-console")]
#[command(version)]
#[command(about = "Developer console for the iii engine", long_about = None)]
struct Args {
    /// Port to run the console server on
    #[arg(short, long, default_value = "3113")]
    port: u16,

    /// Host to bind the console server to
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Host where the iii engine is running
    #[arg(long, default_value = "127.0.0.1")]
    engine_host: String,

    /// Port for the iii engine REST API
    #[arg(long, default_value = "3111")]
    engine_port: u16,

    /// Port for the iii engine WebSocket
    #[arg(long, default_value = "3112")]
    ws_port: u16,

    /// Port for the iii engine bridge WebSocket
    #[arg(long, default_value = "49134")]
    bridge_port: u16,

    /// Disable OpenTelemetry tracing, metrics, and logs export
    #[arg(long, env = "OTEL_DISABLED")]
    no_otel: bool,

    /// OpenTelemetry service name (default: iii-console)
    #[arg(long, env = "OTEL_SERVICE_NAME", default_value = "iii-console")]
    otel_service_name: String,

    /// Enable the experimental flow visualization page
    #[arg(long, env = "III_ENABLE_FLOW")]
    enable_flow: bool,
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    info!("Starting iii-console on {}:{}", args.host, args.port);
    info!(
        "Connecting to engine at {}:{} (WS: {})",
        args.engine_host, args.engine_port, args.ws_port
    );

    let bridge_url = format!("ws://{}:{}", args.engine_host, args.bridge_port);

    let otel_config = if !args.no_otel {
        info!(
            "OpenTelemetry enabled (service: {})",
            args.otel_service_name
        );
        Some(iii_sdk::OtelConfig {
            enabled: Some(true),
            service_name: Some(args.otel_service_name),
            service_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            engine_ws_url: Some(bridge_url.clone()),
            ..Default::default()
        })
    } else {
        info!("OpenTelemetry disabled");
        None
    };

    let bridge = iii_sdk::register_worker(
        &bridge_url,
        iii_sdk::InitOptions {
            metadata: None,
            otel: otel_config,
            headers: None,
        },
    );

    bridge::register_functions(&bridge);

    if let Err(e) = bridge::register_triggers(&bridge) {
        tracing::warn!("Trigger registration failed: {}", e);
    }

    let config = server::ServerConfig {
        port: args.port,
        host: args.host,
        engine_host: args.engine_host,
        engine_port: args.engine_port,
        ws_port: args.ws_port,
        enable_flow: args.enable_flow,
    };

    // Run server with graceful shutdown
    let server = server::run_server(config);

    tokio::select! {
        result = server => result,
        _ = shutdown_signal() => {
            tracing::info!("Shutdown signal received, cleaning up...");
            bridge.shutdown_async().await;
            Ok(())
        }
    }
}
