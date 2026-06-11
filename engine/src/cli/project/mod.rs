// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii project` subcommand dispatch.
//!
//! All template content (the bare scaffold's `config.yaml`/`.gitignore` plus
//! the Docker assets) lives in the canonical templates repo
//! (`iii-hq/templates`). The engine never embeds template content via
//! `include_str!`; everything is fetched at runtime through
//! [`scaffolder_core::TemplateFetcher`]. This decouples template fixes from
//! engine releases — see iii-hq/templates#2 for the templates that back this
//! command.

use clap::{Args, Subcommand};
use colored::Colorize;
use scaffolder_core::cli::{
    apply_template_idempotent, build_fetcher, check_directory_state, print_err, resolve_root,
};
use scaffolder_core::{IiiConfig, TemplateFetcher};
use std::path::{Path, PathBuf};

#[derive(Args, Debug, Clone)]
pub struct ProjectArgs {
    #[command(subcommand)]
    pub action: ProjectAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ProjectAction {
    /// Initialize a new iii project in the current directory
    Init(InitArgs),
    /// Generate Docker assets (Dockerfile, docker-compose.yml, .env) for an existing project
    GenerateDocker(GenerateDockerArgs),
}

#[derive(Args, Debug, Clone)]
pub struct InitArgs {
    /// Project directory (positional). Equivalent to --directory.
    #[arg(value_name = "NAME")]
    pub name: Option<String>,

    /// Target directory (defaults to current directory). Takes precedence over the positional name.
    #[arg(short, long)]
    pub directory: Option<String>,

    /// Also generate Docker assets (Dockerfile, docker-compose.yml, .env)
    #[arg(long)]
    pub docker: bool,

    /// Scaffold from a named template (e.g. "quickstart"). Triggers the
    /// interactive scaffolder TUI.
    #[arg(short, long)]
    pub template: Option<String>,

    /// Local directory to use for templates instead of fetching from remote
    /// (for template development and tests).
    #[arg(long = "template-dir")]
    pub template_dir: Option<String>,

    /// Skip the iii-engine version compatibility check.
    #[arg(long = "skip-iii")]
    pub skip_iii: bool,

    /// Allow scaffolding into a non-empty directory. Without this flag, init
    /// errors out if the target dir contains anything other than hidden
    /// dotfiles (e.g. `.git/`) or iii-managed paths (`.iii/`, `data/`). An
    /// existing `.iii/project.ini` is always rejected; delete the marker
    /// or pick a different directory.
    #[arg(long = "allow-non-empty")]
    pub allow_non_empty: bool,
}

impl InitArgs {
    /// Resolved target directory: --directory wins, positional name is fallback.
    fn target_dir(&self) -> Option<&str> {
        self.directory.as_deref().or(self.name.as_deref())
    }
}

#[derive(Args, Debug, Clone)]
pub struct GenerateDockerArgs {
    /// Target directory (defaults to current directory)
    #[arg(short, long)]
    pub directory: Option<String>,

    /// Local directory to use for templates instead of fetching from remote
    /// (for template development and tests).
    #[arg(long = "template-dir")]
    pub template_dir: Option<String>,
}

fn template_flow_requested(args: &InitArgs) -> bool {
    // Only --template triggers the interactive scaffolder TUI. The bare flow
    // also uses scaffolder-core under the hood, but goes through the
    // non-interactive `apply_template` helper.
    args.template.is_some()
}

pub async fn run(args: ProjectArgs) -> i32 {
    match args.action {
        ProjectAction::Init(init) => run_init(init).await,
        ProjectAction::GenerateDocker(gd) => run_generate_docker(gd).await,
    }
}

async fn run_init(args: InitArgs) -> i32 {
    if template_flow_requested(&args) {
        return run_init_with_template(args).await;
    }

    let target = args.target_dir().map(|s| s.to_string());
    let root = match resolve_root(target.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            return print_err(
                "could not resolve target directory",
                &e,
                "pass --directory <path> or run from a writable cwd",
            );
        }
    };

    if let Err(e) = std::fs::create_dir_all(&root) {
        crate::cli::telemetry::send_project_init_failed("create_dir", &e.to_string());
        return print_err(
            &format!("could not create {}", root.display()),
            &e.to_string(),
            "check parent directory permissions or pick a different --directory",
        );
    }

    if let Err(e) = check_directory_state(&root, args.allow_non_empty, "project.ini") {
        crate::cli::telemetry::send_project_init_failed("non_empty_dir", &e);
        return print_err(
            "target directory is not empty",
            &e,
            "pass --allow-non-empty to scaffold into an existing project, or pick a different directory",
        );
    }

    let device_id = iii::workers::telemetry::environment::get_or_create_device_id();
    let project_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("iii-project")
        .to_string();

    // Fetch + apply the canonical 'bare' template. Existing project_id is
    // preserved on re-runs.
    let mut fetcher = match build_fetcher(args.template_dir.as_deref()) {
        Ok(f) => f,
        Err(e) => {
            crate::cli::telemetry::send_project_init_failed("fetcher", &e.to_string());
            return print_err(
                "could not build template fetcher",
                &e.to_string(),
                "check III_TEMPLATE_URL or pass --template-dir <path>",
            );
        }
    };

    if let Err(e) = apply_template_idempotent(&mut fetcher, "bare", &root).await {
        crate::cli::telemetry::send_project_init_failed("apply_bare", &e.to_string());
        return print_err(
            "could not apply 'bare' template",
            &e.to_string(),
            "see template fetch error above",
        );
    }

    let project_id = match persist_project_ini(&root, &project_name, "init", &device_id).await {
        Ok(id) => id,
        Err(e) => {
            crate::cli::telemetry::send_project_init_failed("write_project_ini", &e.to_string());
            return print_err(
                "could not write .iii/project.ini",
                &e.to_string(),
                "check that the target directory is writable",
            );
        }
    };

    if args.docker
        && let Err(e) = apply_docker(&mut fetcher, &root, &device_id).await
    {
        crate::cli::telemetry::send_project_init_failed("apply_docker", &e.to_string());
        return print_err(
            "could not apply 'docker' template",
            &e.to_string(),
            "remove existing Dockerfile/docker-compose.yml or check write permissions",
        );
    }

    crate::cli::telemetry::send_project_init_succeeded(args.docker, &project_id);

    print_init_success(&project_name, &root, target.is_some(), args.docker);
    0
}

async fn run_init_with_template(args: InitArgs) -> i32 {
    // Restore terminal cursor on panic and on Ctrl+C — scaffolder runs an
    // interactive TUI via cliclack and we don't want to leave the cursor hidden.
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = console::Term::stderr().show_cursor();
        default_panic(info);
    }));
    let _ = ctrlc::set_handler(move || {
        let _ = console::Term::stderr().show_cursor();
        std::process::exit(130);
    });

    let target_dir = args.target_dir().map(PathBuf::from);
    let create_args = scaffolder_core::tui::CreateArgs {
        template_dir: args.template_dir.as_ref().map(PathBuf::from),
        template: args.template.clone(),
        directory: target_dir.clone(),
        languages: None,
        skip_tool_check: args.skip_iii,
        skip_install: false,
        skip_next_steps: false,
        yes: false,
    };

    let result = scaffolder_core::run(&IiiConfig, create_args, env!("CARGO_PKG_VERSION")).await;
    let _ = console::Term::stderr().show_cursor();

    if let Err(e) = result {
        crate::cli::telemetry::send_project_init_failed("scaffolder", &e.to_string());
        return print_err(
            "template scaffold failed",
            &e.to_string(),
            "see scaffolder output above",
        );
    }

    let project_id_for_event = if let Some(root) = target_dir.as_ref() {
        if root.is_dir() {
            let device_id = iii::workers::telemetry::environment::get_or_create_device_id();
            let project_name = root
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("iii-project")
                .to_string();
            let template_label = args.template.as_deref().unwrap_or("init-template");
            let id = persist_project_ini(root, &project_name, template_label, &device_id)
                .await
                .unwrap_or_default();

            if args.docker {
                let mut fetcher = match build_fetcher(args.template_dir.as_deref()) {
                    Ok(f) => f,
                    Err(e) => {
                        crate::cli::telemetry::send_project_init_failed("fetcher", &e.to_string());
                        return print_err(
                            "could not build template fetcher for docker assets",
                            &e.to_string(),
                            "check III_TEMPLATE_URL or pass --template-dir <path>",
                        );
                    }
                };
                if let Err(e) = apply_docker(&mut fetcher, root, &device_id).await {
                    crate::cli::telemetry::send_project_init_failed("apply_docker", &e.to_string());
                    return print_err(
                        "could not apply 'docker' template",
                        &e.to_string(),
                        "remove existing Dockerfile/docker-compose.yml or check write permissions",
                    );
                }
            }

            id
        } else {
            String::new()
        }
    } else {
        // Interactive flow — no known directory, no project_id retrofit.
        String::new()
    };

    crate::cli::telemetry::send_project_init_succeeded(args.docker, &project_id_for_event);
    0
}

async fn run_generate_docker(args: GenerateDockerArgs) -> i32 {
    let root = match resolve_root(args.directory.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            return print_err(
                "could not resolve target directory",
                &e,
                "pass --directory <path> or run from a writable cwd",
            );
        }
    };

    let device_id = resolve_device_id_for_docker(&root);

    let mut fetcher = match build_fetcher(args.template_dir.as_deref()) {
        Ok(f) => f,
        Err(e) => {
            return print_err(
                "could not build template fetcher",
                &e.to_string(),
                "check III_TEMPLATE_URL or pass --template-dir <path>",
            );
        }
    };

    if let Err(e) = apply_docker(&mut fetcher, &root, &device_id).await {
        return print_err(
            "could not apply 'docker' template",
            &e.to_string(),
            "remove existing Dockerfile/docker-compose.yml or check write permissions",
        );
    }

    eprintln!();
    eprintln!(
        "  {} Docker assets generated at {}",
        "✓".green(),
        root.display()
    );
    eprintln!();
    eprintln!("  Next: {}", "docker compose up".bold());
    0
}

// ============================================================================
// Helpers
// ============================================================================

/// Fetch the docker template's two files directly (skipping the shared_files
/// merge that [`copy_template`] applies). We can't go through `copy_template`
/// here because it'd re-copy `config.yaml` / `.gitignore` from `shared_files`
/// and clobber any user customizations — the caller already has those from the
/// 'bare' template or a prior `iii project init`.
///
/// The Dockerfile template carries a literal `__III_DEVICE_ID__` placeholder
/// that we substitute with the actual device_id before writing, so the image
/// no longer needs an `III_HOST_USER_ID` env var at runtime. The generated
/// `.env` only carries the RabbitMQ password (used by the commented-out
/// rabbitmq service in docker-compose.yml).
const DEVICE_ID_PLACEHOLDER: &str = "__III_DEVICE_ID__";

async fn apply_docker(
    fetcher: &mut TemplateFetcher,
    target: &Path,
    device_id: &str,
) -> anyhow::Result<()> {
    let dockerfile_bytes = fetcher.fetch_file_bytes("docker", "Dockerfile").await?;
    let compose = fetcher
        .fetch_file_bytes("docker", "docker-compose.yml")
        .await?;

    let dockerfile = substitute_device_id(&dockerfile_bytes, device_id)?;

    write_if_absent(&target.join("Dockerfile"), &dockerfile)?;
    write_if_absent(&target.join("docker-compose.yml"), &compose)?;
    write_env_if_absent(target)?;
    Ok(())
}

fn substitute_device_id(bytes: &[u8], device_id: &str) -> anyhow::Result<Vec<u8>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|e| anyhow::anyhow!("Dockerfile template is not valid UTF-8: {e}"))?;
    if !text.contains(DEVICE_ID_PLACEHOLDER) {
        anyhow::bail!(
            "Dockerfile template is missing the {DEVICE_ID_PLACEHOLDER} \
             placeholder — the template repo and engine are out of sync"
        );
    }
    Ok(text.replace(DEVICE_ID_PLACEHOLDER, device_id).into_bytes())
}

fn write_if_absent(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, contents)
}

fn write_env_if_absent(target: &Path) -> std::io::Result<()> {
    let path = target.join(".env");
    if path.exists() {
        return Ok(());
    }
    let rabbitmq_pass = uuid::Uuid::new_v4().simple().to_string();
    let contents = format!(
        "# Generated by `iii project generate-docker`. Do not commit.\n\
         RABBITMQ_USER=iii\n\
         RABBITMQ_PASS={rabbitmq_pass}\n",
    );
    std::fs::write(path, contents)
}

/// Persist `.iii/project.ini`, preserving any existing project_id when called
/// against an already-initialized project. Returns the (existing or freshly
/// generated) project_id so the caller can include it in the success event.
async fn persist_project_ini(
    root: &Path,
    project_name: &str,
    source: &str,
    device_id: &str,
) -> anyhow::Result<String> {
    let project_id =
        read_existing_project_id(root).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    scaffolder_core::telemetry::write_project_ini(
        root,
        &project_id,
        project_name,
        source,
        Some(device_id),
    )
    .await?;
    Ok(project_id)
}

fn read_existing_project_id(root: &Path) -> Option<String> {
    read_project_ini_field(root, "project_id")
}

/// Read a single key from `.iii/project.ini` (flat or `[project]`-prefixed
/// format), returning `None` when the file is absent, unreadable, or the key
/// is missing/empty. The format-tolerant parser is shared between
/// `read_existing_project_id` (used by re-init) and
/// `resolve_device_id_for_docker` (used by the docker generator).
fn read_project_ini_field(root: &Path, key: &str) -> Option<String> {
    let path = root.join(".iii").join("project.ini");
    let contents = std::fs::read_to_string(path).ok()?;
    let prefix = format!("{key}=");
    contents
        .lines()
        .find_map(|l| l.trim().strip_prefix(&prefix))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn resolve_device_id_for_docker(root: &Path) -> String {
    let ini_exists = root.join(".iii").join("project.ini").exists();
    match read_project_ini_field(root, "device_id") {
        Some(id) => id,
        None => {
            if ini_exists {
                // Legacy project.ini that pre-dates the device_id field
                // (e.g. created by the old iii-tools or the interactive
                // TUI flow without a --directory). Don't claim the
                // project is uninitialized — it isn't.
                eprintln!(
                    "  {} no device_id in .iii/project.ini; generating a fresh one.",
                    "note:".dimmed()
                );
            } else {
                warn_missing_project_ini(root);
            }
            iii::workers::telemetry::environment::get_or_create_device_id()
        }
    }
}

fn warn_missing_project_ini(root: &Path) {
    eprintln!(
        "  {} project not initialized at {}",
        "warning:".yellow().bold(),
        root.display()
    );
    eprintln!(
        "  {} run `iii project init` here first to persist a project identity.",
        "fix:".dimmed()
    );
}

fn print_init_success(project_name: &str, root: &Path, target_specified: bool, docker: bool) {
    eprintln!();
    eprintln!(
        "  {} iii project '{}' initialized at {}",
        "✓".green(),
        project_name.bold(),
        root.display()
    );
    eprintln!();
    eprintln!("  Next steps:");
    if target_specified {
        eprintln!("    {}", format!("cd {}", root.display()).bold());
    }
    eprintln!(
        "    {}    # add a worker",
        "iii worker add <package>".bold()
    );
    eprintln!(
        "    {}                          # start the engine",
        "iii".bold()
    );
    if docker {
        eprintln!(
            "    {}           # or start in Docker",
            "docker compose up".bold()
        );
    }
    eprintln!();
    eprintln!("  Docs: https://iii.dev/docs/quickstart");
}
