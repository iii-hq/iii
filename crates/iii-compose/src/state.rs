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

/// Directory name for relocated project state, derived from its compose file.
///
/// Two halves for two jobs: the parent directory's name so an operator
/// browsing `$III_COMPOSE_STATE_DIR` recognises what they are looking at, and a hash of
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
    /// Exited unsuccessfully with no eligible retry, exhausted its retries, or
    /// a hook failed.
    Failed,
    /// Stopped by the daemon, or exited successfully without an eligible
    /// restart.
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChildRecord {
    /// `None` for a VM container, which has no local process.
    #[serde(default)]
    pub pid: Option<u32>,
    /// Fingerprint taken when the child was spawned. Without a match, the PID is
    /// never signalled again.
    #[serde(default)]
    pub birth: Option<BirthIdentity>,
    /// The VM a VM container runs, at the adapter that runs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vm: Option<VmRecord>,
    pub status: ChildStatus,
    /// Seconds since the unix epoch, for `compose::status` output.
    pub started_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// One boot of a VM container: `instance` names it, so a call about an older
/// boot cannot touch a newer one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VmRecord {
    pub adapter: String,
    /// Where the adapter's `worker::*` functions are registered.
    pub namespace: String,
    pub vm_id: String,
    pub instance: u64,
}

impl ChildRecord {
    pub fn new(pid: u32, birth: BirthIdentity, status: ChildStatus) -> Self {
        Self {
            pid: Some(pid),
            birth: Some(birth),
            vm: None,
            status,
            started_at: now_unix(),
            last_error: None,
        }
    }

    pub fn from_supervised(child: &crate::process::Supervised, status: ChildStatus) -> Self {
        Self::new(child.pid, child.birth.clone(), status)
    }

    pub fn from_child(child: &crate::lifecycle::Child, status: ChildStatus) -> Self {
        match child {
            crate::lifecycle::Child::Process(process) => Self::from_supervised(process, status),
            crate::lifecycle::Child::Vm(vm) => Self {
                pid: None,
                birth: None,
                vm: Some(vm.record.clone()),
                status,
                started_at: now_unix(),
                last_error: None,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonState {
    /// Compose file this state belongs to, canonicalized. It is also what the
    /// state is bound to — see [`DaemonState::check_binding`].
    pub compose_path: PathBuf,
    pub namespace: String,
    #[serde(default)]
    pub containers: BTreeMap<String, ChildRecord>,
    /// The compose process that opened this project last. `None` in state
    /// written before it was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<Owner>,
    /// Secret that lets this project's engine call the sandbox adapter's
    /// `worker::*` functions. Minted when the project first opens, kept in this
    /// 0600 file so a restarted daemon hands the same value to an adopted
    /// adapter, and given to the adapter container only. `None` in state
    /// written before it existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_token: Option<String>,
}

/// One compose process, fingerprinted like the children it records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owner {
    pub pid: u32,
    pub birth: BirthIdentity,
}

impl Owner {
    pub fn current() -> Self {
        let pid = std::process::id();
        Self {
            pid,
            birth: birth_identity(pid),
        }
    }

    /// Whether this compose process is still running. Children recorded by a
    /// live owner are its own, even when another process reads the state; a
    /// dead owner's survivors are supervised by nobody.
    pub fn is_alive(&self) -> bool {
        // Equality, not `matches`: this process is alive even where no
        // fingerprint can be taken.
        if self.pid == std::process::id() {
            return self.birth == birth_identity(self.pid);
        }
        crate::process::is_running(self.pid) && self.birth.matches(&birth_identity(self.pid))
    }
}

impl DaemonState {
    pub fn new(compose_path: &Path, namespace: &str) -> Self {
        Self {
            compose_path: compose_path.to_path_buf(),
            namespace: namespace.to_string(),
            containers: BTreeMap::new(),
            owner: None,
            sandbox_token: None,
        }
    }

    /// Refuses state recorded for a different compose file.
    ///
    /// Two compose files in the same directory must use different namespaces.
    /// Adopting another file's state would let one project kill its children.
    pub fn check_binding(&self, compose_path: &Path) -> Result<()> {
        if self.compose_path == compose_path {
            return Ok(());
        }
        Err(ComposeError::InvalidState {
            path: compose_path.to_path_buf(),
            message: format!(
                "it records {} instead. Use a different namespace for each compose file in this directory",
                self.compose_path.display()
            ),
        })
    }
}

/// What a restarting daemon should do with one recorded child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reconciliation {
    /// Same process, still running: re-adopt it without restarting anything.
    /// Only a live owner's children are kept that way; see
    /// [`DaemonState::owner`].
    Adopt,
    /// The process is gone. Its `post_run` still owes a run and its local
    /// dependents have to come down.
    Gone,
    /// A live PID that is not provably the recorded process — a recycled PID, or
    /// a platform that cannot fingerprint. Never signalled; reported instead.
    Unverifiable,
    /// This daemon stopped it on purpose, or recorded it as failed and why:
    /// nothing to adopt, nothing to report. `down` leaves the record behind and
    /// `restart` re-opens the project right after it.
    Stopped,
}

/// Read-only: inspects the recorded child and returns what to do. Signals
/// nothing, kills nothing.
pub fn reconcile(record: &ChildRecord) -> Reconciliation {
    // A failed record's last_error already says why; re-checking it would
    // replace that with "exited". A failed record can still be running (an
    // unverifiable pid, or a VM whose worker::stop got no answer); reading it
    // as stopped never signals it, and its adapter still owns the VM.
    if matches!(record.status, ChildStatus::Stopped | ChildStatus::Failed) {
        return Reconciliation::Stopped;
    }
    // No process: a VM container, which only its adapter can answer for.
    let (Some(pid), Some(birth)) = (record.pid, &record.birth) else {
        return Reconciliation::Gone;
    };
    if !crate::process::is_running(pid) {
        return Reconciliation::Gone;
    }
    if birth.matches(&birth_identity(pid)) {
        Reconciliation::Adopt
    } else {
        Reconciliation::Unverifiable
    }
}

// A test's own package cache, so it never reads or deletes the real one.
#[cfg(test)]
tokio::task_local! {
    pub(crate) static TEST_PACKAGE_CACHE: PathBuf;
}

/// Owner-only directory holding one daemon's durable state.
#[derive(Debug, Clone)]
pub struct StateStore {
    dir: PathBuf,
}

impl StateStore {
    /// Shared package storage: `~/.iii/compose`, or `$III_COMPOSE_STATE_DIR`.
    /// Project state lives beside the compose file unless explicitly relocated.
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
        #[cfg(test)]
        if let Ok(cache) = TEST_PACKAGE_CACHE.try_with(Clone::clone) {
            return Ok(cache);
        }
        Ok(Self::root()?.join("packages"))
    }

    /// Where one project's state lives:
    /// `<compose-dir>/.iii/compose/<daemon-namespace>`, or
    /// `$III_COMPOSE_STATE_DIR/<project-slug>/<daemon-namespace>` when relocated.
    ///
    /// Callers pass the canonical compose path so `--file` and symlinks use
    /// the same directory. A shared external root needs a slug to keep projects
    /// apart, while the default is already scoped to the project directory.
    pub fn for_project(daemon_namespace: &str, compose_path: &Path) -> Result<Self> {
        let project_dir = match std::env::var_os("III_COMPOSE_STATE_DIR") {
            Some(root) => PathBuf::from(root).join(project_slug(compose_path)),
            None => compose_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(".iii")
                .join("compose"),
        };
        Ok(Self::at(project_dir.join(daemon_namespace)))
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

pub(crate) fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod reconcile_tests {
    use super::{ChildRecord, ChildStatus, Reconciliation, reconcile};
    use crate::process::BirthIdentity;

    #[test]
    fn a_record_this_daemon_stopped_is_neither_gone_nor_adoptable() {
        let dead = ChildRecord::new(
            u32::MAX - 7,
            BirthIdentity::Unavailable,
            ChildStatus::Stopped,
        );
        assert_eq!(reconcile(&dead), Reconciliation::Stopped);
        // A live, even verifiable-looking pid does not turn a deliberate stop
        // back into an adoption.
        let alive = ChildRecord::new(
            std::process::id(),
            BirthIdentity::Unavailable,
            ChildStatus::Stopped,
        );
        assert_eq!(reconcile(&alive), Reconciliation::Stopped);
        // Anything else still goes through the liveness check.
        let failed = ChildRecord::new(u32::MAX - 7, BirthIdentity::Unavailable, ChildStatus::Ready);
        assert_eq!(reconcile(&failed), Reconciliation::Gone);
    }

    #[test]
    fn a_failed_record_keeps_its_error_across_a_restart() {
        // A start that never registered: no process, no VM.
        let never_ran = ChildRecord {
            pid: None,
            birth: None,
            vm: None,
            status: ChildStatus::Failed,
            started_at: 1,
            last_error: Some("container 'mailer' was not ready after 2s".into()),
        };
        assert_eq!(reconcile(&never_ran), Reconciliation::Stopped);
        // A restart that failed reuses the stopped record, dead pid and all.
        let restarted = ChildRecord {
            last_error: never_ran.last_error.clone(),
            ..ChildRecord::new(
                u32::MAX - 7,
                BirthIdentity::Unavailable,
                ChildStatus::Failed,
            )
        };
        assert_eq!(reconcile(&restarted), Reconciliation::Stopped);
        // Stopped is also what keeps a failed VM's record from its adapter.
        let vm = ChildRecord {
            vm: Some(super::VmRecord {
                adapter: "sandbox-libkrun".into(),
                namespace: "app".into(),
                vm_id: "ab".into(),
                instance: 2,
            }),
            ..never_ran
        };
        assert_eq!(reconcile(&vm), Reconciliation::Stopped);
    }

    #[test]
    fn a_pre_0_26_state_file_still_loads() {
        let state: super::DaemonState = serde_json::from_str(
            r#"{"compose_path": "/srv/app/worker-compose.yaml", "namespace": "app", "containers": {"api": {"pid": 42, "birth": {"start_time": 7}, "status": "ready", "started_at": 1}}}"#,
        )
        .unwrap();
        let api = &state.containers["api"];
        assert_eq!(api.pid, Some(42));
        assert_eq!(api.birth, Some(BirthIdentity::StartTime(7)));
        assert_eq!(api.vm, None);
        assert_eq!(state.sandbox_token, None);
    }

    #[test]
    fn a_vm_record_round_trips_and_reconciles_as_gone() {
        let record = ChildRecord {
            pid: None,
            birth: None,
            vm: Some(super::VmRecord {
                adapter: "sandbox-libkrun".into(),
                namespace: "app".into(),
                vm_id: "ab".into(),
                instance: 2,
            }),
            status: ChildStatus::Ready,
            started_at: 1,
            last_error: None,
        };
        let text = serde_json::to_string(&record).unwrap();
        assert_eq!(serde_json::from_str::<ChildRecord>(&text).unwrap(), record);
        assert_eq!(reconcile(&record), Reconciliation::Gone);
    }
}
