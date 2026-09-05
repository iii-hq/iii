// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Durable per-child state and restart reconciliation.
//!
//! A compose daemon can die while its children keep running — that is the point
//! of putting them in their own process groups. On the next start the daemon has
//! to work out, for each child it recorded, whether that process is still the
//! one it started.
//!
//! The answer is never guessed. [`reconcile`] only reads: it compares the
//! recorded birth fingerprint against the live one and reports
//! [`Reconciliation::Unverifiable`] when they disagree, so a recycled PID is
//! surfaced for manual cleanup instead of being signalled.

use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    error::{ComposeError, Result},
    process::{BirthIdentity, birth_identity},
};

pub const STATE_FILE: &str = "state.json";

/// Directory name for one project's state, derived from its compose file.
///
/// Two halves for two jobs: the parent directory's name so an operator
/// browsing `~/.iii/compose` recognises what they are looking at, and a hash of
/// the canonical path so two projects that happen to share a directory name
/// stay apart. The file name itself is nearly always `worker-compose.yaml`, so
/// it carries nothing.
pub fn project_slug(compose_path: &Path) -> String {
    use sha2::{Digest, Sha256};

    let readable = compose_path
        .parent()
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        .map(|name| {
            name.chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect::<String>()
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "project".to_string());

    let digest = hex::encode(Sha256::digest(compose_path.as_os_str().as_encoded_bytes()));
    format!("{readable}-{}", &digest[..8])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChildStatus {
    /// Spawned, not yet visible in the engine.
    Starting,
    /// Registered in the engine under `(namespace, container)`.
    Ready,
    /// Exited after it was ready, declared a `restart` policy, and is inside
    /// the wait before the supervisor's next attempt. Nothing is running under
    /// this name right now, which is what separates it from `Ready`, and the
    /// supervisor has not given up on it, which is what separates it from
    /// `Failed`.
    Restarting,
    /// Exited unexpectedly, or a hook failed.
    Failed,
    /// Stopped on purpose by this daemon.
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChildRecord {
    pub pid: u32,
    /// Fingerprint taken when the child was spawned. Without a match, the PID is
    /// never signalled again.
    pub birth: BirthIdentity,
    pub status: ChildStatus,
    /// Seconds since the unix epoch, for `compose::status` output.
    pub started_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl ChildRecord {
    pub fn new(pid: u32, birth: BirthIdentity, status: ChildStatus) -> Self {
        Self {
            pid,
            birth,
            status,
            started_at: now_unix(),
            last_error: None,
        }
    }

    pub fn from_supervised(child: &crate::process::Supervised, status: ChildStatus) -> Self {
        Self::new(child.pid, child.birth.clone(), status)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonState {
    /// Compose file this state belongs to, canonicalized. It is also what the
    /// state directory is derived from, so the two can only disagree if the
    /// derivation collided — see [`DaemonState::check_binding`].
    pub compose_path: PathBuf,
    pub namespace: String,
    #[serde(default)]
    pub containers: BTreeMap<String, ChildRecord>,
}

impl DaemonState {
    pub fn new(compose_path: &Path, namespace: &str) -> Self {
        Self {
            compose_path: compose_path.to_path_buf(),
            namespace: namespace.to_string(),
            containers: BTreeMap::new(),
        }
    }

    /// Refuses state recorded for a different compose file.
    ///
    /// Not an operator error any more: the directory is derived from the path,
    /// so reaching this means two paths produced one slug. Rare enough to be a
    /// surprise and dangerous enough to refuse — adopting it would let one
    /// project kill another's children.
    pub fn check_binding(&self, compose_path: &Path) -> Result<()> {
        if self.compose_path == compose_path {
            return Ok(());
        }
        Err(ComposeError::InvalidState {
            path: compose_path.to_path_buf(),
            message: format!(
                "it records {} instead. Two compose files resolved to one state directory",
                self.compose_path.display()
            ),
        })
    }
}

/// What a restarting daemon should do with one recorded child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reconciliation {
    /// Same process, still running: re-adopt it without restarting anything.
    Adopt,
    /// The process is gone. Its `post_run` still owes a run and its local
    /// dependents have to come down.
    Gone,
    /// A live PID that is not provably the recorded process — a recycled PID, or
    /// a platform that cannot fingerprint. Never signalled; reported instead.
    Unverifiable,
}

/// Read-only: inspects the recorded child and returns what to do. Signals
/// nothing, kills nothing.
pub fn reconcile(record: &ChildRecord) -> Reconciliation {
    if !crate::process::is_running(record.pid) {
        return Reconciliation::Gone;
    }
    if record.birth.matches(&birth_identity(record.pid)) {
        Reconciliation::Adopt
    } else {
        Reconciliation::Unverifiable
    }
}

/// Owner-only directory holding one daemon's durable state.
#[derive(Debug, Clone)]
pub struct StateStore {
    dir: PathBuf,
}

impl StateStore {
    /// Everything compose keeps on this machine: `~/.iii/compose`, or
    /// `$III_COMPOSE_STATE_DIR` when the operator relocates it.
    pub fn root() -> Result<PathBuf> {
        match std::env::var_os("III_COMPOSE_STATE_DIR") {
            Some(root) => Ok(PathBuf::from(root)),
            None => Ok(dirs::home_dir()
                .ok_or(ComposeError::StateDirUnavailable)?
                .join(".iii")
                .join("compose")),
        }
    }

    /// Registry packages are shared by every Compose daemon and project on
    /// this machine.
    pub fn package_cache() -> Result<PathBuf> {
        Ok(Self::root()?.join("packages"))
    }

    /// Where one project's state lives:
    /// `~/.iii/compose/<daemon-namespace>/<project-slug>`, or under
    /// `$III_COMPOSE_STATE_DIR` when the operator relocates it (a read-only
    /// home, a tmpfs, a test).
    ///
    /// The slug comes from the compose file, not from a name anyone chose:
    /// there is nothing else that identifies a project, and a chosen name is a
    /// second identity that can be pointed at the wrong file.
    pub fn for_project(daemon_namespace: &str, compose_path: &Path) -> Result<Self> {
        Ok(Self::at(
            Self::root()?
                .join(daemon_namespace)
                .join(project_slug(compose_path)),
        ))
    }

    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(STATE_FILE)
    }

    /// `None` when this daemon has never written state. A corrupt file is an
    /// error, never a silent reset: that is exactly the moment children may be
    /// running unaccounted for.
    pub fn load(&self) -> Result<Option<DaemonState>> {
        let path = self.path();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(ComposeError::Io { path, source }),
        };
        serde_json::from_str(&text)
            .map(Some)
            .map_err(|err| ComposeError::InvalidState {
                path,
                message: err.to_string(),
            })
    }

    /// Writes through a temp file and renames, so a crash mid-write leaves the
    /// previous state intact rather than a truncated file.
    pub fn save(&self, state: &DaemonState) -> Result<()> {
        self.ensure_dir()?;

        let text =
            serde_json::to_string_pretty(state).map_err(|err| ComposeError::InvalidState {
                path: self.path(),
                message: err.to_string(),
            })?;

        let temp = self.dir.join(format!("{STATE_FILE}.tmp"));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        let mut file = options.open(&temp).map_err(|source| ComposeError::Io {
            path: temp.clone(),
            source,
        })?;
        file.write_all(text.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|source| ComposeError::Io {
                path: temp.clone(),
                source,
            })?;
        drop(file);

        std::fs::rename(&temp, self.path()).map_err(|source| ComposeError::Io {
            path: self.path(),
            source,
        })
    }

    /// Drops recorded state after a clean shutdown.
    pub fn clear(&self) -> Result<()> {
        match std::fs::remove_file(self.path()) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(ComposeError::Io {
                path: self.path(),
                source,
            }),
        }
    }

    fn ensure_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.dir).map_err(|source| ComposeError::Io {
            path: self.dir.clone(),
            source,
        })?;
        // State records PIDs this daemon is willing to signal; other users have
        // no business reading or editing it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o700));
        }
        Ok(())
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}
