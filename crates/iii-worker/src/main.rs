// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

mod cli;

use clap::{Parser, Subcommand};

/// Default engine WebSocket port (must match engine's DEFAULT_PORT).
const DEFAULT_PORT: u16 = 49134;

#[derive(Parser, Debug)]
#[command(name = "iii-worker", version, about = "iii managed worker runtime")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Add a worker from the registry or by OCI image reference
    Add {
        /// Worker name or OCI image reference (e.g., "pdfkit", "pdfkit@1.0.0", "ghcr.io/org/worker:tag")
        #[arg(value_name = "WORKER[@VERSION]")]
        worker_name: String,

        /// Container runtime
        #[arg(long, default_value = "libkrun")]
        runtime: String,

        /// Engine host address
        #[arg(long, default_value = "localhost")]
        address: String,

        /// Engine WebSocket port
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },

    /// Remove a worker (stops and removes the container)
    Remove {
        /// Worker name to remove (e.g., "pdfkit")
        #[arg(value_name = "WORKER")]
        worker_name: String,

        /// Engine host address
        #[arg(long, default_value = "localhost")]
        address: String,

        /// Engine WebSocket port
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },

    /// Start a previously stopped managed worker container
    Start {
        /// Worker name to start
        #[arg(value_name = "WORKER")]
        worker_name: String,

        /// Engine host address
        #[arg(long, default_value = "localhost")]
        address: String,

        /// Engine WebSocket port
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },

    /// Stop a managed worker container
    Stop {
        /// Worker name to stop
        #[arg(value_name = "WORKER")]
        worker_name: String,

        /// Engine host address
        #[arg(long, default_value = "localhost")]
        address: String,

        /// Engine WebSocket port
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },

    /// Run a worker project in an isolated environment for development.
    ///
    /// Auto-detects the project type (package.json, Cargo.toml, pyproject.toml)
    /// and runs it inside a VM (libkrun) connected
    /// to the engine.
    Dev {
        /// Path to the worker project directory
        #[arg(value_name = "PATH")]
        path: String,

        /// Sandbox name (defaults to directory name)
        #[arg(long)]
        name: Option<String>,

        /// Runtime to use (auto-detected if not set)
        #[arg(long, value_parser = ["libkrun"])]
        runtime: Option<String>,

        /// Force rebuild: re-run setup and install scripts (libkrun only)
        #[arg(long)]
        rebuild: bool,

        /// Engine host address
        #[arg(long, default_value = "localhost")]
        address: String,

        /// Engine WebSocket port
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },

    /// List all workers and their status
    List,

    /// Show logs from a managed worker container
    Logs {
        /// Worker name
        #[arg(value_name = "WORKER")]
        worker_name: String,

        /// Follow log output
        #[arg(long, short)]
        follow: bool,

        /// Engine host address
        #[arg(long, default_value = "localhost")]
        address: String,

        /// Engine WebSocket port
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },

    /// Start all workers declared in iii.workers.yaml (used by engine lifecycle shim)
    #[command(name = "start-all")]
    StartAll {
        /// Engine WebSocket URL
        #[arg(long)]
        engine_url: String,
    },

    /// Stop all running managed workers (used by engine lifecycle shim)
    #[command(name = "stop-all")]
    StopAll,

    /// Internal: boot a libkrun VM (crash-isolated subprocess)
    #[command(name = "__vm-boot", hide = true)]
    VmBoot(cli::vm_boot::VmBootArgs),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli_args = Cli::parse();

    let exit_code = match cli_args.command {
        Commands::Add {
            worker_name,
            runtime,
            address,
            port,
        } => cli::managed::handle_managed_add(&worker_name, &runtime, &address, port).await,
        Commands::Remove {
            worker_name,
            address,
            port,
        } => cli::managed::handle_managed_remove(&worker_name, &address, port).await,
        Commands::Start {
            worker_name,
            address,
            port,
        } => cli::managed::handle_managed_start(&worker_name, &address, port).await,
        Commands::Stop {
            worker_name,
            address,
            port,
        } => cli::managed::handle_managed_stop(&worker_name, &address, port).await,
        Commands::Dev {
            path,
            name,
            runtime,
            rebuild,
            address,
            port,
        } => {
            cli::managed::handle_worker_dev(
                &path,
                name.as_deref(),
                runtime.as_deref(),
                rebuild,
                &address,
                port,
            )
            .await
        }
        Commands::List => cli::managed::handle_worker_list().await,
        Commands::Logs {
            worker_name,
            follow,
            address,
            port,
        } => cli::managed::handle_managed_logs(&worker_name, follow, &address, port).await,
        Commands::StartAll { engine_url } => {
            cli::managed::start_managed_workers(&engine_url).await;
            0
        }
        Commands::StopAll => {
            cli::managed::stop_managed_workers().await;
            0
        }
        Commands::VmBoot(args) => {
            // Crash-isolated subprocess: boot VM and never return.
            cli::vm_boot::run(&args);
        }
    };

    std::process::exit(exit_code);
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn worker_add_parses_with_worker_name() {
        let cli = Cli::try_parse_from(["iii-worker", "add", "pdfkit@1.0.0"])
            .expect("should parse worker add with worker name");
        match cli.command {
            Commands::Add {
                worker_name,
                runtime,
                ..
            } => {
                assert_eq!(worker_name, "pdfkit@1.0.0");
                assert_eq!(runtime, "libkrun");
            }
            _ => panic!("expected Add subcommand"),
        }
    }

    #[test]
    fn worker_list_parses() {
        let cli = Cli::try_parse_from(["iii-worker", "list"]).expect("should parse worker list");
        match cli.command {
            Commands::List => {}
            _ => panic!("expected List subcommand"),
        }
    }

    #[test]
    fn start_all_parses() {
        let cli = Cli::try_parse_from([
            "iii-worker",
            "start-all",
            "--engine-url",
            "ws://localhost:49134",
        ])
        .expect("should parse start-all");
        match cli.command {
            Commands::StartAll { engine_url } => {
                assert_eq!(engine_url, "ws://localhost:49134");
            }
            _ => panic!("expected StartAll subcommand"),
        }
    }

    #[test]
    fn stop_all_parses() {
        let cli = Cli::try_parse_from(["iii-worker", "stop-all"]).expect("should parse stop-all");
        match cli.command {
            Commands::StopAll => {}
            _ => panic!("expected StopAll subcommand"),
        }
    }

    #[test]
    fn worker_dev_parses_with_path() {
        let cli = Cli::try_parse_from(["iii-worker", "dev", ".", "--port", "5000"])
            .expect("should parse worker dev with path");
        match cli.command {
            Commands::Dev { path, port, .. } => {
                assert_eq!(path, ".");
                assert_eq!(port, 5000);
            }
            _ => panic!("expected Dev subcommand"),
        }
    }

    #[test]
    fn worker_dev_parses_with_rebuild() {
        let cli = Cli::try_parse_from([
            "iii-worker",
            "dev",
            "/tmp/project",
            "--rebuild",
            "--name",
            "my-worker",
        ])
        .expect("should parse worker dev with rebuild");
        match cli.command {
            Commands::Dev {
                path,
                rebuild,
                name,
                ..
            } => {
                assert_eq!(path, "/tmp/project");
                assert!(rebuild);
                assert_eq!(name, Some("my-worker".to_string()));
            }
            _ => panic!("expected Dev subcommand"),
        }
    }

    #[test]
    fn worker_remove_parses() {
        let cli = Cli::try_parse_from(["iii-worker", "remove", "pdfkit"])
            .expect("should parse worker remove");
        match cli.command {
            Commands::Remove { worker_name, .. } => {
                assert_eq!(worker_name, "pdfkit");
            }
            _ => panic!("expected Remove subcommand"),
        }
    }

    #[test]
    fn worker_start_parses() {
        let cli = Cli::try_parse_from(["iii-worker", "start", "pdfkit", "--port", "8080"])
            .expect("should parse worker start");
        match cli.command {
            Commands::Start {
                worker_name, port, ..
            } => {
                assert_eq!(worker_name, "pdfkit");
                assert_eq!(port, 8080);
            }
            _ => panic!("expected Start subcommand"),
        }
    }

    #[test]
    fn worker_stop_parses() {
        let cli = Cli::try_parse_from(["iii-worker", "stop", "pdfkit"])
            .expect("should parse worker stop");
        match cli.command {
            Commands::Stop { worker_name, .. } => {
                assert_eq!(worker_name, "pdfkit");
            }
            _ => panic!("expected Stop subcommand"),
        }
    }

    #[test]
    fn worker_logs_parses_with_follow() {
        let cli = Cli::try_parse_from(["iii-worker", "logs", "image-resize", "--follow"])
            .expect("should parse worker logs with follow");
        match cli.command {
            Commands::Logs {
                worker_name,
                follow,
                ..
            } => {
                assert_eq!(worker_name, "image-resize");
                assert!(follow);
            }
            _ => panic!("expected Logs subcommand"),
        }
    }
}
