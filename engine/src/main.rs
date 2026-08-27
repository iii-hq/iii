// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

mod cli;
mod cli_trigger;
mod legacy_worker_functions;

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

    /// Path to the config file [default: config.yaml]. When the file does
    /// not exist, `iii` offers to create it with an empty workers list (and
    /// creates it without asking in non-interactive sessions).
    // Option (not default_value) so direct engine startup can distinguish an
    // explicit file from its `config.yaml` fallback.
    #[arg(short, long)]
    config: Option<String>,

    /// Print version and exit.
    #[arg(short = 'v', long)]
    version: bool,

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
    #[command(visible_alias = "t")]
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

    /// Manage iii projects (init, generate-docker)
    Project(crate::cli::project::ProjectArgs),

    /// Serve worker-compose projects or prepare their registry packages.
    ///
    /// Without `--up`, worker-compose.yaml supplies daemon defaults but no
    /// project starts. Projects are then managed through `compose::*` calls.
    /// With `--up`, the initial project also starts, together with its declared
    /// engine unless `--engine` selects an existing one.
    /// `build` downloads packages without starting an engine or worker.
    Compose(iii_compose::ComposeCli),

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

fn passthrough_command_path(command: &str, args: &[String]) -> String {
    match args.first() {
        Some(arg) if !arg.starts_with('-') => format!("{command} {arg}"),
        _ => command.to_string(),
    }
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
        Some(Commands::Project(args)) => match args.action {
            cli::project::ProjectAction::Init(_) => "project init".to_string(),
            cli::project::ProjectAction::GenerateDocker(_) => "project generate-docker".to_string(),
        },
        Some(Commands::Compose(args)) => match &args.command {
            Some(iii_compose::ComposeSubcommand::Build(_)) => "compose build".to_string(),
            Some(iii_compose::ComposeSubcommand::Logs(_)) => "compose logs".to_string(),
            None => "compose".to_string(),
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

/// Make sure the config file exists before the engine loads it.
///
/// Missing file: on an interactive terminal, ask before writing (running
/// `iii` in the wrong directory shouldn't silently litter a config.yaml
/// there); headless sessions (containers, CI, service managers) create it
/// without asking so `iii` keeps booting unattended. Returns `false` when
/// the user declined — the caller aborts without creating anything.
fn ensure_config_file(path: &str) -> anyhow::Result<bool> {
    use std::io::{IsTerminal, Write};

    if std::path::Path::new(path).exists() {
        return Ok(true);
    }

    let dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        eprint!(
            "No {} found in {}. Create it and start the engine? [Y/n] ",
            path,
            dir.display()
        );
        let _ = std::io::stderr().flush();
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        let answer = answer.trim().to_lowercase();
        if !(answer.is_empty() || answer == "y" || answer == "yes") {
            return Ok(false);
        }
    }

    std::fs::write(path, EngineConfig::starter_config_yaml())
        .map_err(|e| anyhow::anyhow!("failed to create config file '{}': {}", path, e))?;
    eprintln!(
        "Created {}. Declare project workers in worker-compose.yaml and start them with `iii compose --up`.",
        path
    );
    Ok(true)
}

/// Effective config file path: the explicit `--config` value, or the
/// `config.yaml` default.
fn config_path_of(cli: &Cli) -> &str {
    cli.config.as_deref().unwrap_or("config.yaml")
}

async fn run_serve(cli: &Cli) -> anyhow::Result<()> {
    let config_path = config_path_of(cli);
    if !ensure_config_file(config_path)? {
        eprintln!(
            "Aborted. Run `iii` from your project directory, or pass --config <path> to an existing config file."
        );
        std::process::exit(1);
    }

    let config = EngineConfig::config_file(config_path)?;
    logging::init_log_from_config(Some(config_path));

    let engine = EngineBuilder::new()
        .with_config(config)
        .with_config_path(config_path)
        .build()
        .await?;
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

    cli::telemetry::record_cli_usage(&cli_usage_command_path(&cli_args));

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
            let exit_code =
                cli::handle_dispatch("console", args, cli_args.no_update_check, &[]).await;
            std::process::exit(exit_code);
        }
        Some(Commands::Cloud { args }) => {
            let exit_code =
                cli::handle_dispatch("cloud", args, cli_args.no_update_check, &[]).await;
            std::process::exit(exit_code);
        }
        Some(Commands::Project(args)) => {
            let exit_code = cli::project::run(args.clone()).await;
            std::process::exit(exit_code);
        }
        // Compose owns its own lifecycle. Its file supplies daemon defaults;
        // an explicit --engine overrides them, and --up decides whether an
        // unoverridden engine section starts a managed engine.
        Some(Commands::Compose(args)) => {
            if cli_args.config.is_some() {
                anyhow::bail!(
                    "--config cannot be used with `iii compose`. Put managed engine settings under \
                     engine: in worker-compose.yaml, or start the external engine separately"
                );
            }
            let exit_code = iii_compose::run(args.clone()).await;
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
    fn trigger_alias_parses_with_kv_pairs() {
        let cli = Cli::try_parse_from(["iii", "t", "my::fn", "a=10", "b=hello"])
            .expect("should parse t alias with kv args");
        match cli.command {
            Some(Commands::Trigger(args)) => {
                assert_eq!(args.function_path.as_deref(), Some("my::fn"));
                assert_eq!(args.kv, vec!["a=10", "b=hello"]);
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
    fn worker_is_no_longer_a_public_command() {
        let result = Cli::try_parse_from(["iii", "worker", "add", "http"]);
        assert!(
            result.is_err(),
            "iii worker must be rejected by the root CLI"
        );
    }

    #[test]
    fn version_flag_works_globally() {
        let cli = Cli::try_parse_from(["iii", "--version"]).expect("should parse --version");
        assert!(cli.version);
        assert_eq!(cli_usage_command_path(&cli), "version");
    }

    #[test]
    fn use_default_config_is_no_longer_a_flag() {
        // `--use-default-config` was removed: `iii` now creates config.yaml
        // when it's missing, so a file-less mode flag has nothing to do and
        // only bypassed the config.yaml setup (worker add, reload watcher).
        let result = Cli::try_parse_from(["iii", "--use-default-config"]);
        assert!(
            result.is_err(),
            "--use-default-config should no longer parse"
        );
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
        // Sandboxes are managed through the iii-sandbox worker's function surface.
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

    /// Bare `iii compose` stays the daemon command. Project lifecycle actions
    /// remain `compose::*` calls; `build` only prepares local packages.
    #[test]
    fn compose_mounts_as_a_single_command() {
        let cli = Cli::try_parse_from(["iii", "compose"]).expect("should parse compose");
        assert_eq!(cli_usage_command_path(&cli), "compose");
        match cli.command {
            Some(Commands::Compose(args)) => {
                assert!(args.engine.is_none());
                assert!(args.ns.is_none());
                assert!(args.command.is_none());
            }
            _ => panic!("expected Compose subcommand"),
        }
    }

    #[test]
    fn compose_build_has_its_own_telemetry_path() {
        let cli =
            Cli::try_parse_from(["iii", "compose", "build"]).expect("should parse compose build");
        assert_eq!(cli_usage_command_path(&cli), "compose build");
        match cli.command {
            Some(Commands::Compose(args)) => {
                assert!(matches!(
                    args.command,
                    Some(iii_compose::ComposeSubcommand::Build(_))
                ));
            }
            _ => panic!("expected Compose subcommand"),
        }
    }

    /// The flags that survived say where the daemon runs and which namespace
    /// it answers in, not what it does. They are parsed here and resolved in
    /// the crate.
    #[test]
    fn compose_carries_its_placement_flags() {
        let cli = Cli::try_parse_from(["iii", "compose", "-n", "dev", "--engine", "ws://host:1"])
            .expect("should parse a placed compose");
        assert_eq!(cli_usage_command_path(&cli), "compose");
        match cli.command {
            Some(Commands::Compose(args)) => {
                assert_eq!(args.ns.as_deref(), Some("dev"));
                assert_eq!(args.engine.as_deref(), Some("ws://host:1"));
            }
            _ => panic!("expected Compose subcommand"),
        }
    }

    /// A word that is not a subcommand must fail rather than be swallowed as a
    /// stray argument: `iii compose down` doing nothing quietly is the failure
    /// mode this guards. Starting the initial project is a flag, not an
    /// exception to this rule.
    #[test]
    fn a_word_that_is_not_a_subcommand_does_not_parse() {
        for removed in [
            ["iii", "compose", "up"],
            ["iii", "compose", "down"],
            ["iii", "compose", "validate"],
        ] {
            assert!(
                Cli::try_parse_from(removed).is_err(),
                "{removed:?} should not parse"
            );
        }
    }

    /// The one step that cannot be a `compose::*` call: a daemon has to exist
    /// before a call can reach it.
    #[test]
    fn compose_up_reaches_the_flag() {
        let cli =
            Cli::try_parse_from(["iii", "compose", "--up", "-n", "dev"]).expect("should parse");
        match cli.command {
            Some(Commands::Compose(args)) => {
                assert_eq!(args.ns.as_deref(), Some("dev"));
                assert!(args.up, "--up should reach the compose arguments");
            }
            _ => panic!("expected Compose subcommand"),
        }
    }

    /// The trigger alias shares the top-level argument space with every
    /// subcommand, so a new one must not shadow it.
    #[test]
    fn compose_does_not_shadow_the_trigger_alias() {
        let cli = Cli::try_parse_from(["iii", "trigger", "compose::up", "id=host-a"])
            .expect("trigger should still parse");
        assert_eq!(cli_usage_command_path(&cli), "trigger");
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
        let cli = Cli::try_parse_from(["iii", "--config", "foo.yaml", "compose", "--up"])
            .expect("config before subcommand should still parse");
        assert_eq!(cli.config.as_deref(), Some("foo.yaml"));
    }

    #[test]
    fn compose_uses_the_root_engine_config_selection() {
        let explicit =
            Cli::try_parse_from(["iii", "--config", "custom.yaml", "compose", "--up"]).unwrap();
        let default = Cli::try_parse_from(["iii", "compose", "--up"]).unwrap();

        assert_eq!(config_path_of(&explicit), "custom.yaml");
        assert_eq!(config_path_of(&default), "config.yaml");
    }

    #[test]
    fn trigger_parses_namespace_flag_alongside_kv_pairs() {
        let cli = Cli::try_parse_from([
            "iii",
            "trigger",
            "compose::up",
            "--namespace",
            "host-a",
            "container=api",
        ])
        .expect("should parse trigger --namespace");

        match cli.command {
            Some(Commands::Trigger(args)) => {
                assert_eq!(args.namespace.as_deref(), Some("host-a"));
                assert_eq!(args.function_path.as_deref(), Some("compose::up"));
                assert_eq!(args.kv, vec!["container=api".to_string()]);
            }
            _ => panic!("expected Trigger subcommand"),
        }
    }

    #[test]
    fn compose_add_parses_repeated_worker_arguments() {
        let cli = Cli::try_parse_from([
            "iii",
            "trigger",
            "-n",
            "dev",
            "compose::add",
            "worker=database",
            "worker=web",
        ])
        .expect("compose::add should parse repeated workers");

        match cli.command {
            Some(Commands::Trigger(args)) => {
                assert_eq!(args.namespace.as_deref(), Some("dev"));
                assert_eq!(args.function_path.as_deref(), Some("compose::add"));
                assert_eq!(args.kv, vec!["worker=database", "worker=web"]);
            }
            _ => panic!("expected Trigger subcommand"),
        }
    }

    #[test]
    fn trigger_without_the_flag_carries_no_namespace() {
        let cli = Cli::try_parse_from(["iii", "trigger", "state::get", "key=a"])
            .expect("should parse a plain trigger");
        match cli.command {
            Some(Commands::Trigger(args)) => assert!(args.namespace.is_none()),
            _ => panic!("expected Trigger subcommand"),
        }
    }
}
