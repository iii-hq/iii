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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChildStatus {
    /// Spawned, not yet visible in the engine.
    Starting,
    /// Registered in the engine under `(namespace, container)`.
    Ready,
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
    pub daemon_id: String,
    /// Compose file this daemon is bound to, canonicalized. A daemon is bound to
    /// one file for its whole lifetime; see [`DaemonState::check_binding`].
    pub compose_path: PathBuf,
    pub namespace: String,
    #[serde(default)]
    pub containers: BTreeMap<String, ChildRecord>,
}

impl DaemonState {
    pub fn new(daemon_id: &str, compose_path: &Path, namespace: &str) -> Self {
        Self {
            daemon_id: daemon_id.to_string(),
            compose_path: compose_path.to_path_buf(),
            namespace: namespace.to_string(),
            containers: BTreeMap::new(),
        }
    }

    /// Refuses to reuse state recorded for a different compose file. Reusing it
    /// would let a daemon adopt — and later kill — another project's children.
    pub fn check_binding(&self, compose_path: &Path) -> Result<()> {
        if self.compose_path == compose_path {
            return Ok(());
        }
        Err(ComposeError::StateBindingMismatch {
            daemon_id: self.daemon_id.clone(),
            recorded: self.compose_path.clone(),
            requested: compose_path.to_path_buf(),
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
    /// `~/.iii/compose/<daemon-id>`, or `$III_COMPOSE_STATE_DIR/<daemon-id>`
    /// when the operator relocates it (a read-only home, a tmpfs, a test).
    pub fn for_daemon(daemon_id: &str) -> Result<Self> {
        if let Some(root) = std::env::var_os("III_COMPOSE_STATE_DIR") {
            return Ok(Self::at(PathBuf::from(root).join(daemon_id)));
        }
        let home = dirs::home_dir().ok_or(ComposeError::StateDirUnavailable)?;
        Ok(Self::at(home.join(".iii").join("compose").join(daemon_id)))
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
