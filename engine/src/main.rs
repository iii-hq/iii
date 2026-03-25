// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

mod cli;
mod cli_trigger;

use clap::{Parser, Subcommand};
use cli_trigger::TriggerArgs;
use iii::{EngineBuilder, logging, modules::config::EngineConfig};

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
    #[command(subcommand)]
    Worker(WorkerCommands),

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

#[derive(Subcommand, Debug)]
enum WorkerCommands {
    /// Add a worker from the registry (or all workers from iii.toml if no name given)
    Add {
        /// Worker name to install, optionally with version (e.g., "pdfkit" or "pdfkit@1.0.0")
        #[arg(value_name = "WORKER[@VERSION]")]
        worker_name: Option<String>,

        /// Overwrite existing config.yaml entries without prompting
        #[arg(long, short)]
        force: bool,
    },

    /// Remove a worker (removes binary, manifest entry, and config)
    Remove {
        /// Worker name to remove (e.g., "pdfkit")
        #[arg(value_name = "WORKER")]
        worker_name: String,
    },

    /// List installed workers and their versions
    List,

    /// Show details about a worker from the registry
    Info {
        /// Worker name to inspect (e.g., "pdfkit")
        #[arg(value_name = "WORKER")]
        worker_name: String,
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

    EngineBuilder::new()
        .with_config(config)
        .build()
        .await?
        .serve()
        .await?;

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
        Some(Commands::Worker(worker_cmd)) => {
            let exit_code = match worker_cmd {
                WorkerCommands::Add { worker_name, force } => {
                    cli::handle_install(worker_name.as_deref(), *force).await
                }
                WorkerCommands::Remove { worker_name } => cli::handle_uninstall(worker_name),
                WorkerCommands::List => cli::handle_worker_list(),
                WorkerCommands::Info { worker_name } => cli::handle_info(worker_name).await,
            };
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
    use iii::modules::worker::DEFAULT_PORT;

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
    fn worker_add_parses_with_worker_name() {
        let cli = Cli::try_parse_from(["iii", "worker", "add", "pdfkit@1.0.0"])
            .expect("should parse worker add with worker name");
        match cli.command {
            Some(Commands::Worker(WorkerCommands::Add { worker_name, force })) => {
                assert_eq!(worker_name.as_deref(), Some("pdfkit@1.0.0"));
                assert!(!force);
            }
            _ => panic!("expected Worker Add subcommand"),
        }
    }

    #[test]
    fn worker_add_parses_with_force_flag() {
        let cli = Cli::try_parse_from(["iii", "worker", "add", "pdfkit", "--force"])
            .expect("should parse worker add with force flag");
        match cli.command {
            Some(Commands::Worker(WorkerCommands::Add { worker_name, force })) => {
                assert_eq!(worker_name.as_deref(), Some("pdfkit"));
                assert!(force);
            }
            _ => panic!("expected Worker Add subcommand"),
        }
    }

    #[test]
    fn worker_add_parses_without_worker_name() {
        let cli = Cli::try_parse_from(["iii", "worker", "add"])
            .expect("should parse worker add without worker");
        match cli.command {
            Some(Commands::Worker(WorkerCommands::Add { worker_name, force })) => {
                assert!(worker_name.is_none());
                assert!(!force);
            }
            _ => panic!("expected Worker Add subcommand"),
        }
    }

    #[test]
    fn worker_remove_parses_worker_name() {
        let cli = Cli::try_parse_from(["iii", "worker", "remove", "pdfkit"])
            .expect("should parse worker remove");
        match cli.command {
            Some(Commands::Worker(WorkerCommands::Remove { worker_name })) => {
                assert_eq!(worker_name, "pdfkit");
            }
            _ => panic!("expected Worker Remove subcommand"),
        }
    }

    #[test]
    fn worker_list_parses() {
        let cli = Cli::try_parse_from(["iii", "worker", "list"]).expect("should parse worker list");
        match cli.command {
            Some(Commands::Worker(WorkerCommands::List)) => {}
            _ => panic!("expected Worker List subcommand"),
        }
    }

    #[test]
    fn worker_info_parses_worker_name() {
        let cli = Cli::try_parse_from(["iii", "worker", "info", "pdfkit"])
            .expect("should parse worker info command");
        match cli.command {
            Some(Commands::Worker(WorkerCommands::Info { worker_name })) => {
                assert_eq!(worker_name, "pdfkit");
            }
            _ => panic!("expected Worker Info subcommand"),
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
