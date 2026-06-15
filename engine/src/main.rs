// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

mod cli;
mod cli_trigger;

use clap::{CommandFactory, Parser, Subcommand};
use cli_trigger::TriggerArgs;
use iii::{EngineBuilder, logging, workers::config::EngineConfig};

/// Walk the clap Command tree to find the deepest matching subcommand for the
/// given argv. Skips flags and the auto-generated `help` token (so
/// `iii help update` resolves to the same Command as `iii update --help`).
/// Falls back to the root command on miss.
fn resolve_help_target<'a>(root: &'a clap::Command, argv: &[String]) -> &'a clap::Command {
    let mut cmd = root;
    for token in argv.iter().skip(1) {
        if token.starts_with('-') || token == "help" {
            continue;
        }
        match cmd.find_subcommand(token) {
            Some(sub) => cmd = sub,
            None => break,
        }
    }
    cmd
}

/// Render a clap Command's help via clap-help, then exit.
fn print_help_and_exit(argv: &[String]) -> ! {
    let mut root = Cli::command();
    root.build();
    let target = resolve_help_target(&root, argv).clone();
    render_clap_help(target);
    std::process::exit(0);
}

/// Render a clap Command's help via clap-help with our shared styling
/// (suppress the empty author stub, surface `about` under the title, and
/// append a Commands listing because clap-help 1.x has no subcommand
/// section). Does not exit.
pub fn render_clap_help(target: clap::Command) {
    let mut printer = clap_help::Printer::new(target.clone());
    // Author line is rendered as a useless "by " stub when no author is set.
    printer.set_template("author", "");
    // Surface the command's `about` text under the title. clap-help 1.x does
    // not pull `about` from the Command, so inject it manually.
    if let Some(about) = target.get_about() {
        printer.expander_mut().set("about", about.to_string());
        printer.set_template("introduction", "\n${about}\n");
    }
    printer.print_help();
    print_subcommands_section(&target);
}

/// Look up a subcommand on the Cli command tree by name.
pub fn cli_subcommand(name: &str) -> Option<clap::Command> {
    let mut root = Cli::command();
    root.build();
    root.find_subcommand(name).cloned()
}

/// clap-help 1.x does not render subcommand listings; print our own table.
fn print_subcommands_section(cmd: &clap::Command) {
    use colored::Colorize;
    let subs: Vec<&clap::Command> = cmd.get_subcommands().filter(|s| !s.is_hide_set()).collect();
    if subs.is_empty() {
        return;
    }
    let max_name = subs.iter().map(|s| s.get_name().len()).max().unwrap_or(0);
    println!();
    println!("{}", "Commands:".bold());
    for sub in subs {
        let name = sub.get_name();
        let about = sub.get_about().map(|s| s.to_string()).unwrap_or_default();
        let padded = format!("{:<width$}", name, width = max_name);
        if about.is_empty() {
            println!("  {}", padded.bold());
        } else {
            println!("  {}  {}", padded.bold(), about);
        }
    }
    println!();
}

#[cfg(test)]
#[allow(unused_imports)]
use cli::project::{InitArgs, ProjectAction};

#[derive(Parser, Debug)]
#[command(name = "iii", about = "Process communication engine")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path to the config file.
    #[arg(short, long, default_value = "config.yaml")]
    config: String,

    /// Print version and exit.
    #[arg(short = 'v', long)]
    version: bool,

    /// Run with built-in defaults instead of a config file.
    /// Cannot be combined with --config.
    #[arg(long, conflicts_with = "config")]
    use_default_config: bool,

    /// Disable background update and security advisory checks.
    #[arg(long)]
    no_update_check: bool,

    /// Initialize telemetry IDs and optionally emit install lifecycle events.
    #[arg(long, hide = true)]
    install_only_generate_ids: bool,

    /// Install lifecycle event type (e.g. install_succeeded, upgrade_succeeded).
    #[arg(long, hide = true, requires = "install_only_generate_ids")]
    install_event_type: Option<String>,

    /// Install lifecycle event properties as JSON.
    #[arg(long, hide = true, requires = "install_only_generate_ids")]
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

    /// Manage workers (add, remove, list)
    #[command(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        disable_help_flag = true
    )]
    Worker {
        #[arg(num_args = 0..)]
        args: Vec<String>,
    },

    /// Manage iii projects (init, generate-docker)
    Project(crate::cli::project::ProjectArgs),

    /// Generate the committed MDX CLI reference page from this binary's
    /// clap definitions (build tooling; see scripts/generate-cli-docs.sh)
    #[command(name = "gen-cli-docs", hide = true)]
    GenDocs {
        /// Write the page to this file instead of stdout
        #[arg(long, value_name = "FILE")]
        out: Option<std::path::PathBuf>,
    },

    /// Update iii and managed binaries to their latest versions
    Update {
        /// Specific command or binary to update (e.g., "console", "self").
        /// Use "self" or "iii" to update only iii.
        /// If omitted, updates iii and all installed binaries.
        #[arg(
            name = "command",
            value_name = "COMMAND",
            conflicts_with = "list_targets"
        )]
        target: Option<String>,

        /// List the targets you can pass to `iii update [COMMAND]` and exit.
        #[arg(long = "list-targets")]
        list_targets: bool,
    },
}

fn should_init_logging_from_engine_config(cli: &Cli) -> bool {
    cli.use_default_config
}

fn passthrough_command_path(command: &str, args: &[String]) -> String {
    for arg in args {
        if arg.starts_with('-') {
            break;
        }
        return format!("{command} {arg}");
    }
    command.to_string()
}

fn cli_usage_command_path(cli: &Cli) -> String {
    if cli.version {
        return "version".to_string();
    }
    if cli.install_only_generate_ids {
        return "install-only-generate-ids".to_string();
    }

    match &cli.command {
        Some(Commands::Trigger(_)) => "trigger".to_string(),
        Some(Commands::Console { args }) => passthrough_command_path("console", args),
        Some(Commands::Cloud { args }) => passthrough_command_path("cloud", args),
        Some(Commands::Worker { args }) => passthrough_command_path("worker", args),
        Some(Commands::Project(args)) => match args.action {
            cli::project::ProjectAction::Init(_) => "project init".to_string(),
            cli::project::ProjectAction::GenerateDocker(_) => "project generate-docker".to_string(),
        },
        Some(Commands::GenDocs { .. }) => "gen-cli-docs".to_string(),
        Some(Commands::Update {
            list_targets: true, ..
        }) => "update list-targets".to_string(),
        Some(Commands::Update {
            target: Some(_), ..
        }) => "update target".to_string(),
        Some(Commands::Update { target: None, .. }) => "update".to_string(),
        None => "serve".to_string(),
    }
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
    let argv: Vec<String> = std::env::args().collect();
    let cli_args = match Cli::try_parse_from(&argv) {
        Ok(c) => c,
        Err(err) => match err.kind() {
            // Intercept clap's default help output and re-render it via
            // clap-help for a friendlier layout. Trigger has its own dynamic
            // help (engine query) and is opted out via disable_help_flag, so
            // this only fires for root + non-trigger subcommands.
            clap::error::ErrorKind::DisplayHelp
            | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
                print_help_and_exit(&argv);
            }
            _ => err.exit(),
        },
    };

    // Docs generation is offline build tooling: handle it before telemetry
    // and any engine setup so the output stays deterministic.
    if let Some(Commands::GenDocs { out }) = &cli_args.command {
        cli::gen_docs::run(Cli::command(), out.as_deref())?;
        return Ok(());
    }

    cli::telemetry::send_cli_usage(&cli_usage_command_path(&cli_args));

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
        Some(Commands::Trigger(args)) => match cli_trigger::run_trigger(args).await {
            Ok(()) => Ok(()),
            // exec::invoke already printed the structured JSON; exit silently.
            Err(cli_trigger::TriggerCliError::RemoteAlreadyReported) => std::process::exit(1),
            Err(cli_trigger::TriggerCliError::Other(e)) => Err(e),
        },
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
        Some(Commands::Project(args)) => {
            let exit_code = cli::project::run(args.clone()).await;
            std::process::exit(exit_code);
        }
        // Handled before telemetry above.
        Some(Commands::GenDocs { .. }) => unreachable!("gen-cli-docs returns early"),
        Some(Commands::Update {
            target,
            list_targets,
        }) => {
            if *list_targets {
                cli::update::print_targets();
                std::process::exit(0);
            }
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
    fn trigger_parses_with_positional_fn_path_only() {
        let cli = Cli::try_parse_from(["iii", "trigger", "my::fn"])
            .expect("should parse trigger with fn path only");
        match cli.command {
            Some(Commands::Trigger(args)) => {
                assert_eq!(args.function_path.as_deref(), Some("my::fn"));
                assert!(args.kv.is_empty());
                assert!(args.json.is_none());
                assert_eq!(args.address, "localhost");
                assert_eq!(args.port, DEFAULT_PORT);
                assert_eq!(args.timeout_ms, 30_000);
            }
            _ => panic!("expected Trigger subcommand"),
        }
    }

    #[test]
    fn trigger_parses_with_kv_pairs() {
        let cli = Cli::try_parse_from(["iii", "trigger", "my::fn", "a=10", "b=hello"])
            .expect("should parse trigger with kv args");
        match cli.command {
            Some(Commands::Trigger(args)) => {
                assert_eq!(args.function_path.as_deref(), Some("my::fn"));
                assert_eq!(args.kv, vec!["a=10", "b=hello"]);
            }
            _ => panic!("expected Trigger subcommand"),
        }
    }

    #[test]
    fn trigger_parses_with_json_flag() {
        let cli = Cli::try_parse_from(["iii", "trigger", "my::fn", "--json", r#"{"a":1}"#])
            .expect("should parse trigger --json");
        match cli.command {
            Some(Commands::Trigger(args)) => {
                assert_eq!(args.function_path.as_deref(), Some("my::fn"));
                assert_eq!(args.json.as_deref(), Some(r#"{"a":1}"#));
            }
            _ => panic!("expected Trigger subcommand"),
        }
    }

    #[test]
    fn trigger_parses_with_json_and_kv_together() {
        let cli = Cli::try_parse_from([
            "iii",
            "trigger",
            "my::fn",
            "--json",
            r#"{"a":1,"b":2}"#,
            "a=99",
        ])
        .expect("should parse trigger with --json and kv simultaneously");
        match cli.command {
            Some(Commands::Trigger(args)) => {
                assert_eq!(args.function_path.as_deref(), Some("my::fn"));
                assert_eq!(args.kv, vec!["a=99"]);
                assert_eq!(args.json.as_deref(), Some(r#"{"a":1,"b":2}"#));
            }
            _ => panic!("expected Trigger subcommand"),
        }
    }

    #[test]
    fn trigger_legacy_function_id_flag_rejected() {
        let result = Cli::try_parse_from(["iii", "trigger", "--function-id", "my::fn"]);
        assert!(result.is_err(), "--function-id should fail to parse");
    }

    #[test]
    fn trigger_legacy_payload_flag_rejected() {
        let result = Cli::try_parse_from(["iii", "trigger", "my::fn", "--payload", r#"{"a":1}"#]);
        assert!(result.is_err(), "--payload should fail to parse");
    }

    #[test]
    fn no_subcommand_falls_through_to_serve() {
        let cli = Cli::try_parse_from(["iii"]).expect("should parse with no subcommand");
        assert!(cli.command.is_none());
        assert_eq!(cli_usage_command_path(&cli), "serve");
    }

    #[test]
    fn version_flag_works_globally() {
        let cli = Cli::try_parse_from(["iii", "--version"]).expect("should parse --version");
        assert!(cli.version);
        assert_eq!(cli_usage_command_path(&cli), "version");
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
    fn cli_usage_command_path_keeps_passthrough_command_but_not_values() {
        let cli = Cli::try_parse_from(["iii", "worker", "add", "--secret", "value"])
            .expect("should parse worker passthrough");
        assert_eq!(cli_usage_command_path(&cli), "worker add");
    }

    #[test]
    fn cli_usage_command_path_covers_worker_commands() {
        let cli = Cli::try_parse_from(["iii", "worker", "logs", "pdf-worker"])
            .expect("should parse worker logs passthrough");
        assert_eq!(cli_usage_command_path(&cli), "worker logs");
    }

    #[test]
    fn cli_usage_command_path_covers_cloud_commands() {
        let cli = Cli::try_parse_from(["iii", "cloud", "deploy", "--config", "prod.yaml"])
            .expect("should parse cloud deploy passthrough");
        assert_eq!(cli_usage_command_path(&cli), "cloud deploy");
    }

    #[test]
    fn cli_usage_command_path_covers_update_modes() {
        let cli =
            Cli::try_parse_from(["iii", "update", "console"]).expect("should parse update target");
        assert_eq!(cli_usage_command_path(&cli), "update target");

        let cli = Cli::try_parse_from(["iii", "update", "--list-targets"])
            .expect("should parse update --list-targets");
        assert_eq!(cli_usage_command_path(&cli), "update list-targets");
    }

    #[test]
    fn cli_usage_command_path_does_not_capture_flag_values_as_subcommands() {
        let cli = Cli::try_parse_from(["iii", "console", "--port", "3000"])
            .expect("should parse console passthrough");
        assert_eq!(cli_usage_command_path(&cli), "console");
    }

    #[test]
    fn cli_usage_command_path_does_not_capture_trigger_function_id() {
        let cli = Cli::try_parse_from(["iii", "trigger", "orders::charge"])
            .expect("should parse trigger");
        assert_eq!(cli_usage_command_path(&cli), "trigger");
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
    fn update_parses_with_target() {
        let cli = Cli::try_parse_from(["iii", "update", "console"])
            .expect("should parse update with target");
        match cli.command {
            Some(Commands::Update {
                target,
                list_targets,
            }) => {
                assert_eq!(target.as_deref(), Some("console"));
                assert!(!list_targets);
            }
            _ => panic!("expected Update subcommand"),
        }
    }

    #[test]
    fn update_parses_without_target() {
        let cli =
            Cli::try_parse_from(["iii", "update"]).expect("should parse update without target");
        match cli.command {
            Some(Commands::Update {
                target,
                list_targets,
            }) => {
                assert!(target.is_none());
                assert!(!list_targets);
            }
            _ => panic!("expected Update subcommand"),
        }
    }

    #[test]
    fn update_parses_with_list_targets_flag() {
        let cli = Cli::try_parse_from(["iii", "update", "--list-targets"])
            .expect("should parse update --list-targets");
        match cli.command {
            Some(Commands::Update {
                target,
                list_targets,
            }) => {
                assert!(target.is_none());
                assert!(list_targets);
            }
            _ => panic!("expected Update subcommand"),
        }
    }

    #[test]
    fn update_target_and_list_targets_conflict() {
        let result = Cli::try_parse_from(["iii", "update", "console", "--list-targets"]);
        assert!(
            result.is_err(),
            "--list-targets should conflict with positional target"
        );
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
    fn sandbox_is_no_longer_a_valid_subcommand() {
        // `iii sandbox` was moved to `iii worker sandbox` for managing ephemeral VMs.
        // As well as `iii trigger sandbox::<op>` for working with the sandbox through the sandbox worker.
        // Bare `iii sandbox` should now fail to parse.
        let result = Cli::try_parse_from(["iii", "sandbox"]);
        assert!(
            result.is_err(),
            "\"sandbox\" should no longer be a valid subcommand"
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
            Some(Commands::Update {
                target,
                list_targets: _,
            }) => {
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
        assert_eq!(cli_usage_command_path(&cli), "project init");
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
        assert_eq!(cli_usage_command_path(&cli), "project generate-docker");
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
            "--skip-iii",
        ])
        .expect("should parse full template arg set");
        match cli.command {
            Some(Commands::Project(args)) => match args.action {
                ProjectAction::Init(init) => {
                    assert_eq!(init.template.as_deref(), Some("node-pdfkit"));
                    assert_eq!(init.directory.as_deref(), Some("myapp"));
                    assert!(init.skip_iii);
                }
                _ => panic!("expected Init action"),
            },
            _ => panic!("expected Project subcommand"),
        }
    }

    #[test]
    fn config_flag_is_not_global_on_subcommands() {
        // After dropping global=true, the engine config flags should only
        // be parseable before a subcommand. A trailing --config on a
        // subcommand that doesn't define the flag itself must error.
        let result = Cli::try_parse_from(["iii", "project", "init", "--config", "foo.yaml"]);
        assert!(
            result.is_err(),
            "--config after a subcommand should no longer parse globally"
        );
    }

    #[test]
    fn config_flag_still_works_before_subcommand() {
        let cli = Cli::try_parse_from(["iii", "--config", "foo.yaml", "worker", "add", "x"])
            .expect("config before subcommand should still parse");
        assert_eq!(cli.config, "foo.yaml");
    }
}
