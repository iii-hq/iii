// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! CLI command handlers for managing OCI-based workers.

use colored::Colorize;

use super::binary_download;
use super::builtin_defaults::get_builtin_default;
use super::lifecycle::build_container_spec;
use super::registry::{
    MANIFEST_PATH, WorkerType, fetch_registry, parse_worker_input, resolve_image,
};
use super::worker_manager::state::WorkerDef;

pub use super::dev::handle_worker_dev;

pub async fn handle_binary_add(input: &str, _runtime: &str, _address: &str, _port: u16) -> i32 {
    let (worker_name, version_override) = parse_worker_input(input);

    eprintln!("  Resolving {}...", worker_name.bold());
    let registry = match fetch_registry().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };

    let entry = match registry.workers.get(&worker_name) {
        Some(e) => e,
        None => {
            eprintln!(
                "{} Worker '{}' not found in registry",
                "error:".red(),
                worker_name
            );
            return 1;
        }
    };

    let repo = match &entry.repo {
        Some(r) => r.clone(),
        None => {
            eprintln!(
                "{} Registry entry for '{}' is missing 'repo' field",
                "error:".red(),
                worker_name
            );
            return 1;
        }
    };

    let tag_prefix = match &entry.tag_prefix {
        Some(t) => t.clone(),
        None => worker_name.clone(),
    };

    let version = version_override
        .or_else(|| entry.version.clone())
        .unwrap_or_else(|| "latest".to_string());

    let supported_targets = entry.supported_targets.clone().unwrap_or_default();
    let has_checksum = entry.has_checksum.unwrap_or(false);

    let target = binary_download::current_target();
    eprintln!(
        "  {} Resolved to {} (binary v{})",
        "✓".green(),
        format!("{}", repo).dimmed(),
        version
    );

    eprintln!("  Downloading {}...", worker_name.bold());
    let install_path = match binary_download::download_and_install_binary(
        &worker_name,
        &repo,
        &tag_prefix,
        &version,
        &supported_targets,
        has_checksum,
    )
    .await
    {
        Ok(path) => path,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };

    eprintln!("  {} Downloaded successfully", "✓".green());

    // Show metadata matching OCI worker style
    eprintln!("  {}: {}", "Name".bold(), worker_name);
    eprintln!("  {}: {}", "Version".bold(), version);
    if !entry.description.is_empty() {
        eprintln!("  {}: {}", "Description".bold(), entry.description);
    }
    eprintln!("  {}: {}", "Platform".bold(), target);
    if let Ok(metadata) = std::fs::metadata(&install_path) {
        eprintln!(
            "  {}: {:.1} MB",
            "Size".bold(),
            metadata.len() as f64 / 1_048_576.0
        );
    }

    let config_yaml = entry
        .default_config
        .as_ref()
        .and_then(|dc| dc.get("config"))
        .map(|v| serde_yaml::to_string(v).unwrap_or_default());

    if let Err(e) = super::config_file::append_worker(&worker_name, config_yaml.as_deref()) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }

    eprintln!(
        "\n  {} Worker {} added to {}",
        "✓".green(),
        worker_name.bold(),
        "config.yaml".dimmed(),
    );
    eprintln!("  Start the engine to run it, or edit config.yaml to customize.");
    0
}

pub async fn handle_managed_add(
    image_or_name: &str,
    _runtime: &str,
    _address: &str,
    _port: u16,
) -> i32 {
    // Check for engine-builtin workers first (no network needed).
    if let Some(default_yaml) = get_builtin_default(image_or_name) {
        let already_exists = super::config_file::worker_exists(image_or_name);
        if let Err(e) = super::config_file::append_worker(image_or_name, Some(default_yaml)) {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
        if already_exists {
            eprintln!(
                "\n  {} Worker {} updated in {} (merged with builtin defaults)",
                "✓".green(),
                image_or_name.bold(),
                "config.yaml".dimmed(),
            );
        } else {
            eprintln!(
                "\n  {} Worker {} added to {}",
                "✓".green(),
                image_or_name.bold(),
                "config.yaml".dimmed(),
            );
        }
        eprintln!("  Start the engine to run it, or edit config.yaml to customize.");
        return 0;
    }

    // Route binary workers to handle_binary_add; for OCI workers found in the
    // registry, use the already-fetched entry to avoid a second HTTP roundtrip.
    if !image_or_name.contains('/') && !image_or_name.contains(':') {
        let (name, _) = parse_worker_input(image_or_name);
        if let Ok(registry) = fetch_registry().await {
            if let Some(entry) = registry.workers.get(&name) {
                if matches!(entry.worker_type, Some(WorkerType::Binary)) {
                    return handle_binary_add(image_or_name, _runtime, _address, _port).await;
                }
                // OCI worker found in registry — use already-fetched entry
                if let (Some(img), Some(ver)) = (&entry.image, &entry.latest) {
                    let image_ref = format!("{}:{}", img, ver);
                    eprintln!("  {} Resolved to {}", "✓".green(), image_ref.dimmed());
                    return handle_oci_pull_and_add(&name, &image_ref).await;
                }
            }
        }
    }

    eprintln!("  Resolving {}...", image_or_name.bold());
    let (image_ref, name) = match resolve_image(image_or_name).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };
    eprintln!("  {} Resolved to {}", "✓".green(), image_ref.dimmed());
    handle_oci_pull_and_add(&name, &image_ref).await
}

async fn handle_oci_pull_and_add(name: &str, image_ref: &str) -> i32 {
    let adapter = super::worker_manager::create_adapter("libkrun");

    eprintln!("  Pulling {}...", image_ref.bold());
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

    // Extract OCI env vars from the pulled image rootfs and write as config:
    let rootfs_dir = {
        use sha2::Digest;
        let mut hasher = sha2::Sha256::new();
        hasher.update(image_ref.as_bytes());
        let hash = hex::encode(&hasher.finalize()[..8]);
        dirs::home_dir()
            .unwrap_or_default()
            .join(".iii")
            .join("images")
            .join(hash)
    };
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
    eprintln!(
        "\n  {} Worker {} added to {}",
        "✓".green(),
        name.bold(),
        "config.yaml".dimmed(),
    );
    eprintln!("  Start the engine to run it, or edit config.yaml to customize.");
    0
}

pub async fn handle_managed_remove(worker_name: &str, _address: &str, _port: u16) -> i32 {
    if let Err(e) = super::registry::validate_worker_name(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }
    if let Err(e) = super::config_file::remove_worker(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }
    eprintln!(
        "  {} {} removed from {}",
        "✓".green(),
        worker_name.bold(),
        "config.yaml".dimmed(),
    );
    0
}

pub async fn handle_managed_stop(worker_name: &str, _address: &str, _port: u16) -> i32 {
    if let Err(e) = super::registry::validate_worker_name(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }
    let home = dirs::home_dir().unwrap_or_default();

    // Check OCI worker PID file
    let oci_pid_file = home.join(".iii/managed").join(worker_name).join("vm.pid");
    // Check binary worker PID file
    let binary_pid_file = home
        .join(".iii/workers")
        .join(worker_name)
        .join("worker.pid");

    let (pid_file, is_oci) = if oci_pid_file.exists() {
        (oci_pid_file, true)
    } else if binary_pid_file.exists() {
        (binary_pid_file, false)
    } else {
        eprintln!("{} Worker '{}' is not running", "error:".red(), worker_name);
        return 1;
    };

    match std::fs::read_to_string(&pid_file) {
        Ok(pid_str) => {
            let pid = pid_str.trim();
            eprintln!("  Stopping {}...", worker_name.bold());
            if is_oci {
                let adapter = super::worker_manager::create_adapter("libkrun");
                let _ = adapter.stop(pid, 10).await;
            } else {
                // Kill binary worker process directly
                if let Ok(pid_num) = pid.parse::<i32>() {
                    #[cfg(unix)]
                    {
                        use nix::sys::signal::{Signal, kill};
                        use nix::unistd::Pid;
                        let _ = kill(Pid::from_raw(pid_num), Signal::SIGTERM);
                        // Wait briefly then SIGKILL if still alive
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        let _ = kill(Pid::from_raw(pid_num), Signal::SIGKILL);
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = pid_num; // suppress unused warning
                        eprintln!(
                            "{} Binary worker stop not supported on this platform",
                            "error:".red()
                        );
                    }
                }
            }
            let _ = std::fs::remove_file(&pid_file);
            eprintln!("  {} {} stopped", "✓".green(), worker_name.bold());
            0
        }
        Err(_) => {
            eprintln!("{} Worker '{}' is not running", "error:".red(), worker_name);
            1
        }
    }
}

pub async fn handle_managed_start(worker_name: &str, _address: &str, port: u16) -> i32 {
    if let Err(e) = super::registry::validate_worker_name(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }
    // Check if this is an OCI worker (has image: in config.yaml)
    if let Some((image_ref, env)) = super::config_file::get_worker_start_info(worker_name) {
        let worker_def = WorkerDef::Managed {
            image: image_ref,
            env,
            resources: None,
        };
        return start_oci_worker(worker_name, &worker_def, port).await;
    }

    // Check if this is a binary worker (~/.iii/workers/{name} exists)
    let binary_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".iii/workers")
        .join(worker_name);

    if binary_path.exists() {
        return start_binary_worker(worker_name, &binary_path).await;
    }

    // Not found locally — try remote registry for auto-install
    eprintln!(
        "  Worker '{}' not found locally, checking registry...",
        worker_name
    );
    match fetch_registry().await {
        Ok(registry) => {
            if let Some(entry) = registry.workers.get(worker_name) {
                if matches!(entry.worker_type, Some(WorkerType::Binary)) {
                    // Auto-download binary worker
                    let repo = match &entry.repo {
                        Some(r) => r.clone(),
                        None => {
                            eprintln!(
                                "{} Registry entry for '{}' missing 'repo' field",
                                "error:".red(),
                                worker_name
                            );
                            return 1;
                        }
                    };
                    let tag_prefix = entry
                        .tag_prefix
                        .clone()
                        .unwrap_or_else(|| worker_name.to_string());
                    let version = entry
                        .version
                        .clone()
                        .or_else(|| entry.latest.clone())
                        .unwrap_or_else(|| "latest".to_string());
                    let supported_targets = entry.supported_targets.clone().unwrap_or_default();
                    let has_checksum = entry.has_checksum.unwrap_or(false);

                    eprintln!("  Installing {} (binary v{})...", worker_name, version);
                    match binary_download::download_and_install_binary(
                        worker_name,
                        &repo,
                        &tag_prefix,
                        &version,
                        &supported_targets,
                        has_checksum,
                    )
                    .await
                    {
                        Ok(installed_path) => {
                            eprintln!("  {} Installed successfully", "✓".green());
                            return start_binary_worker(worker_name, &installed_path).await;
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
                } else {
                    // OCI/managed worker from registry — resolve image and start
                    let image_ref = match &entry.image {
                        Some(img) => {
                            let version = entry.latest.as_deref().unwrap_or("latest");
                            format!("{}:{}", img, version)
                        }
                        None => {
                            eprintln!(
                                "{} Registry entry for '{}' missing 'image' field",
                                "error:".red(),
                                worker_name
                            );
                            return 1;
                        }
                    };
                    let worker_def = WorkerDef::Managed {
                        image: image_ref,
                        env: std::collections::HashMap::new(),
                        resources: None,
                    };
                    return start_oci_worker(worker_name, &worker_def, port).await;
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to fetch registry: {}", e);
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

async fn start_oci_worker(worker_name: &str, worker_def: &WorkerDef, port: u16) -> i32 {
    #[cfg(all(target_os = "linux", not(target_env = "musl")))]
    if let Err(e) = super::firmware::download::ensure_libkrunfw().await {
        tracing::warn!(error = %e, "failed to ensure libkrunfw availability");
    }

    if !super::worker_manager::sandbox_available() {
        eprintln!(
            "{} VM sandbox is not available.\n  \
             On Linux: rebuild with --features embed-libkrunfw or place libkrunfw in ~/.iii/lib/\n  \
             On macOS/musl: VM sandbox is not yet supported on this platform.",
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
    if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
        let _ = adapter.stop(pid_str.trim(), 5).await;
        let _ = adapter.remove(pid_str.trim()).await;
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
            // Write PID file for stop/status tracking
            let pid_dir = dirs::home_dir()
                .unwrap_or_default()
                .join(".iii/workers")
                .join(worker_name);
            let _ = std::fs::create_dir_all(&pid_dir);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&pid_dir, std::fs::Permissions::from_mode(0o700));
            }
            if let Some(pid) = child.id() {
                let pid_path = pid_dir.join("worker.pid");
                let _ = std::fs::write(&pid_path, pid.to_string());
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ =
                        std::fs::set_permissions(&pid_path, std::fs::Permissions::from_mode(0o600));
                }
            }
            eprintln!(
                "  {} {} started (pid: {:?})",
                "✓".green(),
                worker_name.bold(),
                child.id()
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
    let names = super::config_file::list_worker_names();

    if names.is_empty() {
        eprintln!("  No workers. Use `iii worker add` to get started.");
        return 0;
    }

    eprintln!();
    eprintln!("  {:25} {}", "NAME".bold(), "STATUS".bold());
    eprintln!("  {:25} {}", "----".dimmed(), "------".dimmed());

    for name in &names {
        let binary_path = dirs::home_dir()
            .unwrap_or_default()
            .join(".iii/workers")
            .join(name);
        let status = if binary_path.exists() {
            "binary (installed)".green().to_string()
        } else {
            "configured".dimmed().to_string()
        };
        eprintln!("  {:25} {}", name, status);
    }
    eprintln!();
    0
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

        if let Some(modified) = latest {
            if best.as_ref().map_or(true, |(_, t)| modified > *t) {
                best = Some((dir.clone(), modified));
            }
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

        if let Ok(contents) = std::fs::read_to_string(&stdout_path) {
            if !contents.is_empty() {
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
        }

        if let Ok(contents) = std::fs::read_to_string(&stderr_path) {
            if !contents.is_empty() {
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
}
