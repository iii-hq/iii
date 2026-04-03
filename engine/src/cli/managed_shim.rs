// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Lightweight lifecycle shim for managed workers.
//!
//! When the engine boots and `iii.workers.yaml` declares workers, this module
//! locates (or auto-downloads) the `iii-worker` binary and spawns it to
//! start/stop worker VMs. No VMM code is linked into the engine.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

const WORKERS_FILE: &str = "iii.workers.yaml";

/// Minimal representation of iii.workers.yaml — only needs to know worker names.
#[derive(Debug, Deserialize, Default)]
struct WorkersFile {
    #[serde(default)]
    workers: HashMap<String, serde_yaml::Value>,
}

impl WorkersFile {
    fn load() -> Option<Self> {
        let path = Path::new(WORKERS_FILE);
        if !path.exists() {
            return None;
        }
        let data = std::fs::read_to_string(path).ok()?;
        let file: WorkersFile = serde_yaml::from_str(&data).ok()?;
        if file.workers.is_empty() {
            return None;
        }
        Some(file)
    }
}

/// Resolve the `iii-worker` binary path.
///
/// Checks the managed binary directory and PATH. If not found, downloads it
/// using the same mechanism as other dispatched binaries (iii-console, etc.).
async fn resolve_worker_binary() -> Option<std::path::PathBuf> {
    use super::{download, github, platform, registry, state};

    let spec = match registry::resolve_command("worker") {
        Ok((spec, _)) => spec,
        Err(_) => return None,
    };

    // Check managed dir first
    let managed_path = platform::binary_path(spec.name);
    if managed_path.exists() {
        return Some(managed_path);
    }

    // Check PATH
    if let Some(existing) = platform::find_existing_binary(spec.name) {
        return Some(existing);
    }

    // Auto-download
    tracing::info!("iii-worker binary not found, downloading...");

    let client = match github::build_client() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "failed to create HTTP client for iii-worker download");
            return None;
        }
    };

    let release = match github::fetch_latest_release(&client, spec).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "failed to fetch iii-worker release");
            return None;
        }
    };

    let asset_name = platform::asset_name(spec.name);
    let asset = match github::find_asset(&release, &asset_name) {
        Some(a) => a,
        None => {
            tracing::warn!(asset = %asset_name, "iii-worker release asset not found");
            return None;
        }
    };

    let checksum_url = if spec.has_checksum {
        let checksum_name = platform::checksum_asset_name(spec.name);
        github::find_asset(&release, &checksum_name).map(|a| a.browser_download_url.clone())
    } else {
        None
    };

    if let Err(e) = download::download_and_install(
        &client,
        spec,
        asset,
        checksum_url.as_deref(),
        &managed_path,
    )
    .await
    {
        tracing::warn!(error = %e, "failed to download iii-worker");
        return None;
    }

    // Record installation in state
    if let Ok(mut app_state) = state::AppState::load(&platform::state_file_path()) {
        let version = github::parse_release_version(&release.tag_name)
            .unwrap_or_else(|_| semver::Version::new(0, 0, 0));
        app_state.record_install(spec.name, version, asset_name);
        let _ = app_state.save(&platform::state_file_path());
    }

    tracing::info!("iii-worker downloaded successfully");
    Some(managed_path)
}

/// Start all workers declared in `iii.workers.yaml`.
///
/// Runs in a background task so engine boot is not blocked. If `iii-worker`
/// is not installed, it is downloaded automatically.
pub async fn start_managed_workers(engine_url: &str) {
    let workers_file = match WorkersFile::load() {
        Some(f) => f,
        None => return,
    };

    let worker_binary = match resolve_worker_binary().await {
        Some(p) => p,
        None => {
            tracing::warn!(
                "iii-worker binary not available; managed workers from iii.workers.yaml will not start"
            );
            return;
        }
    };

    tracing::info!(
        count = workers_file.workers.len(),
        "Starting managed workers from iii.workers.yaml..."
    );

    // Extract port from engine_url for --port flag
    let port = engine_url
        .rsplit_once(':')
        .and_then(|(_, p)| p.parse::<u16>().ok())
        .unwrap_or(49134);

    for name in workers_file.workers.keys() {
        let result = tokio::process::Command::new(&worker_binary)
            .args(["start", name, "--port", &port.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await;

        match result {
            Ok(status) if status.success() => {
                tracing::info!(worker = %name, "Managed worker started");
            }
            Ok(status) => {
                tracing::warn!(worker = %name, exit_code = ?status.code(), "Failed to start managed worker");
            }
            Err(e) => {
                tracing::warn!(worker = %name, error = %e, "Failed to spawn iii-worker start");
            }
        }
    }
}

/// Stop all managed worker VMs.
///
/// Tries to use `iii-worker stop` if the binary is available. Falls back to
/// direct PID-file-based SIGTERM if not.
pub async fn stop_managed_workers() {
    let workers_file = match WorkersFile::load() {
        Some(f) => f,
        None => return,
    };

    tracing::info!(
        count = workers_file.workers.len(),
        "Stopping managed workers..."
    );

    // Try to find iii-worker binary (but don't download it for shutdown)
    let worker_binary = {
        let managed_path = super::platform::binary_path("iii-worker");
        if managed_path.exists() {
            Some(managed_path)
        } else {
            super::platform::find_existing_binary("iii-worker")
        }
    };

    let futures: Vec<_> = workers_file
        .workers
        .keys()
        .map(|name| {
            let name = name.clone();
            let binary = worker_binary.clone();
            async move {
                if let Some(ref bin) = binary {
                    // Use iii-worker stop
                    let result = tokio::process::Command::new(bin)
                        .args(["stop", &name])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status()
                        .await;

                    match result {
                        Ok(status) if status.success() => return,
                        _ => {} // Fall through to PID-based stop
                    }
                }

                // Fallback: direct PID-file-based SIGTERM
                let pid_file = dirs::home_dir()
                    .unwrap_or_default()
                    .join(".iii/managed")
                    .join(&name)
                    .join("vm.pid");

                if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
                    if let Ok(pid) = pid_str.trim().parse::<i32>() {
                        #[cfg(unix)]
                        {
                            let _ = nix::sys::signal::kill(
                                nix::unistd::Pid::from_raw(pid),
                                nix::sys::signal::Signal::SIGTERM,
                            );
                        }
                        let _ = std::fs::remove_file(&pid_file);
                        tracing::info!(worker = %name, pid = pid, "Sent SIGTERM to managed worker (fallback)");
                    }
                }
            }
        })
        .collect();

    futures::future::join_all(futures).await;

    tracing::info!("Managed workers stopped");
}
