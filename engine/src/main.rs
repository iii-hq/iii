// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

mod cli;
mod cli_trigger;

use clap::{Parser, Subcommand};
use cli_trigger::TriggerArgs;
use iii::{EngineBuilder, logging, workers::config::EngineConfig};

#[cfg(test)]
#[allow(unused_imports)]
use cli::project::{InitArgs, ProjectAction};

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

    /// Initialize telemetry IDs and optionally emit install lifecycle events.
    #[arg(long, hide = true, global = true)]
    install_only_generate_ids: bool,

    /// Install lifecycle event type (e.g. install_succeeded, upgrade_succeeded).
    #[arg(
        long,
        hide = true,
        global = true,
        requires = "install_only_generate_ids"
    )]
    install_event_type: Option<String>,

    /// Install lifecycle event properties as JSON.
    #[arg(
        long,
        hide = true,
        global = true,
        requires = "install_only_generate_ids"
    )]
    install_event_properties: Option<String>,
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

    /// Manage iii Cloud deployments
    #[command(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        disable_help_flag = true
    )]
    Cloud {
        #[arg(num_args = 0..)]
        args: Vec<String>,
    },

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

    /// Spawn and manage ephemeral sandbox VMs (run, list, stop)
    #[command(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        disable_help_flag = true
    )]
    Sandbox {
        #[arg(num_args = 0..)]
        args: Vec<String>,
    },

    /// Manage iii projects (init, generate-docker)
    Project(crate::cli::project::ProjectArgs),

    /// Update iii and managed binaries to their latest versions
    Update {
        /// Specific command or binary to update (e.g., "console", "self").
        /// Use "self" or "iii" to update only iii.
        /// If omitted, updates iii and all installed binaries.
        #[arg(name = "command")]
        target: Option<String>,
    },
}

fn should_init_logging_from_engine_config(cli: &Cli) -> bool {
    cli.use_default_config
}

async fn run_serve(cli: &Cli) -> anyhow::Result<()> {
    let config = if cli.use_default_config {
        EngineConfig::default_config()
    } else {
        EngineConfig::config_file(&cli.config)?
    };

    if should_init_logging_from_engine_config(cli) {
        logging::init_log_from_engine_config(&config);
    } else {
        logging::init_log_from_config(Some(&cli.config));
    }

    let mut builder = EngineBuilder::new().with_config(config);
    if !cli.use_default_config {
        builder = builder.with_config_path(&cli.config);
    }
    let engine = builder.build().await?;
    engine.serve().await?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli_args = Cli::parse();

    if cli_args.version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if cli_args.install_only_generate_ids {
        let _ = iii::workers::telemetry::environment::get_or_create_device_id();
        let _ = iii::workers::telemetry::environment::resolve_execution_context();

        if let Some(event_type) = cli_args.install_event_type.as_deref() {
            let properties = if let Some(raw) = cli_args.install_event_properties.as_deref() {
                serde_json::from_str(raw).map_err(|e| {
                    anyhow::anyhow!("invalid --install-event-properties JSON '{}': {}", raw, e)
                })?
            } else {
                serde_json::json!({})
            };
            cli::telemetry::send_install_lifecycle_event(event_type, properties).await;
        }
        return Ok(());
    }

    match &cli_args.command {
        Some(Commands::Trigger(args)) => cli_trigger::run_trigger(args).await,
        Some(Commands::Console { args }) => {
            let exit_code = cli::handle_dispatch("console", args, cli_args.no_update_check).await;
            std::process::exit(exit_code);
        }
        Some(Commands::Cloud { args }) => {
            let exit_code = cli::handle_dispatch("cloud", args, cli_args.no_update_check).await;
            std::process::exit(exit_code);
        }
        Some(Commands::Worker { args }) => {
            let exit_code = cli::handle_dispatch("worker", args, cli_args.no_update_check).await;
            std::process::exit(exit_code);
        }
        Some(Commands::Sandbox { args }) => {
            let exit_code = cli::handle_dispatch("sandbox", args, cli_args.no_update_check).await;
            std::process::exit(exit_code);
        }
        Some(Commands::Project(args)) => {
            let exit_code = cli::project::run(args.clone()).await;
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

    #[test]
    fn use_default_config_uses_engine_config_for_logging() {
        let cli = Cli::try_parse_from(["iii", "--use-default-config"]).unwrap();
        assert!(should_init_logging_from_engine_config(&cli));
    }

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
    fn create_is_no_longer_a_subcommand() {
        // `iii create` was removed in favor of `iii project init --template`.
        // Bare `iii create` should now fail to parse.
        let result = Cli::try_parse_from(["iii", "create"]);
        assert!(
            result.is_err(),
            "\"create\" should no longer be a valid subcommand"
        );
    }

    #[test]
    fn cloud_parses_with_passthrough_args() {
        let cli =
            Cli::try_parse_from(["iii", "cloud", "deploy", "--project", "abc", "--tag", "v1"])
                .expect("should parse cloud with args");
        match cli.command {
            Some(Commands::Cloud { args }) => {
                assert_eq!(args, vec!["deploy", "--project", "abc", "--tag", "v1"]);
            }
            _ => panic!("expected Cloud subcommand"),
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
    fn sandbox_parses_with_passthrough_args() {
        let cli = Cli::try_parse_from(["iii", "sandbox", "run", "python", "--cpus", "2"])
            .expect("should parse sandbox with passthrough args");
        match cli.command {
            Some(Commands::Sandbox { args }) => {
                assert_eq!(args, vec!["run", "python", "--cpus", "2"]);
            }
            _ => panic!("expected Sandbox subcommand"),
        }
    }

    #[test]
    fn sandbox_parses_with_no_args() {
        let cli =
            Cli::try_parse_from(["iii", "sandbox"]).expect("should parse sandbox with no args");
        match cli.command {
            Some(Commands::Sandbox { args }) => {
                assert!(args.is_empty());
            }
            _ => panic!("expected Sandbox subcommand"),
        }
    }

    #[test]
    fn sandbox_list_parses_passthrough() {
        let cli = Cli::try_parse_from(["iii", "sandbox", "list", "--all"])
            .expect("should parse sandbox list --all");
        match cli.command {
            Some(Commands::Sandbox { args }) => {
                assert_eq!(args, vec!["list", "--all"]);
            }
            _ => panic!("expected Sandbox subcommand"),
        }
    }

    #[test]
    fn sandbox_run_parses_trailing_cmd_with_dashdash() {
        // Mirrors the docs' recommended syntax:
        //   iii sandbox run python -- python3 -c 'print("hi")'
        let cli = Cli::try_parse_from([
            "iii",
            "sandbox",
            "run",
            "python",
            "--",
            "python3",
            "-c",
            "print(\"hi\")",
        ])
        .expect("should parse sandbox run with trailing command");
        match cli.command {
            Some(Commands::Sandbox { args }) => {
                assert_eq!(
                    args,
                    vec!["run", "python", "--", "python3", "-c", "print(\"hi\")"]
                );
            }
            _ => panic!("expected Sandbox subcommand"),
        }
    }

    #[test]
    fn sandbox_dispatch_resolves_to_iii_worker() {
        use crate::cli::registry::resolve_command;
        let (spec, binary_subcommand) = resolve_command("sandbox").expect("sandbox should resolve");
        assert_eq!(spec.name, "iii-worker");
        assert_eq!(binary_subcommand, Some("sandbox"));
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
    fn hidden_install_only_generate_ids_parses() {
        let cli = Cli::try_parse_from(["iii", "--install-only-generate-ids"])
            .expect("should parse hidden install-only flag");
        assert!(cli.install_only_generate_ids);
    }

    #[test]
    fn hidden_install_event_fields_parse() {
        let cli = Cli::try_parse_from([
            "iii",
            "--install-only-generate-ids",
            "--install-event-type",
            "install_succeeded",
            "--install-event-properties",
            r#"{"target_binary":"iii"}"#,
        ])
        .expect("should parse hidden install event flags");
        assert_eq!(cli.install_event_type.as_deref(), Some("install_succeeded"));
        assert_eq!(
            cli.install_event_properties.as_deref(),
            Some(r#"{"target_binary":"iii"}"#)
        );
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

    #[test]
    fn project_init_parses() {
        let cli =
            Cli::try_parse_from(["iii", "project", "init"]).expect("should parse project init");
        match cli.command {
            Some(Commands::Project(args)) => match args.action {
                ProjectAction::Init(_) => {}
                _ => panic!("expected Init action"),
            },
            _ => panic!("expected Project subcommand"),
        }
    }

    #[test]
    fn project_init_with_positional_name_parses() {
        let cli = Cli::try_parse_from(["iii", "project", "init", "myapp"])
            .expect("should parse project init <name>");
        match cli.command {
            Some(Commands::Project(args)) => match args.action {
                ProjectAction::Init(init) => {
                    assert_eq!(init.name.as_deref(), Some("myapp"));
                    assert!(init.directory.is_none());
                }
                _ => panic!("expected Init action"),
            },
            _ => panic!("expected Project subcommand"),
        }
    }

    #[test]
    fn project_init_with_directory_parses() {
        let cli = Cli::try_parse_from(["iii", "project", "init", "--directory", "myapp"])
            .expect("should parse project init --directory");
        match cli.command {
            Some(Commands::Project(args)) => match args.action {
                ProjectAction::Init(init) => assert_eq!(init.directory.as_deref(), Some("myapp")),
                _ => panic!("expected Init action"),
            },
            _ => panic!("expected Project subcommand"),
        }
    }

    #[test]
    fn project_init_with_docker_flag_parses() {
        let cli = Cli::try_parse_from(["iii", "project", "init", "--docker"])
            .expect("should parse project init --docker");
        match cli.command {
            Some(Commands::Project(args)) => match args.action {
                ProjectAction::Init(init) => assert!(init.docker),
                _ => panic!("expected Init action"),
            },
            _ => panic!("expected Project subcommand"),
        }
    }

    #[test]
    fn project_generate_docker_parses() {
        let cli = Cli::try_parse_from(["iii", "project", "generate-docker"])
            .expect("should parse project generate-docker");
        match cli.command {
            Some(Commands::Project(args)) => match args.action {
                ProjectAction::GenerateDocker(_) => {}
                _ => panic!("expected GenerateDocker action"),
            },
            _ => panic!("expected Project subcommand"),
        }
    }

    #[test]
    fn project_init_with_template_parses() {
        let cli = Cli::try_parse_from(["iii", "project", "init", "--template", "node-pdfkit"])
            .expect("should parse project init --template");
        match cli.command {
            Some(Commands::Project(args)) => match args.action {
                ProjectAction::Init(init) => {
                    assert_eq!(init.template.as_deref(), Some("node-pdfkit"));
                    assert!(!init.yes);
                    assert!(!init.skip_iii);
                }
                _ => panic!("expected Init action"),
            },
            _ => panic!("expected Project subcommand"),
        }
    }

    #[test]
    fn project_init_template_full_arg_set_parses() {
        let cli = Cli::try_parse_from([
            "iii",
            "project",
            "init",
            "--template",
            "node-pdfkit",
            "--directory",
            "myapp",
            "--languages",
            "ts,py",
            "--skip-iii",
            "--yes",
        ])
        .expect("should parse full template arg set");
        match cli.command {
            Some(Commands::Project(args)) => match args.action {
                ProjectAction::Init(init) => {
                    assert_eq!(init.template.as_deref(), Some("node-pdfkit"));
                    assert_eq!(init.directory.as_deref(), Some("myapp"));
                    assert_eq!(
                        init.languages.as_ref().map(|v| v.as_slice()),
                        Some(&["ts".to_string(), "py".to_string()][..])
                    );
                    assert!(init.skip_iii);
                    assert!(init.yes);
                }
                _ => panic!("expected Init action"),
            },
            _ => panic!("expected Project subcommand"),
        }
    }
}
