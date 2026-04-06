// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! CLI command handlers for managing OCI-based workers.

use colored::Colorize;
use std::collections::HashMap;

use super::lifecycle::{build_container_spec, worker_status_label};
use super::registry::{MANIFEST_PATH, resolve_image};
use super::worker_manager::state::{WorkerDef, WorkerResources, WorkersFile};

pub use super::dev::handle_worker_dev;
pub use super::lifecycle::{start_managed_workers, stop_managed_workers};

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
