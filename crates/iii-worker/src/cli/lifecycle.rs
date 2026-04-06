// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Worker lifecycle: start/stop all managed workers, container spec building.

use futures::future::join_all;

use super::worker_manager::adapter::ContainerSpec;
use super::worker_manager::state::{WorkerDef, WorkersFile};

pub fn build_container_spec(name: &str, def: &WorkerDef, _engine_url: &str) -> ContainerSpec {
    let env = def.env.clone();

    ContainerSpec {
        name: name.to_string(),
        image: def.image.clone(),
        env,
        memory_limit: def.resources.as_ref().and_then(|r| r.memory.clone()),
        cpu_limit: def.resources.as_ref().and_then(|r| r.cpus.clone()),
    }
}

pub async fn worker_status_label(
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

    tracing::info!("Starting workers from iii.workers.yaml...");

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
                tracing::debug!(worker = %name, "managed worker process exited successfully");
            }
            Err(e) => {
                tracing::warn!(worker = %name, error = %e, "Failed to start managed worker");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::worker_manager::state::WorkerResources;
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn build_container_spec_maps_fields() {
        let def = WorkerDef {
            image: "ghcr.io/iii-hq/image-resize:0.1.2".to_string(),
            env: {
                let mut m = HashMap::new();
                m.insert("FOO".to_string(), "bar".to_string());
                m
            },
            resources: Some(WorkerResources {
                cpus: Some("4".to_string()),
                memory: Some("4096Mi".to_string()),
            }),
        };

        let spec = build_container_spec("my-worker", &def, "ws://localhost:49134");

        assert_eq!(spec.name, "my-worker");
        assert_eq!(spec.image, "ghcr.io/iii-hq/image-resize:0.1.2");
        assert_eq!(spec.env.get("FOO").unwrap(), "bar");
        assert_eq!(spec.cpu_limit.as_deref(), Some("4"));
        assert_eq!(spec.memory_limit.as_deref(), Some("4096Mi"));
    }

    #[test]
    fn worker_status_label_no_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("vm.pid");

        let rt = tokio::runtime::Runtime::new().unwrap();
        let adapter = super::super::worker_manager::create_adapter("libkrun");
        let (label, color) = rt.block_on(worker_status_label(&pid_file, adapter.as_ref()));

        assert_eq!(label, "stopped");
        assert_eq!(color, "dimmed");
    }
}
