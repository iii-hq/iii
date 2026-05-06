// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

pub mod docker;
pub mod project_ini;
pub mod scaffold;

use clap::{Args, Subcommand};

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

    /// Scaffold from a named template (e.g. "node-pdfkit"). Triggers the
    /// interactive template flow and supersedes the bare scaffold.
    #[arg(short, long)]
    pub template: Option<String>,

    /// Local directory to use for templates instead of fetching from remote
    /// (for template development).
    #[arg(long = "template-dir")]
    pub template_dir: Option<String>,

    /// Languages to include (comma-separated: ts,js,py).
    #[arg(short, long, value_delimiter = ',')]
    pub languages: Option<Vec<String>>,

    /// Skip the iii-engine version compatibility check.
    #[arg(long = "skip-iii")]
    pub skip_iii: bool,

    /// Auto-confirm all prompts (non-interactive mode).
    #[arg(short, long)]
    pub yes: bool,
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
}

use colored::Colorize;
use scaffolder_core::ProductConfig;

#[derive(Clone)]
struct IiiConfig;

impl ProductConfig for IiiConfig {
    fn name(&self) -> &'static str {
        "iii"
    }
    fn display_name(&self) -> &'static str {
        "iii"
    }
    fn default_template_url(&self) -> &'static str {
        "https://github.com/iii-hq/templates.git"
    }
    fn template_url_env(&self) -> &'static str {
        "III_TEMPLATE_URL"
    }
    fn requires_iii(&self) -> bool {
        true
    }
    fn docs_url(&self) -> &'static str {
        "https://iii.dev/docs"
    }
    fn cli_description(&self) -> &'static str {
        "CLI for scaffolding iii projects"
    }
    fn upgrade_command(&self) -> &'static str {
        "iii update"
    }
}

fn template_flow_requested(args: &InitArgs) -> bool {
    // Template flow is triggered by template-specific flags only.
    // --yes and --skip-iii are pass-through args that modify scaffolder
    // behavior but do not by themselves indicate intent to use templates.
    args.template.is_some() || args.template_dir.is_some() || args.languages.is_some()
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

    let device_id = iii::workers::telemetry::environment::get_or_create_device_id();
    let project_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("iii-project")
        .to_string();
    let project_id = uuid::Uuid::new_v4().to_string();

    let ini = project_ini::ProjectIni {
        project_id: Some(project_id.clone()),
        project_name: Some(project_name.clone()),
        source: Some("init".to_string()),
        device_id: Some(device_id.clone()),
    };
    if let Err(e) = ini.write(&root) {
        crate::cli::telemetry::send_project_init_failed("write_project_ini", &e.to_string());
        return print_err(
            "could not write .iii/project.ini",
            &e.to_string(),
            "check that the target directory is writable",
        );
    }

    if let Err(e) = scaffold::write_scaffold(&root) {
        crate::cli::telemetry::send_project_init_failed("write_scaffold", &e.to_string());
        return print_err(
            "could not write scaffold files",
            &e.to_string(),
            "check disk space and target directory permissions",
        );
    }

    if args.docker {
        if let Err(e) = docker::write_docker_assets(&root, &device_id) {
            crate::cli::telemetry::send_project_init_failed("write_docker", &e.to_string());
            return print_err(
                "could not write Docker assets",
                &e.to_string(),
                "remove existing Dockerfile/docker-compose.yml or check write permissions",
            );
        }
    }

    crate::cli::telemetry::send_project_init_succeeded(args.docker, &project_id);

    eprintln!();
    eprintln!(
        "  {} iii project '{}' initialized at {}",
        "✓".green(),
        project_name.bold(),
        root.display()
    );
    eprintln!();
    eprintln!("  Next steps:");
    if target.is_some() {
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
    if args.docker {
        eprintln!(
            "    {}           # or start in Docker",
            "docker compose up".bold()
        );
    }
    eprintln!();
    eprintln!("  Docs: https://iii.dev/docs/quickstart");
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

    let target_dir = args.target_dir().map(std::path::PathBuf::from);
    let create_args = scaffolder_core::tui::CreateArgs {
        template_dir: args.template_dir.as_ref().map(std::path::PathBuf::from),
        template: args.template.clone(),
        directory: target_dir.clone(),
        languages: args.languages.clone(),
        skip_tool_check: args.skip_iii,
        yes: args.yes,
    };

    let result = scaffolder_core::run(&IiiConfig, create_args, env!("CARGO_PKG_VERSION")).await;
    let _ = console::Term::stderr().show_cursor();

    if let Err(e) = result {
        crate::cli::telemetry::send_project_init_failed("scaffolder", &e.to_string());
        return print_err(
            "template scaffold failed",
            &e.to_string(),
            "see scaffolder output above; re-run with --template <name> --yes to skip prompts",
        );
    }

    // If a target directory was provided (--directory or positional) we
    // know where the project landed; persist device_id into .iii/project.ini
    // so the engine telemetry pipeline can associate runs with this project.
    // For interactive directory selection we skip this — the user can run
    // `iii project init` afterwards if they want a project.ini.
    let project_id_for_event = if let Some(root) = target_dir.as_ref() {
        if root.is_dir() {
            let device_id = iii::workers::telemetry::environment::get_or_create_device_id();
            let project_name = root
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("iii-project")
                .to_string();
            let mut ini = project_ini::ProjectIni::read(&root).unwrap_or_default();
            if ini.project_id.is_none() {
                ini.project_id = Some(uuid::Uuid::new_v4().to_string());
            }
            if ini.project_name.is_none() {
                ini.project_name = Some(project_name);
            }
            ini.source
                .get_or_insert_with(|| "init-template".to_string());
            ini.device_id.get_or_insert(device_id);
            let id = ini.project_id.clone().unwrap_or_default();
            if let Err(e) = ini.write(&root) {
                eprintln!(
                    "  {} could not persist .iii/project.ini: {}",
                    "warning:".yellow().bold(),
                    e
                );
            }
            id
        } else {
            String::new()
        }
    } else {
        // Interactive flow — emit a generic success event without project_id.
        String::new()
    };

    // The conversion event fires whenever the scaffolder succeeded, even if
    // the post-process project.ini write failed. The user got a working
    // project; the missing project.ini just means metrics won't link back.
    crate::cli::telemetry::send_project_init_succeeded(false, &project_id_for_event);

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

    if let Err(e) = docker::write_docker_assets(&root, &device_id) {
        return print_err(
            "could not write Docker assets",
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

fn resolve_device_id_for_docker(root: &std::path::Path) -> String {
    match project_ini::ProjectIni::read(root) {
        Ok(ini) => match ini.device_id {
            Some(id) => id,
            None => {
                warn_missing_project_ini(root, "device_id missing in .iii/project.ini");
                iii::workers::telemetry::environment::get_or_create_device_id()
            }
        },
        Err(_) => {
            warn_missing_project_ini(root, "no .iii/project.ini found");
            iii::workers::telemetry::environment::get_or_create_device_id()
        }
    }
}

fn warn_missing_project_ini(root: &std::path::Path, problem: &str) {
    eprintln!(
        "  {} {} at {}",
        "warning:".yellow().bold(),
        problem,
        root.display()
    );
    eprintln!(
        "  {} using a fresh device_id; metrics will not link to a project.",
        "impact:".dimmed()
    );
    eprintln!(
        "  {} run `iii project init` here to persist a project identity.",
        "fix:".dimmed()
    );
}

fn resolve_root(dir: Option<&str>) -> Result<std::path::PathBuf, String> {
    match dir {
        Some(d) => Ok(std::path::PathBuf::from(d)),
        None => std::env::current_dir().map_err(|e| format!("cannot read cwd: {}", e)),
    }
}

fn print_err(problem: &str, cause: &str, fix: &str) -> i32 {
    eprintln!("{} {}", "error:".red().bold(), problem);
    eprintln!("  {} {}", "cause:".dimmed(), cause);
    eprintln!("  {} {}", "fix:".dimmed(), fix);
    1
}
