// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! CLI commands for managing OCI-based workers.
//!
//! These commands manage OCI-based workers running in libkrun VMs.
//! No external dependencies (Podman, Docker) needed.

use colored::Colorize;
use serde::Deserialize;
use std::collections::HashMap;

use super::worker_manager::adapter::ContainerSpec;
use super::worker_manager::state::{WorkerDef, WorkerResources, WorkersFile};

const MANIFEST_PATH: &str = "/iii/worker.yaml";

// ---------------------------------------------------------------------------
// Registry v2
// ---------------------------------------------------------------------------

const DEFAULT_REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/iii-hq/workers/main/registry/index.json";

#[derive(Debug, Deserialize)]
struct RegistryV2Entry {
    #[allow(dead_code)]
    description: String,
    image: String,
    latest: String,
}

#[derive(Debug, Deserialize)]
struct RegistryV2 {
    #[allow(dead_code)]
    version: u32,
    workers: HashMap<String, RegistryV2Entry>,
}

/// Resolve a user-supplied image argument to a full OCI image reference.
///
/// If the argument already contains `/` or `:` it is assumed to be a full
/// reference and returned as-is. Otherwise it is treated as a shorthand name
/// and looked up in the v2 registry.
async fn resolve_image(input: &str) -> Result<(String, String), String> {
    if input.contains('/') || input.contains(':') {
        let name = input
            .rsplit('/')
            .next()
            .unwrap_or(input)
            .split(':')
            .next()
            .unwrap_or(input);
        return Ok((input.to_string(), name.to_string()));
    }

    let url =
        std::env::var("III_REGISTRY_URL").unwrap_or_else(|_| DEFAULT_REGISTRY_URL.to_string());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let body = if url.starts_with("file://") {
        let path = url.strip_prefix("file://").unwrap();
        std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read local registry at {}: {}", path, e))?
    } else {
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch registry: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!("Registry returned HTTP {}", resp.status()));
        }
        resp.text()
            .await
            .map_err(|e| format!("Failed to read registry body: {}", e))?
    };

    let registry: RegistryV2 =
        serde_json::from_str(&body).map_err(|e| format!("Failed to parse registry: {}", e))?;

    let entry = registry
        .workers
        .get(input)
        .ok_or_else(|| format!("Worker '{}' not found in registry", input))?;

    let image_ref = format!("{}:{}", entry.image, entry.latest);
    Ok((image_ref, input.to_string()))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build engine WebSocket URL from a bind address like "0.0.0.0:49134".
/// libkrun uses TSI networking where localhost works directly.
pub fn managed_engine_url(bind_addr: &str) -> String {
    let (_host, port) = match bind_addr.rsplit_once(':') {
        Some((h, p)) => (h, p),
        None => (bind_addr, "49134"),
    };
    format!("ws://localhost:{}", port)
}

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

/// `iii worker add <image>`
/// Resolves the image, pulls it, extracts manifest info, and writes an entry
/// to iii.workers.yaml. Does NOT start the worker — that happens on engine startup.
pub async fn handle_managed_add(
    image_or_name: &str,
    _runtime: &str,
    _address: &str,
    _port: u16,
) -> i32 {
    let adapter = super::worker_manager::create_adapter("libkrun");

    // 1. Resolve image reference
    eprintln!("  Resolving {}...", image_or_name.bold());
    let (image_ref, name) = match resolve_image(image_or_name).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };
    eprintln!("  {} Resolved to {}", "✓".green(), image_ref.dimmed());

    // 2. Pull image
    eprintln!("  Pulling {}...", image_ref.bold());
    let pull_info = match adapter.pull(&image_ref).await {
        Ok(info) => info,
        Err(e) => {
            eprintln!("{} Pull failed: {}", "error:".red(), e);
            return 1;
        }
    };

    // 3. Extract manifest for display
    let manifest: Option<serde_json::Value> = match adapter.extract_file(&image_ref, MANIFEST_PATH).await {
        Ok(bytes) => {
            match String::from_utf8(bytes) {
                Ok(yaml_str) => serde_yaml::from_str(&yaml_str).ok(),
                Err(_) => None,
            }
        }
        Err(_) => None,
    };

    // 4. Display info and extract defaults
    let mut memory: Option<String> = None;
    let mut cpus: Option<String> = None;

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
        memory = m.get("resources").and_then(|r| r.get("memory")).and_then(|v| v.as_str()).map(|s| s.to_string());
        cpus = m.get("resources").and_then(|r| r.get("cpu")).and_then(|v| v.as_str()).map(|s| s.to_string());
    } else {
        eprintln!("  {} Image pulled (no manifest found)", "✓".green());
        if let Some(size) = pull_info.size_bytes {
            eprintln!("  {}: {:.1} MB", "Size".bold(), size as f64 / 1_048_576.0);
        }
    }

    // 5. Write entry to iii.workers.yaml
    let resources = if memory.is_some() || cpus.is_some() {
        Some(WorkerResources { cpus, memory })
    } else {
        None
    };

    let mut workers_file = WorkersFile::load().unwrap_or_default();
    workers_file.add_worker(name.clone(), WorkerDef {
        image: image_ref,
        env: HashMap::new(),
        resources,
    });
    if let Err(e) = workers_file.save() {
        eprintln!("{} Failed to save iii.workers.yaml: {}", "error:".red(), e);
        return 1;
    }

    eprintln!(
        "\n  {} Worker {} added to {}",
        "✓".green(),
        name.bold(),
        "iii.workers.yaml".dimmed(),
    );
    eprintln!("  Start the engine to run it, or edit iii.workers.yaml to customize env/resources.");
    0
}

/// `iii worker remove <name>` — remove from iii.workers.yaml.
pub async fn handle_managed_remove(worker_name: &str, _address: &str, _port: u16) -> i32 {
    let mut workers_file = WorkersFile::load().unwrap_or_default();

    if workers_file.get_worker(worker_name).is_none() {
        eprintln!("{} Worker '{}' not found in iii.workers.yaml", "error:".red(), worker_name);
        return 1;
    }

    workers_file.remove_worker(worker_name);
    if let Err(e) = workers_file.save() {
        eprintln!("{} Failed to save iii.workers.yaml: {}", "error:".red(), e);
        return 1;
    }

    eprintln!(
        "  {} {} removed from {}",
        "✓".green(),
        worker_name.bold(),
        "iii.workers.yaml".dimmed(),
    );
    0
}

/// `iii worker stop <name>` — stop a running worker VM by name.
pub async fn handle_managed_stop(worker_name: &str, _address: &str, _port: u16) -> i32 {
    let adapter = super::worker_manager::create_adapter("libkrun");

    // Find PID from the managed worker directory
    let pid_file = dirs::home_dir()
        .unwrap_or_default()
        .join(".iii/managed")
        .join(worker_name)
        .join("vm.pid");

    match std::fs::read_to_string(&pid_file) {
        Ok(pid_str) => {
            let pid = pid_str.trim();
            eprintln!("  Stopping {}...", worker_name.bold());
            let _ = adapter.stop(pid, 10).await;
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

/// `iii worker start <name>` — start a worker declared in iii.workers.yaml.
pub async fn handle_managed_start(worker_name: &str, _address: &str, port: u16) -> i32 {
    let workers_file = WorkersFile::load().unwrap_or_default();

    let worker_def = match workers_file.get_worker(worker_name) {
        Some(w) => w.clone(),
        None => {
            eprintln!("{} Worker '{}' not found in iii.workers.yaml", "error:".red(), worker_name);
            return 1;
        }
    };

    let adapter = super::worker_manager::create_adapter("libkrun");
    eprintln!("  Starting {}...", worker_name.bold());

    let engine_url = format!("ws://localhost:{}", port);
    let spec = build_container_spec(worker_name, &worker_def, &engine_url);

    // Stop existing if running
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

/// `iii worker list` — show workers from iii.workers.yaml and their runtime status.
pub async fn handle_worker_list() -> i32 {
    let workers_file = WorkersFile::load().unwrap_or_default();

    if workers_file.workers.is_empty() {
        eprintln!("  No workers. Use `iii worker add` or `iii worker dev` to get started.");
        return 0;
    }

    eprintln!();
    eprintln!(
        "  {:25} {:40} {}",
        "NAME".bold(),
        "IMAGE".bold(),
        "STATUS".bold(),
    );
    eprintln!(
        "  {:25} {:40} {}",
        "----".dimmed(),
        "-----".dimmed(),
        "------".dimmed(),
    );

    let adapter = super::worker_manager::create_adapter("libkrun");

    for (name, def) in &workers_file.workers {
        // Check if running by looking for PID file
        let pid_file = dirs::home_dir()
            .unwrap_or_default()
            .join(".iii/managed")
            .join(name)
            .join("vm.pid");

        let status_str = if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
            let pid = pid_str.trim();
            match adapter.status(pid).await {
                Ok(cs) if cs.running => "running".green().to_string(),
                _ => "stopped".red().to_string(),
            }
        } else {
            "not started".dimmed().to_string()
        };

        eprintln!(
            "  {:25} {:40} {}",
            name,
            def.image,
            status_str,
        );
    }
    eprintln!();
    0
}

/// `iii worker logs <name> [--follow]`
pub async fn handle_managed_logs(
    worker_name: &str,
    _follow: bool,
    _address: &str,
    _port: u16,
) -> i32 {
    let log_file = dirs::home_dir()
        .unwrap_or_default()
        .join(".iii/managed")
        .join(worker_name)
        .join("vm.log");

    match std::fs::read_to_string(&log_file) {
        Ok(contents) => {
            if contents.is_empty() {
                eprintln!("  No logs available for {}", worker_name.bold());
            } else {
                // Show last 100 lines
                let lines: Vec<&str> = contents.lines().collect();
                let start = if lines.len() > 100 { lines.len() - 100 } else { 0 };
                for line in &lines[start..] {
                    println!("{}", line);
                }
            }
            0
        }
        Err(_) => {
            eprintln!("{} No logs found for '{}'", "error:".red(), worker_name);
            1
        }
    }
}

/// Build a ContainerSpec from a WorkerDef.
/// Env vars come entirely from iii.workers.yaml — no implicit overrides.
fn build_container_spec(name: &str, def: &WorkerDef, _engine_url: &str) -> ContainerSpec {
    let env = def.env.clone();

    ContainerSpec {
        name: name.to_string(),
        image: def.image.clone(),
        env,
        memory_limit: def.resources.as_ref().and_then(|r| r.memory.clone()),
        cpu_limit: def.resources.as_ref().and_then(|r| r.cpus.clone()),
    }
}

// ---------------------------------------------------------------------------
// Worker dev
// ---------------------------------------------------------------------------

struct ProjectInfo {
    name: String,
    language: Option<String>,
    setup_cmd: String,
    install_cmd: String,
    run_cmd: String,
    env: HashMap<String, String>,
}

/// Infer setup/install/start scripts from runtime language and package manager.
/// Used when v2 manifests omit the `scripts` section.
fn infer_scripts(language: &str, package_manager: &str, entry: &str) -> (String, String, String) {
    match (language, package_manager) {
        ("typescript", "bun") => (
            "curl -fsSL https://bun.sh/install | bash".to_string(),
            "export PATH=$HOME/.bun/bin:$PATH && bun install".to_string(),
            format!("export PATH=$HOME/.bun/bin:$PATH && bun {}", entry),
        ),
        ("typescript", "npm") | ("typescript", "yarn") | ("typescript", "pnpm") => (
            "command -v node >/dev/null || (curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y nodejs)".to_string(),
            "npm install".to_string(),
            format!("npx tsx {}", entry),
        ),
        ("python", _) => (
            "command -v python3 >/dev/null || (apt-get update && apt-get install -y python3-venv python3-pip)".to_string(),
            "python3 -m venv .venv && .venv/bin/pip install -e .".to_string(),
            format!(".venv/bin/python -m {}", entry),
        ),
        ("rust", _) => (
            "command -v cargo >/dev/null || (curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y)".to_string(),
            ". $HOME/.cargo/env && cargo build".to_string(),
            ". $HOME/.cargo/env && cargo run".to_string(),
        ),
        _ => (String::new(), String::new(), entry.to_string()),
    }
}

const WORKER_MANIFEST: &str = "iii.worker.yaml";

/// Load project info from `iii.worker.yaml` if present, otherwise auto-detect.
fn load_project_info(path: &std::path::Path) -> Option<ProjectInfo> {
    let manifest_path = path.join(WORKER_MANIFEST);
    if manifest_path.exists() {
        return load_from_manifest(&manifest_path);
    }
    auto_detect_project(path)
}

fn load_from_manifest(manifest_path: &std::path::Path) -> Option<ProjectInfo> {
    let content = std::fs::read_to_string(manifest_path).ok()?;
    let doc: serde_yaml::Value = serde_yaml::from_str(&content).ok()?;
    let name = doc.get("name")?.as_str()?.to_string();

    // Extract runtime info for auto-detection
    let runtime = doc.get("runtime");
    let language = runtime
        .and_then(|r| r.get("language"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let package_manager = runtime
        .and_then(|r| r.get("package_manager"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let entry = runtime
        .and_then(|r| r.get("entry"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // `scripts.setup/install/start` — falls back to auto-detection from runtime
    let scripts = doc.get("scripts");
    let (setup_cmd, install_cmd, run_cmd) = if scripts.is_some() {
        let setup = scripts
            .and_then(|s| s.get("setup"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let install = scripts
            .and_then(|s| s.get("install"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let start = scripts
            .and_then(|s| s.get("start"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        (setup, install, start)
    } else {
        // Auto-detect from runtime.language + runtime.package_manager
        infer_scripts(language, package_manager, entry)
    };

    // III_URL and III_ENGINE_URL are auto-injected, skip them from manifest
    let mut env = HashMap::new();
    if let Some(env_map) = doc.get("env").and_then(|e| e.as_mapping()) {
        for (k, v) in env_map {
            if let (Some(key), Some(val)) = (k.as_str(), v.as_str()) {
                if key != "III_URL" && key != "III_ENGINE_URL" {
                    env.insert(key.to_string(), val.to_string());
                }
            }
        }
    }

    Some(ProjectInfo {
        name,
        language: Some(language.to_string()),
        setup_cmd,
        install_cmd,
        run_cmd,
        env,
    })
}

/// Auto-detect project type from directory contents.
fn auto_detect_project(path: &std::path::Path) -> Option<ProjectInfo> {
    let info = if path.join("package.json").exists() {
        if path.join("bun.lock").exists() || path.join("bun.lockb").exists() {
            ProjectInfo {
                name: "node (bun)".into(),
                language: Some("typescript".into()),
                setup_cmd: "curl -fsSL https://bun.sh/install | bash".into(),
                install_cmd: "$HOME/.bun/bin/bun install".into(),
                run_cmd: "$HOME/.bun/bin/bun run dev".into(),
                env: HashMap::new(),
            }
        } else {
            ProjectInfo {
                name: "node (npm)".into(),
                language: Some("typescript".into()),
                setup_cmd: "command -v node >/dev/null || (curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y nodejs)".into(),
                install_cmd: "npm install".into(),
                run_cmd: "npm run dev".into(),
                env: HashMap::new(),
            }
        }
    } else if path.join("Cargo.toml").exists() {
        ProjectInfo {
            name: "rust".into(),
            language: Some("rust".into()),
            setup_cmd: "command -v cargo >/dev/null || (curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && . $HOME/.cargo/env)".into(),
            install_cmd: ". $HOME/.cargo/env && cargo build --release".into(),
            run_cmd: ". $HOME/.cargo/env && cargo run --release".into(),
            env: HashMap::new(),
        }
    } else if path.join("pyproject.toml").exists() || path.join("requirements.txt").exists() {
        ProjectInfo {
            name: "python".into(),
            language: Some("python".into()),
            setup_cmd: "command -v python3 >/dev/null || (apt-get update && apt-get install -y python3 python3-pip python3-venv)".into(),
            install_cmd: "python3 -m pip install -e .".into(),
            run_cmd: "python3 -m iii".into(),
            env: HashMap::new(),
        }
    } else {
        return None;
    };
    Some(info)
}

/// Detect the host's LAN IP address for VM networking.
async fn detect_lan_ip() -> Option<String> {
    use tokio::process::Command;
    // macOS: route get default → interface → ifconfig <iface>
    let route = Command::new("route")
        .args(["-n", "get", "default"])
        .output()
        .await
        .ok()?;
    let route_out = String::from_utf8_lossy(&route.stdout);
    let iface = route_out
        .lines()
        .find(|l| l.contains("interface:"))?
        .split(':')
        .nth(1)?
        .trim()
        .to_string();

    let ifconfig = Command::new("ifconfig")
        .arg(&iface)
        .output()
        .await
        .ok()?;
    let ifconfig_out = String::from_utf8_lossy(&ifconfig.stdout);
    let ip = ifconfig_out
        .lines()
        .find(|l| l.contains("inet ") && !l.contains("127.0.0.1"))?
        .split_whitespace()
        .nth(1)?
        .to_string();

    Some(ip)
}

/// Build engine WebSocket URL for the VM runtime.
/// libkrun uses TSI — guest TCP appears as local on host, so localhost works.
fn engine_url_for_runtime(_runtime: &str, _address: &str, port: u16, _lan_ip: &Option<String>) -> String {
    format!("ws://localhost:{}", port)
}

/// `iii worker dev <path>` — run a worker project inside a VM.
pub async fn handle_worker_dev(
    path: &str,
    name: Option<&str>,
    runtime: Option<&str>,
    rebuild: bool,
    address: &str,
    port: u16,
) -> i32 {
    // 1. Resolve absolute path
    let project_path = match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{} Invalid path '{}': {}", "error:".red(), path, e);
            return 1;
        }
    };

    // 2. Detect runtime: explicit flag > libkrun > error
    let selected_runtime = match detect_dev_runtime(runtime).await {
        Some(rt) => rt,
        None => {
            eprintln!(
                "{} No dev runtime available.\n  \
                 Rebuild with: cargo build --features libkrun",
                "error:".red()
            );
            return 1;
        }
    };

    // 3. Load project info (from iii.worker.yaml or auto-detect)
    let project = match load_project_info(&project_path) {
        Some(p) => p,
        None => {
            eprintln!(
                "{} Could not detect project type in '{}'. Add iii.worker.yaml or use package.json/Cargo.toml/pyproject.toml.",
                "error:".red(),
                project_path.display()
            );
            return 1;
        }
    };

    let has_manifest = project_path.join(WORKER_MANIFEST).exists();
    if has_manifest {
        eprintln!("  {} loaded from {}", "✓".green(), WORKER_MANIFEST.bold());
    }

    let dir_name = project_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("worker");
    let sb_name = name.map(|n| n.to_string()).unwrap_or_else(|| format!("iii-dev-{}", dir_name));
    let project_str = project_path.to_string_lossy();

    // 4. Detect LAN IP and compute engine URL
    let lan_ip = detect_lan_ip().await;
    let engine_url = engine_url_for_runtime(&selected_runtime, address, port, &lan_ip);

    eprintln!("  Runtime: {}", selected_runtime.bold());
    eprintln!("  {} project detected: {}", "✓".green(), project.name.bold());
    eprintln!("  Sandbox: {}", sb_name.bold());
    eprintln!("  Path: {}", project_str.dimmed());
    eprintln!("  Engine: {}", engine_url.bold());
    eprintln!();

    // run_dev_worker handles Ctrl+C internally (kills the VM child process).
    let exit_code = run_dev_worker(&selected_runtime, &sb_name, &project_str, &project, &engine_url, rebuild).await;

    exit_code
}

/// Detect which dev runtime to use.
///
/// Priority: explicit --runtime flag > libkrun (if available) > None
async fn detect_dev_runtime(explicit: Option<&str>) -> Option<String> {
    // Explicit flag — use as-is (validation already done by clap)
    if let Some(rt) = explicit {
        return Some(rt.to_string());
    }

    // Auto-detect libkrun
    
    {
        if super::worker_manager::libkrun::libkrun_available() {
            return Some("libkrun".to_string());
        }
    }

    None
}

/// Dispatch to the appropriate dev runtime.
async fn run_dev_worker(
    runtime: &str,
    sb_name: &str,
    project_str: &str,
    project: &ProjectInfo,
    engine_url: &str,
    rebuild: bool,
) -> i32 {
    match runtime {
        
        "libkrun" => {
            let language = project.language.as_deref().unwrap_or("typescript");
            let env = build_dev_env(engine_url, &project.env);

            // 1. Prepare base rootfs (OCI image, shared across projects)
            let base_rootfs = match super::worker_manager::libkrun::prepare_rootfs(language).await {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("{} {}", "error:".red(), e);
                    return 1;
                }
            };

            // 2. Per-project rootfs: clone of base with deps cached across runs.
            //    Setup+install run once; subsequent runs skip straight to start.
            let dev_dir = match dirs::home_dir() {
                Some(h) => h.join(".iii").join("dev").join(sb_name),
                None => {
                    eprintln!("{} Cannot determine home directory", "error:".red());
                    return 1;
                }
            };
            let prepared_marker = dev_dir.join("var").join(".iii-prepared");

            if rebuild && dev_dir.exists() {
                eprintln!("  Rebuilding: clearing cached rootfs...");
                let _ = std::fs::remove_dir_all(&dev_dir);
            }

            if !dev_dir.exists() {
                eprintln!("  Creating project rootfs (first run)...");
                if let Err(e) = clone_rootfs(&base_rootfs, &dev_dir) {
                    eprintln!("{} Failed to create project rootfs: {}", "error:".red(), e);
                    return 1;
                }
            }

            let is_prepared = prepared_marker.exists();
            if is_prepared {
                eprintln!("  Using cached deps (use --rebuild to reinstall)");
            }

            // 3. Build script: skip setup+install if already prepared
            let script = build_libkrun_dev_script(project, engine_url, is_prepared);

            let script_path = dev_dir.join("tmp").join("iii-dev-run.sh");
            if let Err(e) = std::fs::write(&script_path, &script) {
                eprintln!("{} Failed to write dev script: {}", "error:".red(), e);
                return 1;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755));
            }

            // 4. Copy project source into /workspace (always fresh)
            let workspace = dev_dir.join("workspace");
            std::fs::create_dir_all(&workspace).ok();
            if let Err(e) = copy_dir_contents(std::path::Path::new(project_str), &workspace) {
                eprintln!("{} Failed to copy project to rootfs: {}", "error:".red(), e);
                return 1;
            }

            let exec_path = "/bin/sh";
            let args = vec![
                "-c".to_string(),
                "cd /workspace && exec bash /tmp/iii-dev-run.sh".to_string(),
            ];
            let manifest_path = std::path::Path::new(project_str).join(WORKER_MANIFEST);
            let (vcpus, ram) = parse_manifest_resources(&manifest_path);

            super::worker_manager::libkrun::run_dev(
                language,
                project_str,
                exec_path,
                &args,
                env,
                vcpus,
                ram,
                dev_dir,
            ).await
        }
        _ => {
            eprintln!("{} Unknown runtime: {}", "error:".red(), runtime);
            1
        }
    }
}

/// Build environment variables for a dev worker session.

/// Parse resource limits (vcpus, memory) from iii.worker.yaml manifest.
/// Returns (vcpus, ram_mib) with defaults of (1, 512).
fn parse_manifest_resources(manifest_path: &std::path::Path) -> (u32, u32) {
    let default = (1, 512);
    let content = match std::fs::read_to_string(manifest_path) {
        Ok(c) => c,
        Err(_) => return default,
    };
    let yaml: serde_yml::Value = match serde_yml::from_str(&content) {
        Ok(v) => v,
        Err(_) => return default,
    };
    let cpus = yaml.get("resources")
        .and_then(|r| r.get("cpus"))
        .and_then(|v| v.as_u64())
        .unwrap_or(1) as u32;
    let memory = yaml.get("resources")
        .and_then(|r| r.get("memory"))
        .and_then(|v| v.as_u64())
        .unwrap_or(512) as u32;
    (cpus, memory)
}

/// Clone a base rootfs to a project-specific directory.
/// Uses APFS clonefile on macOS (instant, copy-on-write) with cp -a fallback.

fn clone_rootfs(base: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {}", e))?;
    }
    let status = std::process::Command::new("cp")
        .args(if cfg!(target_os = "macos") {
            vec!["-c", "-a"]  // APFS clone (zero-cost copy-on-write)
        } else {
            vec!["--reflink=auto", "-a"]  // btrfs/xfs reflink, fallback to copy
        })
        .arg(base.as_os_str())
        .arg(dest.as_os_str())
        .status()
        .map_err(|e| format!("cp: {}", e))?;
    if !status.success() {
        return Err(format!("cp exited with {}", status));
    }
    Ok(())
}

/// Recursively copy directory contents from src to dst.
/// Skips node_modules, .git, target, and other build artifacts.
fn copy_dir_contents(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    let skip = ["node_modules", ".git", "target", "__pycache__", ".venv", "dist"];
    for entry in std::fs::read_dir(src).map_err(|e| format!("Failed to read {}: {}", src.display(), e))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if skip.iter().any(|s| *s == name_str.as_ref()) {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(&name);
        if src_path.is_dir() {
            std::fs::create_dir_all(&dst_path).map_err(|e| e.to_string())?;
            copy_dir_contents(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Build a dev script for the libkrun runtime.
/// Runs the manifest's setup/install/start scripts inside the VM,
/// with optional caching (skips setup+install if already prepared).

fn build_libkrun_dev_script(project: &ProjectInfo, engine_url: &str, prepared: bool) -> String {
    let env_exports = build_env_exports(engine_url, &project.env);
    let mut parts: Vec<String> = Vec::new();

    // Ensure common runtime paths are available inside the VM.
    parts.push("export PATH=/usr/local/bin:/usr/bin:/bin:$PATH".to_string());

    if !prepared {
        // First run: execute setup + install, then create marker so next run skips them.
        if !project.setup_cmd.is_empty() {
            parts.push(project.setup_cmd.clone());
        }
        if !project.install_cmd.is_empty() {
            parts.push(project.install_cmd.clone());
        }
        parts.push("mkdir -p /var && touch /var/.iii-prepared".to_string());
    }

    parts.push(format!("{} && {}", env_exports, project.run_cmd));
    parts.join("\n")
}


fn build_dev_env(engine_url: &str, project_env: &HashMap<String, String>) -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert("III_ENGINE_URL".to_string(), engine_url.to_string());
    for (key, value) in project_env {
        if key != "III_ENGINE_URL" && key != "III_URL" {
            env.insert(key.clone(), value.clone());
        }
    }
    env
}

/// Build environment variable export string for worker execution.
/// Values are shell-escaped (single quotes with `'` → `'\''`).
/// Keys are validated to contain only `[A-Za-z_][A-Za-z0-9_]*`.
fn build_env_exports(engine_url: &str, env: &HashMap<String, String>) -> String {
    let escaped_url = shell_escape(engine_url);
    let mut exports = format!(
        "export III_ENGINE_URL='{}' && export III_URL='{}'",
        escaped_url, escaped_url
    );
    for (k, v) in env {
        if k == "III_ENGINE_URL" || k == "III_URL" {
            continue;
        }
        // Skip keys with invalid characters to prevent injection
        if !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') || k.is_empty() {
            continue;
        }
        exports.push_str(&format!(" && export {}='{}'", k, shell_escape(v)));
    }
    exports
}

/// Escape a string for safe inclusion inside single quotes in shell.
/// The only character that can break single quotes is `'` itself.
fn shell_escape(s: &str) -> String {
    s.replace('\'', "'\\''")
}

// ---------------------------------------------------------------------------
// Lifecycle: start on boot, stop on shutdown
// ---------------------------------------------------------------------------

/// Stop all managed worker VMs.
/// Called on engine shutdown so VMs don't keep running as orphans.
pub async fn stop_managed_workers() {
    let workers_file = match WorkersFile::load() {
        Ok(f) => f,
        Err(_) => return,
    };

    if workers_file.workers.is_empty() {
        return;
    }

    let adapter = super::worker_manager::create_adapter("libkrun");

    tracing::info!(count = workers_file.workers.len(), "Stopping managed workers...");

    for name in workers_file.workers.keys() {
        let pid_file = dirs::home_dir()
            .unwrap_or_default()
            .join(".iii/managed")
            .join(name)
            .join("vm.pid");
        if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
            let _ = adapter.stop(pid_str.trim(), 5).await;
        }
    }

    tracing::info!("Managed workers stopped");
}

/// Start all workers declared in iii.workers.yaml.
/// Called by the engine on startup.
pub async fn start_managed_workers(engine_url: &str) {
    let workers_file = match WorkersFile::load() {
        Ok(f) => f,
        Err(_) => return, // no iii.workers.yaml — nothing to do
    };

    if workers_file.workers.is_empty() {
        return;
    }

    let adapter = super::worker_manager::create_adapter("libkrun");

    tracing::info!(
        count = workers_file.workers.len(),
        "Starting managed workers from iii.workers.yaml..."
    );

    for (name, def) in &workers_file.workers {
        // Pull image if needed
        if let Err(e) = adapter.pull(&def.image).await {
            tracing::warn!(worker = %name, error = %e, "Failed to pull image, skipping");
            continue;
        }

        let spec = build_container_spec(name, def, engine_url);

        // Stop stale VM if running
        let pid_file = dirs::home_dir()
            .unwrap_or_default()
            .join(".iii/managed")
            .join(name)
            .join("vm.pid");
        if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
            let _ = adapter.stop(pid_str.trim(), 5).await;
            let _ = adapter.remove(pid_str.trim()).await;
        }

        match adapter.start(&spec).await {
            Ok(_) => {
                tracing::info!(worker = %name, "Managed worker started");
            }
            Err(e) => {
                tracing::warn!(worker = %name, error = %e, "Failed to start managed worker");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolve_image_full_ref_passthrough() {
        let (image, name) = resolve_image("ghcr.io/iii-hq/image-resize:0.1.2")
            .await
            .unwrap();
        assert_eq!(image, "ghcr.io/iii-hq/image-resize:0.1.2");
        assert_eq!(name, "image-resize");
    }

    #[tokio::test]
    async fn resolve_image_with_slash_no_tag() {
        let (image, name) = resolve_image("ghcr.io/iii-hq/image-resize")
            .await
            .unwrap();
        assert_eq!(image, "ghcr.io/iii-hq/image-resize");
        assert_eq!(name, "image-resize");
    }

    #[test]
    fn managed_engine_url_uses_localhost() {
        let url = managed_engine_url("0.0.0.0:49134");
        assert_eq!(url, "ws://localhost:49134");
    }

    #[tokio::test]
    async fn resolve_image_shorthand_uses_registry() {
        let dir = tempfile::TempDir::new().unwrap();
        let registry_path = dir.path().join("index.json");
        std::fs::write(
            &registry_path,
            r#"{
                "version": 2,
                "workers": {
                    "image-resize": {
                        "description": "Image resize and format conversion",
                        "image": "ghcr.io/iii-hq/image-resize",
                        "latest": "0.1.2"
                    }
                }
            }"#,
        )
        .unwrap();

        unsafe {
            std::env::set_var(
                "III_REGISTRY_URL",
                format!("file://{}", registry_path.display()),
            );
        }
        let result = resolve_image("image-resize").await;
        unsafe {
            std::env::remove_var("III_REGISTRY_URL");
        }

        let (image, name) = result.unwrap();
        assert_eq!(image, "ghcr.io/iii-hq/image-resize:0.1.2");
        assert_eq!(name, "image-resize");
    }

    #[tokio::test]
    async fn resolve_image_shorthand_not_found() {
        let dir = tempfile::TempDir::new().unwrap();
        let registry_path = dir.path().join("index.json");
        std::fs::write(
            &registry_path,
            r#"{ "version": 2, "workers": {} }"#,
        )
        .unwrap();

        unsafe {
            std::env::set_var(
                "III_REGISTRY_URL",
                format!("file://{}", registry_path.display()),
            );
        }
        let result = resolve_image("nonexistent").await;
        unsafe {
            std::env::remove_var("III_REGISTRY_URL");
        }

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found in registry"));
    }

    #[test]
    fn load_manifest_with_explicit_scripts() {
        let dir = tempfile::TempDir::new().unwrap();
        let manifest = dir.path().join("iii.worker.yaml");
        std::fs::write(
            &manifest,
            "iii: v1\nname: test-worker\nruntime:\n  language: typescript\n  package_manager: bun\n  entry: src/index.ts\nscripts:\n  setup: \"custom setup\"\n  install: \"custom install\"\n  start: \"custom start\"\nenv:\n  CUSTOM: \"val\"\n  III_URL: \"should-be-skipped\"\n",
        ).unwrap();

        let info = load_from_manifest(&manifest).unwrap();
        assert_eq!(info.name, "test-worker");
        assert_eq!(info.setup_cmd, "custom setup");
        assert_eq!(info.install_cmd, "custom install");
        assert_eq!(info.run_cmd, "custom start");
        assert_eq!(info.env.get("CUSTOM").unwrap(), "val");
        assert!(info.env.get("III_URL").is_none());
    }

    #[test]
    fn load_manifest_auto_detects_scripts() {
        let dir = tempfile::TempDir::new().unwrap();
        let manifest = dir.path().join("iii.worker.yaml");
        std::fs::write(
            &manifest,
            "iii: v1\nname: auto-detect-test\nruntime:\n  language: typescript\n  package_manager: bun\n  entry: src/index.ts\n",
        ).unwrap();

        let info = load_from_manifest(&manifest).unwrap();
        assert_eq!(info.name, "auto-detect-test");
        assert!(info.setup_cmd.contains("bun.sh/install"));
        assert!(info.install_cmd.contains("bun install"));
        assert!(info.run_cmd.contains("bun src/index.ts"));
    }

    #[test]
    fn load_manifest_filters_engine_url_env() {
        let dir = tempfile::TempDir::new().unwrap();
        let manifest = dir.path().join("iii.worker.yaml");
        std::fs::write(
            &manifest,
            "iii: v1\nname: env-test\nruntime:\n  language: typescript\n  package_manager: bun\n  entry: src/index.ts\nenv:\n  FOO: \"bar\"\n  III_URL: \"skip\"\n  III_ENGINE_URL: \"skip\"\n",
        ).unwrap();

        let info = load_from_manifest(&manifest).unwrap();
        assert_eq!(info.env.get("FOO").unwrap(), "bar");
        assert!(info.env.get("III_URL").is_none());
        assert!(info.env.get("III_ENGINE_URL").is_none());
    }

    #[test]
    fn infer_scripts_python() {
        let (setup, install, start) = infer_scripts("python", "pip", "image_resize_demo");
        assert!(setup.contains("python3-venv"));
        assert!(install.contains(".venv/bin/pip"));
        assert!(start.contains(".venv/bin/python -m image_resize_demo"));
    }

    #[test]
    fn infer_scripts_rust() {
        let (setup, install, start) = infer_scripts("rust", "cargo", "src/main.rs");
        assert!(setup.contains("rustup"));
        assert!(install.contains("cargo build"));
        assert!(start.contains("cargo run"));
    }

    #[test]
    fn engine_url_for_runtime_uses_localhost() {
        let url = engine_url_for_runtime("libkrun", "0.0.0.0", 49134, &None);
        assert_eq!(url, "ws://localhost:49134");
    }

    #[test]
    fn build_env_exports_includes_engine_url() {
        let env = HashMap::new();
        let exports = build_env_exports("ws://localhost:49134", &env);
        assert!(exports.contains("III_ENGINE_URL='ws://localhost:49134'"));
        assert!(exports.contains("III_URL='ws://localhost:49134'"));
    }

    #[test]
    fn build_env_exports_skips_engine_url_keys() {
        let mut env = HashMap::new();
        env.insert("III_ENGINE_URL".to_string(), "should-be-skipped".to_string());
        env.insert("III_URL".to_string(), "should-be-skipped".to_string());
        env.insert("CUSTOM_VAR".to_string(), "custom-val".to_string());

        let exports = build_env_exports("ws://localhost:49134", &env);
        assert!(!exports.contains("should-be-skipped"));
        assert!(exports.contains("CUSTOM_VAR='custom-val'"));
    }

    #[test]
    fn engine_url_for_runtime_libkrun_uses_localhost() {
        // libkrun uses TSI — guest TCP appears as local on host, so localhost works
        let url = engine_url_for_runtime("libkrun", "0.0.0.0", 49134, &None);
        assert_eq!(url, "ws://localhost:49134");

        // Even with a LAN IP provided, libkrun should still use localhost
        let lan = Some("192.168.1.50".to_string());
        let url = engine_url_for_runtime("libkrun", "10.0.0.1", 8080, &lan);
        assert_eq!(url, "ws://localhost:8080");
    }

}
