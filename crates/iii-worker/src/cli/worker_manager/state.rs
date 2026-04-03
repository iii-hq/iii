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

#[cfg(test)]
mod tests {
    use super::*;

    // --- 5.1: WorkersFile default empty ---
    #[test]
    fn test_workers_file_default_empty() {
        let wf = WorkersFile::default();
        assert!(wf.workers.is_empty());
    }

    // --- 5.2: WorkersFile add and get ---
    #[test]
    fn test_workers_file_add_and_get() {
        let mut wf = WorkersFile::default();
        let mut env = HashMap::new();
        env.insert("FOO".to_string(), "bar".to_string());
        wf.add_worker(
            "test-worker".to_string(),
            WorkerDef {
                image: "ghcr.io/iii-hq/test:latest".to_string(),
                env: env.clone(),
                resources: None,
            },
        );
        let worker = wf.get_worker("test-worker").unwrap();
        assert_eq!(worker.image, "ghcr.io/iii-hq/test:latest");
        assert_eq!(worker.env.get("FOO").unwrap(), "bar");
    }

    // --- 5.3: WorkersFile remove ---
    #[test]
    fn test_workers_file_remove() {
        let mut wf = WorkersFile::default();
        wf.add_worker(
            "test-worker".to_string(),
            WorkerDef {
                image: "ghcr.io/iii-hq/test:latest".to_string(),
                env: HashMap::new(),
                resources: None,
            },
        );
        wf.remove_worker("test-worker");
        assert!(wf.get_worker("test-worker").is_none());
    }

    // --- 5.4: WorkersFile remove returns old def ---
    #[test]
    fn test_workers_file_remove_returns_old_def() {
        let mut wf = WorkersFile::default();
        wf.add_worker(
            "test-worker".to_string(),
            WorkerDef {
                image: "ghcr.io/iii-hq/test:latest".to_string(),
                env: HashMap::new(),
                resources: None,
            },
        );
        let removed = wf.remove_worker("test-worker");
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().image, "ghcr.io/iii-hq/test:latest");
    }

    // --- 5.5: WorkersFile YAML roundtrip ---
    #[test]
    fn test_workers_file_yaml_roundtrip() {
        let mut wf = WorkersFile::default();
        let mut env = HashMap::new();
        env.insert("MY_VAR".to_string(), "my_value".to_string());
        wf.add_worker(
            "roundtrip-worker".to_string(),
            WorkerDef {
                image: "ghcr.io/iii-hq/roundtrip:1.0".to_string(),
                env,
                resources: Some(WorkerResources {
                    cpus: Some("4".to_string()),
                    memory: Some("2048Mi".to_string()),
                }),
            },
        );

        let yaml = serde_yaml::to_string(&wf).unwrap();
        let deserialized: WorkersFile = serde_yaml::from_str(&yaml).unwrap();

        let worker = deserialized.get_worker("roundtrip-worker").unwrap();
        assert_eq!(worker.image, "ghcr.io/iii-hq/roundtrip:1.0");
        assert_eq!(worker.env.get("MY_VAR").unwrap(), "my_value");
        let resources = worker.resources.as_ref().unwrap();
        assert_eq!(resources.cpus.as_deref(), Some("4"));
        assert_eq!(resources.memory.as_deref(), Some("2048Mi"));
    }
}
