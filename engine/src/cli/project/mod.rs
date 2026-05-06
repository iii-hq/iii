// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
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
    /// Target directory (defaults to current directory)
    #[arg(short, long)]
    pub directory: Option<String>,

    /// Also generate Docker assets (Dockerfile, docker-compose.yml, .env)
    #[arg(long)]
    pub docker: bool,
}

#[derive(Args, Debug, Clone)]
pub struct GenerateDockerArgs {
    /// Target directory (defaults to current directory)
    #[arg(short, long)]
    pub directory: Option<String>,
}

use colored::Colorize;

pub async fn run(args: ProjectArgs) -> i32 {
    match args.action {
        ProjectAction::Init(init) => run_init(init).await,
        ProjectAction::GenerateDocker(gd) => run_generate_docker(gd).await,
    }
}

async fn run_init(args: InitArgs) -> i32 {
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
    if args.directory.is_some() {
        eprintln!("    {}", format!("cd {}", project_name).bold());
    }
    eprintln!("    {}    # add a worker", "iii worker add <package>".bold());
    eprintln!("    {}                          # start the engine", "iii".bold());
    if args.docker {
        eprintln!("    {}           # or start in Docker", "docker compose up".bold());
    }
    eprintln!();
    eprintln!("  Docs: https://iii.dev/docs/quickstart");
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
    eprintln!("  {} {} at {}", "warning:".yellow().bold(), problem, root.display());
    eprintln!("  {} using a fresh device_id; metrics will not link to a project.", "impact:".dimmed());
    eprintln!("  {} run `iii project init` here to persist a project identity.", "fix:".dimmed());
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
