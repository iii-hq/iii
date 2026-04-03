use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

const WORKERS_FILE: &str = "iii.workers.yaml";

/// A worker declaration in iii.workers.yaml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerDef {
    pub image: String,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub resources: Option<WorkerResources>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerResources {
    pub cpus: Option<String>,
    pub memory: Option<String>,
}

/// The iii.workers.yaml file — declares all managed workers.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkersFile {
    #[serde(default)]
    pub workers: HashMap<String, WorkerDef>,
}

impl WorkersFile {
    /// Load from iii.workers.yaml in the current directory.
    pub fn load() -> Result<Self> {
        let path = Path::new(WORKERS_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", WORKERS_FILE))?;
        let file: WorkersFile = serde_yaml::from_str(&data)
            .with_context(|| format!("failed to parse {}", WORKERS_FILE))?;
        Ok(file)
    }

    /// Save to iii.workers.yaml.
    pub fn save(&self) -> Result<()> {
        let data =
            serde_yaml::to_string(self).with_context(|| "failed to serialize workers file")?;
        std::fs::write(WORKERS_FILE, data)
            .with_context(|| format!("failed to write {}", WORKERS_FILE))?;
        Ok(())
    }

    pub fn add_worker(&mut self, name: String, def: WorkerDef) {
        self.workers.insert(name, def);
    }

    pub fn remove_worker(&mut self, name: &str) -> Option<WorkerDef> {
        self.workers.remove(name)
    }

    pub fn get_worker(&self, name: &str) -> Option<&WorkerDef> {
        self.workers.get(name)
    }
}
