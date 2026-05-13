// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii worker init` -- scaffold a standalone worker repo. Picks one language
//! (typescript / javascript / python / rust) and drops a runnable hello-world
//! plus the iii SDK pinned in the language's manifest.
//!
//! Implementation strategy mirrors `iii project init --template foo`
//! (`engine/src/cli/project/mod.rs:run_init_with_template`):
//!   1. Resolve target dir and check state up front.
//!   2. Build `scaffolder_core::tui::CreateArgs` with template fixed to
//!      `worker-bare`, directory pre-set, languages pre-set when `--language`
//!      was given.
//!   3. Hand off to `scaffolder_core::run` -- that drives the cliclack TUI
//!      when interactive, or a non-interactive single-language scaffold
//!      when languages are pre-set.
//!   4. Post-process: substitute `{{worker_name}}` / `{{worker_language}}` /
//!      `{{worker_entry}}` into `iii.worker.yaml`, persist `.iii/worker.ini`,
//!      print worker-specific success message.

use clap::Args;
use colored::Colorize;
use scaffolder_core::cli::{check_directory_state, print_err, resolve_root};
use scaffolder_core::{IiiConfig, tui};
use std::path::{Path, PathBuf};

/// One language per worker. `parse_language_arg` normalises long/short
/// aliases (`typescript` <-> `ts`, etc.) into the short form before this
/// enum is constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerLanguage {
    Ts,
    Js,
    Py,
    Rust,
}

impl WorkerLanguage {
    pub fn short(&self) -> &'static str {
        match self {
            WorkerLanguage::Ts => "ts",
            WorkerLanguage::Js => "js",
            WorkerLanguage::Py => "py",
            WorkerLanguage::Rust => "rust",
        }
    }

    /// Map to the scaffolder-core language key used by template manifests
    /// (`typescript`, `javascript`, `python`, `rust`).
    pub fn manifest_key(&self) -> &'static str {
        match self {
            WorkerLanguage::Ts => "typescript",
            WorkerLanguage::Js => "javascript",
            WorkerLanguage::Py => "python",
            WorkerLanguage::Rust => "rust",
        }
    }

    /// Default entry file for the language. Substituted into the scaffolded
    /// `iii.worker.yaml` via the `{{worker_entry}}` placeholder.
    pub fn default_entry(&self) -> &'static str {
        match self {
            WorkerLanguage::Ts => "./src/index.ts",
            WorkerLanguage::Js => "./src/index.js",
            WorkerLanguage::Py => "./main.py",
            WorkerLanguage::Rust => "./src/main.rs",
        }
    }
}

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

    /// Allow scaffolding into a non-empty directory. Re-running init in a
    /// directory with `.iii/worker.ini` is always allowed (idempotent re-init).
    #[arg(long = "allow-non-empty")]
    pub allow_non_empty: bool,

    /// Worker language (typescript | javascript | python | rust). Accepts
    /// short aliases (`ts`, `js`, `py`, `rust`, `rs`). When omitted, the
    /// user is prompted interactively. Providing this flag fully scripts
    /// the scaffold.
    #[arg(short = 'l', long, value_name = "LANG", value_parser = parse_language_arg)]
    pub language: Option<String>,

    /// Skip the iii-engine version compatibility check enforced by the
    /// scaffolder. Mirrors the flag on `iii project init`.
    #[arg(long = "skip-iii")]
    pub skip_iii: bool,
}

/// Accept long and short language aliases; normalize to short form (`ts`,
/// `js`, `py`, `rust`) so downstream code matches on one set.
fn parse_language_arg(s: &str) -> Result<String, String> {
    match s.to_ascii_lowercase().as_str() {
        "ts" | "typescript" => Ok("ts".into()),
        "js" | "javascript" => Ok("js".into()),
        "py" | "python" => Ok("py".into()),
        "rust" | "rs" => Ok("rust".into()),
        other => Err(format!(
            "invalid value '{other}' for '--language': possible values: ts, js, py, rust"
        )),
    }
}

impl InitArgs {
    pub(crate) fn target_dir(&self) -> Option<&str> {
        self.directory.as_deref().or(self.name.as_deref())
    }

    /// Resolve the chosen language as a `WorkerLanguage`. Returns `None`
    /// when `--language` was not given (interactive mode prompts the user).
    pub(crate) fn resolved_language(&self) -> Option<WorkerLanguage> {
        match self.language.as_deref() {
            Some("ts") => Some(WorkerLanguage::Ts),
            Some("js") => Some(WorkerLanguage::Js),
            Some("py") => Some(WorkerLanguage::Py),
            Some("rust") => Some(WorkerLanguage::Rust),
            // `parse_language_arg` guards every other branch.
            Some(_) | None => None,
        }
    }
}

pub async fn run(args: InitArgs) -> i32 {
    // Restore terminal cursor on panic / Ctrl+C -- scaffolder runs cliclack.
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = console::Term::stderr().show_cursor();
        default_panic(info);
    }));
    let _ = ctrlc::set_handler(move || {
        let _ = console::Term::stderr().show_cursor();
        std::process::exit(130);
    });

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

    // Snapshot pre-existing files so re-init can restore user edits. The
    // scaffolder's `copy_template` is not write-if-absent; without this
    // snapshot, a second `iii worker init` would clobber an edited
    // `iii.worker.yaml`, `package.json`, etc.
    let snapshots = snapshot_existing_files(&root);

    let resolved_lang = args.resolved_language();

    // Cliclack needs a TTY to prompt. In CI / piped invocations the prompt
    // would hang or panic -- short-circuit with a helpful error instead.
    if resolved_lang.is_none() {
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            return print_err(
                "no language selected",
                "stdin is not a TTY, so the interactive language picker cannot run",
                "pass --language <ts|js|py|rust>",
            );
        }
    }

    let languages_arg: Option<Vec<String>> =
        resolved_lang.map(|l| vec![l.manifest_key().to_string()]);

    // When the user pre-selected a language (and therefore can't be
    // interrupted by a language picker), set `yes: true` so scaffolder
    // also skips its directory-confirm prompt. With no language set,
    // `yes` stays false and cliclack drives the full TTY flow.
    let create_args = tui::CreateArgs {
        template_dir: args.template_dir.as_ref().map(PathBuf::from),
        template: Some("worker-bare".to_string()),
        directory: Some(root.clone()),
        languages: languages_arg,
        skip_tool_check: args.skip_iii,
        yes: resolved_lang.is_some(),
    };

    let result = scaffolder_core::run(&IiiConfig, create_args, env!("CARGO_PKG_VERSION")).await;
    let _ = console::Term::stderr().show_cursor();

    if let Err(e) = result {
        return print_err(
            "could not scaffold worker",
            &e.to_string(),
            "see scaffolder output above; pass --language <lang> for non-interactive mode",
        );
    }

    // Restore pre-existing files that the scaffolder overwrote. Skipped
    // for fresh inits (`snapshots` is empty).
    if let Err(e) = restore_snapshots(&root, &snapshots) {
        return print_err(
            "could not restore pre-existing files after scaffold",
            &e.to_string(),
            "inspect the target dir manually",
        );
    }

    // For `--language` runs we already know the language. For interactive
    // runs, recover it by parsing the `language:` line of the scaffolded
    // `iii.worker.yaml` (scaffolder substituted it via the shared file's
    // `{{worker_language}}` placeholder).
    let final_lang = resolved_lang.or_else(|| detect_language_from_yaml(&root));
    let final_lang = match final_lang {
        Some(l) => l,
        None => {
            return print_err(
                "could not determine the scaffolded language",
                "iii.worker.yaml did not include a recognized language field",
                "re-run with --language <ts|js|py|rust>",
            );
        }
    };

    if let Err(e) = substitute_yaml_placeholders(&root, &worker_name, final_lang) {
        return print_err(
            "could not write iii.worker.yaml",
            &e.to_string(),
            "check that iii.worker.yaml is writable",
        );
    }

    let worker_id = match persist_worker_ini(&root, &worker_name, "init", final_lang) {
        Ok(id) => id,
        Err(e) => {
            return print_err(
                "could not write .iii/worker.ini",
                &e.to_string(),
                "check that the target directory is writable",
            );
        }
    };

    print_init_success(
        &worker_name,
        &root,
        target.is_some(),
        &worker_id,
        final_lang,
    );
    0
}

/// Read every file under `root` into memory so the caller can restore
/// originals after a clobber-prone scaffold pass. Skips `.iii/` (we own
/// that dir) and entries that don't exist or fail to read; an empty map
/// means "nothing to restore" and `restore_snapshots` becomes a no-op.
fn snapshot_existing_files(root: &Path) -> std::collections::HashMap<PathBuf, Vec<u8>> {
    let mut out = std::collections::HashMap::new();
    if !root.exists() {
        return out;
    }
    fn walk(
        dir: &Path,
        root: &Path,
        out: &mut std::collections::HashMap<PathBuf, Vec<u8>>,
    ) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            // Skip iii-managed state; we rewrite it post-scaffold.
            if path
                .strip_prefix(root)
                .ok()
                .and_then(|rel| rel.components().next())
                .map(|c| c.as_os_str() == ".iii")
                .unwrap_or(false)
            {
                continue;
            }
            let ft = entry.file_type()?;
            if ft.is_dir() {
                walk(&path, root, out)?;
            } else if ft.is_file()
                && let Ok(bytes) = std::fs::read(&path)
                && let Ok(rel) = path.strip_prefix(root)
            {
                out.insert(rel.to_path_buf(), bytes);
            }
        }
        Ok(())
    }
    let _ = walk(root, root, &mut out);
    out
}

/// Re-write the snapshotted files over whatever the scaffolder wrote.
/// Effectively makes the scaffold write-if-absent at the worker-init level
/// without changing `copy_template` semantics for everyone else.
fn restore_snapshots(
    root: &Path,
    snapshots: &std::collections::HashMap<PathBuf, Vec<u8>>,
) -> std::io::Result<()> {
    for (rel, bytes) in snapshots {
        let dst = root.join(rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dst, bytes)?;
    }
    Ok(())
}

/// Substitute `{{worker_name}}`, `{{worker_language}}`, `{{worker_entry}}`
/// into `iii.worker.yaml`. No-op if the file is absent (test fixture
/// short-circuit) or the placeholders are gone (idempotent re-run).
fn substitute_yaml_placeholders(
    root: &Path,
    worker_name: &str,
    lang: WorkerLanguage,
) -> std::io::Result<()> {
    let path = root.join("iii.worker.yaml");
    if !path.exists() {
        return Ok(());
    }
    let contents = std::fs::read_to_string(&path)?;
    if !contents.contains("{{") {
        return Ok(());
    }
    let replaced = contents
        .replace("{{worker_name}}", worker_name)
        .replace("{{worker_language}}", lang.manifest_key())
        .replace("{{worker_entry}}", lang.default_entry());
    std::fs::write(&path, replaced)
}

/// Detect the language the scaffolder picked. Two strategies:
///
/// 1. Read `iii.worker.yaml`'s `language:` line. Works only when scaffolder
///    already substituted `{{worker_language}}` (it doesn't on the
///    interactive path; the engine substitutes post-scaffold).
/// 2. Sniff language-specific files the scaffolder dropped: `Cargo.toml`
///    -> Rust, `pyproject.toml` -> Python, `tsconfig.json` -> TypeScript,
///    `package.json` (without `tsconfig.json`) -> JavaScript. This is the
///    primary signal on the interactive path because the language picker's
///    selection ends up as file presence, not as a yaml field.
fn detect_language_from_yaml(root: &Path) -> Option<WorkerLanguage> {
    // Strategy 1: explicit `language:` field with a known value.
    let yaml = root.join("iii.worker.yaml");
    if let Ok(contents) = std::fs::read_to_string(&yaml) {
        for line in contents.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("language:") {
                let value = rest.trim().trim_matches('"').trim_matches('\'');
                let parsed = match value {
                    "typescript" | "ts" => Some(WorkerLanguage::Ts),
                    "javascript" | "js" => Some(WorkerLanguage::Js),
                    "python" | "py" => Some(WorkerLanguage::Py),
                    "rust" | "rs" => Some(WorkerLanguage::Rust),
                    _ => None, // includes the un-substituted "{{worker_language}}"
                };
                if parsed.is_some() {
                    return parsed;
                }
                break;
            }
        }
    }

    // Strategy 2: file-presence heuristic. The scaffolder only drops the
    // files matching the selected language (gated by root manifest's
    // `language_files`), so file presence is a reliable proxy.
    if root.join("Cargo.toml").exists() {
        return Some(WorkerLanguage::Rust);
    }
    if root.join("pyproject.toml").exists() {
        return Some(WorkerLanguage::Py);
    }
    if root.join("tsconfig.json").exists() {
        return Some(WorkerLanguage::Ts);
    }
    if root.join("package.json").exists() {
        // No tsconfig.json -> JS scaffold.
        return Some(WorkerLanguage::Js);
    }
    None
}

fn persist_worker_ini(
    root: &Path,
    worker_name: &str,
    source: &str,
    lang: WorkerLanguage,
) -> anyhow::Result<String> {
    let worker_id =
        read_existing_worker_id(root).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let ini_dir = root.join(".iii");
    std::fs::create_dir_all(&ini_dir)?;
    let contents = format!(
        "[worker]\n\
         worker_id={worker_id}\n\
         name={worker_name}\n\
         source={source}\n\
         language={lang_short}\n",
        lang_short = lang.short(),
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

fn print_init_success(
    worker_name: &str,
    root: &Path,
    target_specified: bool,
    worker_id: &str,
    lang: WorkerLanguage,
) {
    eprintln!();
    eprintln!(
        "  {} iii worker '{}' ({}) scaffolded at {}",
        "✓".green(),
        worker_name.bold(),
        lang.short(),
        root.display()
    );
    eprintln!("  {} {}", "id:".dimmed(), worker_id);
    eprintln!();
    eprintln!("  Next steps:");
    if target_specified {
        eprintln!("    {}", format!("cd {}", root.display()).bold());
    }
    let install_hint = match lang {
        WorkerLanguage::Ts | WorkerLanguage::Js => "npm install",
        WorkerLanguage::Py => "uv sync",
        WorkerLanguage::Rust => "cargo build",
    };
    eprintln!("    {}    # install the iii SDK", install_hint.bold());
    eprintln!(
        "    edit {}    # add your function handlers",
        lang.default_entry().bold()
    );
    eprintln!(
        "    {}    # from a parent iii project",
        "iii worker add ./path/to/this-worker".bold()
    );
    eprintln!();
    eprintln!("  Docs: https://iii.dev/docs/quickstart");
}
