// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Worker lifecycle: container spec building.

use super::worker_manager::adapter::ContainerSpec;
use super::worker_manager::state::WorkerDef;

pub fn build_container_spec(name: &str, def: &WorkerDef, _engine_url: &str) -> ContainerSpec {
    match def {
        WorkerDef::Managed {
            image,
            env,
            resources,
        } => ContainerSpec {
            name: name.to_string(),
            image: image.clone(),
            env: env.clone(),
            memory_limit: resources.as_ref().and_then(|r| r.memory.clone()),
            cpu_limit: resources.as_ref().and_then(|r| r.cpus.clone()),
        },
        WorkerDef::Binary { .. } => ContainerSpec {
            name: name.to_string(),
            image: String::new(),
            env: std::collections::HashMap::new(),
            memory_limit: None,
            cpu_limit: None,
        },
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

#[cfg(test)]
mod tests {
    use super::super::worker_manager::state::WorkerResources;
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn build_container_spec_maps_fields() {
        let def = WorkerDef::Managed {
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

    #[test]
    fn build_container_spec_binary_variant() {
        let def = WorkerDef::Binary {
            version: "0.1.2".to_string(),
            config: None,
        };

        let spec = build_container_spec("bin-worker", &def, "ws://localhost:49134");

        assert_eq!(spec.name, "bin-worker");
        assert!(
            spec.image.is_empty(),
            "Binary variant should have empty image"
        );
        assert!(spec.env.is_empty(), "Binary variant should have empty env");
        assert!(spec.memory_limit.is_none());
        assert!(spec.cpu_limit.is_none());
    }
}
