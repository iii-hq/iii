// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::core::error::WorkerOpError;

const LOCKFILE_NAME: &str = ".iii-worker.lock";
const CONFIG_NAME: &str = "iii.config.yaml";

/// Checked in priority order: canonical first, then legacy `config.yaml`.
const CONFIG_CANDIDATES: &[&str] = &["iii.config.yaml", "config.yaml"];

/// RAII guard for the project-wide install/lifecycle mutex.
#[derive(Debug)]
pub struct ProjectOperationLock {
    path: PathBuf,
    /// Exact bytes this guard wrote into the lockfile. `Drop` only unlinks
    /// when the file's current contents still match — otherwise another
    /// owner (recreated after a stale guard was reaped) keeps its lock.
    marker: String,
}

impl ProjectOperationLock {
    pub fn acquire(root: &Path) -> Result<Self, WorkerOpError> {
        let path = root.join(LOCKFILE_NAME);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                let marker = format!("pid={}\n", std::process::id());
                file.write_all(marker.as_bytes())
                    .map_err(|source| WorkerOpError::LockIo {
                        path: path.clone(),
                        source,
                    })?;
                Ok(Self { path, marker })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let holder_pid = fs::read_to_string(&path).ok().and_then(|s| {
                    s.lines()
                        .find_map(|l| l.strip_prefix("pid="))
                        .and_then(|p| p.trim().parse::<u32>().ok())
                });
                Err(WorkerOpError::LockBusy { holder_pid })
            }
            Err(e) => Err(WorkerOpError::LockIo { path, source: e }),
        }
    }
}

impl Drop for ProjectOperationLock {
    fn drop(&mut self) {
        // Compare current contents to our marker so we never unlink a
        // lockfile belonging to a different acquirer (e.g. one that
        // recreated the file after we became a stale guard).
        if fs::read_to_string(&self.path).ok().as_deref() == Some(self.marker.as_str()) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Explicit project root + (optional) lock guard.
#[derive(Debug)]
pub struct ProjectCtx {
    pub root: PathBuf,
    pub lock: Option<ProjectOperationLock>,
}

impl ProjectCtx {
    /// Acquire the project-wide lock. Use for write ops.
    pub fn open(root: PathBuf) -> Result<Self, WorkerOpError> {
        let lock = ProjectOperationLock::acquire(&root)?;
        Ok(Self {
            root,
            lock: Some(lock),
        })
    }

    /// No lock. Read-only callers and the daemon's idle state, which
    /// acquires on demand inside each op.
    pub fn open_unlocked(root: PathBuf) -> Self {
        Self { root, lock: None }
    }

    /// First existing candidate (`iii.config.yaml` then `config.yaml`);
    /// falls back to the canonical name when neither exists yet.
    pub fn config_path(&self) -> PathBuf {
        for name in CONFIG_CANDIDATES {
            let candidate = self.root.join(name);
            if candidate.exists() {
                return candidate;
            }
        }
        self.root.join(CONFIG_NAME)
    }

    pub fn worker_dir(&self, worker: &str) -> PathBuf {
        self.root.join("iii_workers").join(worker)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn open_acquires_lockfile_at_project_root() {
        let dir = TempDir::new().unwrap();
        let ctx = ProjectCtx::open(dir.path().to_path_buf()).unwrap();
        assert!(dir.path().join(".iii-worker.lock").exists());
        drop(ctx);
        assert!(!dir.path().join(".iii-worker.lock").exists());
    }

    #[test]
    fn second_open_in_same_project_returns_lock_busy() {
        let dir = TempDir::new().unwrap();
        let _first = ProjectCtx::open(dir.path().to_path_buf()).unwrap();
        let err = ProjectCtx::open(dir.path().to_path_buf()).unwrap_err();
        assert!(matches!(err, crate::core::WorkerOpError::LockBusy { .. }));
    }

    #[test]
    fn open_unlocked_does_not_create_lockfile() {
        let dir = TempDir::new().unwrap();
        let _ctx = ProjectCtx::open_unlocked(dir.path().to_path_buf());
        assert!(!dir.path().join(".iii-worker.lock").exists());
    }

    #[test]
    fn config_path_falls_back_to_canonical_when_no_file_exists() {
        let dir = TempDir::new().unwrap();
        let ctx = ProjectCtx::open_unlocked(dir.path().to_path_buf());
        assert_eq!(ctx.config_path(), dir.path().join("iii.config.yaml"));
    }

    #[test]
    fn config_path_returns_iii_config_yaml_when_canonical_exists() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("iii.config.yaml"), "workers: []\n").unwrap();
        let ctx = ProjectCtx::open_unlocked(dir.path().to_path_buf());
        assert_eq!(ctx.config_path(), dir.path().join("iii.config.yaml"));
    }

    #[test]
    fn config_path_returns_config_yaml_when_only_legacy_exists() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("config.yaml"), "workers: []\n").unwrap();
        let ctx = ProjectCtx::open_unlocked(dir.path().to_path_buf());
        assert_eq!(ctx.config_path(), dir.path().join("config.yaml"));
    }

    #[test]
    fn config_path_prefers_canonical_when_both_exist() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("iii.config.yaml"), "workers: []\n").unwrap();
        std::fs::write(dir.path().join("config.yaml"), "workers: []\n").unwrap();
        let ctx = ProjectCtx::open_unlocked(dir.path().to_path_buf());
        assert_eq!(ctx.config_path(), dir.path().join("iii.config.yaml"));
    }

    #[test]
    fn worker_dir_joins_root_with_iii_workers_subdir() {
        let dir = TempDir::new().unwrap();
        let ctx = ProjectCtx::open_unlocked(dir.path().to_path_buf());
        assert_eq!(
            ctx.worker_dir("pdfkit"),
            dir.path().join("iii_workers").join("pdfkit"),
        );
    }
}
