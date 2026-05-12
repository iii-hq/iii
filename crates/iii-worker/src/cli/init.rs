// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii worker init` -- scaffold a standalone worker repo from scratch.

use clap::Args;
use colored::Colorize;
use scaffolder_core::cli::{
    apply_template_idempotent, build_fetcher, check_directory_state, print_err, resolve_root,
};
use std::path::Path;

#[derive(Args, Debug, Clone)]
pub struct InitArgs {
    /// Worker directory (positional). Equivalent to --directory.
    #[arg(value_name = "NAME")]
    pub name: Option<String>,

    /// Target directory (defaults to current directory). Takes precedence over the positional name.
    #[arg(short, long)]
    pub directory: Option<String>,

    /// Local directory to use for templates instead of fetching from remote
    /// (for template development and tests).
    #[arg(long = "template-dir")]
    pub template_dir: Option<String>,

    /// Allow scaffolding into a non-empty directory.
    #[arg(long = "allow-non-empty")]
    pub allow_non_empty: bool,
}

impl InitArgs {
    pub(crate) fn target_dir(&self) -> Option<&str> {
        self.directory.as_deref().or(self.name.as_deref())
    }
}

pub async fn run(args: InitArgs) -> i32 {
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
        return print_err(
            &format!("could not create {}", root.display()),
            &e.to_string(),
            "check parent directory permissions or pick a different --directory",
        );
    }

    if let Err(e) = check_directory_state(&root, args.allow_non_empty, "worker.ini") {
        return print_err(
            "target directory is not empty",
            &e,
            "pass --allow-non-empty to scaffold into an existing directory, or pick a different one",
        );
    }

    let worker_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("iii-worker")
        .to_string();

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

    if let Err(e) = apply_template_idempotent(&mut fetcher, "worker-bare", &root).await {
        return print_err(
            "could not apply 'worker-bare' template",
            &e.to_string(),
            "verify --template-dir points at a valid template tree, or check network",
        );
    }

    if let Err(e) = substitute_worker_name(&root, &worker_name) {
        return print_err(
            "could not write iii.worker.yaml with worker name",
            &e.to_string(),
            "check that iii.worker.yaml is writable",
        );
    }

    let worker_id = match persist_worker_ini(&root, &worker_name, "init") {
        Ok(id) => id,
        Err(e) => {
            return print_err(
                "could not write .iii/worker.ini",
                &e.to_string(),
                "check that the target directory is writable",
            );
        }
    };

    print_init_success(&worker_name, &root, target.is_some(), &worker_id);
    0
}

/// Replace the `{{worker_name}}` placeholder in `iii.worker.yaml` with the
/// scaffolded directory name. No-op if the file is absent or already edited.
fn substitute_worker_name(root: &Path, worker_name: &str) -> std::io::Result<()> {
    let path = root.join("iii.worker.yaml");
    if !path.exists() {
        return Ok(());
    }
    let contents = std::fs::read_to_string(&path)?;
    if !contents.contains("{{worker_name}}") {
        return Ok(());
    }
    let replaced = contents.replace("{{worker_name}}", worker_name);
    std::fs::write(&path, replaced)
}

fn persist_worker_ini(root: &Path, worker_name: &str, source: &str) -> anyhow::Result<String> {
    let worker_id =
        read_existing_worker_id(root).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let ini_dir = root.join(".iii");
    std::fs::create_dir_all(&ini_dir)?;
    let contents = format!(
        "[worker]\n\
         worker_id={worker_id}\n\
         name={worker_name}\n\
         source={source}\n",
    );
    std::fs::write(ini_dir.join("worker.ini"), contents)?;
    Ok(worker_id)
}

fn read_existing_worker_id(root: &Path) -> Option<String> {
    let path = root.join(".iii").join("worker.ini");
    let contents = std::fs::read_to_string(path).ok()?;
    contents
        .lines()
        .find_map(|l| l.trim().strip_prefix("worker_id="))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn print_init_success(worker_name: &str, root: &Path, target_specified: bool, worker_id: &str) {
    eprintln!();
    eprintln!(
        "  {} iii worker '{}' scaffolded at {}",
        "✓".green(),
        worker_name.bold(),
        root.display()
    );
    eprintln!("  {} {}", "id:".dimmed(), worker_id);
    eprintln!();
    eprintln!("  Next steps:");
    if target_specified {
        eprintln!("    {}", format!("cd {}", root.display()).bold());
    }
    eprintln!(
        "    edit {}    # declare your entry point",
        "iii.worker.yaml".bold()
    );
    eprintln!(
        "    {}    # from a parent iii project",
        "iii worker add ./path/to/this-worker".bold()
    );
    eprintln!();
    eprintln!("  Docs: https://iii.dev/docs/quickstart");
}
