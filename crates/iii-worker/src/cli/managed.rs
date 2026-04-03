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
use futures::future::join_all;
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

/// Build engine WebSocket URL from a bind address.
pub(crate) fn managed_engine_url(bind_addr: &str) -> String {
    let (_host, port) = match bind_addr.rsplit_once(':') {
        Some((h, p)) => (h, p),
        None => (bind_addr, "49134"),
    };
    format!("ws://localhost:{}", port)
}

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

pub async fn handle_managed_add(
    image_or_name: &str,
    _runtime: &str,
    _address: &str,
    _port: u16,
) -> i32 {
    let adapter = super::worker_manager::create_adapter("libkrun");

    eprintln!("  Resolving {}...", image_or_name.bold());
    let (image_ref, name) = match resolve_image(image_or_name).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };
    eprintln!("  {} Resolved to {}", "✓".green(), image_ref.dimmed());

    eprintln!("  Pulling {}...", image_ref.bold());
    let pull_info = match adapter.pull(&image_ref).await {
        Ok(info) => info,
        Err(e) => {
            eprintln!("{} Pull failed: {}", "error:".red(), e);
            return 1;
        }
    };

    let manifest: Option<serde_json::Value> =
        match adapter.extract_file(&image_ref, MANIFEST_PATH).await {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(yaml_str) => serde_yaml::from_str(&yaml_str).ok(),
                Err(_) => None,
            },
            Err(_) => None,
        };

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
        memory = m
            .get("resources")
            .and_then(|r| r.get("memory"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        cpus = m
            .get("resources")
            .and_then(|r| r.get("cpu"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    } else {
        eprintln!("  {} Image pulled (no manifest found)", "✓".green());
        if let Some(size) = pull_info.size_bytes {
            eprintln!("  {}: {:.1} MB", "Size".bold(), size as f64 / 1_048_576.0);
        }
    }

    let resources = if memory.is_some() || cpus.is_some() {
        Some(WorkerResources { cpus, memory })
    } else {
        None
    };

    let mut workers_file = WorkersFile::load().unwrap_or_default();
    workers_file.add_worker(
        name.clone(),
        WorkerDef {
            image: image_ref,
            env: HashMap::new(),
            resources,
        },
    );
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

pub async fn handle_managed_remove(worker_name: &str, _address: &str, _port: u16) -> i32 {
    let mut workers_file = WorkersFile::load().unwrap_or_default();

    if workers_file.get_worker(worker_name).is_none() {
        eprintln!(
            "{} Worker '{}' not found in iii.workers.yaml",
            "error:".red(),
            worker_name
        );
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

pub async fn handle_managed_stop(worker_name: &str, _address: &str, _port: u16) -> i32 {
    let adapter = super::worker_manager::create_adapter("libkrun");

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

pub async fn handle_managed_start(worker_name: &str, _address: &str, port: u16) -> i32 {
    let workers_file = WorkersFile::load().unwrap_or_default();

    let worker_def = match workers_file.get_worker(worker_name) {
        Some(w) => w.clone(),
        None => {
            eprintln!(
                "{} Worker '{}' not found in iii.workers.yaml",
                "error:".red(),
                worker_name
            );
            return 1;
        }
    };

    // Ensure libkrunfw (firmware) is available.
    // msb_krun (the VMM) is compiled directly into the iii-worker binary.
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
    eprintln!("  Starting {}...", worker_name.bold());

    let engine_url = format!("ws://localhost:{}", port);
    let spec = build_container_spec(worker_name, &worker_def, &engine_url);

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

async fn worker_status_label(
    pid_file: &std::path::Path,
    adapter: &dyn super::worker_manager::adapter::RuntimeAdapter,
) -> (&'static str, &'static str) {
    if let Ok(pid_str) = std::fs::read_to_string(pid_file) {
        let pid = pid_str.trim();
        match adapter.status(pid).await {
            Ok(cs) if cs.running => ("running", "green"),
            _ => ("crashed", "yellow"),
        }
    } else {
        ("stopped", "dimmed")
    }
}

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
        let pid_file = dirs::home_dir()
            .unwrap_or_default()
            .join(".iii/managed")
            .join(name)
            .join("vm.pid");

        let (label, color) = worker_status_label(&pid_file, adapter.as_ref()).await;
        let status_str = match color {
            "green" => label.green().to_string(),
            "yellow" => label.yellow().to_string(),
            "dimmed" => label.dimmed().to_string(),
            _ => label.to_string(),
        };

        eprintln!("  {:25} {:40} {}", name, def.image, status_str,);
    }
    eprintln!();
    0
}

pub async fn handle_managed_logs(
    worker_name: &str,
    _follow: bool,
    _address: &str,
    _port: u16,
) -> i32 {
    let worker_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".iii/managed")
        .join(worker_name);

    let logs_dir = worker_dir.join("logs");
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
            0
        }
        Err(_) => {
            eprintln!("{} No logs found for '{}'", "error:".red(), worker_name);
            1
        }
    }
}

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
        infer_scripts(language, package_manager, entry)
    };

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

async fn detect_lan_ip() -> Option<String> {
    use tokio::process::Command;
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

    let ifconfig = Command::new("ifconfig").arg(&iface).output().await.ok()?;
    let ifconfig_out = String::from_utf8_lossy(&ifconfig.stdout);
    let ip = ifconfig_out
        .lines()
        .find(|l| l.contains("inet ") && !l.contains("127.0.0.1"))?
        .split_whitespace()
        .nth(1)?
        .to_string();

    Some(ip)
}

fn engine_url_for_runtime(
    _runtime: &str,
    _address: &str,
    port: u16,
    _lan_ip: &Option<String>,
) -> String {
    format!("ws://localhost:{}", port)
}

/// Ensure the terminal is in cooked mode with proper NL→CRNL translation.
#[cfg(unix)]
pub fn restore_terminal_cooked_mode() {
    let stderr = std::io::stderr();
    if let Ok(mut termios) = nix::sys::termios::tcgetattr(&stderr) {
        termios
            .output_flags
            .insert(nix::sys::termios::OutputFlags::OPOST);
        termios
            .output_flags
            .insert(nix::sys::termios::OutputFlags::ONLCR);
        let _ = nix::sys::termios::tcsetattr(
            &stderr,
            nix::sys::termios::SetArg::TCSANOW,
            &termios,
        );
    }
}

pub async fn handle_worker_dev(
    path: &str,
    name: Option<&str>,
    runtime: Option<&str>,
    rebuild: bool,
    address: &str,
    port: u16,
) -> i32 {
    #[cfg(unix)]
    restore_terminal_cooked_mode();

    let project_path = match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{} Invalid path '{}': {}", "error:".red(), path, e);
            return 1;
        }
    };

    // Ensure libkrunfw (firmware) is available.
    // msb_krun (the VMM) is compiled directly into the iii-worker binary.
    if let Err(e) = super::firmware::download::ensure_libkrunfw().await {
        tracing::warn!(error = %e, "failed to ensure libkrunfw");
    }

    let selected_runtime = match detect_dev_runtime(runtime).await {
        Some(rt) => rt,
        None => {
            eprintln!(
                "{} No dev runtime available.\n  \
                 Rebuild with --features embed-libkrunfw or place libkrunfw in ~/.iii/lib/",
                "error:".red()
            );
            return 1;
        }
    };

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
    let sb_name = name
        .map(|n| n.to_string())
        .unwrap_or_else(|| format!("iii-dev-{}", dir_name));
    let project_str = project_path.to_string_lossy();

    let lan_ip = detect_lan_ip().await;
    let engine_url = engine_url_for_runtime(&selected_runtime, address, port, &lan_ip);

    eprintln!("  Runtime: {}", selected_runtime.bold());
    eprintln!(
        "  {} project detected: {}",
        "✓".green(),
        project.name.bold()
    );
    eprintln!("  Sandbox: {}", sb_name.bold());
    eprintln!("  Path: {}", project_str.dimmed());
    eprintln!("  Engine: {}", engine_url.bold());
    eprintln!();

    let exit_code = run_dev_worker(
        &selected_runtime,
        &sb_name,
        &project_str,
        &project,
        &engine_url,
        rebuild,
    )
    .await;

    exit_code
}

async fn detect_dev_runtime(explicit: Option<&str>) -> Option<String> {
    if let Some(rt) = explicit {
        return Some(rt.to_string());
    }

    {
        if super::worker_manager::libkrun::libkrun_available() {
            return Some("libkrun".to_string());
        }
    }

    None
}

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

            let base_rootfs = match super::worker_manager::libkrun::prepare_rootfs(language).await {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("{} {}", "error:".red(), e);
                    return 1;
                }
            };

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

            let script = build_libkrun_dev_script(project, is_prepared);

            let script_path = dev_dir.join("tmp").join("iii-dev-run.sh");
            if let Err(e) = std::fs::write(&script_path, &script) {
                eprintln!("{} Failed to write dev script: {}", "error:".red(), e);
                return 1;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ =
                    std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755));
            }

            let workspace = dev_dir.join("workspace");
            std::fs::create_dir_all(&workspace).ok();
            if let Err(e) = copy_dir_contents(std::path::Path::new(project_str), &workspace) {
                eprintln!("{} Failed to copy project to rootfs: {}", "error:".red(), e);
                return 1;
            }

            // Ensure iii-init is available. When not embedded, download and copy to rootfs.
            let init_path = match super::firmware::download::ensure_init_binary().await {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("{} Failed to provision iii-init: {}", "error:".red(), e);
                    return 1;
                }
            };

            // If init is not embedded, copy the binary into the rootfs as /init.krun
            // so PassthroughFs serves it from disk instead of from INIT_BYTES.
            if !iii_filesystem::init::has_init() {
                let dest = dev_dir.join("init.krun");
                if let Err(e) = std::fs::copy(&init_path, &dest) {
                    eprintln!(
                        "{} Failed to copy iii-init to rootfs: {}",
                        "error:".red(),
                        e
                    );
                    return 1;
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(
                        &dest,
                        std::fs::Permissions::from_mode(0o755),
                    );
                }
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
            )
            .await
        }
        _ => {
            eprintln!("{} Unknown runtime: {}", "error:".red(), runtime);
            1
        }
    }
}

fn parse_manifest_resources(manifest_path: &std::path::Path) -> (u32, u32) {
    let default = (2, 2048);
    let content = match std::fs::read_to_string(manifest_path) {
        Ok(c) => c,
        Err(_) => return default,
    };
    let yaml: serde_yml::Value = match serde_yml::from_str(&content) {
        Ok(v) => v,
        Err(_) => return default,
    };
    let cpus = yaml
        .get("resources")
        .and_then(|r| r.get("cpus"))
        .and_then(|v| v.as_u64())
        .unwrap_or(2) as u32;
    let memory = yaml
        .get("resources")
        .and_then(|r| r.get("memory"))
        .and_then(|v| v.as_u64())
        .unwrap_or(2048) as u32;
    (cpus, memory)
}

fn clone_rootfs(base: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {}", e))?;
    }
    let status = std::process::Command::new("cp")
        .args(if cfg!(target_os = "macos") {
            vec!["-c", "-a"]
        } else {
            vec!["--reflink=auto", "-a"]
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

fn copy_dir_contents(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    let skip = [
        "node_modules",
        ".git",
        "target",
        "__pycache__",
        ".venv",
        "dist",
    ];
    for entry in
        std::fs::read_dir(src).map_err(|e| format!("Failed to read {}: {}", src.display(), e))?
    {
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

fn build_libkrun_dev_script(project: &ProjectInfo, prepared: bool) -> String {
    let env_exports = build_env_exports(&project.env);
    let mut parts: Vec<String> = Vec::new();

    parts.push("export PATH=/usr/local/bin:/usr/bin:/bin:$PATH".to_string());
    parts.push("echo $$ > /sys/fs/cgroup/worker/cgroup.procs 2>/dev/null || true".to_string());

    if !prepared {
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

fn build_dev_env(
    engine_url: &str,
    project_env: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert("III_ENGINE_URL".to_string(), engine_url.to_string());
    env.insert("III_URL".to_string(), engine_url.to_string());
    for (key, value) in project_env {
        if key != "III_ENGINE_URL" && key != "III_URL" {
            env.insert(key.clone(), value.clone());
        }
    }
    env
}

fn build_env_exports(env: &HashMap<String, String>) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (k, v) in env {
        if k == "III_ENGINE_URL" || k == "III_URL" {
            continue;
        }
        if !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') || k.is_empty() {
            continue;
        }
        parts.push(format!("export {}='{}'", k, shell_escape(v)));
    }
    if parts.is_empty() {
        "true".to_string()
    } else {
        parts.join(" && ")
    }
}

fn shell_escape(s: &str) -> String {
    s.replace('\'', "'\\''")
}

// ---------------------------------------------------------------------------
// Lifecycle: start on boot, stop on shutdown
// ---------------------------------------------------------------------------

/// Stop all managed worker VMs.
pub async fn stop_managed_workers() {
    let workers_file = match WorkersFile::load() {
        Ok(f) => f,
        Err(_) => return,
    };

    if workers_file.workers.is_empty() {
        return;
    }

    tracing::info!(
        count = workers_file.workers.len(),
        "Stopping managed workers..."
    );

    let stop_futures: Vec<_> = workers_file
        .workers
        .keys()
        .filter_map(|name| {
            let pid_file = dirs::home_dir()
                .unwrap_or_default()
                .join(".iii/managed")
                .join(name)
                .join("vm.pid");
            let pid_str = std::fs::read_to_string(&pid_file).ok()?;
            let pid = pid_str.trim().to_string();
            let worker_name = name.clone();
            Some(async move {
                let adapter = super::worker_manager::create_adapter("libkrun");
                if let Err(e) = adapter.stop(&pid, 5).await {
                    tracing::warn!(worker = %worker_name, error = %e, "Failed to stop worker");
                }
            })
        })
        .collect();

    join_all(stop_futures).await;

    tracing::info!("Managed workers stopped");
}

/// Start all workers declared in iii.workers.yaml.
pub async fn start_managed_workers(engine_url: &str) {
    let workers_file = match WorkersFile::load() {
        Ok(f) => f,
        Err(_) => return,
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
        if let Err(e) = adapter.pull(&def.image).await {
            tracing::warn!(worker = %name, error = %e, "Failed to pull image, skipping");
            continue;
        }

        let spec = build_container_spec(name, def, engine_url);

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

    #[test]
    fn managed_engine_url_uses_localhost() {
        let url = managed_engine_url("0.0.0.0:49134");
        assert_eq!(url, "ws://localhost:49134");
    }

    #[test]
    fn build_env_exports_excludes_engine_urls() {
        let mut env = HashMap::new();
        env.insert("III_ENGINE_URL".to_string(), "ws://localhost:49134".to_string());
        env.insert("III_URL".to_string(), "ws://localhost:49134".to_string());
        env.insert("CUSTOM_VAR".to_string(), "custom-val".to_string());

        let exports = build_env_exports(&env);
        assert!(!exports.contains("III_ENGINE_URL"));
        assert!(!exports.contains("III_URL"));
        assert!(exports.contains("CUSTOM_VAR='custom-val'"));
    }

    #[test]
    fn build_env_exports_empty_env() {
        let env = HashMap::new();
        let exports = build_env_exports(&env);
        assert_eq!(exports, "true");
    }

    #[test]
    fn engine_url_for_runtime_libkrun_uses_localhost() {
        let url = engine_url_for_runtime("libkrun", "0.0.0.0", 49134, &None);
        assert_eq!(url, "ws://localhost:49134");
    }
}
