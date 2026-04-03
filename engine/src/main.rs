// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

mod cli;
mod cli_trigger;

use clap::{Parser, Subcommand};
use cli_trigger::TriggerArgs;
use iii::{EngineBuilder, logging, workers::config::EngineConfig, workers::worker::DEFAULT_PORT};

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

    /// Disable background update and advisory checks
    #[arg(long, global = true)]
    no_update_check: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Invoke a function on a running iii engine
    Trigger(TriggerArgs),

    /// Launch the iii web console
    #[command(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        disable_help_flag = true
    )]
    Console {
        #[arg(num_args = 0..)]
        args: Vec<String>,
    },

    /// Create a new iii project from a template
    #[command(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        disable_help_flag = true
    )]
    Create {
        #[arg(num_args = 0..)]
        args: Vec<String>,
    },

    /// Manage SDKs powered by Motia
    #[command(subcommand)]
    Sdk(SdkCommands),

    /// Manage workers (add, remove, list, info)
    #[command(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        disable_help_flag = true
    )]
    Worker {
        #[arg(num_args = 0..)]
        args: Vec<String>,
    },

    /// Update iii and managed binaries to their latest versions
    Update {
        /// Specific command or binary to update (e.g., "console", "self").
        /// Use "self" or "iii" to update only iii.
        /// If omitted, updates iii and all installed binaries.
        #[arg(name = "command")]
        target: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum SdkCommands {
    /// Motia SDK tools
    #[command(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        disable_help_flag = true
    )]
    Motia {
        #[arg(num_args = 0..)]
        args: Vec<String>,
    },
}

async fn run_serve(cli: &Cli) -> anyhow::Result<()> {
    let config = if cli.use_default_config {
        EngineConfig::default_config()
    } else {
        EngineConfig::config_file(&cli.config)?
    };

    logging::init_log_from_config(if cli.use_default_config {
        None
    } else {
        Some(&cli.config)
    });

    let engine = EngineBuilder::new().with_config(config).build().await?;

    // Start managed workers in background so engine boot is not blocked by image pulls.
    let engine_url = format!("ws://localhost:{}", DEFAULT_PORT);
    tokio::spawn(async move {
        cli::managed_shim::start_managed_workers(&engine_url).await;
    });

    engine.serve().await?;

    // Engine shutdown complete (workers destroyed). Stop managed worker VMs.
    cli::managed_shim::stop_managed_workers().await;

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli_args = Cli::parse();

    if cli_args.version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    match &cli_args.command {
        Some(Commands::Trigger(args)) => cli_trigger::run_trigger(args).await,
        Some(Commands::Console { args }) => {
            let exit_code = cli::handle_dispatch("console", args, cli_args.no_update_check).await;
            std::process::exit(exit_code);
        }
        Some(Commands::Create { args }) => {
            let exit_code = cli::handle_dispatch("create", args, cli_args.no_update_check).await;
            std::process::exit(exit_code);
        }
        Some(Commands::Sdk(SdkCommands::Motia { args })) => {
            let exit_code = cli::handle_dispatch("motia", args, cli_args.no_update_check).await;
            std::process::exit(exit_code);
        }
        Some(Commands::Worker { args }) => {
            let exit_code = cli::handle_dispatch("worker", &args, cli_args.no_update_check).await;
            std::process::exit(exit_code);
        }
        Some(Commands::Update { target }) => {
            let exit_code = cli::handle_update(target.as_deref()).await;
            std::process::exit(exit_code);
        }
        None => run_serve(&cli_args).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use iii::workers::worker::DEFAULT_PORT;

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
                assert_eq!(args.timeout_ms, 30_000);
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
                assert_eq!(args.timeout_ms, 30_000);
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
    fn no_subcommand_falls_through_to_serve() {
        let cli = Cli::try_parse_from(["iii"]).expect("should parse with no subcommand");
        assert!(cli.command.is_none());
    }

    #[test]
    fn version_flag_works_globally() {
        let cli = Cli::try_parse_from(["iii", "--version"]).expect("should parse --version");
        assert!(cli.version);
    }

    // --- New subcommand parse tests ---

    #[test]
    fn console_parses_with_passthrough_args() {
        let cli = Cli::try_parse_from(["iii", "console", "--port", "3000"])
            .expect("should parse console with args");
        match cli.command {
            Some(Commands::Console { args }) => {
                assert_eq!(args, vec!["--port", "3000"]);
            }
            _ => panic!("expected Console subcommand"),
        }
    }

    #[test]
    fn console_parses_with_no_args() {
        let cli =
            Cli::try_parse_from(["iii", "console"]).expect("should parse console with no args");
        match cli.command {
            Some(Commands::Console { args }) => {
                assert!(args.is_empty());
            }
            _ => panic!("expected Console subcommand"),
        }
    }

    #[test]
    fn create_parses_with_passthrough_args() {
        let cli = Cli::try_parse_from(["iii", "create", "my-project", "--template", "default"])
            .expect("should parse create with args");
        match cli.command {
            Some(Commands::Create { args }) => {
                assert_eq!(args, vec!["my-project", "--template", "default"]);
            }
            _ => panic!("expected Create subcommand"),
        }
    }

    #[test]
    fn create_parses_with_no_args() {
        let cli = Cli::try_parse_from(["iii", "create"]).expect("should parse create with no args");
        match cli.command {
            Some(Commands::Create { args }) => {
                assert!(args.is_empty());
            }
            _ => panic!("expected Create subcommand"),
        }
    }

    #[test]
    fn sdk_motia_parses_with_passthrough_args() {
        let cli = Cli::try_parse_from(["iii", "sdk", "motia", "init", "--lang", "typescript"])
            .expect("should parse sdk motia with args");
        match cli.command {
            Some(Commands::Sdk(SdkCommands::Motia { args })) => {
                assert_eq!(args, vec!["init", "--lang", "typescript"]);
            }
            _ => panic!("expected Sdk Motia subcommand"),
        }
    }

    #[test]
    fn sdk_motia_parses_with_no_args() {
        let cli = Cli::try_parse_from(["iii", "sdk", "motia"])
            .expect("should parse sdk motia with no args");
        match cli.command {
            Some(Commands::Sdk(SdkCommands::Motia { args })) => {
                assert!(args.is_empty());
            }
            _ => panic!("expected Sdk Motia subcommand"),
        }
    }

    #[test]
    fn worker_parses_with_passthrough_args() {
        let cli = Cli::try_parse_from(["iii", "worker", "add", "pdfkit@1.0.0"])
            .expect("should parse worker with passthrough args");
        match cli.command {
            Some(Commands::Worker { args }) => {
                assert_eq!(args, vec!["add", "pdfkit@1.0.0"]);
            }
            _ => panic!("expected Worker subcommand"),
        }
    }

    #[test]
    fn worker_parses_with_no_args() {
        let cli = Cli::try_parse_from(["iii", "worker"]).expect("should parse worker with no args");
        match cli.command {
            Some(Commands::Worker { args }) => {
                assert!(args.is_empty());
            }
            _ => panic!("expected Worker subcommand"),
        }
    }

    #[test]
    fn worker_dev_parses_passthrough() {
        let cli = Cli::try_parse_from(["iii", "worker", "dev", ".", "--rebuild", "--port", "5000"])
            .expect("should parse worker dev with passthrough args");
        match cli.command {
            Some(Commands::Worker { args }) => {
                assert_eq!(args, vec!["dev", ".", "--rebuild", "--port", "5000"]);
            }
            _ => panic!("expected Worker subcommand"),
        }
    }

    #[test]
    fn worker_list_parses_passthrough() {
        let cli = Cli::try_parse_from(["iii", "worker", "list"]).expect("should parse worker list");
        match cli.command {
            Some(Commands::Worker { args }) => {
                assert_eq!(args, vec!["list"]);
            }
            _ => panic!("expected Worker subcommand"),
        }
    }

    #[test]
    fn worker_logs_parses_passthrough() {
        let cli = Cli::try_parse_from(["iii", "worker", "logs", "image-resize", "--follow"])
            .expect("should parse worker logs --follow");
        match cli.command {
            Some(Commands::Worker { args }) => {
                assert_eq!(args, vec!["logs", "image-resize", "--follow"]);
            }
            _ => panic!("expected Worker subcommand"),
        }
    }

    #[test]
    fn update_parses_with_target() {
        let cli = Cli::try_parse_from(["iii", "update", "console"])
            .expect("should parse update with target");
        match cli.command {
            Some(Commands::Update { target }) => {
                assert_eq!(target.as_deref(), Some("console"));
            }
            _ => panic!("expected Update subcommand"),
        }
    }

    #[test]
    fn update_parses_without_target() {
        let cli =
            Cli::try_parse_from(["iii", "update"]).expect("should parse update without target");
        match cli.command {
            Some(Commands::Update { target }) => {
                assert!(target.is_none());
            }
            _ => panic!("expected Update subcommand"),
        }
    }

    #[test]
    fn start_is_not_a_valid_subcommand() {
        let result = Cli::try_parse_from(["iii", "start"]);
        assert!(
            result.is_err(),
            "\"start\" should not be a valid subcommand (engine runs via default serve mode)"
        );
    }

    #[test]
    fn no_update_check_flag_works_globally() {
        let cli = Cli::try_parse_from(["iii", "--no-update-check"])
            .expect("should parse --no-update-check");
        assert!(cli.no_update_check);
        assert!(cli.command.is_none());
    }

    #[test]
    fn no_update_check_flag_works_with_subcommand() {
        let cli = Cli::try_parse_from(["iii", "--no-update-check", "console"])
            .expect("should parse --no-update-check with subcommand");
        assert!(cli.no_update_check);
        match cli.command {
            Some(Commands::Console { .. }) => {}
            _ => panic!("expected Console subcommand"),
        }
    }

    #[test]
    fn update_iii_cli_target_is_accepted() {
        // Users with old iii-cli may type "iii update iii-cli" — this must
        // parse successfully (the handler treats it as self-update).
        let cli = Cli::try_parse_from(["iii", "update", "iii-cli"])
            .expect("should parse 'update iii-cli' for backward compat");
        match cli.command {
            Some(Commands::Update { target }) => {
                assert_eq!(target.as_deref(), Some("iii-cli"));
            }
            _ => panic!("expected Update subcommand"),
        }
    }

    #[test]
    fn error_messages_do_not_contain_iii_cli() {
        // Read the error.rs source and verify it never references "iii-cli" in user-facing strings.
        // This is a compile-time / source-level regression check.
        let error_source = include_str!("cli/error.rs");
        assert!(
            !error_source.contains("iii-cli"),
            "error.rs should not contain 'iii-cli' references — the binary is now 'iii'"
        );
    }
}
