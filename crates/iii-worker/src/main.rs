// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use clap::Parser;
use iii_worker::{Cli, Commands};

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
        Commands::Add { args, force } => {
            if force {
                // Force mode: process each worker individually with force logic
                let mut fail_count = 0;
                for name in &args.worker_names {
                    let result = iii_worker::cli::managed::handle_managed_add(
                        name,
                        false,
                        None,
                        force,
                        args.reset_config,
                    )
                    .await;
                    if result != 0 {
                        fail_count += 1;
                    }
                }
                if fail_count == 0 { 0 } else { 1 }
            } else {
                iii_worker::cli::managed::handle_managed_add_many(&args.worker_names).await
            }
        }
        Commands::Remove { worker_names } => {
            iii_worker::cli::managed::handle_managed_remove_many(&worker_names).await
        }
        Commands::Reinstall { args } => {
            let mut fail_count = 0;
            for name in &args.worker_names {
                let result = iii_worker::cli::managed::handle_managed_add(
                    name,
                    false,
                    None,
                    true,
                    args.reset_config,
                )
                .await;
                if result != 0 {
                    fail_count += 1;
                }
            }
            if fail_count == 0 { 0 } else { 1 }
        }
        Commands::Clear { worker_name, yes } => {
            iii_worker::cli::managed::handle_managed_clear(worker_name.as_deref(), yes)
        }
        Commands::Start {
            worker_name,
            address,
            port,
        } => iii_worker::cli::managed::handle_managed_start(&worker_name, &address, port).await,
        Commands::Stop {
            worker_name,
            address,
            port,
        } => iii_worker::cli::managed::handle_managed_stop(&worker_name, &address, port).await,
        Commands::List => iii_worker::cli::managed::handle_worker_list().await,
        Commands::Logs {
            worker_name,
            follow,
            address,
            port,
        } => {
            iii_worker::cli::managed::handle_managed_logs(&worker_name, follow, &address, port)
                .await
        }
        Commands::VmBoot(args) => {
            iii_worker::cli::vm_boot::run(&args);
        }
    };

    std::process::exit(exit_code);
}
