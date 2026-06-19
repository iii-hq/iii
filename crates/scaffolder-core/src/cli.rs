// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Shared scaffolding helpers used by both `iii project init` (engine) and
//! `iii worker init` (iii-worker crate). Keeping these in one place prevents
//! the dotfile-exemption and `data/`-filter rules from drifting.

use crate::{IiiConfig, ProductConfig, TemplateFetcher, copy_template};
use colored::Colorize;
use std::path::{Path, PathBuf};

/// Resolve the scaffold target directory.
///
/// `None` -> current working directory.
/// `Some("")` -> error (empty argument).
/// `Some(path)` -> that path as a `PathBuf`.
pub fn resolve_root(dir: Option<&str>) -> Result<PathBuf, String> {
    match dir {
        Some(d) if d.trim().is_empty() => Err("directory argument cannot be empty".to_string()),
        Some(d) => Ok(PathBuf::from(d)),
        None => std::env::current_dir().map_err(|e| format!("cannot read cwd: {}", e)),
    }
}

/// Reject scaffolding into a non-empty directory unless the user opted in via
/// `allow_non_empty`, OR the directory is already initialized (detected by
/// the presence of `.iii/<marker_file>`).
///
/// Hidden dotfiles (`.git/`, `.gitignore`, etc.) are not considered "user
/// content"; anything else, including a `data/` directory, blocks the
/// scaffold (an existing `data/` belongs to some engine and clobbering its
/// project would orphan that state).
///
/// `marker_file` lets each caller specify its own marker: project init uses
/// `"project.ini"`, worker init uses `"worker.ini"`.
pub fn check_directory_state(
    root: &Path,
    allow_non_empty: bool,
    marker_file: &str,
) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    if !root.is_dir() {
        return Err(format!("{} exists but is not a directory", root.display()));
    }
    if root.join(".iii").join(marker_file).exists() {
        return Ok(());
    }
    if allow_non_empty {
        return Ok(());
    }
    let entries: Vec<String> = match std::fs::read_dir(root) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| !name.starts_with('.'))
            .collect(),
        Err(e) => return Err(format!("read {}: {e}", root.display())),
    };
    if entries.is_empty() {
        Ok(())
    } else {
        let mut sample = entries.clone();
        sample.sort();
        let preview: Vec<String> = sample.iter().take(5).cloned().collect();
        let suffix = if sample.len() > 5 {
            format!(", and {} more", sample.len() - 5)
        } else {
            String::new()
        };
        Err(format!(
            "{} contains {}{}",
            root.display(),
            preview.join(", "),
            suffix
        ))
    }
}

/// Build a `TemplateFetcher` -- local-dir fixture when `template_dir` is set,
/// otherwise the canonical remote source from `IiiConfig`.
pub fn build_fetcher(template_dir: Option<&str>) -> anyhow::Result<TemplateFetcher> {
    if let Some(dir) = template_dir {
        Ok(TemplateFetcher::from_local(
            PathBuf::from(dir),
            IiiConfig.name(),
        ))
    } else {
        TemplateFetcher::from_config(&IiiConfig)
    }
}

/// Apply a template to `target`, skipping any files that already exist at the
/// destination. This is the idempotent variant -- safe to re-run without
/// clobbering user edits to `.gitignore`, `iii.worker.yaml`, or any other
/// scaffolded file. Equivalent to `write_if_absent` per-file.
///
/// Merges the root manifest's `language_files` with the per-template overrides.
pub async fn apply_template_idempotent(
    fetcher: &mut TemplateFetcher,
    template_name: &str,
    target: &Path,
) -> anyhow::Result<()> {
    let root_manifest = fetcher.fetch_root_manifest().await?;
    let manifest = fetcher.fetch_template_manifest(template_name).await?;
    let mut language_files = root_manifest.language_files.clone();
    language_files.merge(&manifest.language_files);

    // copy_template writes files unconditionally. To get write-if-absent
    // behavior, scaffold into a tempdir first, then walk it and copy each
    // entry to `target` only when the destination doesn't already exist.
    let staging = tempfile::tempdir()?;
    copy_template(
        fetcher,
        template_name,
        &manifest,
        staging.path(),
        &[],
        &language_files,
    )
    .await?;
    merge_dir_if_absent(staging.path(), target)?;
    Ok(())
}

fn merge_dir_if_absent(src: &Path, dst: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let ft = entry.file_type()?;
        if ft.is_dir() {
            std::fs::create_dir_all(&to)?;
            merge_dir_if_absent(&from, &to)?;
        } else if !to.exists() {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Print a standardized error block (red header, dimmed cause + fix lines) and
/// return exit code `1`.
pub fn print_err(problem: &str, cause: &str, fix: &str) -> i32 {
    eprintln!("{} {}", "error:".red().bold(), problem);
    eprintln!("  {} {}", "cause:".dimmed(), cause);
    eprintln!("  {} {}", "fix:".dimmed(), fix);
    1
}
