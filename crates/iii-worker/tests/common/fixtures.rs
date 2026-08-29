// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Typed fixture builders for config YAML test data.
//!
//! Generates YAML in the exact format expected by `config_file.rs` line-based
//! parser, ensuring roundtrip compatibility.

use std::path::{Path, PathBuf};

/// Describes a single worker entry in config.yaml.
#[derive(Clone, Debug)]
struct WorkerEntry {
    name: String,
    image: Option<String>,
}

/// Typed builder for `config.yaml` test fixtures.
///
/// Produces YAML that roundtrips through `config_file.rs` public API
/// (`worker_exists`, `get_worker_image`, `list_worker_names`).
pub struct TestConfigBuilder {
    workers: Vec<WorkerEntry>,
}

impl TestConfigBuilder {
    /// Create a new builder with no workers.
    pub fn new() -> Self {
        Self {
            workers: Vec::new(),
        }
    }

    /// Add a config-only worker. Local sources belong in worker-compose.yaml.
    pub fn with_worker(mut self, name: &str) -> Self {
        self.workers.push(WorkerEntry {
            name: name.to_string(),
            image: None,
        });
        self
    }

    /// Add an OCI worker with the given image reference.
    pub fn with_oci_worker(mut self, name: &str, image: &str) -> Self {
        self.workers.push(WorkerEntry {
            name: name.to_string(),
            image: Some(image.to_string()),
        });
        self
    }

    /// Write `config.yaml` to `dir` and return its path.
    ///
    /// The generated YAML matches the exact format produced by
    /// `config_file.rs`, ensuring full roundtrip compatibility with the
    /// line-based parser.
    pub fn build(&self, dir: &Path) -> PathBuf {
        let mut yaml = String::from("workers:\n");

        for entry in &self.workers {
            yaml.push_str(&format!("  - name: {}\n", entry.name));
            if let Some(ref img) = entry.image {
                yaml.push_str(&format!("    image: {}\n", img));
            }
        }

        let path = dir.join("config.yaml");
        std::fs::write(&path, &yaml).unwrap();
        path
    }
}
