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
///
/// Backed by a kernel advisory lock (`flock(2)`), so the lock dies with the
/// process: a SIGKILLed or crashed holder can never strand the project the
/// way the previous pidfile scheme did (a dead pid in `.iii-worker.lock`
/// returned W120 forever until the file was removed by hand). The lockfile
/// itself persists across acquisitions — unlinking a flocked path would let
/// a new acquirer lock a fresh inode while a stale holder still owns the
/// old one — and only carries the holder pid as W120 diagnostics.
#[derive(Debug)]
pub struct ProjectOperationLock {
    _lock: nix::fcntl::Flock<fs::File>,
}

impl ProjectOperationLock {
    pub fn acquire(root: &Path) -> Result<Self, WorkerOpError> {
        let path = root.join(LOCKFILE_NAME);
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|source| WorkerOpError::LockIo {
                path: path.clone(),
                source,
            })?;
        match nix::fcntl::Flock::lock(file, nix::fcntl::FlockArg::LockExclusiveNonblock) {
            Ok(lock) => {
                let marker = format!("pid={}\n", std::process::id());
                (&*lock)
                    .set_len(0)
                    .and_then(|_| (&*lock).write_all(marker.as_bytes()))
                    .map_err(|source| WorkerOpError::LockIo { path, source })?;
                Ok(Self { _lock: lock })
            }
            Err((_, nix::errno::Errno::EWOULDBLOCK)) => {
                let holder_pid = fs::read_to_string(&path).ok().and_then(|s| {
                    s.lines()
                        .find_map(|l| l.strip_prefix("pid="))
                        .and_then(|p| p.trim().parse::<u32>().ok())
                });
                // flock ownership is per open-file-description, so a second
                // acquire in the SAME process (a concurrent op in this daemon)
                // also lands here — flag it so the error can tell the caller
                // the holder is the daemon itself, not a stale process.
                let holder_is_self = holder_pid == Some(std::process::id());
                Err(WorkerOpError::LockBusy {
                    holder_pid,
                    holder_is_self,
                })
            }
            Err((_, errno)) => Err(WorkerOpError::LockIo {
                path,
                source: errno.into(),
            }),
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
        // The file persists (flock semantics) but the lock must be released:
        // a fresh acquisition succeeds immediately.
        ProjectCtx::open(dir.path().to_path_buf())
            .expect("lock must be released when the guard drops");
    }

    #[test]
    fn stale_lockfile_from_dead_process_does_not_block() {
        // Regression: a holder that died without cleanup (SIGKILL, crash)
        // used to strand the project with W120 forever — the pidfile scheme
        // never checked holder liveness. With flock, an unheld lockfile is
        // just a file: acquisition must succeed no matter what pid it names.
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".iii-worker.lock"), "pid=98708\n").unwrap();
        ProjectCtx::open(dir.path().to_path_buf())
            .expect("an orphaned lockfile must not block acquisition");
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
