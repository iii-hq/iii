// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

mod cli_trigger;

use clap::{Parser, Subcommand};
use cli_trigger::TriggerArgs;
use iii::{
    EngineBuilder, logging,
    modules::config::{DEFAULT_PORT, EngineConfig},
};

#[derive(Parser, Debug)]
#[command(name = "iii", about = "Process communication engine")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path to the config file (default: config.yaml)
    #[arg(short, long, default_value = "config.yaml", global = true)]
    config: String,

    /// Print version and exit
    #[arg(short = 'v', long, global = true)]
    version: bool,

    /// Run with built-in defaults instead of a config file.
    /// Cannot be combined with --config.
    #[arg(long, global = true, conflicts_with = "config")]
    use_default_config: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Invoke a function on a running iii engine
    Trigger(TriggerArgs),
}

async fn run_serve(cli: &Cli) -> anyhow::Result<()> {
    if cli.use_default_config {
        logging::init_log_from_config(None);
        let config = EngineConfig::default_config();
        let port = if config.port == 0 {
            DEFAULT_PORT
        } else {
            config.port
        };

        EngineBuilder::new()
            .default_config()
            .address(format!("0.0.0.0:{}", port).as_str())
            .build()
            .await?
            .serve()
            .await?;
    } else {
        logging::init_log_from_config(Some(&cli.config));
        let config = EngineConfig::config_file(&cli.config)?;
        let port = if config.port == 0 {
            DEFAULT_PORT
        } else {
            config.port
        };

        EngineBuilder::new()
            .config_file(&cli.config)?
            .address(format!("0.0.0.0:{}", port).as_str())
            .build()
            .await?
            .serve()
            .await?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    match &cli.command {
        Some(Commands::Trigger(args)) => cli_trigger::run_trigger(args).await,
        None => run_serve(&cli).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn trigger_parses_all_arguments() {
        let cli = Cli::try_parse_from([
            "iii",
            "trigger",
            "--function-id",
            "iii::queue::redrive",
            "--payload",
            r#"{"queue":"payment"}"#,
            "--address",
            "10.0.0.1",
            "--port",
            "9999",
        ])
        .expect("should parse valid trigger args");

        match cli.command {
            Some(Commands::Trigger(args)) => {
                assert_eq!(args.function_id, "iii::queue::redrive");
                assert_eq!(args.payload, r#"{"queue":"payment"}"#);
                assert_eq!(args.address, "10.0.0.1");
                assert_eq!(args.port, 9999);
            }
            _ => panic!("expected Trigger subcommand"),
        }
    }

    #[test]
    fn trigger_uses_defaults_for_address_and_port() {
        let cli = Cli::try_parse_from([
            "iii",
            "trigger",
            "--function-id",
            "test::fn",
            "--payload",
            "{}",
        ])
        .expect("should parse with defaults");

        match cli.command {
            Some(Commands::Trigger(args)) => {
                assert_eq!(args.address, "localhost");
                assert_eq!(args.port, DEFAULT_PORT);
            }
            _ => panic!("expected Trigger subcommand"),
        }
    }

    #[test]
    fn trigger_requires_function_id() {
        let result = Cli::try_parse_from(["iii", "trigger", "--payload", "{}"]);
        assert!(result.is_err(), "should fail without --function-id");
    }

    #[test]
    fn trigger_requires_payload() {
        let result = Cli::try_parse_from(["iii", "trigger", "--function-id", "test::fn"]);
        assert!(result.is_err(), "should fail without --payload");
    }

    #[test]
    fn no_subcommand_falls_through_to_serve() {
        let cli = Cli::try_parse_from(["iii"]).expect("should parse with no subcommand");
        assert!(cli.command.is_none());
    }

    #[test]
    fn version_flag_works_globally() {
        let cli = Cli::try_parse_from(["iii", "--version"]).expect("should parse --version");
        assert!(cli.version);
    }
}
