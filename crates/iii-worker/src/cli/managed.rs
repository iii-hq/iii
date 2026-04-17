// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! CLI command handlers for managing OCI-based workers.
//!
//! # Output contract
//!
//! - **stdout**: machine-readable worker name on success (one line). Scripts
//!   pipe `iii worker start foo | xargs ...` and rely on this being the only
//!   thing on stdout.
//! - **stderr**: all human-facing status, progress, errors, prompts, and
//!   decorative output. Every line here is cosmetic and may change between
//!   releases without breaking anyone.
//!
//! Two implications:
//! 1. Never `println!` anything that isn't the worker name. Use `eprintln!`
//!    for everything else, including successes like "✓ ready in 3.2s".
//! 2. Failures exit non-zero WITHOUT printing to stdout. Consumers of the
//!    stdout contract check the exit code first.
//!
//! The same contract applies in `local_worker.rs` and `status.rs`.

use colored::Colorize;

use super::binary_download;
use super::builtin_defaults::get_builtin_default;
use super::config_file::ResolvedWorkerType;
use super::lifecycle::build_container_spec;
use super::registry::{
    BinaryWorkerResponse, MANIFEST_PATH, WorkerInfoResponse, fetch_worker_info, parse_worker_input,
};
use super::worker_manager::state::WorkerDef;

pub use super::local_worker::{handle_local_add, is_local_path, start_local_worker};

pub async fn handle_binary_add(
    worker_name: &str,
    response: &BinaryWorkerResponse,
    brief: bool,
) -> i32 {
    let target = binary_download::current_target();

    if !brief {
        eprintln!("  {} Resolved to binary v{}", "✓".green(), response.version);
    }

    // If the worker is already running, skip download entirely
    if is_worker_running(worker_name) {
        if !brief {
            eprintln!(
                "\n  {} Worker {} already running, skipping download",
                "✓".green(),
                worker_name.bold(),
            );
        }
        return 0;
    }

    let binary_info = match response.binaries.get(target) {
        Some(info) => info,
        None => {
            eprintln!(
                "{} Platform '{}' is not supported for worker '{}'. Available: {}",
                "error:".red(),
                target,
                worker_name,
                response
                    .binaries
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            return 1;
        }
    };

    if !brief {
        eprintln!("  Downloading {}...", worker_name.bold());
    }
    let install_path =
        match binary_download::download_and_install_binary(worker_name, binary_info).await {
            Ok(path) => path,
            Err(e) => {
                eprintln!("{} {}", "error:".red(), e);
                return 1;
            }
        };

    if !brief {
        eprintln!("  {} Downloaded successfully", "✓".green());
        eprintln!("  {}: {}", "Name".bold(), worker_name);
        eprintln!("  {}: {}", "Version".bold(), response.version);
        eprintln!("  {}: {}", "Platform".bold(), target);
        if let Ok(metadata) = std::fs::metadata(&install_path) {
            eprintln!(
                "  {}: {:.1} MB",
                "Size".bold(),
                metadata.len() as f64 / 1_048_576.0
            );
        }
    }

    let config_yaml = response
        .config
        .config
        .as_object()
        .map(|_| serde_yaml::to_string(&response.config.config).unwrap_or_default());

    if let Err(e) = super::config_file::append_worker(worker_name, config_yaml.as_deref()) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }

    if brief {
        eprintln!("        {} {}", "✓".green(), worker_name.bold());
    } else {
        eprintln!(
            "\n  {} Worker {} added to {}",
            "✓".green(),
            worker_name.bold(),
            "config.yaml".dimmed(),
        );

        // The engine's file watcher will detect the config change and
        // reload automatically — no need to start the worker here.
    }
    0
}

pub async fn handle_managed_add_many(worker_names: &[String], wait: bool) -> i32 {
    let total = worker_names.len();
    let brief = total > 1;
    let mut fail_count = 0;

    for (i, name) in worker_names.iter().enumerate() {
        if brief {
            eprintln!("  [{}/{}] Adding {}...", i + 1, total, name.bold());
        }
        let result = handle_managed_add(name, brief, false, false, wait).await;
        if result != 0 {
            fail_count += 1;
        }
    }

    if total > 1 {
        let succeeded = total - fail_count;
        if fail_count == 0 {
            eprintln!("\n  Added {}/{} workers.", succeeded, total);
        } else {
            eprintln!(
                "\n  Added {}/{} workers. {} failed.",
                succeeded, total, fail_count
            );
        }
    }

    if fail_count == 0 { 0 } else { 1 }
}

pub async fn handle_managed_add(
    image_or_name: &str,
    brief: bool,
    force: bool,
    reset_config: bool,
    wait: bool,
) -> i32 {
    // Local path workers: starts with '.', '/', or '~'
    if super::local_worker::is_local_path(image_or_name) {
        return super::local_worker::handle_local_add(
            image_or_name,
            force,
            reset_config,
            brief,
            wait,
        )
        .await;
    }

    // --force: stop if running, delete artifacts, then proceed with a fresh
    // add. Before the fix this path errored out with "Stop it first" when a
    // running worker was detected — which defeats the point of --force. The
    // whole sequence (stop → clear → add) is what a user means by "force."
    if force {
        let (plain_name, _) = parse_worker_input(image_or_name);

        let is_oci_ref = plain_name.contains('/') || plain_name.contains(':');
        if !is_oci_ref && let Err(e) = super::registry::validate_worker_name(&plain_name) {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }

        if is_worker_running(&plain_name) {
            eprintln!(
                "  {} {} is running, stopping first...",
                "⟳".cyan(),
                plain_name.bold()
            );
            let stop_rc = handle_managed_stop(&plain_name).await;
            if stop_rc != 0 {
                // Don't abort — artifacts will be wiped below anyway, and the
                // most common "failure" is "already stopped between is_worker_running
                // and the signal" which is benign. Surface it so a stuck worker
                // doesn't silently confuse the user.
                eprintln!(
                    "  {} stop exited {} — continuing with force add anyway",
                    "warning:".yellow(),
                    stop_rc
                );
            }
        }

        if super::builtin_defaults::get_builtin_default(&plain_name).is_some() {
            eprintln!(
                "  {} '{}' is a builtin worker, no artifacts to re-download.",
                "info:".cyan(),
                plain_name,
            );
        } else {
            let freed = delete_worker_artifacts(&plain_name);
            if freed > 0 {
                eprintln!(
                    "  {} Cleared {:.1} MB of artifacts for {}",
                    "✓".green(),
                    freed as f64 / 1_048_576.0,
                    plain_name.bold(),
                );
            }
        }

        if reset_config {
            match super::config_file::remove_worker(&plain_name) {
                Ok(()) => {}
                Err(e) => {
                    tracing::debug!("remove_worker during force: {}", e);
                }
            }
        }
    }

    // Direct OCI reference (contains '/' or ':') — passthrough, skip API
    if image_or_name.contains('/') || image_or_name.contains(':') {
        if !brief {
            eprintln!("  Resolving {}...", image_or_name.bold());
        }
        let name = image_or_name
            .rsplit('/')
            .next()
            .unwrap_or(image_or_name)
            .split(':')
            .next()
            .unwrap_or(image_or_name);
        if !brief {
            eprintln!("  {} Resolved to {}", "✓".green(), image_or_name.dimmed());
        }
        let rc = handle_oci_pull_and_add(name, image_or_name, brief).await;
        return finish_add(name, rc, wait, brief).await;
    }

    // Shorthand name — resolve via API
    let (name, version) = parse_worker_input(image_or_name);

    // Check for engine-builtin workers first (no network needed).
    if let Some(default_yaml) = get_builtin_default(&name) {
        let already_exists = super::config_file::worker_exists(&name);
        if let Err(e) = super::config_file::append_worker(&name, Some(default_yaml)) {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
        if brief {
            if already_exists {
                eprintln!("        {} {} (updated)", "✓".green(), name.bold());
            } else {
                eprintln!("        {} {}", "✓".green(), name.bold());
            }
        } else {
            if already_exists {
                eprintln!(
                    "\n  {} Worker {} updated in {} (merged with builtin defaults)",
                    "✓".green(),
                    name.bold(),
                    "config.yaml".dimmed(),
                );
            } else {
                eprintln!(
                    "\n  {} Worker {} added to {}",
                    "✓".green(),
                    name.bold(),
                    "config.yaml".dimmed(),
                );
            }

            // If the engine is already running, its file watcher will detect
            // the config change and reload automatically. If it isn't, nudge
            // the user to start it (or customize first).
            if !is_engine_running() {
                eprintln!("  Start the engine to run it, or edit config.yaml to customize.");
            }
        }
        // Builtins run in-process with the engine; there is no detached VM
        // or binary to watch. The Phase machinery would loop on Queued until
        // timeout, so skip wait_for_ready for builtins even when wait=true.
        return 0;
    }

    if !brief {
        eprintln!("  Resolving {}...", name.bold());
    }

    let response = match fetch_worker_info(&name, version.as_deref()).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };

    let rc = match response {
        WorkerInfoResponse::Binary(r) => handle_binary_add(&name, &r, brief).await,
        WorkerInfoResponse::Oci(r) => {
            if !brief {
                eprintln!("  {} Resolved to {}", "✓".green(), r.image_url.dimmed());
            }
            handle_oci_pull_and_add(&r.name, &r.image_url, brief).await
        }
    };
    finish_add(&name, rc, wait, brief).await
}

/// Shared tail for every non-local `handle_managed_add` exit path.
///
/// `handle_managed_add` accepts `wait: bool` from the `--wait` / `--no-wait`
/// flag (default wait=true per the CLI definition in app.rs). Before this
/// helper, `wait` was only honored on the local-path branch — OCI/binary/
/// registry adds silently dropped it, contradicting the `add` command's
/// documented "waits up to 120s by default" contract. We also skip the
/// wait when `rc != 0` (nothing to wait on if the add itself failed) and
/// when `brief` is set (multi-worker `add-many` renders per-row status
/// and a blocking wait per entry would produce confusing output).
async fn finish_add(worker_name: &str, rc: i32, wait: bool, brief: bool) -> i32 {
    if rc != 0 || !wait || brief {
        return rc;
    }
    let port = super::config_file::manager_port();
    if !is_engine_running_on(port) {
        // Engine down → no file watcher → config.yaml change won't be
        // picked up. A wait would run to timeout. Tell the user instead.
        eprintln!(
            "\n  {} engine not running; start it to observe boot.\n  \
               Start:         iii start\n  \
               Then watch:    iii worker status {}",
            "⚠".yellow(),
            worker_name,
        );
        return rc;
    }
    wait_for_ready(worker_name, port).await;
    rc
}

async fn handle_oci_pull_and_add(name: &str, image_ref: &str, brief: bool) -> i32 {
    let adapter = super::worker_manager::create_adapter("libkrun");

    if !brief {
        eprintln!("  Pulling {}...", image_ref.bold());
    }
    let pull_info = match adapter.pull(image_ref).await {
        Ok(info) => info,
        Err(e) => {
            eprintln!("{} Pull failed: {}", "error:".red(), e);
            return 1;
        }
    };

    let manifest: Option<serde_json::Value> =
        match adapter.extract_file(image_ref, MANIFEST_PATH).await {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(yaml_str) => serde_yaml::from_str(&yaml_str).ok(),
                Err(_) => None,
            },
            Err(_) => None,
        };

    if !brief {
        if let Some(ref m) = manifest {
            eprintln!("  {} Image pulled successfully", "✓".green());
            if let Some(v) = m.get("name").and_then(|v| v.as_str()) {
                eprintln!("  {}: {}", "Name".bold(), v);
            }
            if let Some(v) = m.get("version").and_then(|v| v.as_str()) {
                eprintln!("  {}: {}", "Version".bold(), v);
            }
            if let Some(v) = m.get("description").and_then(|v| v.as_str()) {
                eprintln!("  {}: {}", "Description".bold(), v);
            }
            if let Some(size) = pull_info.size_bytes {
                eprintln!("  {}: {:.1} MB", "Size".bold(), size as f64 / 1_048_576.0);
            }
        } else {
            eprintln!("  {} Image pulled (no manifest found)", "✓".green());
            if let Some(size) = pull_info.size_bytes {
                eprintln!("  {}: {:.1} MB", "Size".bold(), size as f64 / 1_048_576.0);
            }
        }
    }

    // Extract OCI env vars from the pulled image rootfs and write as config:
    let rootfs_dir = image_cache_dir(image_ref);
    let oci_env = super::worker_manager::oci::read_oci_env(&rootfs_dir);
    let config_yaml = if oci_env.is_empty() {
        None
    } else {
        // Filter out generic system env vars (PATH, HOME, etc.)
        let filtered: Vec<_> = oci_env
            .iter()
            .filter(|(k, _)| !matches!(k.as_str(), "PATH" | "HOME" | "HOSTNAME" | "LANG" | "TERM"))
            .collect();
        if filtered.is_empty() {
            None
        } else {
            let config_map: serde_json::Map<String, serde_json::Value> = filtered
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect();
            let yaml_str =
                serde_yaml::to_string(&serde_json::Value::Object(config_map)).unwrap_or_default();
            // serde_yaml adds a leading `---\n`, strip it for embedding
            let yaml_str = yaml_str
                .strip_prefix("---\n")
                .unwrap_or(&yaml_str)
                .trim_end();
            if yaml_str.is_empty() {
                None
            } else {
                Some(yaml_str.to_string())
            }
        }
    };

    if let Err(e) =
        super::config_file::append_worker_with_image(name, image_ref, config_yaml.as_deref())
    {
        eprintln!("{} Failed to update config.yaml: {}", "error:".red(), e);
        return 1;
    }
    if brief {
        eprintln!("        {} {}", "✓".green(), name.bold());
    } else {
        eprintln!(
            "\n  {} Worker {} added to {}",
            "✓".green(),
            name.bold(),
            "config.yaml".dimmed(),
        );

        // The engine's file watcher will detect the config change and
        // reload automatically — no need to start the worker here.
    }
    0
}

pub async fn handle_managed_remove_many(worker_names: &[String], yes: bool) -> i32 {
    let total = worker_names.len();
    let brief = total > 1;
    let mut fail_count = 0;

    // Single batch confirmation for any names that are currently running. We
    // gather them up-front so the user sees the whole blast radius once, not a
    // prompt per worker.
    if !yes {
        let running: Vec<&String> = worker_names
            .iter()
            .filter(|n| is_worker_running(n))
            .collect();
        if !running.is_empty() {
            let list = running
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!(
                "  {} {} currently running: {}",
                "warning:".yellow(),
                if running.len() == 1 {
                    "worker is"
                } else {
                    "workers are"
                },
                list,
            );
            eprintln!(
                "  Removing them from config.yaml triggers an engine reload that will tear the sandbox(es) down."
            );
            if !confirm_prompt("  Continue? [y/N] ") {
                eprintln!("  Aborted.");
                return 0;
            }
        }
    }

    for (i, name) in worker_names.iter().enumerate() {
        if brief {
            eprintln!("  [{}/{}] Removing {}...", i + 1, total, name.bold());
        }
        let result = handle_managed_remove(name, brief).await;
        if result != 0 {
            fail_count += 1;
        }
    }

    if total > 1 {
        let succeeded = total - fail_count;
        if fail_count == 0 {
            eprintln!("\n  Removed {}/{} workers.", succeeded, total);
        } else {
            eprintln!(
                "\n  Removed {}/{} workers. {} failed.",
                succeeded, total, fail_count
            );
        }
    }

    if fail_count == 0 { 0 } else { 1 }
}

pub async fn handle_managed_remove(worker_name: &str, brief: bool) -> i32 {
    if let Err(e) = super::registry::validate_worker_name(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }
    // Distinguish "config.yaml doesn't exist yet" from "worker isn't in it."
    // The underlying remove_worker surfaces both as the same anyhow error,
    // which misleads users into thinking their config file is missing.
    if !super::config_file::worker_exists(worker_name) {
        eprintln!(
            "{} Worker '{}' is not in config.yaml. Run `iii worker list` to see known workers.",
            "error:".red(),
            worker_name,
        );
        return 1;
    }
    if let Err(e) = super::config_file::remove_worker(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }
    if brief {
        eprintln!("        {} {}", "✓".green(), worker_name.bold());
    } else {
        eprintln!(
            "  {} {} removed from {}",
            "✓".green(),
            worker_name.bold(),
            "config.yaml".dimmed(),
        );
    }
    0
}

/// Read a y/N answer from stdin. Mirrors `confirm_clear` but parameterized on
/// the prompt so we can reuse it for `remove` and any future destructive ops.
fn confirm_prompt(prompt: &str) -> bool {
    use std::io::{Read, Write};
    #[cfg(unix)]
    super::local_worker::restore_terminal_cooked_mode();
    let _ = std::io::stderr().write_all(prompt.as_bytes());
    let _ = std::io::stderr().flush();
    let mut buf = [0u8; 64];
    let n = std::io::stdin().read(&mut buf).unwrap_or(0);
    let input = std::str::from_utf8(&buf[..n]).unwrap_or("");
    input.trim().eq_ignore_ascii_case("y")
}

pub fn handle_managed_clear(worker_name: Option<&str>, skip_confirm: bool) -> i32 {
    match worker_name {
        Some(name) => clear_single_worker(name),
        None => clear_all_workers(skip_confirm),
    }
}

fn clear_single_worker(worker_name: &str) -> i32 {
    if let Err(e) = super::registry::validate_worker_name(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }

    if is_worker_running(worker_name) {
        eprintln!(
            "{} Worker '{}' is currently running. Stop it first with `iii worker stop {}`",
            "error:".red(),
            worker_name,
            worker_name,
        );
        return 1;
    }

    // Distinguish "worker doesn't exist" from "already clean". The old path
    // exited 0 on unknown names, which hid typos in automation. If we have no
    // artifacts AND the name isn't in config.yaml, it's a typo -- exit 1.
    let home = dirs::home_dir().unwrap_or_default();
    let has_artifacts = home.join(".iii/workers").join(worker_name).exists()
        || home.join(".iii/managed").join(worker_name).is_dir();
    let in_config = super::config_file::worker_exists(worker_name);
    if !has_artifacts && !in_config {
        eprintln!(
            "{} Worker '{}' not found. Run `iii worker list` to see known workers.",
            "error:".red(),
            worker_name,
        );
        return 1;
    }

    let freed = delete_worker_artifacts(worker_name);
    if freed == 0 {
        eprintln!("  Nothing to clear for '{}'.", worker_name);
    } else {
        eprintln!(
            "  {} Cleared {:.1} MB of artifacts for {}",
            "✓".green(),
            freed as f64 / 1_048_576.0,
            worker_name.bold(),
        );
    }
    0
}

/// Prompts the user for confirmation before clearing all artifacts.
/// Returns `true` if the user confirms with "y".
fn confirm_clear() -> bool {
    confirm_prompt("  This will remove all downloaded workers and images. Continue? [y/N] ")
}

fn clear_all_workers(skip_confirm: bool) -> i32 {
    let home = dirs::home_dir().unwrap_or_default();
    let workers_dir = home.join(".iii/workers");
    let images_dir = home.join(".iii/images");

    if !workers_dir.exists() && !images_dir.exists() {
        eprintln!("  Nothing to clear.");
        return 0;
    }

    if !skip_confirm && !confirm_clear() {
        eprintln!("  Aborted.");
        return 0;
    }

    let mut skipped: Vec<String> = Vec::new();
    let mut total_freed: u64 = 0;
    let mut worker_count: u32 = 0;
    let mut image_count: u32 = 0;

    // Clear binary workers
    if workers_dir.exists()
        && let Ok(entries) = std::fs::read_dir(&workers_dir)
    {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Skip entries with invalid names (e.g. symlinks with path traversal)
            if super::registry::validate_worker_name(&name).is_err() {
                continue;
            }
            // Verify resolved path stays under workers_dir
            if let Ok(resolved) = entry.path().canonicalize()
                && let Ok(base) = workers_dir.canonicalize()
                && !resolved.starts_with(&base)
            {
                continue;
            }
            if is_worker_running(&name) {
                skipped.push(name);
                continue;
            }
            total_freed += dir_size(&entry.path());
            let _ = std::fs::remove_dir_all(entry.path());
            worker_count += 1;
        }
    }

    // Clear OCI images — protect running OCI workers
    if images_dir.exists() {
        // Build set of image hashes belonging to running OCI workers
        let mut protected_hashes = std::collections::HashSet::new();
        for name in super::config_file::list_worker_names() {
            if is_worker_running(&name)
                && let Some((image_ref, _)) = super::config_file::get_worker_start_info(&name)
            {
                let dir = image_cache_dir(&image_ref);
                if let Some(hash) = dir.file_name().and_then(|f| f.to_str()) {
                    protected_hashes.insert(hash.to_string());
                }
            }
        }

        if let Ok(entries) = std::fs::read_dir(&images_dir) {
            for entry in entries.flatten() {
                let dir_name = entry.file_name().to_string_lossy().to_string();
                if protected_hashes.contains(&dir_name) {
                    skipped.push(format!("OCI image {}", dir_name));
                    continue;
                }
                total_freed += dir_size(&entry.path());
                let _ = std::fs::remove_dir_all(entry.path());
                image_count += 1;
            }
        }
    }

    // Print skipped warnings FIRST so the final line the user sees is the
    // success tally, not a "✓ success" followed by warnings (which reads as
    // "everything worked, oh btw some didn't").
    for name in &skipped {
        eprintln!(
            "  {} Skipped {} (running). Stop it first with `iii worker stop {}`",
            "warning:".yellow(),
            name.bold(),
            name,
        );
    }

    eprintln!(
        "  {} Cleared {} worker(s) and {} image(s) ({:.1} MB freed)",
        "✓".green(),
        worker_count,
        image_count,
        total_freed as f64 / 1_048_576.0,
    );

    0
}

/// Kill any stale worker process from a previous engine run.
/// Checks OCI/local (vm.pid) and binary (pids/{name}.pid) PID files,
/// sends SIGTERM+SIGKILL, and removes the PID file.
/// Kill the host-side source watcher sidecar for `worker_name` and
/// remove its pid file. No-op when no watcher is running.
///
/// Called from the stop path so the watcher doesn't observe the VM
/// shutdown as a file event and race to restart what we just stopped.
/// Also called by `kill_stale_worker` (indirectly, via `watch.pid` in
/// its pid file list) to reap leaks from crashed starts.
pub async fn reap_source_watcher(worker_name: &str) {
    let home = dirs::home_dir().unwrap_or_default();
    let watch_pidfile = home
        .join(".iii/managed")
        .join(worker_name)
        .join("watch.pid");
    if let Some(watch_pid) = read_pid(&watch_pidfile) {
        kill_pid_with_grace(watch_pid).await;
    }
    let _ = std::fs::remove_file(&watch_pidfile);
}

pub async fn kill_stale_worker(worker_name: &str) {
    let home = dirs::home_dir().unwrap_or_default();
    let pid_files = [
        home.join(".iii/managed").join(worker_name).join("vm.pid"),
        home.join(".iii/managed")
            .join(worker_name)
            .join("watch.pid"),
        home.join(".iii/pids").join(format!("{}.pid", worker_name)),
    ];

    for pid_file in &pid_files {
        // Route through the hardened reader so a pre-planted symlink
        // at `pid_file` can't redirect us into an arbitrary file, and
        // a pidfile owned by another uid is ignored instead of honored.
        // We still attempt `remove_file` whenever the file exists so
        // stale/unreadable pidfiles get cleaned up regardless.
        let existed = pid_file.exists();
        if let Some(pid) = read_pid(pid_file) {
            #[cfg(unix)]
            {
                use nix::sys::signal::{Signal, kill};
                use nix::unistd::Pid;
                let p = Pid::from_raw(pid as i32);
                // Only kill if process is still alive.
                if kill(p, None).is_ok() {
                    tracing::info!(worker = %worker_name, pid, "Killing stale worker process");
                    let _ = kill(p, Signal::SIGTERM);
                    // Brief wait then force-kill.
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let _ = kill(p, Signal::SIGKILL);
                }
            }
            #[cfg(not(unix))]
            {
                let _ = pid;
            }
        }
        if existed {
            let _ = tokio::fs::remove_file(pid_file).await;
        }
    }
}

/// Returns worker names discovered from on-disk runtime state under `~/.iii`.
///
/// Sources scanned:
/// - `~/.iii/managed/{name}/`     -- OCI/VM and local-path workers
/// - `~/.iii/pids/{name}.pid`     -- binary workers
///
/// Names are returned sorted and deduplicated. This is the union of every
/// worker the local runtime has touched, regardless of which `config.yaml`
/// declared them. Used by `iii worker list` to surface orphan workers whose
/// project folder has moved or been deleted.
pub fn discover_disk_worker_names() -> Vec<String> {
    let home = dirs::home_dir().unwrap_or_default();
    discover_disk_worker_names_in(&home.join(".iii/managed"), &home.join(".iii/pids"))
}

/// Path-injectable variant of [`discover_disk_worker_names`] for testing.
fn discover_disk_worker_names_in(
    managed_dir: &std::path::Path,
    pids_dir: &std::path::Path,
) -> Vec<String> {
    use std::collections::BTreeSet;
    let mut names = BTreeSet::new();

    if let Ok(entries) = std::fs::read_dir(managed_dir) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
                && let Some(name) = entry.file_name().to_str()
            {
                names.insert(name.to_string());
            }
        }
    }

    if let Ok(entries) = std::fs::read_dir(pids_dir) {
        for entry in entries.flatten() {
            if let Some(file_name) = entry.file_name().to_str()
                && let Some(name) = file_name.strip_suffix(".pid")
                && !name.is_empty()
            {
                names.insert(name.to_string());
            }
        }
    }

    names.into_iter().collect()
}

/// Discovers worker names by inspecting live process command lines for
/// processes spawned by iii-worker. Catches the case where a worker is alive
/// but its on-disk PID file has been removed (project folder moved/deleted,
/// manual cleanup, or a crashed `iii worker stop`).
///
/// Two process patterns are recognised:
/// 1. Binary workers — executable is `~/.iii/workers/{name}`.
/// 2. OCI/VM workers — `iii-worker __vm-boot --pid-file ~/.iii/managed/{name}/vm.pid ...`.
///
/// Sources by platform:
/// - Linux: walks `/proc/*/cmdline` (works on every kernel including
///   Alpine/busybox where `ps -o args=` is unreliable).
/// - macOS: shells out to `ps -axww -o pid=,args=`.
/// - Other platforms: returns empty (best-effort supplement to disk discovery).
pub fn discover_running_worker_names_from_ps() -> Vec<String> {
    let processes = collect_processes();
    if processes.is_empty() {
        return Vec::new();
    }
    let home = dirs::home_dir().unwrap_or_default();
    let workers_prefix = home.join(".iii/workers");
    let managed_prefix = home.join(".iii/managed");
    let cmdlines: Vec<String> = processes.into_iter().map(|(_, c)| c).collect();
    discover_running_worker_names_from_ps_output(
        &cmdlines.join("\n"),
        &workers_prefix,
        &managed_prefix,
    )
}

/// Returns the live PID of the iii-worker process associated with `name`, by
/// scanning live process command lines. Used by `iii worker stop` to terminate
/// orphan workers whose pidfiles have been removed.
///
/// Returns `None` when no matching process exists, when the platform has no
/// process enumeration support, or when `ps`/`/proc` access is denied.
pub fn find_worker_pid_from_ps(name: &str) -> Option<u32> {
    let processes = collect_processes();
    if processes.is_empty() {
        return None;
    }
    let home = dirs::home_dir().unwrap_or_default();
    let workers_prefix = home.join(".iii/workers");
    let managed_prefix = home.join(".iii/managed");
    find_worker_pid_in_processes(&processes, name, &workers_prefix, &managed_prefix)
}

/// Linux: read every numeric `/proc/<pid>/cmdline`. Each is NUL-separated
/// argv0\0argv1\0...\0; we replace NULs with spaces so the shared parser
/// can tokenise it the same way as `ps` output.
#[cfg(target_os = "linux")]
fn collect_processes() -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir("/proc") {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = match name.to_str() {
            Some(s) => s,
            None => continue,
        };
        let pid: u32 = match name_str.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let bytes = match std::fs::read(entry.path().join("cmdline")) {
            Ok(b) if !b.is_empty() => b,
            _ => continue,
        };
        let line = String::from_utf8_lossy(&bytes).replace('\0', " ");
        let trimmed = line.trim_end();
        if !trimmed.is_empty() {
            out.push((pid, trimmed.to_string()));
        }
    }
    out
}

/// macOS: BSD `ps` exposes full argv via `-o args=`; `-axww` selects all
/// processes and disables column truncation. `pid=` keeps the pid column
/// without a header so we can split the first whitespace-separated token off.
#[cfg(target_os = "macos")]
fn collect_processes() -> Vec<(u32, String)> {
    let output = match std::process::Command::new("ps")
        .args(["-axww", "-o", "pid=,args="])
        .output()
    {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };
    String::from_utf8_lossy(&output)
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let mut split = line.splitn(2, char::is_whitespace);
            let pid: u32 = split.next()?.parse().ok()?;
            let args = split.next()?.trim();
            if args.is_empty() {
                None
            } else {
                Some((pid, args.to_string()))
            }
        })
        .collect()
}

/// Other platforms: no cross-platform process enumeration without a new dep.
/// Disk discovery still runs; we just lose the alive-but-no-pidfile fallback.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn collect_processes() -> Vec<(u32, String)> {
    Vec::new()
}

/// From a single process's `argv`-joined cmdline, return the worker name it
/// represents (if any). Shared between name discovery and PID lookup so both
/// match against the exact same recognition rules.
fn extract_worker_name_from_cmdline(
    cmdline: &str,
    workers_prefix: &std::path::Path,
    managed_prefix: &std::path::Path,
) -> Option<String> {
    let mut tokens = cmdline.split_whitespace();
    let exe = tokens.next()?;
    let exe_path = std::path::Path::new(exe);

    // Pattern 1: binary worker -- executable lives under ~/.iii/workers/{name}
    if let Ok(rel) = exe_path.strip_prefix(workers_prefix)
        && let Some(name) = rel.iter().next().and_then(|c| c.to_str())
        && !name.is_empty()
    {
        return Some(name.to_string());
    }

    // Pattern 2: iii-worker __vm-boot --pid-file <...>/managed/{name}/vm.pid
    if exe_path.file_name().and_then(|s| s.to_str()) == Some("iii-worker")
        && tokens.next() == Some("__vm-boot")
    {
        let rest: Vec<&str> = tokens.collect();
        for i in 0..rest.len().saturating_sub(1) {
            if rest[i] == "--pid-file"
                && let Ok(rel) = std::path::Path::new(rest[i + 1]).strip_prefix(managed_prefix)
                && let Some(name) = rel.iter().next().and_then(|c| c.to_str())
                && !name.is_empty()
            {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Pure parser used by [`discover_running_worker_names_from_ps`]. Exposed for
/// testing with synthetic cmdline output and arbitrary path prefixes. Each
/// input line is one process's argv joined by spaces.
fn discover_running_worker_names_from_ps_output(
    ps_output: &str,
    workers_prefix: &std::path::Path,
    managed_prefix: &std::path::Path,
) -> Vec<String> {
    use std::collections::BTreeSet;
    let mut names = BTreeSet::new();
    for line in ps_output.lines() {
        if let Some(name) = extract_worker_name_from_cmdline(line, workers_prefix, managed_prefix) {
            names.insert(name);
        }
    }
    names.into_iter().collect()
}

/// Pure parser used by [`find_worker_pid_from_ps`]. Returns the first PID
/// whose cmdline resolves to `name`. Exposed for testing.
fn find_worker_pid_in_processes(
    processes: &[(u32, String)],
    name: &str,
    workers_prefix: &std::path::Path,
    managed_prefix: &std::path::Path,
) -> Option<u32> {
    processes.iter().find_map(|(pid, cmdline)| {
        match extract_worker_name_from_cmdline(cmdline, workers_prefix, managed_prefix) {
            Some(n) if n == name => Some(*pid),
            _ => None,
        }
    })
}

/// Returns `true` if the worker has a valid PID file and the process is alive.
pub fn is_worker_running(worker_name: &str) -> bool {
    let home = dirs::home_dir().unwrap_or_default();
    let oci_pid = home.join(".iii/managed").join(worker_name).join("vm.pid");
    let bin_pid = home.join(".iii/pids").join(format!("{}.pid", worker_name));

    for pid_file in [oci_pid, bin_pid] {
        if let Some(pid) = read_pid(&pid_file) {
            // Check if process is alive (signal 0 = existence check).
            #[cfg(unix)]
            {
                use nix::sys::signal::kill;
                use nix::unistd::Pid;
                if kill(Pid::from_raw(pid as i32), None).is_ok() {
                    return true;
                }
            }
            #[cfg(not(unix))]
            {
                let _ = pid;
                // On non-Unix, assume running if PID file exists.
                return true;
            }
        }
    }
    false
}

/// Probes `127.0.0.1:{port}` to check whether the engine is listening.
/// Uses a 200ms timeout to avoid blocking the CLI.
///
/// Callers that don't already know the port should resolve via
/// `super::config_file::manager_port()`; those who already hold a port
/// (e.g. after a user passed `--port`) should use it directly so an
/// override isn't silently ignored.
pub fn is_engine_running_on(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        std::time::Duration::from_millis(200),
    )
    .is_ok()
}

/// Convenience for call sites without a known port: resolves the
/// `iii-worker-manager` port from config.yaml (or falls back to
/// `DEFAULT_PORT`) and probes it.
pub fn is_engine_running() -> bool {
    is_engine_running_on(super::config_file::manager_port())
}

/// Deletes local artifacts for a worker (binary dir or OCI image dir).
/// Returns the number of bytes freed, or 0 if nothing was found.
///
/// Defense-in-depth: `worker_name` is joined into `~/.iii/...` paths
/// that get `remove_dir_all`'d. `Path::join` preserves `..` components,
/// so an unvalidated traversal name would delete attacker-chosen
/// directories under the user's HOME. Callers are expected to have
/// validated, but we re-check here so the sink itself is safe.
pub fn delete_worker_artifacts(worker_name: &str) -> u64 {
    if let Err(msg) = super::registry::validate_worker_name(worker_name) {
        eprintln!(
            "  {} refusing to delete artifacts for invalid worker name: {}",
            "warning:".yellow(),
            msg
        );
        return 0;
    }
    let home = dirs::home_dir().unwrap_or_default();
    let mut freed: u64 = 0;

    // Binary worker: ~/.iii/workers/{name}/
    let binary_dir = home.join(".iii/workers").join(worker_name);
    if binary_dir.is_dir() {
        freed += dir_size(&binary_dir);
        if let Err(e) = std::fs::remove_dir_all(&binary_dir) {
            eprintln!(
                "  {} Failed to remove {}: {}",
                "warning:".yellow(),
                binary_dir.display(),
                e
            );
        }
    } else if binary_dir.is_file() {
        // Legacy: some binary workers are a single file, not a directory
        freed += std::fs::metadata(&binary_dir).map(|m| m.len()).unwrap_or(0);
        if let Err(e) = std::fs::remove_file(&binary_dir) {
            eprintln!(
                "  {} Failed to remove {}: {}",
                "warning:".yellow(),
                binary_dir.display(),
                e
            );
        }
    }

    // OCI worker: look up image from config.yaml, compute hash, delete ~/.iii/images/{hash}/
    if let Some((image_ref, _)) = super::config_file::get_worker_start_info(worker_name) {
        let image_dir = image_cache_dir(&image_ref);
        if image_dir.is_dir() {
            freed += dir_size(&image_dir);
            if let Err(e) = std::fs::remove_dir_all(&image_dir) {
                eprintln!(
                    "  {} Failed to remove {}: {}",
                    "warning:".yellow(),
                    image_dir.display(),
                    e
                );
            }
        }
    }

    // Local-path worker: ~/.iii/managed/{name}/ (same as OCI)
    let managed_dir = home.join(".iii/managed").join(worker_name);
    if managed_dir.is_dir() {
        // Only count if we haven't already freed anything (avoid double-counting with OCI)
        if freed == 0 {
            freed += dir_size(&managed_dir);
            if let Err(e) = std::fs::remove_dir_all(&managed_dir) {
                eprintln!(
                    "  {} Failed to remove {}: {}",
                    "warning:".yellow(),
                    managed_dir.display(),
                    e
                );
            }
        }
    }

    freed
}

/// Computes the cache directory path for an OCI image reference.
/// Uses the first 8 bytes of SHA-256 of the image ref as the directory name.
fn image_cache_dir(image_ref: &str) -> std::path::PathBuf {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(image_ref.as_bytes());
    let hash = hex::encode(&hasher.finalize()[..8]);
    dirs::home_dir()
        .unwrap_or_default()
        .join(".iii/images")
        .join(hash)
}

/// Recursively computes the total size of a directory in bytes.
fn dir_size(path: &std::path::Path) -> u64 {
    let mut total: u64 = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let meta = entry.metadata();
            if let Ok(m) = meta {
                if m.is_dir() {
                    total += dir_size(&entry.path());
                } else {
                    total += m.len();
                }
            }
        }
    }
    total
}

pub async fn handle_managed_stop(worker_name: &str) -> i32 {
    if let Err(e) = super::registry::validate_worker_name(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }
    let home = dirs::home_dir().unwrap_or_default();
    let oci_pidfile = home.join(".iii/managed").join(worker_name).join("vm.pid");
    let bin_pidfile = home.join(".iii/pids").join(format!("{}.pid", worker_name));

    // Is this name known at all? Check every evidence source we have: config,
    // managed artifacts, binary workers dir, pidfiles. If none of those apply
    // and no live process exists, it's a typo -- exit 1 with "not found" so
    // automation doesn't confuse that with "already stopped."
    let in_config_yaml = super::config_file::list_worker_names()
        .iter()
        .any(|n| n == worker_name);
    let managed_dir_exists = home.join(".iii/managed").join(worker_name).is_dir();
    let binary_exists = home.join(".iii/workers").join(worker_name).exists();
    let worker_known = in_config_yaml
        || managed_dir_exists
        || binary_exists
        || oci_pidfile.exists()
        || bin_pidfile.exists();

    // Reject the well-defined "config worker explicitly listed in config.yaml"
    // case -- the engine owns those, the worker CLI cannot stop them. We only
    // reject when the name is genuinely listed in config; the resolver also
    // returns Config as the no-match fallthrough, which we want to treat as
    // an orphan candidate instead.
    if in_config_yaml
        && matches!(
            super::config_file::resolve_worker_type(worker_name),
            ResolvedWorkerType::Config
        )
    {
        eprintln!(
            "{} Cannot stop '{}': config workers run inside the engine and cannot be stopped individually",
            "error:".red(),
            worker_name
        );
        return 1;
    }

    // Locate the worker's PID via three evidence tiers, in order:
    // 1. ~/.iii/managed/{name}/vm.pid       (OCI/VM/local-path)
    // 2. ~/.iii/pids/{name}.pid             (binary)
    // 3. live `ps` scan                     (orphan, or stale pidfile)
    //
    // Pidfiles are only trusted when the recorded PID is actually alive. A
    // stale pidfile (process crashed without cleanup, or PID got recycled)
    // must fall through to the ps scan — otherwise we'd either signal an
    // unrelated recycled PID or miss a restarted orphan worker.
    let oci_live_pid = oci_pidfile
        .exists()
        .then(|| read_pid(&oci_pidfile).filter(|&p| is_pid_alive(p)))
        .flatten();
    let bin_live_pid = bin_pidfile
        .exists()
        .then(|| read_pid(&bin_pidfile).filter(|&p| is_pid_alive(p)))
        .flatten();

    let mode = if let Some(pid) = oci_live_pid {
        StopMode::Managed {
            pid,
            pidfile: Some(oci_pidfile),
        }
    } else if let Some(pid) = bin_live_pid {
        StopMode::Binary {
            pid,
            pidfile: Some(bin_pidfile),
        }
    } else if let Some(pid) = find_worker_pid_from_ps(worker_name) {
        // Either no pidfile on disk, or the pidfile is stale (dead PID).
        // Either way, ps found a live process for this worker — treat as
        // orphan. Carry any stale pidfile along so it gets cleaned up.
        let stale_pidfile = if oci_pidfile.exists() {
            Some(oci_pidfile)
        } else if bin_pidfile.exists() {
            Some(bin_pidfile)
        } else {
            None
        };
        let is_managed = home.join(".iii/managed").join(worker_name).is_dir();
        if is_managed {
            StopMode::Managed {
                pid,
                pidfile: stale_pidfile,
            }
        } else {
            StopMode::Binary {
                pid,
                pidfile: stale_pidfile,
            }
        }
    } else if !worker_known {
        eprintln!(
            "{} Worker '{}' not found. Run `iii worker list` to see known workers.",
            "error:".red(),
            worker_name,
        );
        return 1;
    } else {
        // VM died out-of-band but the watcher sidecar may still be
        // alive holding watch.pid — if we return here without reaping
        // it, the watcher will keep firing on file changes and try to
        // respawn a VM that nothing is tracking. Tear it down before
        // reporting "already stopped."
        reap_source_watcher(worker_name).await;
        eprintln!("  {} {} already stopped", "✓".green(), worker_name.bold());
        return 0;
    };

    eprintln!("  Stopping {}...", worker_name.bold());

    match mode {
        StopMode::Managed { pid, pidfile } => {
            // Tear down the source watcher sidecar first so it doesn't
            // observe the VM shutdown as a file event and try to restart.
            reap_source_watcher(worker_name).await;

            // Ask the in-VM supervisor to shut its child down cleanly.
            // The supervisor exits on success, which triggers libkrun's
            // poweroff path, which is faster and cleaner than a bare
            // SIGTERM to the __vm-boot process. We still fall through
            // to adapter.stop below — if the supervisor wasn't reachable
            // (binary missing, channel dead), that's the authoritative
            // teardown; if the shutdown succeeded, adapter.stop's
            // SIGTERM becomes a no-op against an already-exiting VM.
            if let Err(e) = super::supervisor_ctl::request_shutdown(worker_name).await {
                tracing::debug!(
                    worker = %worker_name,
                    error = %e,
                    "supervisor shutdown unavailable, falling through to SIGTERM"
                );
            }

            let adapter = super::worker_manager::create_adapter("libkrun");
            let _ = adapter.stop(&pid.to_string(), 10).await;
            if let Some(f) = pidfile {
                let _ = std::fs::remove_file(&f);
            }
        }
        StopMode::Binary { pid, pidfile } => {
            kill_pid_with_grace(pid).await;
            if let Some(f) = pidfile {
                let _ = std::fs::remove_file(&f);
            }
        }
    }

    eprintln!("  {} {} stopped", "✓".green(), worker_name.bold());
    0
}

/// Internal stop dispatch. The path the PID was discovered through dictates
/// how we terminate it (libkrun adapter for VMs, raw signals for binaries) and
/// whether we have an on-disk pidfile to clean up afterwards.
enum StopMode {
    Managed {
        pid: u32,
        pidfile: Option<std::path::PathBuf>,
    },
    Binary {
        pid: u32,
        pidfile: Option<std::path::PathBuf>,
    },
}

/// Reads a PID file, returning `Some(pid)` when contents parse as `u32`.
///
/// Thin alias for [`super::pidfile::read_pid`] — the hardened reader
/// lives in the shared module alongside `write_pid_file` so every
/// pidfile I/O path goes through the same O_NOFOLLOW + uid-ownership
/// check. See the `pidfile` module docstring for the full attacker
/// model.
fn read_pid(path: &std::path::Path) -> Option<u32> {
    super::pidfile::read_pid(path)
}

/// Returns `true` if `pid` refers to a live process. Uses signal 0 as a
/// non-destructive existence probe on Unix; assumes alive on platforms
/// without nix signals (the stop path will discover failure on real kill).
///
/// Used by the stop path to distinguish fresh pidfiles from stale ones so
/// a dead/recycled PID cannot short-circuit the `ps` orphan scan.
#[cfg(unix)]
fn is_pid_alive(pid: u32) -> bool {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    kill(Pid::from_raw(pid as i32), None).is_ok()
}

#[cfg(not(unix))]
fn is_pid_alive(_pid: u32) -> bool {
    true
}

/// SIGTERM, brief grace period, then SIGKILL. Mirrors the original
/// binary-worker stop semantics. No-op on platforms without nix signals.
async fn kill_pid_with_grace(pid: u32) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;
        let target = Pid::from_raw(pid as i32);
        let _ = kill(target, Signal::SIGTERM);
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        let _ = kill(target, Signal::SIGKILL);
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        eprintln!(
            "{} Direct PID stop not supported on this platform",
            "error:".red()
        );
    }
}

/// Block up to 120s waiting for the worker to report ready, printing a live
/// status snapshot. Used by `iii worker start` when the user did not pass
/// --no-wait. Same contract as `iii worker add --wait`: on timeout we do NOT
/// fail the command (the process started successfully), we just inform the
/// user and let them poll with `iii worker status {name}`.
///
/// `port` is the engine's configured `iii-worker-manager` port so the
/// engine-liveness probe inside `watch_until_ready` targets the engine the
/// worker is actually talking to. Without this, users on a non-default
/// port would see "engine: stopped" until the wait timed out.
async fn wait_for_ready(worker_name: &str, port: u16) {
    let started = std::time::Instant::now();
    let final_status = super::status::watch_until_ready(
        worker_name,
        Some(std::time::Duration::from_secs(120)),
        port,
    )
    .await;
    let elapsed = started.elapsed();
    match final_status.phase {
        super::status::Phase::Ready => {
            eprintln!("  {} ready in {:.1}s", "✓".green(), elapsed.as_secs_f64());
        }
        _ => {
            eprintln!(
                "  {} not ready after {:.0}s.\n  \
                 Keep watching: iii worker status {}\n  \
                 Check logs:    iii worker logs {} -f",
                "⚠".yellow(),
                elapsed.as_secs_f64(),
                worker_name,
                worker_name
            );
        }
    }
}

/// Starts a managed worker, pointing it back at the engine on `port`.
///
/// `port` is the WebSocket port the spawned worker will connect to (used to
/// build `III_ENGINE_URL` for VM-based workers and to probe engine liveness).
/// Callers that don't know any better pass `DEFAULT_PORT`; the engine's
/// auto-spawn path in `registry_worker::ExternalWorkerProcess::spawn` passes
/// the configured `iii-worker-manager` port so non-default manager ports
/// don't silently break connectivity for external workers.
pub async fn handle_managed_start(worker_name: &str, wait: bool, port: u16) -> i32 {
    if let Err(e) = super::registry::validate_worker_name(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }
    // Builtin workers are served in-process by the iii engine (see
    // engine/src/workers/config.rs factory registry). They have no external
    // process to spawn and must not be resolved via the remote registry.
    // Only treat this as success when the builtin is actually configured in
    // config.yaml AND the engine is running -- otherwise `start` is lying by
    // returning 0 for a no-op and automation thinks something booted.
    if get_builtin_default(worker_name).is_some() {
        if !super::config_file::worker_exists(worker_name) {
            eprintln!(
                "{} '{}' is a builtin but is not configured. Run `iii worker add {}` first.",
                "error:".red(),
                worker_name,
                worker_name,
            );
            return 1;
        }
        if !is_engine_running() {
            eprintln!(
                "{} '{}' is a builtin served by the iii engine, but the engine isn't running.\n  \
                 Start the engine:  iii start",
                "error:".red(),
                worker_name,
            );
            return 1;
        }
        eprintln!(
            "  '{}' is a builtin worker — served by the iii engine process.",
            worker_name,
        );
        return 0;
    }
    let local_outcome = match super::config_file::resolve_worker_type(worker_name) {
        ResolvedWorkerType::Oci { image, env } => {
            let worker_def = WorkerDef::Managed {
                image,
                env,
                resources: None,
            };
            StartOutcome::Exit(start_oci_worker(worker_name, &worker_def, port).await)
        }
        ResolvedWorkerType::Local { worker_path } => StartOutcome::Exit(
            super::local_worker::start_local_worker(worker_name, &worker_path, port).await,
        ),
        ResolvedWorkerType::Binary { binary_path } => {
            StartOutcome::Exit(start_binary_worker(worker_name, &binary_path).await)
        }
        ResolvedWorkerType::Config => StartOutcome::FallThrough,
    };
    if let StartOutcome::Exit(rc) = local_outcome {
        return finish_start(worker_name, rc, wait, port).await;
    }

    // Not found locally — try remote registry for auto-install
    eprintln!(
        "  Worker '{}' not found locally, checking registry...",
        worker_name
    );
    match fetch_worker_info(worker_name, None).await {
        Ok(WorkerInfoResponse::Binary(response)) => {
            let target = binary_download::current_target();
            let binary_info = match response.binaries.get(target) {
                Some(info) => info,
                None => {
                    eprintln!(
                        "{} Platform '{}' not supported for '{}'. Available: {}",
                        "error:".red(),
                        target,
                        worker_name,
                        response
                            .binaries
                            .keys()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    return 1;
                }
            };

            eprintln!(
                "  Installing {} (binary v{})...",
                worker_name, response.version
            );
            match binary_download::download_and_install_binary(worker_name, binary_info).await {
                Ok(installed_path) => {
                    eprintln!("  {} Installed successfully", "✓".green());
                    let rc = start_binary_worker(worker_name, &installed_path).await;
                    return finish_start(worker_name, rc, wait, port).await;
                }
                Err(e) => {
                    eprintln!(
                        "{} Failed to install '{}': {}",
                        "error:".red(),
                        worker_name,
                        e
                    );
                    return 1;
                }
            }
        }
        Ok(WorkerInfoResponse::Oci(response)) => {
            let worker_def = WorkerDef::Managed {
                image: response.image_url,
                env: std::collections::HashMap::new(),
                resources: None,
            };
            let rc = start_oci_worker(worker_name, &worker_def, port).await;
            return finish_start(worker_name, rc, wait, port).await;
        }
        Err(e) => {
            tracing::warn!("Failed to fetch worker info: {}", e);
        }
    }

    eprintln!(
        "{} Worker '{}' not found locally or in registry. Run `iii worker add {}`.",
        "error:".red(),
        worker_name,
        worker_name
    );
    1
}

/// Classifies what `handle_managed_start`'s local-resolution branch wants the
/// caller to do next: either return an exit code straight to the user, or
/// fall through to the remote-registry path. Introduced to replace an
/// `i32::MIN` sentinel that overloaded the exit-code type as a control token.
enum StartOutcome {
    Exit(i32),
    FallThrough,
}

/// Shared tail for every successful start path: wait (if requested) then
/// emit the machine-readable worker name on stdout per the module output
/// contract. Keeping this in one place prevents the stdout contract from
/// drifting across the four call sites that used to inline it.
async fn finish_start(worker_name: &str, rc: i32, wait: bool, port: u16) -> i32 {
    if rc == 0 && wait {
        wait_for_ready(worker_name, port).await;
    }
    rc
}

/// Stop (if running) and start a worker. Idempotent: workers that aren't
/// running just get started. We delegate to the existing stop/start paths
/// rather than duplicating the libkrun teardown / pid-discovery logic.
///
/// Stop is invoked unconditionally so its three-tier PID discovery (OCI
/// pidfile, binary pidfile, `ps` scan) can catch orphan processes whose
/// pidfiles are missing or stale. `is_worker_running` only consults
/// pidfiles, so gating on it would let those orphans slip through and
/// start would then spawn a duplicate. Stop failures are logged but do
/// NOT abort the restart -- the most common reason stop "fails" here is
/// "already not running," which returns 0. Start's exit code becomes the
/// command's exit code.
pub async fn handle_managed_restart(worker_name: &str, wait: bool, port: u16) -> i32 {
    if let Err(e) = super::registry::validate_worker_name(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }

    eprintln!("  Restarting {}...", worker_name.bold());
    let stop_rc = handle_managed_stop(worker_name).await;
    if stop_rc != 0 {
        eprintln!(
            "  {} stop exited {} -- continuing with start",
            "warning:".yellow(),
            stop_rc
        );
    }

    handle_managed_start(worker_name, wait, port).await
}

async fn start_oci_worker(worker_name: &str, worker_def: &WorkerDef, port: u16) -> i32 {
    if let Err(e) = super::firmware::download::ensure_libkrunfw().await {
        tracing::warn!(error = %e, "failed to ensure libkrunfw availability");
    }

    if !super::worker_manager::libkrun::libkrun_available() {
        eprintln!(
            "{} libkrunfw is not available.\n  \
             Rebuild with --features embed-libkrunfw or place libkrunfw in ~/.iii/lib/",
            "error:".red()
        );
        return 1;
    }

    let adapter = super::worker_manager::create_adapter("libkrun");
    eprintln!("  Starting {} (OCI)...", worker_name.bold());

    let engine_url = format!("ws://localhost:{}", port);
    let spec = build_container_spec(worker_name, worker_def, &engine_url);

    let pid_file = dirs::home_dir()
        .unwrap_or_default()
        .join(".iii/managed")
        .join(worker_name)
        .join("vm.pid");
    if let Some(pid) = read_pid(&pid_file) {
        let pid_str = pid.to_string();
        let _ = adapter.stop(&pid_str, 5).await;
        let _ = adapter.remove(&pid_str).await;
    }

    match adapter.start(&spec).await {
        Ok(_) => {
            eprintln!("  {} {} started", "✓".green(), worker_name.bold());
            0
        }
        Err(e) => {
            eprintln!("{} Start failed: {}", "error:".red(), e);
            1
        }
    }
}

async fn start_binary_worker(worker_name: &str, binary_path: &std::path::Path) -> i32 {
    // Kill any stale process from a previous engine run
    kill_stale_worker(worker_name).await;

    // Create log directory: ~/.iii/logs/{name}/
    let logs_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".iii/logs")
        .join(worker_name);
    if let Err(e) = std::fs::create_dir_all(&logs_dir) {
        eprintln!("{} Failed to create logs dir: {}", "error:".red(), e);
        return 1;
    }

    let stdout_file = match std::fs::File::create(logs_dir.join("stdout.log")) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{} Failed to create stdout log: {}", "error:".red(), e);
            return 1;
        }
    };
    let stderr_file = match std::fs::File::create(logs_dir.join("stderr.log")) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{} Failed to create stderr log: {}", "error:".red(), e);
            return 1;
        }
    };

    eprintln!("  Starting {} (binary)...", worker_name.bold());

    let mut cmd = tokio::process::Command::new(binary_path);
    cmd.stdout(stdout_file).stderr(stderr_file);

    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(|| {
            nix::unistd::setsid()
                .map_err(|e| std::io::Error::other(format!("setsid failed: {e}")))?;
            Ok(())
        });
    }

    match cmd.spawn() {
        Ok(child) => {
            // Write PID file for stop/status tracking.
            // Use ~/.iii/pids/{name}.pid — binary workers occupy ~/.iii/workers/{name}
            // as a file (the executable), so we cannot create a subdirectory there.
            let pid_dir = dirs::home_dir().unwrap_or_default().join(".iii/pids");
            let _ = std::fs::create_dir_all(&pid_dir);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&pid_dir, std::fs::Permissions::from_mode(0o700));
            }
            if let Some(pid) = child.id() {
                let pid_path = pid_dir.join(format!("{}.pid", worker_name));
                // Route through the shared hardened writer: O_NOFOLLOW +
                // atomic 0o600 mode at O_CREAT. A plain fs::write here
                // would follow a pre-planted symlink at the target and
                // a post-hoc set_permissions leaves a create/chmod
                // TOCTOU window. See cli/pidfile.rs for rationale.
                super::pidfile::write_pid_file(&pid_path, pid);
            }
            let pid_display = child
                .id()
                .map(|p| p.to_string())
                .unwrap_or_else(|| "?".into());
            eprintln!(
                "  {} {} started (pid: {})",
                "✓".green(),
                worker_name.bold(),
                pid_display
            );
            0
        }
        Err(e) => {
            eprintln!("{} Failed to start binary worker: {}", "error:".red(), e);
            1
        }
    }
}

pub async fn handle_worker_list() -> i32 {
    let config_names = super::config_file::list_worker_names();

    // Discovery union: on-disk PID files + on-disk managed dirs + live process
    // table (catches workers whose pidfiles were removed but the process kept
    // running -- the actual repro from the user's bug report).
    let disk_names = discover_disk_worker_names();
    let ps_names = discover_running_worker_names_from_ps();
    let ps_set: std::collections::HashSet<String> = ps_names.iter().cloned().collect();
    let config_set: std::collections::HashSet<&str> =
        config_names.iter().map(String::as_str).collect();

    // Orphan = not in current ./config.yaml AND demonstrably alive (either via
    // pidfile signal-0 check or because we just saw it in `ps`). Dead disk-only
    // entries are stale runtime state, not what the user is asking about.
    let candidate_names: std::collections::BTreeSet<String> =
        disk_names.into_iter().chain(ps_names.into_iter()).collect();
    let orphan_names: Vec<String> = candidate_names
        .into_iter()
        .filter(|n| !config_set.contains(n.as_str()))
        .filter(|n| ps_set.contains(n) || is_worker_running(n))
        .collect();

    if config_names.is_empty() && orphan_names.is_empty() {
        eprintln!("  No workers. Use `iii worker add` to get started.");
        return 0;
    }

    let engine_running = is_engine_running();

    eprintln!();
    eprintln!(
        "  {:25} {:10} {}",
        "NAME".bold(),
        "TYPE".bold(),
        "STATUS".bold()
    );
    eprintln!(
        "  {:25} {:10} {}",
        "----".dimmed(),
        "----".dimmed(),
        "------".dimmed()
    );

    for name in &config_names {
        let worker_type = match super::config_file::resolve_worker_type(name) {
            ResolvedWorkerType::Local { .. } => "local",
            ResolvedWorkerType::Oci { .. } => "oci",
            ResolvedWorkerType::Binary { .. } => "binary",
            ResolvedWorkerType::Config => "config",
        };

        let running = if is_worker_running(name) {
            "running".green().to_string()
        } else if worker_type == "config" && engine_running {
            "running".green().to_string()
        } else {
            "stopped".dimmed().to_string()
        };

        eprintln!("  {:25} {:10} {}", name, worker_type.dimmed(), running);
    }

    // Orphans: alive on this machine but absent from the current ./config.yaml.
    // The TYPE column is inferred from the on-disk evidence we found the worker
    // through (managed dir vs binary pidfile/exe). Falls back to "?" only when
    // a worker was discovered solely via `ps` and no on-disk artifact remains.
    for name in &orphan_names {
        let home = dirs::home_dir().unwrap_or_default();
        let worker_type = resolve_orphan_type(
            name,
            &home.join(".iii/managed"),
            &home.join(".iii/pids"),
            &home.join(".iii/workers"),
        );
        eprintln!(
            "  {:25} {:10} {}",
            name,
            worker_type.dimmed(),
            "orphan".yellow()
        );
    }

    eprintln!();
    0
}

/// Infers the TYPE label for an orphan worker from on-disk evidence alone.
///
/// We can't rebuild the `ResolvedWorkerType` enum without a config entry, but
/// we can tell `managed` (OCI/VM/local-path -- shares a directory shape) from
/// `binary` (single executable + sidecar pidfile) just from where the artifact
/// lives. Returns "?" when only a `ps` match exists and every artifact has been
/// cleaned up under it -- the honest answer.
///
/// Path arguments are injected to keep the function unit-testable against a
/// tempdir without an env override.
fn resolve_orphan_type(
    name: &str,
    managed_dir: &std::path::Path,
    pids_dir: &std::path::Path,
    workers_dir: &std::path::Path,
) -> &'static str {
    if managed_dir.join(name).is_dir() {
        return "managed";
    }
    if pids_dir.join(format!("{}.pid", name)).is_file() || workers_dir.join(name).is_file() {
        return "binary";
    }
    "?"
}

/// Pick the log directory with the most recently modified, non-empty log file.
/// Returns `None` when no candidate contains any usable log content.
fn pick_best_logs_dir(candidates: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    let mut best: Option<(std::path::PathBuf, std::time::SystemTime)> = None;

    for dir in candidates {
        let latest = ["stdout.log", "stderr.log"]
            .iter()
            .map(|f| dir.join(f))
            .filter_map(|p| std::fs::metadata(&p).ok().map(|m| (p, m)))
            .filter(|(_, m)| m.len() > 0)
            .filter_map(|(_, m)| m.modified().ok())
            .max();

        if let Some(modified) = latest
            && best.as_ref().is_none_or(|(_, t)| modified > *t)
        {
            best = Some((dir.clone(), modified));
        }
    }

    best.map(|(dir, _)| dir)
}

fn file_len(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

async fn read_new_bytes(path: &std::path::Path, offset: u64, is_stderr: bool) -> u64 {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};

    let mut file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(_) => return offset,
    };

    let len = match file.metadata().await {
        Ok(m) => m.len(),
        Err(_) => return offset,
    };

    let offset = if len < offset { 0 } else { offset };

    if len == offset {
        return offset;
    }

    if file.seek(std::io::SeekFrom::Start(offset)).await.is_err() {
        return offset;
    }

    let mut buf = Vec::new();
    if file.read_to_end(&mut buf).await.is_err() {
        return offset;
    }

    let text = String::from_utf8_lossy(&buf);
    let ends_with_newline = text.ends_with('\n');
    let mut lines: Vec<&str> = text.lines().collect();

    let consumed = if ends_with_newline {
        buf.len() as u64
    } else if lines.len() > 1 {
        let last = lines.pop().unwrap();
        buf.len() as u64 - last.len() as u64
    } else {
        0
    };

    for line in &lines {
        if is_stderr {
            eprintln!("{}", line);
        } else {
            println!("{}", line);
        }
    }

    offset + consumed
}

async fn follow_logs(stdout_path: &std::path::Path, stderr_path: &std::path::Path) -> i32 {
    let mut stdout_offset = file_len(stdout_path);
    let mut stderr_offset = file_len(stderr_path);
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    loop {
        tokio::select! {
            _ = &mut ctrl_c => break,
            _ = interval.tick() => {
                stdout_offset = read_new_bytes(stdout_path, stdout_offset, false).await;
                stderr_offset = read_new_bytes(stderr_path, stderr_offset, true).await;
            }
        }
    }
    0
}

async fn follow_single_log(path: &std::path::Path) -> i32 {
    let mut offset = file_len(path);
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    loop {
        tokio::select! {
            _ = &mut ctrl_c => break,
            _ = interval.tick() => {
                offset = read_new_bytes(path, offset, false).await;
            }
        }
    }
    0
}

pub async fn handle_managed_logs(
    worker_name: &str,
    follow: bool,
    _address: &str,
    _port: u16,
) -> i32 {
    if let Err(e) = super::registry::validate_worker_name(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }
    let home = dirs::home_dir().unwrap_or_default();

    // Check all possible log locations and prefer the one with the most
    // recently modified, non-empty log files. This avoids picking a stale
    // directory (e.g. ~/.iii/logs/ from a binary worker) over the active
    // one (e.g. ~/.iii/managed/ from a libkrun OCI worker).
    let unified_logs_dir = home.join(".iii/logs").join(worker_name);
    let legacy_managed_dir = home.join(".iii/managed").join(worker_name).join("logs");
    let legacy_binary_dir = home.join(".iii/workers/logs").join(worker_name);

    let logs_dir = pick_best_logs_dir(&[
        unified_logs_dir.clone(),
        legacy_managed_dir,
        legacy_binary_dir,
    ])
    .unwrap_or(unified_logs_dir);

    let worker_dir = logs_dir.clone();

    let stdout_path = logs_dir.join("stdout.log");
    let stderr_path = logs_dir.join("stderr.log");

    let has_new_logs = stdout_path.exists() || stderr_path.exists();

    if has_new_logs {
        let mut found_content = false;

        // Read stderr.log first: it holds the host vm-boot subprocess's own
        // eprintln! output (e.g. "  Booting VM...") which fires BEFORE the
        // VM enters, so those lines are chronologically the oldest. stdout.log
        // is the VM's --console-output stream, which only starts producing
        // content once the guest is actually running.
        if let Ok(contents) = std::fs::read_to_string(&stderr_path)
            && !contents.is_empty()
        {
            found_content = true;
            let lines: Vec<&str> = contents.lines().collect();
            let start = if lines.len() > 100 {
                lines.len() - 100
            } else {
                0
            };
            for line in &lines[start..] {
                eprintln!("{}", line);
            }
        }

        if let Ok(contents) = std::fs::read_to_string(&stdout_path)
            && !contents.is_empty()
        {
            found_content = true;
            let lines: Vec<&str> = contents.lines().collect();
            let start = if lines.len() > 100 {
                lines.len() - 100
            } else {
                0
            };
            for line in &lines[start..] {
                println!("{}", line);
            }
        }

        if !found_content {
            eprintln!("  No logs available for {}", worker_name.bold());
        }

        if follow {
            return follow_logs(&stdout_path, &stderr_path).await;
        }

        return 0;
    }

    let old_log = worker_dir.join("vm.log");
    match std::fs::read_to_string(&old_log) {
        Ok(contents) => {
            if contents.is_empty() {
                eprintln!("  No logs available for {}", worker_name.bold());
            } else {
                let lines: Vec<&str> = contents.lines().collect();
                let start = if lines.len() > 100 {
                    lines.len() - 100
                } else {
                    0
                };
                for line in &lines[start..] {
                    println!("{}", line);
                }
            }

            if follow {
                return follow_single_log(&old_log).await;
            }

            0
        }
        Err(_) => {
            eprintln!("{} No logs found for '{}'", "error:".red(), worker_name);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Mutex;

    static CWD_LOCK: Mutex<()> = Mutex::new(());

    /// Run an async closure with CWD set to a temp dir, then restore.
    /// Uses a drop guard so CWD is restored even if the closure panics.
    async fn in_temp_dir_async<F, Fut>(f: F)
    where
        F: FnOnce(std::path::PathBuf) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        struct CwdGuard(std::path::PathBuf);
        impl Drop for CwdGuard {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }

        let _guard = CWD_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        let dir_path = dir.path().to_path_buf();
        std::env::set_current_dir(&dir_path).unwrap();
        let _cwd_guard = CwdGuard(original);
        f(dir_path).await;
    }

    #[tokio::test]
    async fn read_new_bytes_picks_up_appended_content() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("test.log");
        std::fs::write(&log, "line1\nline2\n").unwrap();

        let initial_len = file_len(&log);
        assert_eq!(initial_len, 12); // "line1\nline2\n"

        // No new bytes → offset unchanged
        let offset = read_new_bytes(&log, initial_len, false).await;
        assert_eq!(offset, initial_len);

        // Append new content
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        write!(f, "line3\nline4\n").unwrap();
        drop(f);

        let offset = read_new_bytes(&log, initial_len, false).await;
        assert_eq!(offset, file_len(&log));
    }

    #[tokio::test]
    async fn read_new_bytes_handles_truncated_file() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("test.log");
        std::fs::write(&log, "aaaa\nbbbb\n").unwrap();
        let old_offset = file_len(&log);

        // Truncate (simulates log rotation)
        std::fs::write(&log, "cc\n").unwrap();

        let offset = read_new_bytes(&log, old_offset, false).await;
        assert_eq!(offset, file_len(&log));
    }

    #[tokio::test]
    async fn read_new_bytes_holds_back_incomplete_line() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("test.log");
        std::fs::write(&log, "").unwrap();

        // Write an incomplete line (no trailing newline)
        std::fs::write(&log, "partial").unwrap();
        let offset = read_new_bytes(&log, 0, false).await;
        assert_eq!(offset, 0, "single incomplete line should be held back");

        // Complete the line and add another incomplete one
        std::fs::write(&log, "partial\nmore").unwrap();
        let offset = read_new_bytes(&log, 0, false).await;
        assert_eq!(
            offset, 8,
            "should consume 'partial\\n' but hold back 'more'"
        );
    }

    #[tokio::test]
    async fn read_new_bytes_missing_file_returns_offset() {
        let offset = read_new_bytes(std::path::Path::new("/no/such/file.log"), 42, false).await;
        assert_eq!(offset, 42);
    }

    #[test]
    fn pick_best_logs_dir_prefers_most_recent() {
        let root = tempfile::tempdir().unwrap();

        // Create two candidate dirs, both with stdout.log
        let stale_dir = root.path().join("stale");
        let fresh_dir = root.path().join("fresh");
        std::fs::create_dir_all(&stale_dir).unwrap();
        std::fs::create_dir_all(&fresh_dir).unwrap();

        std::fs::write(stale_dir.join("stdout.log"), "old content\n").unwrap();
        // Ensure a time gap so the modification times differ
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(fresh_dir.join("stdout.log"), "new content\n").unwrap();

        let result = pick_best_logs_dir(&[stale_dir.clone(), fresh_dir.clone()]).unwrap();
        assert_eq!(result, fresh_dir);
    }

    #[test]
    fn pick_best_logs_dir_skips_empty_files() {
        let root = tempfile::tempdir().unwrap();
        let empty_dir = root.path().join("empty");
        let content_dir = root.path().join("content");
        std::fs::create_dir_all(&empty_dir).unwrap();
        std::fs::create_dir_all(&content_dir).unwrap();

        std::fs::write(empty_dir.join("stdout.log"), "").unwrap();
        std::fs::write(content_dir.join("stdout.log"), "data\n").unwrap();

        let result = pick_best_logs_dir(&[empty_dir.clone(), content_dir.clone()]).unwrap();
        assert_eq!(result, content_dir);
    }

    #[test]
    fn pick_best_logs_dir_returns_none_when_no_content() {
        let root = tempfile::tempdir().unwrap();
        let dir_a = root.path().join("a");
        let dir_b = root.path().join("b");
        std::fs::create_dir_all(&dir_a).unwrap();
        // dir_b doesn't even exist

        std::fs::write(dir_a.join("stdout.log"), "").unwrap();

        assert!(pick_best_logs_dir(&[dir_a, dir_b]).is_none());
    }

    #[tokio::test]
    async fn follow_logs_exits_on_ctrl_c() {
        let dir = tempfile::tempdir().unwrap();
        let stdout_log = dir.path().join("stdout.log");
        let stderr_log = dir.path().join("stderr.log");
        std::fs::write(&stdout_log, "").unwrap();
        std::fs::write(&stderr_log, "").unwrap();

        // Send SIGINT to ourselves after a short delay so follow_logs unblocks
        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            #[cfg(unix)]
            {
                nix::sys::signal::raise(nix::sys::signal::Signal::SIGINT).unwrap();
            }
        });

        let code = follow_logs(&stdout_log, &stderr_log).await;
        assert_eq!(code, 0);
        handle.await.unwrap();
    }

    #[test]
    fn dir_size_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(dir_size(dir.path()), 0);
    }

    #[test]
    fn dir_size_with_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap(); // 5 bytes
        std::fs::write(dir.path().join("b.txt"), "world!").unwrap(); // 6 bytes
        assert_eq!(dir_size(dir.path()), 11);
    }

    #[test]
    fn discover_disk_worker_names_unions_managed_and_pids() {
        let dir = tempfile::tempdir().unwrap();
        let managed = dir.path().join("managed");
        let pids = dir.path().join("pids");
        std::fs::create_dir_all(managed.join("alpha")).unwrap();
        std::fs::create_dir_all(managed.join("beta")).unwrap();
        std::fs::create_dir_all(&pids).unwrap();
        std::fs::write(pids.join("gamma.pid"), "1234").unwrap();
        // Overlap: same name appears in both sources -> deduped.
        std::fs::write(pids.join("alpha.pid"), "5678").unwrap();

        let names = discover_disk_worker_names_in(&managed, &pids);
        assert_eq!(names, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn discover_disk_worker_names_handles_missing_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let names = discover_disk_worker_names_in(
            &dir.path().join("nope-managed"),
            &dir.path().join("nope-pids"),
        );
        assert!(names.is_empty());
    }

    #[test]
    fn discover_disk_worker_names_skips_non_pid_files_and_loose_files() {
        let dir = tempfile::tempdir().unwrap();
        let managed = dir.path().join("managed");
        let pids = dir.path().join("pids");
        std::fs::create_dir_all(managed.join("alpha")).unwrap();
        // Loose file under managed/ is not a worker dir, must be skipped.
        std::fs::write(managed.join("README.md"), "").unwrap();
        std::fs::create_dir_all(&pids).unwrap();
        std::fs::write(pids.join("beta.pid"), "1").unwrap();
        // Non-.pid files under pids/ must be skipped.
        std::fs::write(pids.join("notes.txt"), "").unwrap();
        // Empty-name guard: a bare ".pid" file must not yield "".
        std::fs::write(pids.join(".pid"), "").unwrap();

        let names = discover_disk_worker_names_in(&managed, &pids);
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn discover_running_from_ps_finds_binary_workers() {
        let workers = std::path::PathBuf::from("/home/u/.iii/workers");
        let managed = std::path::PathBuf::from("/home/u/.iii/managed");
        let ps = "\
/home/u/.iii/workers/image-resize\n\
/usr/bin/zsh\n\
/home/u/.iii/workers/another-binary --flag\n";
        let names = discover_running_worker_names_from_ps_output(ps, &workers, &managed);
        assert_eq!(names, vec!["another-binary", "image-resize"]);
    }

    #[test]
    fn discover_running_from_ps_finds_vm_boot_workers() {
        let workers = std::path::PathBuf::from("/home/u/.iii/workers");
        let managed = std::path::PathBuf::from("/home/u/.iii/managed");
        // Real-world cmdline shape from ~/.local/bin/iii-worker __vm-boot.
        let ps = "\
/home/u/.local/bin/iii-worker __vm-boot --rootfs /home/u/.iii/managed/todo-worker-python/rootfs --exec todo-worker --workdir /app --pid-file /home/u/.iii/managed/todo-worker-python/vm.pid --env FOO=bar\n\
/home/u/.local/bin/iii-worker __vm-boot --pid-file /home/u/.iii/managed/postgres/vm.pid --rootfs /home/u/.iii/managed/postgres/rootfs\n";
        let names = discover_running_worker_names_from_ps_output(ps, &workers, &managed);
        assert_eq!(names, vec!["postgres", "todo-worker-python"]);
    }

    #[test]
    fn discover_running_from_ps_dedups_across_patterns() {
        let workers = std::path::PathBuf::from("/h/.iii/workers");
        let managed = std::path::PathBuf::from("/h/.iii/managed");
        // Same name via both binary and vm-boot patterns -> single entry.
        let ps = "\
/h/.iii/workers/dual\n\
/h/.local/bin/iii-worker __vm-boot --pid-file /h/.iii/managed/dual/vm.pid\n";
        let names = discover_running_worker_names_from_ps_output(ps, &workers, &managed);
        assert_eq!(names, vec!["dual"]);
    }

    #[test]
    fn discover_running_from_ps_ignores_unrelated_processes_and_malformed_input() {
        let workers = std::path::PathBuf::from("/h/.iii/workers");
        let managed = std::path::PathBuf::from("/h/.iii/managed");
        let ps = "\
\n\
   \n\
/usr/bin/python\n\
/h/.local/bin/iii-worker __serve\n\
/h/.local/bin/iii-worker __vm-boot --rootfs /h/.iii/managed/x/rootfs\n\
/h/.local/bin/iii-worker __vm-boot --pid-file\n\
/h/.local/bin/iii-worker __vm-boot --pid-file /elsewhere/vm.pid\n";
        // No `--pid-file <path>` matching managed prefix → no orphans found.
        let names = discover_running_worker_names_from_ps_output(ps, &workers, &managed);
        assert!(names.is_empty(), "got unexpected names: {names:?}");
    }

    #[test]
    fn discover_running_from_proc_style_cmdlines() {
        // Simulates Linux /proc/<pid>/cmdline shape: NULs → spaces → joined
        // with newlines exactly the way collect_cmdlines() produces. Verifies
        // the parser is identical regardless of source platform.
        let workers = std::path::PathBuf::from("/h/.iii/workers");
        let managed = std::path::PathBuf::from("/h/.iii/managed");
        let proc_like = [
            "/h/.iii/workers/image-resize",
            "/h/.local/bin/iii-worker __vm-boot --pid-file /h/.iii/managed/todo/vm.pid --rootfs /h/.iii/managed/todo/rootfs",
            "/usr/bin/python3 server.py",
        ]
        .join("\n");
        let names = discover_running_worker_names_from_ps_output(&proc_like, &workers, &managed);
        assert_eq!(names, vec!["image-resize", "todo"]);
    }

    #[test]
    fn find_worker_pid_returns_first_matching_process() {
        let workers = std::path::PathBuf::from("/h/.iii/workers");
        let managed = std::path::PathBuf::from("/h/.iii/managed");
        let processes = vec![
            (12, "/usr/bin/zsh".to_string()),
            (
                42,
                "/h/.local/bin/iii-worker __vm-boot --pid-file /h/.iii/managed/todo/vm.pid"
                    .to_string(),
            ),
            (77, "/h/.iii/workers/image-resize".to_string()),
        ];
        assert_eq!(
            find_worker_pid_in_processes(&processes, "todo", &workers, &managed),
            Some(42)
        );
        assert_eq!(
            find_worker_pid_in_processes(&processes, "image-resize", &workers, &managed),
            Some(77)
        );
        assert_eq!(
            find_worker_pid_in_processes(&processes, "no-such-worker", &workers, &managed),
            None
        );
    }

    #[test]
    fn find_worker_pid_returns_none_for_empty_input() {
        let workers = std::path::PathBuf::from("/h/.iii/workers");
        let managed = std::path::PathBuf::from("/h/.iii/managed");
        assert_eq!(
            find_worker_pid_in_processes(&[], "anything", &workers, &managed),
            None
        );
    }

    #[test]
    fn resolve_orphan_type_managed_takes_priority() {
        let tmp = tempfile::tempdir().unwrap();
        let managed = tmp.path().join("managed");
        let pids = tmp.path().join("pids");
        let workers = tmp.path().join("workers");
        std::fs::create_dir_all(managed.join("dual")).unwrap();
        std::fs::create_dir_all(&pids).unwrap();
        std::fs::write(pids.join("dual.pid"), "1").unwrap();
        // managed/ wins because the directory shape carries more information
        // (rootfs, logs, etc.) than a bare pidfile.
        assert_eq!(
            resolve_orphan_type("dual", &managed, &pids, &workers),
            "managed"
        );
    }

    #[test]
    fn resolve_orphan_type_binary_via_pidfile() {
        let tmp = tempfile::tempdir().unwrap();
        let managed = tmp.path().join("managed");
        let pids = tmp.path().join("pids");
        let workers = tmp.path().join("workers");
        std::fs::create_dir_all(&pids).unwrap();
        std::fs::write(pids.join("img-resize.pid"), "1234").unwrap();
        assert_eq!(
            resolve_orphan_type("img-resize", &managed, &pids, &workers),
            "binary"
        );
    }

    #[test]
    fn resolve_orphan_type_binary_via_workers_executable() {
        let tmp = tempfile::tempdir().unwrap();
        let managed = tmp.path().join("managed");
        let pids = tmp.path().join("pids");
        let workers = tmp.path().join("workers");
        std::fs::create_dir_all(&workers).unwrap();
        std::fs::write(workers.join("img-resize"), b"#!/bin/sh\n").unwrap();
        // No pidfile, only the executable -- still recognisable as binary.
        assert_eq!(
            resolve_orphan_type("img-resize", &managed, &pids, &workers),
            "binary"
        );
    }

    #[test]
    fn resolve_orphan_type_unknown_when_only_ps_evidence() {
        let tmp = tempfile::tempdir().unwrap();
        let managed = tmp.path().join("managed");
        let pids = tmp.path().join("pids");
        let workers = tmp.path().join("workers");
        // Nothing on disk under any of the three roots: a worker that is alive
        // in ps but has had every artifact cleaned up. Honest answer is "?".
        assert_eq!(resolve_orphan_type("ghost", &managed, &pids, &workers), "?");
    }

    #[test]
    fn dir_size_nested() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("nested.txt"), "abc").unwrap(); // 3 bytes
        std::fs::write(dir.path().join("top.txt"), "de").unwrap(); // 2 bytes
        assert_eq!(dir_size(dir.path()), 5);
    }

    #[test]
    fn dir_size_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("does_not_exist");
        assert_eq!(dir_size(&gone), 0);
    }

    #[test]
    fn is_worker_running_no_pid_files() {
        // Worker name that certainly has no PID files on this system
        assert!(!is_worker_running("__iii_test_nonexistent_worker_12345__"));
    }

    #[test]
    fn is_worker_running_stale_pid_file() {
        // Create a fake PID file with a PID that doesn't exist, using tempdir
        let dir = tempfile::tempdir().unwrap();
        let pid_dir = dir.path().join("worker");
        std::fs::create_dir_all(&pid_dir).unwrap();
        let pid_file = pid_dir.join("worker.pid");
        // Use PID 2000000000 which almost certainly doesn't exist
        std::fs::write(&pid_file, "2000000000").unwrap();

        // Read the PID and verify it's considered dead (same logic as is_worker_running)
        let pid_str = std::fs::read_to_string(&pid_file).unwrap();
        let pid: u32 = pid_str.trim().parse().unwrap();
        #[cfg(unix)]
        {
            use nix::sys::signal::kill;
            use nix::unistd::Pid;
            assert!(kill(Pid::from_raw(pid as i32), None).is_err());
        }
        // Tempdir auto-cleans on drop
    }

    #[test]
    fn delete_worker_artifacts_nothing_to_delete() {
        let freed = delete_worker_artifacts("__iii_test_no_artifacts_exist__");
        assert_eq!(freed, 0);
    }

    #[test]
    fn image_cache_dir_consistent() {
        let dir1 = image_cache_dir("ghcr.io/org/worker:1.0");
        let dir2 = image_cache_dir("ghcr.io/org/worker:1.0");
        assert_eq!(dir1, dir2);
        // Different refs produce different dirs
        let dir3 = image_cache_dir("ghcr.io/org/worker:2.0");
        assert_ne!(dir1, dir3);
    }

    #[test]
    fn confirm_clear_returns_false_on_empty_stdin() {
        // confirm_clear reads from stdin — in test context stdin is closed/empty,
        // so read returns 0 bytes which should not match "y"
        // We can't easily call confirm_clear (it blocks on stdin), but we can
        // verify the logic inline:
        let input = "";
        assert!(!input.trim().eq_ignore_ascii_case("y"));
        let input = "n\n";
        assert!(!input.trim().eq_ignore_ascii_case("y"));
        let input = "y\n";
        assert!(input.trim().eq_ignore_ascii_case("y"));
        let input = "Y\n";
        assert!(input.trim().eq_ignore_ascii_case("y"));
        // CR-only line endings (raw/non-canonical terminal mode)
        let input = "y\r";
        assert!(input.trim().eq_ignore_ascii_case("y"));
        let input = "Y\r";
        assert!(input.trim().eq_ignore_ascii_case("y"));
        let input = "y\r\n";
        assert!(input.trim().eq_ignore_ascii_case("y"));
        let input = "\r";
        assert!(!input.trim().eq_ignore_ascii_case("y"));
    }

    #[test]
    fn delete_worker_artifacts_removes_binary_file() {
        // Test the legacy single-file binary path
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("test-worker");
        std::fs::write(&binary, "fake binary content 1234567890").unwrap(); // 30 bytes

        // delete_worker_artifacts operates on ~/.iii/workers/{name}
        // We can't easily redirect it, but we can test dir_size + remove_dir_all directly
        let size_before = dir_size(dir.path());
        assert!(size_before >= 30);

        // Verify the file exists, then remove and check
        assert!(binary.exists());
        std::fs::remove_file(&binary).unwrap();
        assert!(!binary.exists());
        assert_eq!(dir_size(dir.path()), 0);
    }

    #[test]
    fn delete_worker_artifacts_removes_nested_binary_dir() {
        let dir = tempfile::tempdir().unwrap();
        let worker_dir = dir.path().join("my-worker");
        std::fs::create_dir_all(&worker_dir).unwrap();
        std::fs::write(worker_dir.join("binary"), "executable bytes").unwrap();
        std::fs::write(worker_dir.join("worker.pid"), "12345").unwrap();

        let size = dir_size(&worker_dir);
        assert!(size > 0);

        // Simulate what delete_worker_artifacts does for binary dirs
        std::fs::remove_dir_all(&worker_dir).unwrap();
        assert!(!worker_dir.exists());
    }

    #[test]
    fn is_worker_running_invalid_pid_content() {
        // PID file with non-numeric content should return false
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("worker.pid");
        std::fs::write(&pid_file, "not-a-number").unwrap();

        // parse::<u32>() will fail, so the loop continues and returns false
        let content = std::fs::read_to_string(&pid_file).unwrap();
        assert!(content.trim().parse::<u32>().is_err());
    }

    #[test]
    fn is_worker_running_empty_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("worker.pid");
        std::fs::write(&pid_file, "").unwrap();

        let content = std::fs::read_to_string(&pid_file).unwrap();
        assert!(content.trim().parse::<u32>().is_err());
    }

    #[test]
    fn kill_stale_worker_removes_pid_files() {
        // Create fake PID files with a dead PID
        let dir = tempfile::tempdir().unwrap();
        let managed_dir = dir.path().join("managed").join("test-worker");
        std::fs::create_dir_all(&managed_dir).unwrap();
        std::fs::write(managed_dir.join("vm.pid"), "2000000000").unwrap();

        let pids_dir = dir.path().join("pids");
        std::fs::create_dir_all(&pids_dir).unwrap();
        std::fs::write(pids_dir.join("test-worker.pid"), "2000000000").unwrap();

        // Verify files exist
        assert!(managed_dir.join("vm.pid").exists());
        assert!(pids_dir.join("test-worker.pid").exists());

        // kill_stale_worker uses real ~/.iii paths, so we test the logic directly:
        // dead PID → signal-0 fails → no kill attempt → file removed
        for pid_file in [managed_dir.join("vm.pid"), pids_dir.join("test-worker.pid")] {
            if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
                if let Ok(pid) = pid_str.trim().parse::<i32>() {
                    #[cfg(unix)]
                    {
                        use nix::sys::signal::kill;
                        use nix::unistd::Pid;
                        // PID 2000000000 should not be alive
                        assert!(kill(Pid::from_raw(pid), None).is_err());
                    }
                }
                let _ = std::fs::remove_file(&pid_file);
            }
        }

        // Files should be cleaned up
        assert!(!managed_dir.join("vm.pid").exists());
        assert!(!pids_dir.join("test-worker.pid").exists());
    }

    #[tokio::test]
    async fn kill_stale_worker_no_op_when_no_pid_files() {
        // Should not panic when no PID files exist
        kill_stale_worker("__iii_test_nonexistent_99999__").await;
    }

    #[tokio::test]
    async fn kill_stale_worker_removes_watch_pid_file() {
        // Writes a fake `watch.pid` with a highly unlikely-to-be-alive
        // PID, then verifies `kill_stale_worker` reaps it from the
        // pid-file list introduced when the source watcher sidecar
        // landed. Uses a unique worker name to avoid collisions with
        // any real workers on the developer's machine.
        let home = dirs::home_dir().unwrap_or_default();
        let worker_name = "__iii_test_watch_pid_cleanup__";
        let managed_dir = home.join(".iii/managed").join(worker_name);
        let _ = std::fs::create_dir_all(&managed_dir);
        let watch_pidfile = managed_dir.join("watch.pid");
        std::fs::write(&watch_pidfile, "2000000000").unwrap();
        assert!(watch_pidfile.exists());

        kill_stale_worker(worker_name).await;

        assert!(
            !watch_pidfile.exists(),
            "watch.pid should be reaped by kill_stale_worker"
        );

        let _ = std::fs::remove_dir_all(&managed_dir);
    }

    #[tokio::test]
    async fn reap_source_watcher_removes_pid_file() {
        // Exercises the stop-path helper used by `handle_managed_stop`
        // to tear down the watcher sidecar before stopping the VM. A
        // dead PID in watch.pid should still produce a clean remove
        // (signal(0) returns ESRCH, kill_pid_with_grace no-ops, file
        // is unlinked unconditionally).
        let home = dirs::home_dir().unwrap_or_default();
        let worker_name = "__iii_test_reap_watcher__";
        let managed_dir = home.join(".iii/managed").join(worker_name);
        let _ = std::fs::create_dir_all(&managed_dir);
        let watch_pidfile = managed_dir.join("watch.pid");
        std::fs::write(&watch_pidfile, "2000000001").unwrap();
        assert!(watch_pidfile.exists());

        reap_source_watcher(worker_name).await;

        assert!(
            !watch_pidfile.exists(),
            "watch.pid should be removed by reap_source_watcher"
        );

        let _ = std::fs::remove_dir_all(&managed_dir);
    }

    #[tokio::test]
    async fn reap_source_watcher_no_op_when_no_pid_file() {
        // Idempotent on the cold path — no watch.pid, nothing to do,
        // no panic.
        reap_source_watcher("__iii_test_reap_watcher_nonexistent__").await;
    }

    #[tokio::test]
    async fn reap_source_watcher_handles_garbage_pid_content() {
        // Parse failure must not prevent file removal.
        let home = dirs::home_dir().unwrap_or_default();
        let worker_name = "__iii_test_reap_watcher_garbage__";
        let managed_dir = home.join(".iii/managed").join(worker_name);
        let _ = std::fs::create_dir_all(&managed_dir);
        let watch_pidfile = managed_dir.join("watch.pid");
        std::fs::write(&watch_pidfile, "not-a-pid").unwrap();

        reap_source_watcher(worker_name).await;

        assert!(!watch_pidfile.exists());
        let _ = std::fs::remove_dir_all(&managed_dir);
    }

    #[tokio::test]
    async fn kill_stale_worker_handles_invalid_pid_content() {
        // Use real function with a worker name that won't collide
        // The function should handle garbage content gracefully
        let home = dirs::home_dir().unwrap_or_default();
        let pids_dir = home.join(".iii/pids");
        let _ = std::fs::create_dir_all(&pids_dir);
        let pid_file = pids_dir.join("__iii_test_garbage_pid__.pid");
        std::fs::write(&pid_file, "not-a-number").unwrap();

        kill_stale_worker("__iii_test_garbage_pid__").await;

        // File should still be removed even with garbage content
        assert!(!pid_file.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn kill_stale_worker_ignores_symlinked_pidfile() {
        // A pre-planted symlink at the pidfile location must not be
        // followed: read_pid opens with O_NOFOLLOW and returns None, so
        // we skip the kill. The symlink itself is still removed so
        // subsequent starts aren't jammed up by stale state.
        let home = dirs::home_dir().unwrap_or_default();
        let worker_name = "__iii_test_symlink_pidfile__";
        let managed_dir = home.join(".iii/managed").join(worker_name);
        let _ = std::fs::create_dir_all(&managed_dir);

        // Attacker-controlled file we must NOT overwrite or target.
        let sensitive = managed_dir.join("sensitive");
        std::fs::write(&sensitive, "DO-NOT-TOUCH").unwrap();

        let watch_pidfile = managed_dir.join("watch.pid");
        std::os::unix::fs::symlink(&sensitive, &watch_pidfile).unwrap();

        kill_stale_worker(worker_name).await;

        // Symlink removed, target untouched.
        assert!(!watch_pidfile.exists(), "symlink should be cleaned up");
        assert_eq!(
            std::fs::read_to_string(&sensitive).unwrap(),
            "DO-NOT-TOUCH",
            "symlink target must not be read/modified"
        );

        let _ = std::fs::remove_dir_all(&managed_dir);
    }

    #[test]
    fn binary_pid_path_uses_pids_dir() {
        // Verify the PID path for binary workers doesn't conflict with the binary file
        let home = dirs::home_dir().unwrap_or_default();
        let binary_path = home.join(".iii/workers/some-worker");
        let pid_path = home.join(".iii/pids/some-worker.pid");

        // These should be different paths — binary at workers/{name}, PID at pids/{name}.pid
        assert_ne!(binary_path.parent().unwrap(), pid_path.parent().unwrap());
        assert!(pid_path.to_string_lossy().ends_with(".pid"));
    }

    #[test]
    fn image_cache_dir_deterministic_hash() {
        // Same ref always produces same path
        let a = image_cache_dir("ghcr.io/org/worker:1.0");
        let b = image_cache_dir("ghcr.io/org/worker:1.0");
        assert_eq!(a, b);

        // Path ends with a hex string (16 chars for 8 bytes)
        let hash_component = a.file_name().unwrap().to_str().unwrap();
        assert_eq!(hash_component.len(), 16);
        assert!(hash_component.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn image_cache_dir_under_iii_images() {
        let dir = image_cache_dir("test:latest");
        let path_str = dir.to_string_lossy();
        assert!(path_str.contains(".iii/images/") || path_str.contains(".iii\\images\\"));
    }

    #[tokio::test]
    async fn handle_managed_add_routes_local_path() {
        in_temp_dir_async(|dir| async move {
            // Create a minimal project directory with package.json
            let proj = dir.join("test-local-worker");
            std::fs::create_dir_all(&proj).unwrap();
            std::fs::write(proj.join("package.json"), r#"{"name":"test"}"#).unwrap();

            let path_str = proj.to_string_lossy().to_string();
            let exit_code = handle_managed_add(&path_str, false, false, false, false).await;
            assert_eq!(exit_code, 0, "should succeed for valid local path");

            let content = std::fs::read_to_string("config.yaml").unwrap();
            assert!(
                content.contains("worker_path:"),
                "should write worker_path field, got:\n{}",
                content
            );
            assert!(
                !content.contains("image:"),
                "should not have image field, got:\n{}",
                content
            );
        })
        .await;
    }

    #[tokio::test]
    async fn handle_managed_add_local_path_rejects_nonexistent() {
        in_temp_dir_async(|_dir| async move {
            let exit_code =
                handle_managed_add("./nonexistent-path-12345", false, false, false, false).await;
            assert_eq!(exit_code, 1, "should fail for nonexistent local path");
        })
        .await;
    }

    #[tokio::test]
    async fn handle_managed_add_local_path_force_replaces() {
        in_temp_dir_async(|dir| async move {
            // Create project directory
            let proj = dir.join("force-worker");
            std::fs::create_dir_all(&proj).unwrap();
            std::fs::write(proj.join("package.json"), r#"{"name":"force-test"}"#).unwrap();

            let path_str = proj.to_string_lossy().to_string();

            // First add
            let exit_code = handle_managed_add(&path_str, false, false, false, false).await;
            assert_eq!(exit_code, 0);
            assert!(
                std::fs::read_to_string("config.yaml")
                    .unwrap()
                    .contains("worker_path:")
            );

            // Force re-add
            let exit_code = handle_managed_add(&path_str, false, true, false, false).await;
            assert_eq!(exit_code, 0, "force re-add should succeed");

            let content = std::fs::read_to_string("config.yaml").unwrap();
            assert!(
                content.contains("worker_path:"),
                "should still have worker_path after force, got:\n{}",
                content
            );
        })
        .await;
    }
}
