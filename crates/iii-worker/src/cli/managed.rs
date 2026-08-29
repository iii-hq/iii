// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! CLI command handlers for managing OCI-based workers.
//!
//! # Output contract
//!
//! - **stdout**: machine-readable worker name on success (one line). Scripts
//!   pipe `iii worker start foo | xargs ...` and rely on this being the only
//!   thing on stdout.
//! - **stderr**: all human-facing status, progress, errors, prompts, and
//!   decorative output. Every line here is cosmetic and may change between
//!   releases without breaking anyone.
//!
//! Two implications:
//! 1. Never `println!` anything that isn't the worker name. Use `eprintln!`
//!    for everything else, including successes like "✓ ready in 3.2s".
//! 2. Failures exit non-zero WITHOUT printing to stdout. Consumers of the
//!    stdout contract check the exit code first.
//!
//! The same contract applies in `local_worker.rs` and `status.rs`.

use colored::Colorize;

use super::binary_download;
use super::builtin_defaults::{get_builtin_default, is_any_builtin};
use super::config_file::ResolvedWorkerType;
use super::lifecycle::build_container_spec;
use super::registry::parse_worker_input;
use super::worker_manager::state::WorkerDef;
use std::path::PathBuf;

/// Fire `GET /download/{name}` for a resolved worker so the registry increments
/// its telemetry counters. Used for every worker type installed via `/resolve`
/// (engine workers have no artifact; binary/image/bundle workers fetch their
/// artifacts from external URLs the registry never sees, so this is the only
/// install signal it gets). When a CI environment is detected, `ci=true` is
/// also sent so the registry increments parallel `ci_count` columns. The
/// endpoint returns 204 (no artifact); errors are logged as warnings and never
/// block the install.

/// `iii worker add <bundle-name>` handler.
///
/// Runs the bundle install pipeline: acquire fslock + staging, stream
/// archive, verify sha256, extract with bundle-tight limits, validate
/// the package descriptor, atomic install into `~/.iii/workers-bundle/{name}/`
/// (replacing any previous install of the same name), and write a
/// name-only entry to config.yaml so the resolver dispatches the worker
/// on the next `iii worker start`.

pub async fn handle_managed_add_many(worker_names: &[String], wait: bool) -> i32 {
    let total = worker_names.len();
    let brief = total > 1;
    let mut fail_count = 0;

    for (i, name) in worker_names.iter().enumerate() {
        if brief {
            eprintln!("  [{}/{}] Adding {}...", i + 1, total, name.bold());
        }
        let result = handle_managed_add(name, brief, false, false, wait).await;
        if result != 0 {
            fail_count += 1;
        }
    }

    if total > 1 {
        let succeeded = total - fail_count;
        if fail_count == 0 {
            eprintln!("\n  Added {}/{} workers.", succeeded, total);
        } else {
            eprintln!(
                "\n  Added {}/{} workers. {} failed.",
                succeeded, total, fail_count
            );
        }
    }

    if fail_count == 0 { 0 } else { 1 }
}

#[derive(Default)]
struct SyncSummary {
    installed: usize,
    already_current: usize,
    repaired: usize,
    skipped: usize,
    failed: usize,
}

enum PreparedLockedWorker {
    Binary {
        name: String,
        version: String,
        bytes: Vec<u8>,
        existed_before: bool,
    },
    Image {
        name: String,
        version: String,
    },
}

/// Per-worker install mutex. Kernel advisory lock (`flock(2)`), so a
/// crashed installer can never strand the worker — see
/// `core::project::ProjectOperationLock` for the rationale. The lockfile
/// persists; only the kernel lock state matters.
struct WorkerActivationLock {
    _lock: nix::fcntl::Flock<std::fs::File>,
}

impl WorkerActivationLock {
    fn acquire(name: &str) -> Result<Self, String> {
        super::registry::validate_worker_name(name)?;
        let dir = binary_download::binary_workers_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("failed to create worker install directory: {e}"))?;
        let path = dir.join(format!(".{name}.lock"));
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| format!("failed to acquire activation lock for worker `{name}`: {e}"))?;
        match nix::fcntl::Flock::lock(file, nix::fcntl::FlockArg::LockExclusiveNonblock) {
            Ok(lock) => {
                use std::io::Write as _;
                let _ = &lock.set_len(0);
                let _ = writeln!(&*lock, "pid={}", std::process::id());
                Ok(Self { _lock: lock })
            }
            Err((_, nix::errno::Errno::EWOULDBLOCK)) => Err(format!(
                "worker `{name}` is being installed by another process (lock: {}). Wait and rerun \
                 `iii worker sync`.",
                path.display()
            )),
            Err((_, errno)) => Err(format!(
                "failed to acquire activation lock for worker `{name}`: {errno}"
            )),
        }
    }
}

struct ActiveWorkerRestore {
    install_path: PathBuf,
    backup_path: Option<PathBuf>,
    // Hold the per-worker activation lock until the batch commits or rolls
    // back. Dropping it earlier would let a concurrent sync overwrite this
    // worker's install before our rollback runs, causing rollback to delete
    // a newer install and resurrect a stale backup.
    _lock: WorkerActivationLock,
}

impl ActiveWorkerRestore {
    fn rollback(self) {
        let _ = std::fs::remove_file(&self.install_path);
        if let Some(backup_path) = self.backup_path {
            let _ = std::fs::rename(backup_path, self.install_path);
        }
    }

    fn commit(self) {
        if let Some(backup_path) = self.backup_path {
            let _ = std::fs::remove_file(backup_path);
        }
    }
}

pub async fn handle_worker_sync(frozen: bool) -> i32 {
    if frozen {
        let lock_path = super::lockfile::lockfile_path();
        if let Err(e) = super::lockfile::WorkerLockfile::read_from(lock_path) {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }

        return handle_worker_verify(false).await;
    }

    // Acquire the operation lock BEFORE reading iii.lock. Reading first
    // left a TOCTOU window: sync snapshots the lockfile, a concurrent
    // update commits a new one, then sync acquires the lock and installs
    // artifacts from the stale snapshot while iii.lock on disk says
    // otherwise. (The --frozen path above stays outside the lock: it is
    // read-only and lockfile writes are atomic renames, so its reads are
    // always self-consistent.)
    let _operation_lock =
        match crate::core::ProjectOperationLock::acquire(std::path::Path::new(".")) {
            Ok(lock) => lock,
            Err(e) => {
                eprintln!(
                    "{} another iii worker operation is active ({e}). Wait for it to finish.",
                    "error:".red()
                );
                return 1;
            }
        };

    let lock_path = super::lockfile::lockfile_path();
    let lockfile = match super::lockfile::WorkerLockfile::read_from(lock_path) {
        Ok(lockfile) => lockfile,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };

    let config_names = match super::config_file::list_worker_names_result() {
        Ok(names) => names,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };
    let names = lockfile_relevant_config_worker_names(&lockfile, &config_names);
    if let Err(e) =
        lockfile.verify_config_workers_for_target(&names, binary_download::current_target())
    {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }
    let skipped_unmanaged = skipped_unmanaged_config_workers(&lockfile, &config_names);

    match replay_lockfile(&lockfile).await {
        Ok(mut summary) => {
            summary.skipped += skipped_unmanaged.len();
            eprintln!(
                "  {} Synced registry-managed workers from {}",
                "✓".green(),
                "iii.lock".dimmed()
            );
            eprintln!(
                "    installed: {}, already current: {}, repaired: {}, skipped: {}, failed: {}",
                summary.installed,
                summary.already_current,
                summary.repaired,
                summary.skipped,
                summary.failed
            );
            if summary.skipped > 0 {
                eprintln!(
                    "    {} skipped entries are outside the v1 iii.lock replay contract.",
                    "note:".yellow()
                );
                for (name, reason) in skipped_unmanaged {
                    eprintln!("      - {}: {}", name, reason);
                }
            }
            0
        }
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            1
        }
    }
}

fn skipped_unmanaged_config_workers(
    lockfile: &super::lockfile::WorkerLockfile,
    names: &[String],
) -> Vec<(String, &'static str)> {
    names
        .iter()
        .filter(|name| !lockfile.workers.contains_key(name.as_str()))
        .filter_map(|name| {
            if get_builtin_default(name).is_some() {
                return Some((name.clone(), "built-in worker"));
            }
            match super::config_file::resolve_worker_type(name) {
                ResolvedWorkerType::Oci { .. } => Some((name.clone(), "direct OCI worker")),
                ResolvedWorkerType::Bundle { .. } => Some((name.clone(), "bundle worker")),
                ResolvedWorkerType::Binary { .. } | ResolvedWorkerType::Config => None,
            }
        })
        .collect()
}

async fn replay_lockfile(
    lockfile: &super::lockfile::WorkerLockfile,
) -> Result<SyncSummary, String> {
    let mut prepared = Vec::with_capacity(lockfile.workers.len());
    let mut summary = SyncSummary::default();

    for (name, worker) in &lockfile.workers {
        match prepare_locked_worker(name, worker).await {
            Ok(Some(worker)) => prepared.push(worker),
            Ok(None) => summary.skipped += 1,
            Err(e) => return Err(e),
        }
    }

    activate_locked_workers(prepared, &mut summary)?;
    Ok(summary)
}

async fn prepare_locked_worker(
    name: &str,
    worker: &super::lockfile::LockedWorker,
) -> Result<Option<PreparedLockedWorker>, String> {
    let source = match &worker.source {
        Some(source) => source,
        None => return Ok(None),
    };
    match source {
        super::lockfile::LockedSource::Binary { artifacts } => {
            let target = binary_download::current_target();
            if binary_download::archive_extension(target) != "tar.gz" {
                return Err(format!(
                    "worker `{name}` has artifact target `{target}`, but `iii worker sync` currently supports tar.gz binary artifacts only. \
                     Fix: use `iii worker verify --strict` in CI for this target until zip replay support lands."
                ));
            }
            let artifact = artifacts.get(target).ok_or_else(|| {
                let available = artifacts.keys().cloned().collect::<Vec<_>>().join(", ");
                format!(
                    "iii.lock is missing binary artifact for worker `{name}` target `{target}` (available: {available}). \
                     Fix: run `iii worker update {name}` on a registry version that publishes this target, or restore a lockfile with this artifact."
                )
            })?;
            let archive = binary_download::download_locked_binary_archive(
                name,
                target,
                &super::registry::BinaryInfo {
                    url: artifact.url.clone(),
                    sha256: artifact.sha256.clone(),
                },
            )
            .await?;
            let bytes = binary_download::extract_binary_from_targz(name, &archive)?;
            let existed_before = binary_download::binary_worker_path(name).exists();
            Ok(Some(PreparedLockedWorker::Binary {
                name: name.to_string(),
                version: worker.version.clone(),
                bytes,
                existed_before,
            }))
        }
        super::lockfile::LockedSource::Image { image } => {
            if !image.contains("@sha256:") {
                return Err(format!(
                    "worker `{name}` image source is not digest-pinned. Fix: run `iii worker update {name}` to refresh iii.lock from the registry."
                ));
            }
            let adapter = super::worker_manager::create_adapter("libkrun");
            adapter.pull(image).await.map_err(|e| {
                format!(
                    "failed to pull locked image for worker `{name}` from `{image}`: {e}. \
                     Fix: check registry access or run `iii worker update {name}` only if changing pins is intentional."
                )
            })?;
            Ok(Some(PreparedLockedWorker::Image {
                name: name.to_string(),
                version: worker.version.clone(),
            }))
        }
        super::lockfile::LockedSource::Bundle { .. } => {
            // Bundle lockfile replay lands in T3 alongside the install
            // pipeline. Until then we surface a clear error so `iii worker
            // sync` doesn't silently skip bundle workers.
            Err(format!(
                "worker `{name}` is a bundle worker; `iii worker sync` replay is not yet implemented. \
                 Fix: re-install with `iii worker add {name}`."
            ))
        }
    }
}

fn activate_locked_workers(
    prepared: Vec<PreparedLockedWorker>,
    summary: &mut SyncSummary,
) -> Result<(), String> {
    let mut restores = Vec::new();

    for worker in prepared {
        match activate_locked_worker(worker) {
            Ok(Some(restore)) => {
                if restore.backup_path.is_some() {
                    summary.repaired += 1;
                } else {
                    summary.installed += 1;
                }
                restores.push(restore);
            }
            Ok(None) => summary.already_current += 1,
            Err(e) => {
                for restore in restores.into_iter().rev() {
                    restore.rollback();
                }
                return Err(e);
            }
        }
    }

    for restore in restores {
        restore.commit();
    }
    Ok(())
}

fn activate_locked_worker(
    worker: PreparedLockedWorker,
) -> Result<Option<ActiveWorkerRestore>, String> {
    match worker {
        PreparedLockedWorker::Binary {
            name,
            version,
            bytes,
            existed_before,
        } => {
            let lock = WorkerActivationLock::acquire(&name)?;
            activate_locked_binary(&name, &version, &bytes, existed_before, lock)
        }
        PreparedLockedWorker::Image { name, version } => {
            eprintln!(
                "    {} image worker {} v{} is pinned by digest; no binary artifact to install",
                "✓".green(),
                name.bold(),
                version
            );
            Ok(None)
        }
    }
}

fn activate_locked_binary(
    name: &str,
    version: &str,
    bytes: &[u8],
    existed_before: bool,
    lock: WorkerActivationLock,
) -> Result<Option<ActiveWorkerRestore>, String> {
    let install_dir = binary_download::binary_workers_dir();
    std::fs::create_dir_all(&install_dir)
        .map_err(|e| format!("failed to create worker install directory: {e}"))?;
    let install_path = binary_download::binary_worker_path(name);

    if install_path.exists()
        && let Ok(existing) = std::fs::read(&install_path)
        && existing == bytes
    {
        eprintln!(
            "    {} {} v{} already current",
            "✓".green(),
            name.bold(),
            version
        );
        return Ok(None);
    }

    let tmp_path = binary_download::unique_worker_temp_path(name, "sync.tmp");
    std::fs::write(&tmp_path, bytes)
        .map_err(|e| format!("failed to write temporary binary for `{name}`: {e}"))?;
    if let Err(e) = binary_download::set_executable_permission(&tmp_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }

    let backup_path = if install_path.exists() {
        let backup_path = binary_download::unique_worker_temp_path(name, "sync.bak");
        if let Err(e) = std::fs::rename(&install_path, &backup_path) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(format!("failed to backup active binary for `{name}`: {e}"));
        }
        Some(backup_path)
    } else {
        None
    };

    if let Err(e) = std::fs::rename(&tmp_path, &install_path) {
        let _ = std::fs::remove_file(&tmp_path);
        if let Some(backup_path) = &backup_path {
            let _ = std::fs::rename(backup_path, &install_path);
        }
        return Err(format!("failed to activate binary for `{name}`: {e}"));
    }

    let action = if existed_before {
        "repaired"
    } else {
        "installed"
    };
    eprintln!(
        "    {} {} {} to v{}",
        "✓".green(),
        name.bold(),
        action,
        version
    );

    Ok(Some(ActiveWorkerRestore {
        install_path,
        backup_path,
        _lock: lock,
    }))
}

pub async fn handle_worker_verify(strict: bool) -> i32 {
    let lock_path = super::lockfile::lockfile_path();
    let lockfile = match super::lockfile::WorkerLockfile::read_from(lock_path) {
        Ok(lockfile) => lockfile,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };

    if strict && let Err(e) = verify_lockfile_strict(&lockfile) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }

    let names = match super::config_file::list_worker_names_result() {
        Ok(names) => names,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };
    let names = lockfile_relevant_config_worker_names(&lockfile, &names);
    match lockfile.verify_config_workers_for_target(&names, binary_download::current_target()) {
        Ok(()) => {
            eprintln!("  {} config.yaml matches iii.lock", "✓".green());
            if strict {
                eprintln!("  {} declaration freshness checks passed", "✓".green());
            }
            0
        }
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            1
        }
    }
}

fn verify_lockfile_strict(lockfile: &super::lockfile::WorkerLockfile) -> Result<(), String> {
    for (worker_name, worker) in &lockfile.workers {
        for (dependency, range) in &worker.dependencies {
            let locked_dependency = lockfile.workers.get(dependency).ok_or_else(|| {
                format!(
                    "iii.lock worker `{worker_name}` depends on `{dependency}` but `{dependency}` is missing from iii.lock"
                )
            })?;
            version_satisfies_range(&locked_dependency.version, range).map_err(|e| {
                format!(
                    "iii.lock worker `{worker_name}` dependency `{dependency}` is stale: locked version {} does not satisfy range `{range}` ({e}). \
                     Fix: run `iii worker update {worker_name}` only if changing pins is intentional.",
                    locked_dependency.version
                )
            })?;
        }
    }

    Ok(())
}

fn version_satisfies_range(version: &str, range: &str) -> Result<(), String> {
    let version = semver::Version::parse(version).map_err(|e| format!("invalid version: {e}"))?;
    let range = semver::VersionReq::parse(range).map_err(|e| format!("invalid range: {e}"))?;
    if range.matches(&version) {
        Ok(())
    } else {
        Err("range mismatch".to_string())
    }
}

fn lockfile_relevant_config_worker_names(
    lockfile: &super::lockfile::WorkerLockfile,
    names: &[String],
) -> Vec<String> {
    names
        .iter()
        .filter(|name| should_verify_config_worker(lockfile, name))
        .cloned()
        .collect()
}

fn should_verify_config_worker(lockfile: &super::lockfile::WorkerLockfile, name: &str) -> bool {
    if lockfile.workers.contains_key(name) {
        return true;
    }

    if super::builtin_defaults::is_any_builtin(name) {
        return false;
    }

    match super::config_file::resolve_worker_type(name) {
        ResolvedWorkerType::Oci { .. } | ResolvedWorkerType::Bundle { .. } => false,
        ResolvedWorkerType::Binary { .. } | ResolvedWorkerType::Config => true,
    }
}

pub async fn handle_worker_update(worker_name: Option<&str>) -> i32 {
    if let Some(name) = worker_name
        && let Err(e) = super::registry::validate_worker_name(name)
    {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }

    let lock_path = super::lockfile::lockfile_path();
    if !lock_path.exists() {
        // No lockfile: for a named update this is the same failure as
        // "name not pinned" below; for a bare update it's the same outcome
        // as an empty lockfile — nothing pinned, nothing to do. Surfacing
        // the raw ENOENT here made a fresh project look broken.
        if let Some(name) = worker_name {
            eprintln!("{} Worker '{}' is not in iii.lock", "error:".red(), name);
            return 1;
        }
        eprintln!(
            "  No iii.lock here; nothing to update. Install a worker first with `iii worker add <name>`."
        );
        return 0;
    }

    // Same mutual exclusion as sync: update rewrites config.yaml and
    // iii.lock per resolved root, and a concurrent update/sync would race
    // the read-modify-write (last writer silently wins).
    let _operation_lock =
        match crate::core::ProjectOperationLock::acquire(std::path::Path::new(".")) {
            Ok(lock) => lock,
            Err(e) => {
                eprintln!(
                    "{} another iii worker operation is active ({e}). Wait for it to finish.",
                    "error:".red()
                );
                return 1;
            }
        };

    let lockfile = match super::lockfile::WorkerLockfile::read_from(lock_path) {
        Ok(lockfile) => lockfile,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };

    let names: Vec<String> = match worker_name {
        Some(name) => {
            if !lockfile.workers.contains_key(name) {
                eprintln!("{} Worker '{}' is not in iii.lock", "error:".red(), name);
                return 1;
            }
            vec![name.to_string()]
        }
        None => locked_root_worker_names(&lockfile),
    };

    if names.is_empty() {
        eprintln!("  No workers pinned in iii.lock; nothing to update.");
        return 0;
    }

    let mut fail_count = 0;
    for name in &names {
        let rc = handle_descriptor_registry_add(name, None, false, true, false, false).await;
        if rc != 0 {
            fail_count += 1;
        }
    }

    if fail_count == 0 { 0 } else { 1 }
}

fn locked_root_worker_names(lockfile: &super::lockfile::WorkerLockfile) -> Vec<String> {
    let dependency_names: std::collections::BTreeSet<&str> = lockfile
        .workers
        .values()
        .flat_map(|worker| worker.dependencies.keys().map(String::as_str))
        .collect();

    let roots: Vec<String> = lockfile
        .workers
        .keys()
        .filter(|name| !dependency_names.contains(name.as_str()))
        .cloned()
        .collect();

    if roots.is_empty() {
        lockfile.workers.keys().cloned().collect()
    } else {
        roots
    }
}

struct ConfigYamlSnapshot {
    content: Option<String>,
}

impl ConfigYamlSnapshot {
    fn capture() -> Result<Self, String> {
        let path = super::config_file::config_path();
        if !path.exists() {
            return Ok(Self { content: None });
        }

        let content = std::fs::read_to_string(&path).map_err(|e| {
            format!(
                "failed to read {} before graph install: {e}",
                path.display()
            )
        })?;
        Ok(Self {
            content: Some(content),
        })
    }

    fn restore(&self) -> Result<(), String> {
        let path = super::config_file::config_path();
        match &self.content {
            Some(content) => std::fs::write(&path, content)
                .map_err(|e| format!("failed to restore {}: {e}", path.display())),
            None => match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(format!("failed to remove {}: {e}", path.display())),
            },
        }
    }

    fn restore_after_failure(&self) {
        if let Err(e) = self.restore() {
            eprintln!("{} {}", "error:".red(), e);
        }
    }
}

/// Delete `worker_name` from `iii.lock`. Returns `Ok(true)` when an entry
/// was removed and the lockfile rewritten, `Ok(false)` when there was
/// nothing to do (no lockfile, or the worker wasn't pinned — local and
/// builtin workers never are). Without this, `iii worker sync` keeps
/// replaying the removed worker's artifacts and a bare `iii worker update`
/// re-resolves it as a lock root, reinstalling it into config.yaml.
fn remove_lock_entry(worker_name: &str) -> Result<bool, String> {
    let lock_path = super::lockfile::lockfile_path();
    if !lock_path.exists() {
        return Ok(false);
    }
    let mut lockfile = super::lockfile::WorkerLockfile::read_from(lock_path)?;
    if lockfile.workers.remove(worker_name).is_none() {
        return Ok(false);
    }
    lockfile.write_to(lock_path)?;
    Ok(true)
}

/// Complete every fallible, non-mutating graph check before an install may
/// replace existing worker state. The returned value proves that validation
/// and any required operator consent already succeeded, so callers can carry
/// it across the `--force` cleanup boundary without resolving or prompting
/// twice.

/// Merge N resolved graphs into a single graph. Nodes are deduped by name.
/// If the same name appears at different versions across graphs, returns an
/// error naming the conflicting dep and both versions — this is the cross-dep
/// version-conflict gate.

/// Resolve every declared package-descriptor dependency against the registry and install
/// the full transitive chain into `config.yaml` + `iii.lock` using the same
/// path that `iii worker add <name>` uses.
///
/// Pass-1: resolve each dep via `fetch_resolved_worker_graph` (serial — fine
/// for ≤3 deps; parallel fan-out is a future optimization).
/// Pass-2: merge all graphs into one dependency forest (dedupes shared
/// transitive deps, errors on cross-graph version conflicts).
/// Pass-3: preflight the complete forest once; installation then uses one
/// snapshot/rollback boundary, so no partial-install state is possible.

pub async fn handle_managed_add(
    image_or_name: &str,
    brief: bool,
    force: bool,
    reset_config: bool,
    wait: bool,
) -> i32 {
    use std::io::IsTerminal as _;

    handle_managed_add_with_consent(
        image_or_name,
        brief,
        force,
        reset_config,
        wait,
        false,
        std::io::stdin().is_terminal() && std::io::stderr().is_terminal(),
    )
    .await
}

const PACKAGE_DESCRIPTOR_FILE: &str = ".iii-package-descriptor.json";
const PACKAGE_DESCRIPTOR_DIGEST_FILE: &str = ".iii-package-descriptor.sha256";

fn copy_package_tree(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if ty.is_dir() {
            copy_package_tree(&entry.path(), &target)?;
        } else if ty.is_file() {
            std::fs::copy(entry.path(), target)?;
        } else {
            return Err(std::io::Error::other(
                "package contains a non-regular entry",
            ));
        }
    }
    Ok(())
}

fn install_descriptor_package(
    installed: &iii_compose::registry::InstalledPackage,
) -> Result<(), String> {
    use iii_compose::registry::Payload;

    let default_config = installed
        .descriptor
        .registry
        .config
        .as_ref()
        .map(|config| serde_yaml::to_string(&config.defaults).map_err(|error| error.to_string()))
        .transpose()?;

    match &installed.payload {
        Payload::Binary(program) => {
            let destination = super::binary_download::binary_worker_path(&installed.name);
            std::fs::create_dir_all(super::binary_download::binary_workers_dir())
                .map_err(|error| error.to_string())?;
            std::fs::copy(program, &destination).map_err(|error| {
                format!(
                    "cannot install binary at {}: {error}",
                    destination.display()
                )
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut permissions = std::fs::metadata(&destination)
                    .map_err(|error| error.to_string())?
                    .permissions();
                permissions.set_mode(0o755);
                std::fs::set_permissions(&destination, permissions)
                    .map_err(|error| error.to_string())?;
            }
            super::config_file::append_worker(&installed.name, default_config.as_deref())
                .map_err(|e| e.to_string())?;
        }
        Payload::Bundle(source) => {
            let destination = super::config_file::bundle_worker_path(&installed.name);
            let parent = super::config_file::bundle_workers_dir();
            std::fs::create_dir_all(&parent).map_err(|error| error.to_string())?;
            let staging = parent.join(format!(".{}.descriptor.partial", installed.name));
            if staging.exists() {
                std::fs::remove_dir_all(&staging).map_err(|error| error.to_string())?;
            }
            copy_package_tree(source, &staging).map_err(|error| error.to_string())?;
            std::fs::write(
                staging.join(PACKAGE_DESCRIPTOR_FILE),
                serde_json::to_vec_pretty(&installed.descriptor).map_err(|e| e.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            std::fs::write(
                staging.join(PACKAGE_DESCRIPTOR_DIGEST_FILE),
                format!("{}\n", installed.descriptor_sha256),
            )
            .map_err(|error| error.to_string())?;
            let backup = parent.join(format!(".{}.descriptor.backup", installed.name));
            if backup.exists() {
                std::fs::remove_dir_all(&backup).map_err(|error| error.to_string())?;
            }
            if destination.exists() {
                std::fs::rename(&destination, &backup).map_err(|error| error.to_string())?;
            }
            if let Err(error) = std::fs::rename(&staging, &destination) {
                if backup.exists() {
                    let _ = std::fs::rename(&backup, &destination);
                }
                return Err(error.to_string());
            }
            if backup.exists() {
                let _ = std::fs::remove_dir_all(backup);
            }
            super::config_file::append_worker(&installed.name, default_config.as_deref())
                .map_err(|e| e.to_string())?;
        }
        Payload::Oci(image) => {
            super::config_file::append_worker_with_image(
                &installed.name,
                image,
                default_config.as_deref(),
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn lock_descriptor_packages(
    installed: &[iii_compose::registry::InstalledPackage],
    graph: &iii_compose::registry::Graph,
) -> Result<(), String> {
    use super::lockfile::{LockedBinaryArtifact, LockedSource, LockedWorker, LockedWorkerType};
    use iii_compose::registry::InstalledArtifact;

    let path = super::lockfile::lockfile_path();
    let mut lock = if path.exists() {
        super::lockfile::WorkerLockfile::read_from(path)?
    } else {
        super::lockfile::WorkerLockfile::default()
    };
    let resolved_versions: std::collections::BTreeMap<_, _> = graph
        .nodes
        .iter()
        .map(|node| (node.name.as_str(), node.version.as_str()))
        .collect();
    for package in installed {
        let dependencies = graph
            .edges
            .iter()
            .filter(|(from, _)| from == &package.name)
            .filter_map(|(_, dependency)| {
                resolved_versions
                    .get(dependency.as_str())
                    .map(|version| (dependency.clone(), format!("={version}")))
            })
            .collect();
        let (worker_type, source) = match &package.artifact {
            InstalledArtifact::RustBinary { artifacts } => (
                LockedWorkerType::Binary,
                LockedSource::Binary {
                    artifacts: artifacts
                        .iter()
                        .map(|(target, artifact)| {
                            (
                                target.clone(),
                                LockedBinaryArtifact {
                                    url: artifact.url.clone(),
                                    sha256: artifact.sha256.clone(),
                                },
                            )
                        })
                        .collect(),
                },
            ),
            InstalledArtifact::Bundle {
                archive_url,
                sha256,
            } => (
                LockedWorkerType::Bundle,
                LockedSource::Bundle {
                    archive_url: archive_url.clone(),
                    sha256: sha256.clone(),
                },
            ),
            InstalledArtifact::Oci { image } => (
                LockedWorkerType::Image,
                LockedSource::Image {
                    image: image.clone(),
                },
            ),
        };
        lock.workers.insert(
            package.name.clone(),
            LockedWorker {
                version: package.version.clone(),
                package_descriptor: package.descriptor.clone(),
                descriptor_sha256: package.descriptor_sha256.clone(),
                worker_type,
                dependencies,
                source: Some(source),
            },
        );
    }
    lock.write_to(path)
}

async fn handle_descriptor_registry_add(
    name: &str,
    version: Option<&str>,
    brief: bool,
    force: bool,
    reset_config: bool,
    wait: bool,
) -> i32 {
    let cache = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".iii")
        .join("package-cache");
    let graph = match iii_compose::registry::resolve_graph(name, name, version.unwrap_or("*")).await
    {
        Ok(graph) => graph,
        Err(error) => {
            eprintln!("{} {error}", "error:".red());
            return 1;
        }
    };
    let mut packages = Vec::new();
    for node in &graph.nodes {
        let installed = match iii_compose::registry::install(
            &node.name,
            &node.name,
            &format!("={}", node.version),
            &cache,
        )
        .await
        {
            Ok(installed) => installed,
            Err(error) => {
                eprintln!("{} {error}", "error:".red());
                return 1;
            }
        };
        packages.push(installed);
    }
    if apply_force_replacement(name, force, reset_config).await != 0 {
        return 1;
    }
    for installed in &packages {
        if let Err(error) = install_descriptor_package(installed) {
            eprintln!("{} {error}", "error:".red());
            return 1;
        }
    }
    if let Err(error) = lock_descriptor_packages(&packages, &graph) {
        eprintln!("{} {error}", "error:".red());
        return 1;
    }
    if !brief {
        eprintln!(
            "\n  {} Installed immutable Registry package {}",
            "✓".green(),
            name.bold()
        );
    }
    finish_add(name, 0, wait, brief).await
}

/// Apply the destructive half of `--force`.
///
/// Registry package callers must invoke this only after their source has
/// been resolved and preflighted, including any required operator consent.
async fn apply_force_replacement(worker_name: &str, force: bool, reset_config: bool) -> i32 {
    if !force {
        return 0;
    }

    if let Err(e) = super::registry::validate_worker_name(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }

    if is_worker_running(worker_name) {
        eprintln!(
            "  {} {} is running, stopping first...",
            "⟳".cyan(),
            worker_name.bold()
        );
        let stop_rc = handle_managed_stop(worker_name).await;
        if stop_rc != 0 {
            // Don't abort — artifacts will be wiped below anyway, and the
            // most common "failure" is "already stopped between
            // is_worker_running and the signal" which is benign.
            eprintln!(
                "  {} stop exited {} — continuing with force add anyway",
                "warning:".yellow(),
                stop_rc
            );
        }
    }

    if is_any_builtin(worker_name) {
        eprintln!(
            "  {} '{}' is a builtin worker, no artifacts to re-download.",
            "info:".cyan(),
            worker_name,
        );
    } else {
        let freed = delete_worker_artifacts(worker_name);
        if freed > 0 {
            eprintln!(
                "  {} Cleared {:.1} MB of artifacts for {}",
                "✓".green(),
                freed as f64 / 1_048_576.0,
                worker_name.bold(),
            );
        }
    }

    if reset_config && let Err(e) = super::config_file::remove_worker(worker_name) {
        tracing::debug!("remove_worker during force: {}", e);
    }

    0
}

pub async fn handle_managed_add_with_consent(
    image_or_name: &str,
    brief: bool,
    force: bool,
    reset_config: bool,
    wait: bool,
    _assume_yes: bool,
    _can_prompt: bool,
) -> i32 {
    if image_or_name.starts_with(['.', '/', '~']) || image_or_name.contains(':') {
        eprintln!(
            "{} iii worker add accepts Registry package references only; use worker-compose.yaml and `iii compose up` for local or OCI workers",
            "error:".red()
        );
        return 1;
    }

    let (name, version) = parse_worker_input(image_or_name);

    handle_descriptor_registry_add(&name, version.as_deref(), brief, force, reset_config, wait)
        .await
}

/// Shared tail for every non-local `handle_managed_add` exit path.
///
/// `handle_managed_add` accepts `wait: bool` from the `--wait` / `--no-wait`
/// flag (default wait=true per the CLI definition in app.rs). Before this
/// helper, `wait` was only honored on the local-path branch — OCI/binary/
/// registry adds silently dropped it, contradicting the `add` command's
/// documented "waits up to 120s by default" contract. We also skip the
/// wait when `rc != 0` (nothing to wait on if the add itself failed) and
/// when `brief` is set (multi-worker `add-many` renders per-row status
/// and a blocking wait per entry would produce confusing output).
async fn finish_add(worker_name: &str, rc: i32, wait: bool, brief: bool) -> i32 {
    if rc != 0 || !wait || brief {
        return rc;
    }
    if is_any_builtin(worker_name) {
        return rc;
    }
    let port = super::config_file::manager_port();
    if !is_engine_running_on(port) {
        // Engine down → no file watcher → config.yaml change won't be
        // picked up. A wait would run to timeout. Tell the user instead.
        eprintln!(
            "\n  {} engine not running; start it to observe boot.\n  \
               Start:         iii\n  \
               Then watch:    iii worker status {}",
            "⚠".yellow(),
            worker_name,
        );
        return rc;
    }
    wait_for_ready(worker_name, port).await;
    rc
}

pub async fn handle_managed_remove_many(worker_names: &[String], yes: bool) -> i32 {
    let total = worker_names.len();
    let brief = total > 1;
    let mut fail_count = 0;

    // Single batch confirmation for any names that are currently running. We
    // gather them up-front so the user sees the whole blast radius once, not a
    // prompt per worker.
    if !yes {
        let running: Vec<&String> = worker_names
            .iter()
            .filter(|n| is_worker_running(n))
            .collect();
        if !running.is_empty() {
            let list = running
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!(
                "  {} {} currently running: {}",
                "warning:".yellow(),
                if running.len() == 1 {
                    "worker is"
                } else {
                    "workers are"
                },
                list,
            );
            eprintln!(
                "  Removing them from config.yaml triggers an engine reload that will tear the sandbox(es) down."
            );
            if !confirm_prompt("  Continue? [y/N] ") {
                eprintln!("  Aborted.");
                return 0;
            }
        }
    }

    for (i, name) in worker_names.iter().enumerate() {
        if brief {
            eprintln!("  [{}/{}] Removing {}...", i + 1, total, name.bold());
        }
        let result = handle_managed_remove(name, brief).await;
        if result != 0 {
            fail_count += 1;
        }
    }

    if total > 1 {
        let succeeded = total - fail_count;
        if fail_count == 0 {
            eprintln!("\n  Removed {}/{} workers.", succeeded, total);
        } else {
            eprintln!(
                "\n  Removed {}/{} workers. {} failed.",
                succeeded, total, fail_count
            );
        }
    }

    if fail_count == 0 { 0 } else { 1 }
}

pub async fn handle_managed_remove(worker_name: &str, brief: bool) -> i32 {
    if let Err(e) = super::registry::validate_worker_name(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }
    // Distinguish "config.yaml doesn't exist yet" from "worker isn't in it."
    // The underlying remove_worker surfaces both as the same anyhow error,
    // which misleads users into thinking their config file is missing.
    if !super::config_file::worker_exists(worker_name) {
        eprintln!(
            "{} Worker '{}' is not in config.yaml. Run `iii worker list` to see known workers.",
            "error:".red(),
            worker_name,
        );
        return 1;
    }
    // Same mutual exclusion as sync/update: remove edits config.yaml AND
    // iii.lock, so racing an update's lockfile read-modify-write could
    // silently write the removed worker's pin back, and the snapshot
    // rollback below could wipe a concurrent add's config.yaml append.
    let _operation_lock =
        match crate::core::ProjectOperationLock::acquire(std::path::Path::new(".")) {
            Ok(lock) => lock,
            Err(e) => {
                eprintln!(
                    "{} another iii worker operation is active ({e}). Wait for it to finish.",
                    "error:".red()
                );
                return 1;
            }
        };
    // Snapshot config.yaml so the config and lockfile edits commit together:
    // a worker left in iii.lock after leaving config.yaml gets resurrected by
    // `iii worker update` and replayed by `iii worker sync`.
    let config_snapshot = match ConfigYamlSnapshot::capture() {
        Ok(snapshot) => snapshot,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };
    if let Err(e) = super::config_file::remove_worker(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }
    let removed_from_lock = match remove_lock_entry(worker_name) {
        Ok(removed) => removed,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            config_snapshot.restore_after_failure();
            return 1;
        }
    };
    if brief {
        eprintln!("        {} {}", "✓".green(), worker_name.bold());
    } else if removed_from_lock {
        eprintln!(
            "  {} {} removed from {} and {}",
            "✓".green(),
            worker_name.bold(),
            "config.yaml".dimmed(),
            "iii.lock".dimmed(),
        );
    } else {
        eprintln!(
            "  {} {} removed from {}",
            "✓".green(),
            worker_name.bold(),
            "config.yaml".dimmed(),
        );
    }
    0
}

/// Read a y/N answer from stdin. Mirrors `confirm_clear` but parameterized on
/// the prompt so we can reuse it for `remove` and any future destructive ops.
fn confirm_prompt(prompt: &str) -> bool {
    use std::io::{Read, Write};
    #[cfg(unix)]
    super::local_worker::restore_terminal_cooked_mode();
    let _ = std::io::stderr().write_all(prompt.as_bytes());
    let _ = std::io::stderr().flush();
    let mut buf = [0u8; 64];
    let n = std::io::stdin().read(&mut buf).unwrap_or(0);
    let input = std::str::from_utf8(&buf[..n]).unwrap_or("");
    input.trim().eq_ignore_ascii_case("y")
}

pub fn handle_managed_clear(worker_name: Option<&str>, skip_confirm: bool) -> i32 {
    match worker_name {
        Some(name) => clear_single_worker(name),
        None => clear_all_workers(skip_confirm),
    }
}

fn clear_single_worker(worker_name: &str) -> i32 {
    if let Err(e) = super::registry::validate_worker_name(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }

    if is_worker_running(worker_name) {
        eprintln!(
            "{} Worker '{}' is currently running. Stop it first with `iii worker stop {}`",
            "error:".red(),
            worker_name,
            worker_name,
        );
        return 1;
    }

    // Distinguish "worker doesn't exist" from "already clean". The old path
    // exited 0 on unknown names, which hid typos in automation. If we have no
    // artifacts AND the name isn't in config.yaml, it's a typo -- exit 1.
    let home = dirs::home_dir().unwrap_or_default();
    let has_artifacts = home.join(".iii/workers").join(worker_name).exists()
        || home.join(".iii/managed").join(worker_name).is_dir();
    let in_config = super::config_file::worker_exists(worker_name);
    if !has_artifacts && !in_config {
        eprintln!(
            "{} Worker '{}' not found. Run `iii worker list` to see known workers.",
            "error:".red(),
            worker_name,
        );
        return 1;
    }

    let freed = delete_worker_artifacts(worker_name);
    if freed == 0 {
        eprintln!("  Nothing to clear for '{}'.", worker_name);
    } else {
        eprintln!(
            "  {} Cleared {:.1} MB of artifacts for {}",
            "✓".green(),
            freed as f64 / 1_048_576.0,
            worker_name.bold(),
        );
    }
    0
}

/// Prompts the user for confirmation before clearing all artifacts.
/// Returns `true` if the user confirms with "y".
fn confirm_clear() -> bool {
    confirm_prompt(
        "  This will remove all downloaded workers, images, and managed VM state. Continue? [y/N] ",
    )
}

fn clear_all_workers(skip_confirm: bool) -> i32 {
    let home = dirs::home_dir().unwrap_or_default();
    let workers_dir = home.join(".iii/workers");
    let images_dir = home.join(".iii/images");
    let managed_dir = home.join(".iii/managed");

    if !workers_dir.exists() && !images_dir.exists() && !managed_dir.exists() {
        eprintln!("  Nothing to clear.");
        return 0;
    }

    if !skip_confirm && !confirm_clear() {
        eprintln!("  Aborted.");
        return 0;
    }

    let mut skipped: Vec<String> = Vec::new();
    let mut total_freed: u64 = 0;
    // Unique worker names cleared across ~/.iii/workers and ~/.iii/managed —
    // a binary worker can have artifacts in both, and counting it twice
    // would inflate the tally.
    let mut cleared_workers: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut image_count: u32 = 0;

    // Deletes one artifact dir entry after the shared guards: valid worker
    // name, resolved path stays under `base`, worker not currently running.
    let mut clear_entry = |entry: &std::fs::DirEntry,
                           base: &std::path::Path,
                           skipped: &mut Vec<String>|
     -> Option<String> {
        let name = entry.file_name().to_string_lossy().to_string();
        // Skip entries with invalid names (e.g. symlinks with path traversal)
        if super::registry::validate_worker_name(&name).is_err() {
            return None;
        }
        // Verify resolved path stays under the artifact base dir
        if let Ok(resolved) = entry.path().canonicalize()
            && let Ok(base) = base.canonicalize()
            && !resolved.starts_with(&base)
        {
            return None;
        }
        if is_worker_running(&name) {
            if !skipped.contains(&name) {
                skipped.push(name);
            }
            return None;
        }
        // Legacy binary workers can be a bare FILE at ~/.iii/workers/{name}
        // (see delete_worker_artifacts); remove_dir_all fails on those with
        // NotADirectory. Branch on the entry's own (non-following) file
        // type, and count the entry as cleared — and its bytes as freed —
        // only when the deletion actually succeeded.
        let path = entry.path();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let (bytes, removed) = if is_dir {
            (dir_size(&path), std::fs::remove_dir_all(&path))
        } else {
            let len = std::fs::symlink_metadata(&path)
                .map(|m| m.len())
                .unwrap_or(0);
            (len, std::fs::remove_file(&path))
        };
        if removed.is_err() {
            return None;
        }
        total_freed += bytes;
        Some(name)
    };

    // Clear binary workers
    if workers_dir.exists()
        && let Ok(entries) = std::fs::read_dir(&workers_dir)
    {
        for entry in entries.flatten() {
            if let Some(name) = clear_entry(&entry, &workers_dir, &mut skipped) {
                cleared_workers.insert(name);
            }
        }
    }

    // Clear managed VM state (~/.iii/managed/{name}): rootfs clones, dep
    // caches, and the `.iii-prepared` marker. The per-worker path
    // (`delete_worker_artifacts`) already removes this dir — a stale
    // prepared marker silently skips the in-VM dependency reinstall on the
    // next boot (MOT-3585) — so the wipe-all path must cover it too.
    if managed_dir.exists()
        && let Ok(entries) = std::fs::read_dir(&managed_dir)
    {
        for entry in entries.flatten() {
            if let Some(name) = clear_entry(&entry, &managed_dir, &mut skipped) {
                cleared_workers.insert(name);
            }
        }
    }

    // Clear OCI images — protect running OCI workers
    if images_dir.exists() {
        // Build set of image hashes belonging to running OCI workers
        let mut protected_hashes = std::collections::HashSet::new();
        for name in super::config_file::list_worker_names() {
            if is_worker_running(&name)
                && let Some((image_ref, _)) = super::config_file::get_worker_start_info(&name)
            {
                let dir = image_cache_dir(&image_ref);
                if let Some(hash) = dir.file_name().and_then(|f| f.to_str()) {
                    protected_hashes.insert(hash.to_string());
                }
            }
        }

        if let Ok(entries) = std::fs::read_dir(&images_dir) {
            for entry in entries.flatten() {
                let dir_name = entry.file_name().to_string_lossy().to_string();
                if protected_hashes.contains(&dir_name) {
                    skipped.push(format!("OCI image {}", dir_name));
                    continue;
                }
                total_freed += dir_size(&entry.path());
                let _ = std::fs::remove_dir_all(entry.path());
                image_count += 1;
            }
        }
    }

    // Print skipped warnings FIRST so the final line the user sees is the
    // success tally, not a "✓ success" followed by warnings (which reads as
    // "everything worked, oh btw some didn't").
    for name in &skipped {
        eprintln!(
            "  {} Skipped {} (running). Stop it first with `iii worker stop {}`",
            "warning:".yellow(),
            name.bold(),
            name,
        );
    }

    eprintln!(
        "  {} Cleared {} worker(s) and {} image(s) ({:.1} MB freed)",
        "✓".green(),
        cleared_workers.len(),
        image_count,
        total_freed as f64 / 1_048_576.0,
    );

    0
}

/// Kill any stale worker process from a previous engine run.
/// Checks OCI/local (vm.pid) and binary (pids/{name}.pid) PID files,
/// sends SIGTERM+SIGKILL, and removes the PID file.
/// Kill the host-side source watcher sidecar for `worker_name` and
/// remove its pid file. No-op when no watcher is running.
///
/// Called from the stop path so the watcher doesn't observe the VM
/// shutdown as a file event and race to restart what we just stopped.
/// Also called by `kill_stale_worker` (indirectly, via `watch.pid` in
/// its pid file list) to reap leaks from crashed starts.
pub async fn reap_source_watcher(worker_name: &str) {
    let home = dirs::home_dir().unwrap_or_default();
    let watch_pidfile = home
        .join(".iii/managed")
        .join(worker_name)
        .join("watch.pid");
    if let Some(watch_pid) = read_pid(&watch_pidfile) {
        kill_pid_with_grace(watch_pid).await;
    }
    let _ = std::fs::remove_file(&watch_pidfile);
}

pub async fn kill_stale_worker(worker_name: &str) {
    let home = dirs::home_dir().unwrap_or_default();
    let pid_files = [
        home.join(".iii/managed").join(worker_name).join("vm.pid"),
        home.join(".iii/managed")
            .join(worker_name)
            .join("watch.pid"),
        home.join(".iii/pids").join(format!("{}.pid", worker_name)),
    ];

    for pid_file in &pid_files {
        // Route through the hardened reader so a pre-planted symlink
        // at `pid_file` can't redirect us into an arbitrary file, and
        // a pidfile owned by another uid is ignored instead of honored.
        // We still attempt `remove_file` whenever the file exists so
        // stale/unreadable pidfiles get cleaned up regardless.
        let existed = pid_file.exists();
        if let Some(pid) = read_pid(pid_file) {
            #[cfg(unix)]
            {
                use nix::sys::signal::{Signal, kill};
                use nix::unistd::Pid;
                let p = Pid::from_raw(pid as i32);
                // Only kill if the process is still alive AND is not a
                // recycled PID now hosting an unrelated process. watch.pid
                // points at a watcher helper the argv matcher can't name,
                // so identity is only enforced for the worker pidfiles.
                if kill(p, None).is_ok() {
                    let is_watch_pid =
                        pid_file.file_name().and_then(|f| f.to_str()) == Some("watch.pid");
                    if !is_watch_pid && pid_identity_matches(pid, worker_name) == Some(false) {
                        tracing::warn!(
                            worker = %worker_name, pid,
                            "pidfile PID belongs to an unrelated process (recycled); not killing"
                        );
                    } else {
                        tracing::info!(worker = %worker_name, pid, "Killing stale worker process");
                        let _ = kill(p, Signal::SIGTERM);
                        // Brief wait then force-kill.
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        // Re-check before the force-kill: the PID may have
                        // exited on SIGTERM and been recycled by an
                        // unrelated process during the grace window. `None`
                        // (already dead, or a watch.pid whose argv we can't
                        // name) falls through to a harmless SIGKILL of a
                        // dead/zombie pid.
                        if is_watch_pid || pid_identity_matches(pid, worker_name) != Some(false) {
                            let _ = kill(p, Signal::SIGKILL);
                        }
                    }
                }
            }
            #[cfg(not(unix))]
            {
                let _ = pid;
            }
        }
        if existed {
            let _ = tokio::fs::remove_file(pid_file).await;
        }
    }

    // Pidfile-less leftovers: a VM whose pidfile was lost (crash, manual
    // cleanup, overlapping engine restarts) is invisible to the pass above
    // but must still die before a new instance shares
    // `~/.iii/managed/{worker_name}` (MOT-3931 duplicate-VM race).
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;
        let leftovers = find_worker_pids_from_ps(worker_name);
        if !leftovers.is_empty() {
            tracing::info!(
                worker = %worker_name, pids = ?leftovers,
                "Killing pidfile-less worker process(es)"
            );
            for pid in &leftovers {
                let _ = kill(Pid::from_raw(*pid as i32), Signal::SIGTERM);
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            // Re-scan before the force-kill: a PID from the first snapshot
            // may have exited during the grace window and been recycled by
            // an unrelated process. Force-kill only PIDs present in BOTH
            // snapshots — the fresh scan alone could pick up a
            // concurrently-started sibling this pass must not touch.
            let survivors: std::collections::HashSet<u32> =
                find_worker_pids_from_ps(worker_name).into_iter().collect();
            for pid in leftovers.iter().filter(|p| survivors.contains(p)) {
                let _ = kill(Pid::from_raw(*pid as i32), Signal::SIGKILL);
            }
        }
    }
}

/// Returns worker names discovered from on-disk runtime state under `~/.iii`.
///
/// Sources scanned:
/// - `~/.iii/managed/{name}/`     -- OCI/VM and local-path workers
/// - `~/.iii/pids/{name}.pid`     -- binary workers
///
/// Names are returned sorted and deduplicated. This is the union of every
/// worker the local runtime has touched, regardless of which `config.yaml`
/// declared them. Used by `iii worker list` to surface orphan workers whose
/// project folder has moved or been deleted.
pub fn discover_disk_worker_names() -> Vec<String> {
    let home = dirs::home_dir().unwrap_or_default();
    discover_disk_worker_names_in(&home.join(".iii/managed"), &home.join(".iii/pids"))
}

/// Path-injectable variant of [`discover_disk_worker_names`] for testing.
fn discover_disk_worker_names_in(
    managed_dir: &std::path::Path,
    pids_dir: &std::path::Path,
) -> Vec<String> {
    use std::collections::BTreeSet;
    let mut names = BTreeSet::new();

    if let Ok(entries) = std::fs::read_dir(managed_dir) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
                && let Some(name) = entry.file_name().to_str()
            {
                names.insert(name.to_string());
            }
        }
    }

    if let Ok(entries) = std::fs::read_dir(pids_dir) {
        for entry in entries.flatten() {
            if let Some(file_name) = entry.file_name().to_str()
                && let Some(name) = file_name.strip_suffix(".pid")
                && !name.is_empty()
            {
                names.insert(name.to_string());
            }
        }
    }

    names.into_iter().collect()
}

/// Discovers worker names by inspecting live process command lines for
/// processes spawned by iii-worker. Catches the case where a worker is alive
/// but its on-disk PID file has been removed (project folder moved/deleted,
/// manual cleanup, or a crashed `iii worker stop`).
///
/// Two process patterns are recognised:
/// 1. Binary workers — executable is `~/.iii/workers/{name}`.
/// 2. OCI/VM workers — `iii-worker __vm-boot --pid-file ~/.iii/managed/{name}/vm.pid ...`.
///
/// Sources by platform:
/// - Linux: walks `/proc/*/cmdline` (works on every kernel including
///   Alpine/busybox where `ps -o args=` is unreliable).
/// - macOS: shells out to `ps -axww -o pid=,args=`.
/// - Other platforms: returns empty (best-effort supplement to disk discovery).
pub fn discover_running_worker_names_from_ps() -> Vec<String> {
    let processes = collect_processes();
    if processes.is_empty() {
        return Vec::new();
    }
    let home = dirs::home_dir().unwrap_or_default();
    let workers_prefix = home.join(".iii/workers");
    let managed_prefix = home.join(".iii/managed");
    let cmdlines: Vec<String> = processes.into_iter().map(|(_, c)| c).collect();
    discover_running_worker_names_from_ps_output(
        &cmdlines.join("\n"),
        &workers_prefix,
        &managed_prefix,
    )
}

/// Returns the live PID of the iii-worker process associated with `name`, by
/// scanning live process command lines. Used by `iii worker stop` to terminate
/// orphan workers whose pidfiles have been removed.
///
/// Returns `None` when no matching process exists, when the platform has no
/// process enumeration support, or when `ps`/`/proc` access is denied.
pub fn find_worker_pid_from_ps(name: &str) -> Option<u32> {
    let processes = collect_processes();
    if processes.is_empty() {
        return None;
    }
    let home = dirs::home_dir().unwrap_or_default();
    let workers_prefix = home.join(".iii/workers");
    let managed_prefix = home.join(".iii/managed");
    find_worker_pid_in_processes(&processes, name, &workers_prefix, &managed_prefix)
}

/// Linux: read every numeric `/proc/<pid>/cmdline`. Each is NUL-separated
/// argv0\0argv1\0...\0; we replace NULs with spaces so the shared parser
/// can tokenise it the same way as `ps` output.
#[cfg(target_os = "linux")]
fn collect_processes() -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir("/proc") {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = match name.to_str() {
            Some(s) => s,
            None => continue,
        };
        let pid: u32 = match name_str.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let bytes = match std::fs::read(entry.path().join("cmdline")) {
            Ok(b) if !b.is_empty() => b,
            _ => continue,
        };
        let line = String::from_utf8_lossy(&bytes).replace('\0', " ");
        let trimmed = line.trim_end();
        if !trimmed.is_empty() {
            out.push((pid, trimmed.to_string()));
        }
    }
    out
}

/// macOS: BSD `ps` exposes full argv via `-o args=`; `-axww` selects all
/// processes and disables column truncation. `pid=` keeps the pid column
/// without a header so we can split the first whitespace-separated token off.
#[cfg(target_os = "macos")]
fn collect_processes() -> Vec<(u32, String)> {
    let output = match std::process::Command::new("ps")
        .args(["-axww", "-o", "pid=,args="])
        .output()
    {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };
    String::from_utf8_lossy(&output)
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let mut split = line.splitn(2, char::is_whitespace);
            let pid: u32 = split.next()?.parse().ok()?;
            let args = split.next()?.trim();
            if args.is_empty() {
                None
            } else {
                Some((pid, args.to_string()))
            }
        })
        .collect()
}

/// Other platforms: no cross-platform process enumeration without a new dep.
/// Disk discovery still runs; we just lose the alive-but-no-pidfile fallback.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn collect_processes() -> Vec<(u32, String)> {
    Vec::new()
}

/// From a single process's `argv`-joined cmdline, return the worker name it
/// represents (if any). Shared between name discovery, PID lookup, and the
/// pidfile identity cross-check so all match against the exact same
/// recognition rules.
fn extract_worker_name_from_cmdline(
    cmdline: &str,
    workers_prefix: &std::path::Path,
    managed_prefix: &std::path::Path,
) -> Option<String> {
    let mut tokens = cmdline.split_whitespace();
    let exe = tokens.next()?;
    let exe_path = std::path::Path::new(exe);

    // Pattern 1: binary worker -- executable lives under ~/.iii/workers/{name}
    if let Ok(rel) = exe_path.strip_prefix(workers_prefix)
        && let Some(name) = rel.iter().next().and_then(|c| c.to_str())
        && !name.is_empty()
    {
        return Some(name.to_string());
    }

    // Pattern 2: iii-worker __vm-boot with a path under ~/.iii/managed/{name}
    // in either `--pid-file <...>/managed/{name}/vm.pid` or
    // `--rootfs <...>/managed/{name}`. The `--rootfs` fallback matters for VMs
    // booted by older builds whose dev boot path didn't pass `--pid-file`.
    if exe_path.file_name().and_then(|s| s.to_str()) == Some("iii-worker")
        && tokens.next() == Some("__vm-boot")
    {
        let rest: Vec<&str> = tokens.collect();
        for i in 0..rest.len().saturating_sub(1) {
            let flag = rest[i];
            if flag != "--pid-file" && flag != "--rootfs" {
                continue;
            }
            let Ok(rel) = std::path::Path::new(rest[i + 1]).strip_prefix(managed_prefix) else {
                continue;
            };
            let mut components = rel.iter();
            let Some(name) = components.next().and_then(|c| c.to_str()) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            // `--rootfs` only counts when it IS the managed dir: legacy-cache
            // sandbox VMs boot with `--rootfs ~/.iii/managed/<preset>/rootfs`
            // and must not be claimed as worker `<preset>` (worker dev VMs
            // pass the managed dir itself). `--pid-file` is always
            // `{name}/vm.pid`, so the extra component is expected there.
            if flag == "--rootfs" && components.next().is_some() {
                continue;
            }
            return Some(name.to_string());
        }
    }
    None
}

/// Pure parser used by [`discover_running_worker_names_from_ps`]. Exposed for
/// testing with synthetic cmdline output and arbitrary path prefixes. Each
/// input line is one process's argv joined by spaces.
fn discover_running_worker_names_from_ps_output(
    ps_output: &str,
    workers_prefix: &std::path::Path,
    managed_prefix: &std::path::Path,
) -> Vec<String> {
    use std::collections::BTreeSet;
    let mut names = BTreeSet::new();
    for line in ps_output.lines() {
        if let Some(name) = extract_worker_name_from_cmdline(line, workers_prefix, managed_prefix) {
            names.insert(name);
        }
    }
    names.into_iter().collect()
}

/// Pure form of [`find_worker_pids_from_ps`]: every PID whose cmdline
/// resolves to `name`, in process-table order. Exposed for testing.
fn find_worker_pids_in_processes(
    processes: &[(u32, String)],
    name: &str,
    workers_prefix: &std::path::Path,
    managed_prefix: &std::path::Path,
) -> Vec<u32> {
    processes
        .iter()
        .filter_map(|(pid, cmdline)| {
            match extract_worker_name_from_cmdline(cmdline, workers_prefix, managed_prefix) {
                Some(n) if n == name => Some(*pid),
                _ => None,
            }
        })
        .collect()
}

/// Pure parser used by [`find_worker_pid_from_ps`]. Returns the first PID
/// whose cmdline resolves to `name`. Exposed for testing.
fn find_worker_pid_in_processes(
    processes: &[(u32, String)],
    name: &str,
    workers_prefix: &std::path::Path,
    managed_prefix: &std::path::Path,
) -> Option<u32> {
    find_worker_pids_in_processes(processes, name, workers_prefix, managed_prefix)
        .first()
        .copied()
}

/// Every live PID whose cmdline resolves to `name` — plural sibling of
/// [`find_worker_pid_from_ps`] for callers that must handle duplicate VMs
/// (e.g. two overlapping boots of the same managed worker).
pub fn find_worker_pids_from_ps(name: &str) -> Vec<u32> {
    let processes = collect_processes();
    if processes.is_empty() {
        return Vec::new();
    }
    let home = dirs::home_dir().unwrap_or_default();
    let workers_prefix = home.join(".iii/workers");
    let managed_prefix = home.join(".iii/managed");
    find_worker_pids_in_processes(&processes, name, &workers_prefix, &managed_prefix)
}

/// One process's argv-joined cmdline, by PID. Cheap single-PID lookup — NOT a
/// full process-table scan — because the identity cross-check runs on every
/// `is_worker_running` call (list loops, status --watch ticks).
#[cfg(target_os = "linux")]
fn process_cmdline(pid: u32) -> Option<String> {
    let bytes = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    if bytes.is_empty() {
        // Zombies and kernel threads have an empty cmdline: identity can't
        // be judged, only liveness (which the caller already checked).
        return None;
    }
    let line = String::from_utf8_lossy(&bytes).replace('\0', " ");
    let trimmed = line.trim_end().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(target_os = "macos")]
fn process_cmdline(pid: u32) -> Option<String> {
    let output = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "args="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!line.is_empty()).then_some(line)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn process_cmdline(_pid: u32) -> Option<String> {
    None
}

/// Identity cross-check for a pidfile PID. `Some(true)` when `pid`'s cmdline
/// says it runs `worker_name`, `Some(false)` when it demonstrably hosts
/// something else (the PID number was recycled by an unrelated process),
/// `None` when the cmdline can't be read so identity can't be judged.
fn pid_identity_matches(pid: u32, worker_name: &str) -> Option<bool> {
    let cmdline = process_cmdline(pid)?;
    let home = dirs::home_dir().unwrap_or_default();
    let workers_prefix = home.join(".iii/workers");
    let managed_prefix = home.join(".iii/managed");
    Some(cmdline_matches_worker(
        &cmdline,
        worker_name,
        &workers_prefix,
        &managed_prefix,
    ))
}

/// Pure form of [`pid_identity_matches`]. Deliberately MORE lenient than the
/// discovery matcher: identity asks "could this PID be worker N?", and a
/// false veto makes a live worker read as stopped AND unkillable by
/// `kill_stale_worker`. Beyond the discovery patterns, accept any argv token
/// exactly equal to `~/.iii/workers/{name}` — an interpreter-wrapped binary
/// worker (`#!/bin/sh` payload) runs as `/bin/sh ~/.iii/workers/{name} ...`,
/// where argv[0] is the interpreter.
fn cmdline_matches_worker(
    cmdline: &str,
    worker_name: &str,
    workers_prefix: &std::path::Path,
    managed_prefix: &std::path::Path,
) -> bool {
    if extract_worker_name_from_cmdline(cmdline, workers_prefix, managed_prefix).as_deref()
        == Some(worker_name)
    {
        return true;
    }
    let worker_path = workers_prefix.join(worker_name);
    cmdline
        .split_whitespace()
        .any(|tok| std::path::Path::new(tok) == worker_path)
}

/// Returns `true` if the worker has a valid PID file, the process is alive,
/// and — when the process table is readable — the PID actually belongs to
/// this worker. A stale pidfile whose PID number has been recycled by an
/// unrelated process must not read as alive (MOT-3931).
pub fn is_worker_running(worker_name: &str) -> bool {
    let home = dirs::home_dir().unwrap_or_default();
    let oci_pid = home.join(".iii/managed").join(worker_name).join("vm.pid");
    let bin_pid = home.join(".iii/pids").join(format!("{}.pid", worker_name));

    for pid_file in [oci_pid, bin_pid] {
        if let Some(pid) = read_pid(&pid_file) {
            // Check if process is alive (signal 0 = existence check).
            #[cfg(unix)]
            {
                use nix::sys::signal::kill;
                use nix::unistd::Pid;
                if kill(Pid::from_raw(pid as i32), None).is_ok()
                    // `Some(false)` = the PID is alive but demonstrably not
                    // this worker (recycled). `None` = can't enumerate
                    // processes; fall back to trusting the pidfile.
                    && pid_identity_matches(pid, worker_name) != Some(false)
                {
                    return true;
                }
            }
            #[cfg(not(unix))]
            {
                let _ = pid;
                // On non-Unix, assume running if PID file exists.
                return true;
            }
        }
    }
    false
}

/// Probes `127.0.0.1:{port}` to check whether the engine is listening.
/// Uses a 200ms timeout to avoid blocking the CLI.
///
/// Callers that don't already know the port should resolve via
/// `super::config_file::manager_port()`; those who already hold a port
/// (e.g. after a user passed `--port`) should use it directly so an
/// override isn't silently ignored.
pub fn is_engine_running_on(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        std::time::Duration::from_millis(200),
    )
    .is_ok()
}

/// Convenience for call sites without a known port: resolves the
/// `iii-worker-manager` port from config.yaml (or falls back to
/// `DEFAULT_PORT`) and probes it.
pub fn is_engine_running() -> bool {
    is_engine_running_on(super::config_file::manager_port())
}

/// Absolute path to a worker's managed directory: `~/.iii/managed/{name}`.
/// Single source of truth for the managed-dir scheme so call sites can't
/// drift apart.
pub fn managed_worker_dir(worker_name: &str) -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".iii/managed")
        .join(worker_name)
}

/// Appends the `.iii-prepared` marker suffix to an already-resolved managed
/// dir: `{managed_dir}/var/.iii-prepared`. Prefer this at call sites that
/// already hold a `managed_dir` (e.g. one built behind a strict `home_dir()`
/// guard) so the marker inherits that resolution instead of recomputing
/// `home_dir()` with the weaker `unwrap_or_default()` fallback below.
pub fn prepared_marker_in(managed_dir: &std::path::Path) -> std::path::PathBuf {
    managed_dir.join("var").join(".iii-prepared")
}

/// Absolute path to a worker's `.iii-prepared` marker:
/// `~/.iii/managed/{name}/var/.iii-prepared`. The marker gates the in-VM
/// setup_cmd/install_cmd in `build_libkrun_local_script`; if it drifts or
/// survives a `--force`, a changed lock file silently reuses stale deps
/// (MOT-3585). Keeping the path in one helper is what prevents that drift.
/// Use at call sites that only have a worker name; otherwise prefer
/// `prepared_marker_in` with an already-resolved `managed_dir`.
pub fn prepared_marker_path(worker_name: &str) -> std::path::PathBuf {
    prepared_marker_in(&managed_worker_dir(worker_name))
}

/// Deletes local artifacts for a worker (binary dir or OCI image dir).
/// Returns the number of bytes freed, or 0 if nothing was found.
///
/// Defense-in-depth: `worker_name` is joined into `~/.iii/...` paths
/// that get `remove_dir_all`'d. `Path::join` preserves `..` components,
/// so an unvalidated traversal name would delete attacker-chosen
/// directories under the user's HOME. Callers are expected to have
/// validated, but we re-check here so the sink itself is safe.
pub fn delete_worker_artifacts(worker_name: &str) -> u64 {
    if let Err(msg) = super::registry::validate_worker_name(worker_name) {
        eprintln!(
            "  {} refusing to delete artifacts for invalid worker name: {}",
            "warning:".yellow(),
            msg
        );
        return 0;
    }
    let home = dirs::home_dir().unwrap_or_default();
    let mut freed: u64 = 0;

    // Binary worker: ~/.iii/workers/{name}/
    let binary_dir = home.join(".iii/workers").join(worker_name);
    if binary_dir.is_dir() {
        freed += dir_size(&binary_dir);
        if let Err(e) = std::fs::remove_dir_all(&binary_dir) {
            eprintln!(
                "  {} Failed to remove {}: {}",
                "warning:".yellow(),
                binary_dir.display(),
                e
            );
        }
    } else if binary_dir.is_file() {
        // Legacy: some binary workers are a single file, not a directory
        freed += std::fs::metadata(&binary_dir).map(|m| m.len()).unwrap_or(0);
        if let Err(e) = std::fs::remove_file(&binary_dir) {
            eprintln!(
                "  {} Failed to remove {}: {}",
                "warning:".yellow(),
                binary_dir.display(),
                e
            );
        }
    }

    // OCI worker: look up image from config.yaml, compute hash, delete ~/.iii/images/{hash}/
    if let Some((image_ref, _)) = super::config_file::get_worker_start_info(worker_name) {
        let image_dir = image_cache_dir(&image_ref);
        if image_dir.is_dir() {
            freed += dir_size(&image_dir);
            if let Err(e) = std::fs::remove_dir_all(&image_dir) {
                eprintln!(
                    "  {} Failed to remove {}: {}",
                    "warning:".yellow(),
                    image_dir.display(),
                    e
                );
            }
        }
    }

    // Local-path worker: ~/.iii/managed/{name}/. This is a DISTINCT path from
    // the OCI image cache (~/.iii/images/{hash}/), so there is no double-count
    // to guard against. Always remove it when present — leaving it behind on
    // --force strands the `.iii-prepared` marker and the /var/iii/deps caches,
    // which silently skips the in-VM dependency reinstall (MOT-3585).
    let managed_dir = home.join(".iii/managed").join(worker_name);
    if managed_dir.is_dir() {
        freed += dir_size(&managed_dir);
        if let Err(e) = std::fs::remove_dir_all(&managed_dir) {
            eprintln!(
                "  {} Failed to remove {}: {}",
                "warning:".yellow(),
                managed_dir.display(),
                e
            );
        }
    }

    // Bundle worker: ~/.iii/workers-bundle/{name}/. Without this branch,
    // `iii worker remove foo` and `iii worker clear` would silently leak
    // the bundle install dir: nothing references it anymore, but the
    // machine-wide payload would sit on disk until a re-add replaces it.
    let bundle_dir = super::config_file::bundle_worker_path(worker_name);
    if bundle_dir.is_dir() {
        freed += dir_size(&bundle_dir);
        if let Err(e) = std::fs::remove_dir_all(&bundle_dir) {
            eprintln!(
                "  {} Failed to remove {}: {}",
                "warning:".yellow(),
                bundle_dir.display(),
                e
            );
        }
    }
    // NOTE: do NOT unlink the per-worker fslock file here. The lock
    // file is tiny (~64 bytes) and can persist forever without harm.
    // Removing it races with concurrent installers blocked at
    // `LockFile::open` / `lock_with_pid`: process A unlinks the file,
    // process C creates a NEW file at the same path and acquires its
    // lock against a different inode while process B still believes
    // it holds the original inode's lock. Both B and C then race
    // through atomic_install.

    freed
}

/// Computes the cache directory path for an OCI image reference.
/// Uses the first 8 bytes of SHA-256 of the image ref as the directory name.
fn image_cache_dir(image_ref: &str) -> std::path::PathBuf {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(image_ref.as_bytes());
    let hash = hex::encode(&hasher.finalize()[..8]);
    dirs::home_dir()
        .unwrap_or_default()
        .join(".iii/images")
        .join(hash)
}

/// Recursively computes the total size of a directory in bytes.
fn dir_size(path: &std::path::Path) -> u64 {
    let mut total: u64 = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let meta = entry.metadata();
            if let Ok(m) = meta {
                if m.is_dir() {
                    total += dir_size(&entry.path());
                } else {
                    total += m.len();
                }
            }
        }
    }
    total
}

pub async fn handle_managed_stop(worker_name: &str) -> i32 {
    if let Err(e) = super::registry::validate_worker_name(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }
    let home = dirs::home_dir().unwrap_or_default();
    let oci_pidfile = home.join(".iii/managed").join(worker_name).join("vm.pid");
    let bin_pidfile = home.join(".iii/pids").join(format!("{}.pid", worker_name));

    // Is this name known at all? Check every evidence source we have: config,
    // managed artifacts, binary workers dir, pidfiles. If none of those apply
    // and no live process exists, it's a typo -- exit 1 with "not found" so
    // automation doesn't confuse that with "already stopped."
    let in_config_yaml = super::config_file::list_worker_names()
        .iter()
        .any(|n| n == worker_name);
    let managed_dir_exists = home.join(".iii/managed").join(worker_name).is_dir();
    let binary_exists = home.join(".iii/workers").join(worker_name).exists();
    let worker_known = in_config_yaml
        || managed_dir_exists
        || binary_exists
        || oci_pidfile.exists()
        || bin_pidfile.exists();

    // Reject the well-defined "config worker explicitly listed in config.yaml"
    // case -- the engine owns those, the worker CLI cannot stop them. We only
    // reject when the name is genuinely listed in config; the resolver also
    // returns Config as the no-match fallthrough, which we want to treat as
    // an orphan candidate instead.
    if in_config_yaml
        && matches!(
            super::config_file::resolve_worker_type(worker_name),
            ResolvedWorkerType::Config
        )
    {
        eprintln!(
            "{} Cannot stop '{}': config workers run inside the engine and cannot be stopped individually",
            "error:".red(),
            worker_name
        );
        return 1;
    }

    // Locate the worker's PID via three evidence tiers, in order:
    // 1. ~/.iii/managed/{name}/vm.pid       (OCI/VM/local-path)
    // 2. ~/.iii/pids/{name}.pid             (binary)
    // 3. live `ps` scan                     (orphan, or stale pidfile)
    //
    // Pidfiles are only trusted when the recorded PID is actually alive. A
    // stale pidfile (process crashed without cleanup, or PID got recycled)
    // must fall through to the ps scan — otherwise we'd either signal an
    // unrelated recycled PID or miss a restarted orphan worker.
    let oci_live_pid = oci_pidfile
        .exists()
        .then(|| read_pid(&oci_pidfile).filter(|&p| is_pid_alive(p)))
        .flatten();
    let bin_live_pid = bin_pidfile
        .exists()
        .then(|| read_pid(&bin_pidfile).filter(|&p| is_pid_alive(p)))
        .flatten();

    let mode = if let Some(pid) = oci_live_pid {
        StopMode::Managed {
            pid,
            pidfile: Some(oci_pidfile),
        }
    } else if let Some(pid) = bin_live_pid {
        StopMode::Binary {
            pid,
            pidfile: Some(bin_pidfile),
        }
    } else if let Some(pid) = find_worker_pid_from_ps(worker_name) {
        // Either no pidfile on disk, or the pidfile is stale (dead PID).
        // Either way, ps found a live process for this worker — treat as
        // orphan. Carry any stale pidfile along so it gets cleaned up.
        let stale_pidfile = if oci_pidfile.exists() {
            Some(oci_pidfile)
        } else if bin_pidfile.exists() {
            Some(bin_pidfile)
        } else {
            None
        };
        let is_managed = home.join(".iii/managed").join(worker_name).is_dir();
        if is_managed {
            StopMode::Managed {
                pid,
                pidfile: stale_pidfile,
            }
        } else {
            StopMode::Binary {
                pid,
                pidfile: stale_pidfile,
            }
        }
    } else if !worker_known {
        eprintln!(
            "{} Worker '{}' not found. Run `iii worker list` to see known workers.",
            "error:".red(),
            worker_name,
        );
        return 1;
    } else {
        // VM died out-of-band but the watcher sidecar may still be
        // alive holding watch.pid — if we return here without reaping
        // it, the watcher will keep firing on file changes and try to
        // respawn a VM that nothing is tracking. Tear it down before
        // reporting "already stopped."
        reap_source_watcher(worker_name).await;
        eprintln!("  {} {} already stopped", "✓".green(), worker_name.bold());
        return 0;
    };

    eprintln!("  Stopping {}...", worker_name.bold());

    match mode {
        StopMode::Managed { pid, pidfile } => {
            // Tear down the source watcher sidecar first so it doesn't
            // observe the VM shutdown as a file event and try to restart.
            reap_source_watcher(worker_name).await;

            // Ask the in-VM supervisor to shut its child down cleanly.
            // The supervisor exits on success, which triggers libkrun's
            // poweroff path, which is faster and cleaner than a bare
            // SIGTERM to the __vm-boot process. We still fall through
            // to adapter.stop below — if the supervisor wasn't reachable
            // (binary missing, channel dead), that's the authoritative
            // teardown; if the shutdown succeeded, adapter.stop's
            // SIGTERM becomes a no-op against an already-exiting VM.
            if let Err(e) = super::supervisor_ctl::request_shutdown(worker_name).await {
                tracing::debug!(
                    worker = %worker_name,
                    error = %e,
                    "supervisor shutdown unavailable, falling through to SIGTERM"
                );
            }

            let adapter = super::worker_manager::create_adapter("libkrun");
            let _ = adapter.stop(&pid.to_string(), 10).await;
            if let Some(f) = pidfile {
                let _ = std::fs::remove_file(&f);
            }
        }
        StopMode::Binary { pid, pidfile } => {
            kill_pid_with_grace(pid).await;
            if let Some(f) = pidfile {
                let _ = std::fs::remove_file(&f);
            }
        }
    }

    eprintln!("  {} {} stopped", "✓".green(), worker_name.bold());
    0
}

/// Internal stop dispatch. The path the PID was discovered through dictates
/// how we terminate it (libkrun adapter for VMs, raw signals for binaries) and
/// whether we have an on-disk pidfile to clean up afterwards.
enum StopMode {
    Managed {
        pid: u32,
        pidfile: Option<std::path::PathBuf>,
    },
    Binary {
        pid: u32,
        pidfile: Option<std::path::PathBuf>,
    },
}

/// Reads a PID file, returning `Some(pid)` when contents parse as `u32`.
///
/// Thin alias for [`super::pidfile::read_pid`] — the hardened reader
/// lives in the shared module alongside `write_pid_file` so every
/// pidfile I/O path goes through the same O_NOFOLLOW + uid-ownership
/// check. See the `pidfile` module docstring for the full attacker
/// model.
fn read_pid(path: &std::path::Path) -> Option<u32> {
    super::pidfile::read_pid(path)
}

/// Returns `true` if `pid` refers to a live process. Uses signal 0 as a
/// non-destructive existence probe on Unix; assumes alive on platforms
/// without nix signals (the stop path will discover failure on real kill).
///
/// Used by the stop path to distinguish fresh pidfiles from stale ones so
/// a dead/recycled PID cannot short-circuit the `ps` orphan scan.
#[cfg(unix)]
fn is_pid_alive(pid: u32) -> bool {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    kill(Pid::from_raw(pid as i32), None).is_ok()
}

#[cfg(not(unix))]
fn is_pid_alive(_pid: u32) -> bool {
    true
}

/// SIGTERM, brief grace period, then SIGKILL. Mirrors the original
/// binary-worker stop semantics. No-op on platforms without nix signals.
async fn kill_pid_with_grace(pid: u32) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;
        let target = Pid::from_raw(pid as i32);
        let _ = kill(target, Signal::SIGTERM);
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        let _ = kill(target, Signal::SIGKILL);
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        eprintln!(
            "{} Direct PID stop not supported on this platform",
            "error:".red()
        );
    }
}

/// Block up to 120s waiting for the worker to report ready, printing a live
/// status snapshot. Used by `iii worker start` when the user did not pass
/// --no-wait. Same contract as `iii worker add --wait`: on timeout we do NOT
/// fail the command (the process started successfully), we just inform the
/// user and let them poll with `iii worker status {name}`.
///
/// `port` is the engine's configured `iii-worker-manager` port so the
/// engine-liveness probe inside `watch_until_ready` targets the engine the
/// worker is actually talking to. Without this, users on a non-default
/// port would see "engine: stopped" until the wait timed out.
async fn wait_for_ready(worker_name: &str, port: u16) {
    let started = std::time::Instant::now();
    let final_status = super::status::watch_until_ready(
        worker_name,
        Some(std::time::Duration::from_secs(120)),
        port,
    )
    .await;
    let elapsed = started.elapsed();
    match final_status.phase {
        super::status::Phase::Ready => {
            eprintln!("  {} ready in {:.1}s", "✓".green(), elapsed.as_secs_f64());
        }
        _ => {
            eprintln!(
                "  {} not ready after {:.0}s.\n  \
                 Keep watching: iii worker status {}\n  \
                 Check logs:    iii worker logs {} -f\n  \
                 Engine running in a different directory or port? Target it \
                 directly: iii worker add {} --host <host:port>",
                "⚠".yellow(),
                elapsed.as_secs_f64(),
                worker_name,
                worker_name,
                worker_name,
            );
        }
    }
}

/// Starts a managed worker, pointing it back at the engine on `port`.
///
/// `port` is the WebSocket port the spawned worker will connect to (used to
/// build `III_ENGINE_URL` for VM-based workers and to probe engine liveness).
/// Callers that do not supply an explicit port use `DEFAULT_PORT`. Compose
/// passes the managed engine URL to project workers directly; this helper is
/// retained for internal bundle preparation and sandbox operations.
pub async fn handle_managed_start(
    worker_name: &str,
    wait: bool,
    port: u16,
    config: Option<&std::path::Path>,
) -> i32 {
    if let Err(e) = super::registry::validate_worker_name(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }
    // Builtin workers are served in-process by the iii engine (see
    // engine/src/workers/config.rs factory registry). They have no external
    // process to spawn and must not be resolved via the remote registry.
    // Only treat this as success when the builtin is actually configured in
    // config.yaml AND the engine is running -- otherwise `start` is lying by
    // returning 0 for a no-op and automation thinks something booted.
    if is_any_builtin(worker_name) {
        if !super::config_file::worker_exists(worker_name) {
            eprintln!(
                "{} '{}' is a builtin but is not configured. Run `iii worker add {}` first.",
                "error:".red(),
                worker_name,
                worker_name,
            );
            return 1;
        }
        if !is_engine_running_on(port) {
            eprintln!(
                "{} '{}' is a builtin served by the iii engine, but the engine isn't running.\n  \
                 Start the engine:  iii",
                "error:".red(),
                worker_name,
            );
            return 1;
        }
        eprintln!(
            "  '{}' is a builtin worker — served by the iii engine process.",
            worker_name,
        );
        return 0;
    }
    let local_outcome = match super::config_file::resolve_worker_type(worker_name) {
        ResolvedWorkerType::Oci { image, env } => {
            if config.is_some() {
                tracing::warn!(
                    worker = %worker_name,
                    "--config ignored for OCI workers (requires VM-mount support)"
                );
            }
            let locked =
                match super::lockfile::WorkerLockfile::read_from(super::lockfile::lockfile_path())
                    .and_then(|lock| {
                        lock.workers
                            .get(worker_name)
                            .cloned()
                            .ok_or_else(|| format!("iii.lock is missing worker {worker_name}"))
                    }) {
                    Ok(locked) => locked,
                    Err(error) => {
                        eprintln!("{} {error}", "error:".red());
                        return 1;
                    }
                };
            let locked_image = match locked.source {
                Some(super::lockfile::LockedSource::Image { image }) => image,
                _ => {
                    eprintln!(
                        "{} iii.lock worker {} does not contain an OCI artifact",
                        "error:".red(),
                        worker_name
                    );
                    return 1;
                }
            };
            if image != locked_image {
                eprintln!(
                    "{} config.yaml image for {} does not match the digest-pinned OCI artifact in iii.lock; reinstall the Registry package",
                    "error:".red(),
                    worker_name
                );
                return 1;
            }
            let worker_def = WorkerDef::Managed {
                image: locked_image,
                env,
                resources: None,
            };
            StartOutcome::Exit(start_oci_worker(worker_name, &worker_def, port).await)
        }
        ResolvedWorkerType::Bundle { worker_path } => {
            // Bundle workers run through the local-worker libkrun rails
            // via `start_bundle_worker`, which is identical to
            // `start_local_worker` except the host-side source watcher
            // is suppressed (immutable install).
            if config.is_some() {
                tracing::warn!(
                    worker = %worker_name,
                    "--config ignored for bundle workers"
                );
            }
            let path_str = worker_path.to_string_lossy().to_string();
            StartOutcome::Exit(
                super::local_worker::start_bundle_worker(worker_name, &path_str, port).await,
            )
        }
        ResolvedWorkerType::Binary { binary_path } => {
            StartOutcome::Exit(start_binary_worker(worker_name, &binary_path, config, port).await)
        }
        ResolvedWorkerType::Config => StartOutcome::FallThrough,
    };
    if let StartOutcome::Exit(rc) = local_outcome {
        return finish_start(worker_name, rc, wait, port).await;
    }

    // Missing artifacts are restored only from the immutable descriptor and
    // version already pinned in iii.lock. `start` never consults the legacy
    // metadata endpoints and never chooses a new version.
    let lockfile = match super::lockfile::WorkerLockfile::read_from(super::lockfile::lockfile_path())
    {
        Ok(lockfile) => lockfile,
        Err(error) => {
            eprintln!("{} {error}", "error:".red());
            return 1;
        }
    };
    let Some(locked) = lockfile.workers.get(worker_name) else {
        eprintln!(
            "{} Worker '{}' is not installed. Run `iii worker add {}` first.",
            "error:".red(),
            worker_name,
            worker_name
        );
        return 1;
    };
    handle_descriptor_registry_add(
        worker_name,
        Some(&locked.version),
        false,
        false,
        false,
        wait,
    )
    .await
}

/// Classifies what `handle_managed_start`'s local-resolution branch wants the
/// caller to do next: either return an exit code straight to the user, or
/// fall through to the remote-registry path. Introduced to replace an
/// `i32::MIN` sentinel that overloaded the exit-code type as a control token.
enum StartOutcome {
    Exit(i32),
    FallThrough,
}

/// Shared tail for every successful start path: wait (if requested) then
/// emit the machine-readable worker name on stdout per the module output
/// contract. Keeping this in one place prevents the stdout contract from
/// drifting across the four call sites that used to inline it.
async fn finish_start(worker_name: &str, rc: i32, wait: bool, port: u16) -> i32 {
    if rc == 0 && wait {
        wait_for_ready(worker_name, port).await;
    }
    rc
}

/// Stop (if running) and start a worker. Idempotent: workers that aren't
/// running just get started. We delegate to the existing stop/start paths
/// rather than duplicating the libkrun teardown / pid-discovery logic.
///
/// Stop is invoked unconditionally so its three-tier PID discovery (OCI
/// pidfile, binary pidfile, `ps` scan) can catch orphan processes whose
/// pidfiles are missing or stale. `is_worker_running` only consults
/// pidfiles, so gating on it would let those orphans slip through and
/// start would then spawn a duplicate. Stop failures are logged but do
/// NOT abort the restart -- the most common reason stop "fails" here is
/// "already not running," which returns 0. Start's exit code becomes the
/// command's exit code.
pub async fn handle_managed_restart(
    worker_name: &str,
    wait: bool,
    port: Option<u16>,
    config: Option<&std::path::Path>,
) -> i32 {
    // No explicit --port: target the engine config.yaml actually points at
    // (honors engine-exported III_CONFIG_PATH), not the compiled-in default.
    let port = port.unwrap_or_else(super::config_file::manager_port);
    if let Err(e) = super::registry::validate_worker_name(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }

    eprintln!("  Restarting {}...", worker_name.bold());
    let stop_rc = handle_managed_stop(worker_name).await;
    if stop_rc != 0 {
        eprintln!(
            "  {} stop exited {} -- continuing with start",
            "warning:".yellow(),
            stop_rc
        );
    }

    handle_managed_start(worker_name, wait, port, config).await
}

async fn start_oci_worker(worker_name: &str, worker_def: &WorkerDef, port: u16) -> i32 {
    if let Err(e) = super::firmware::download::ensure_libkrunfw().await {
        tracing::warn!(error = %e, "failed to ensure libkrunfw availability");
    }

    if !super::worker_manager::libkrun::libkrun_available() {
        eprintln!(
            "{} libkrunfw is not available.\n  \
             Rebuild with --features embed-libkrunfw or place libkrunfw in ~/.iii/lib/",
            "error:".red()
        );
        return 1;
    }

    let adapter = super::worker_manager::create_adapter("libkrun");
    eprintln!("  Starting {} (OCI)...", worker_name.bold());

    let engine_url = format!("ws://localhost:{}", port);
    let spec = build_container_spec(worker_name, worker_def, &engine_url);

    let pid_file = dirs::home_dir()
        .unwrap_or_default()
        .join(".iii/managed")
        .join(worker_name)
        .join("vm.pid");
    if let Some(pid) = read_pid(&pid_file) {
        let pid_str = pid.to_string();
        let _ = adapter.stop(&pid_str, 5).await;
        let _ = adapter.remove(&pid_str).await;
    }

    match adapter.start(&spec).await {
        Ok(_) => {
            eprintln!("  {} {} started", "✓".green(), worker_name.bold());
            0
        }
        Err(e) => {
            eprintln!("{} Start failed: {}", "error:".red(), e);
            1
        }
    }
}

async fn start_binary_worker(
    worker_name: &str,
    binary_path: &std::path::Path,
    config: Option<&std::path::Path>,
    port: u16,
) -> i32 {
    // Kill any stale process from a previous engine run
    kill_stale_worker(worker_name).await;

    // Create log directory: ~/.iii/logs/{name}/
    let logs_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".iii/logs")
        .join(worker_name);
    if let Err(e) = std::fs::create_dir_all(&logs_dir) {
        eprintln!("{} Failed to create logs dir: {}", "error:".red(), e);
        return 1;
    }

    // APPEND, not truncate — the parent `iii-worker start` and engine
    // already wrote progress lines to these files; truncating wipes
    // everything the wait UI tails for visibility. Same rationale as
    // the libkrun path.
    let mut open_opts = std::fs::OpenOptions::new();
    open_opts.create(true).append(true);
    let stdout_file = match open_opts.open(logs_dir.join("stdout.log")) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{} Failed to open stdout log: {}", "error:".red(), e);
            return 1;
        }
    };
    let stderr_file = match open_opts.open(logs_dir.join("stderr.log")) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{} Failed to open stderr log: {}", "error:".red(), e);
            return 1;
        }
    };

    eprintln!("  Starting {} (binary)...", worker_name.bold());

    let mut cmd = tokio::process::Command::new(binary_path);
    if let Some(cfg_path) = config {
        cmd.arg("--config").arg(cfg_path);
    }
    // The config.yaml entry name, so SDKs self-report the managed identity;
    // engine truth (`iii worker status`/`list`) matches connections by this
    // name.
    cmd.env("III_WORKER_NAME", worker_name);
    // Engine WS URL — the documented worker contract (III_URL/III_ENGINE_URL
    // are engine-set). Without these a registry binary on a non-default
    // manager port dials its compiled-in ws://127.0.0.1:49134 default and
    // strands retrying (iii-hq/workers#526).
    let engine_url = format!("ws://127.0.0.1:{}", port);
    cmd.env("III_ENGINE_URL", &engine_url);
    cmd.env("III_URL", &engine_url);
    cmd.stdout(stdout_file).stderr(stderr_file);

    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(|| {
            nix::unistd::setsid()
                .map_err(|e| std::io::Error::other(format!("setsid failed: {e}")))?;
            Ok(())
        });
    }

    match cmd.spawn() {
        Ok(child) => {
            // Write PID file for stop/status tracking.
            // Use ~/.iii/pids/{name}.pid — binary workers occupy ~/.iii/workers/{name}
            // as a file (the executable), so we cannot create a subdirectory there.
            let pid_dir = dirs::home_dir().unwrap_or_default().join(".iii/pids");
            let _ = std::fs::create_dir_all(&pid_dir);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&pid_dir, std::fs::Permissions::from_mode(0o700));
            }
            if let Some(pid) = child.id() {
                let pid_path = pid_dir.join(format!("{}.pid", worker_name));
                // Route through the shared hardened writer: O_NOFOLLOW +
                // atomic 0o600 mode at O_CREAT. A plain fs::write here
                // would follow a pre-planted symlink at the target and
                // a post-hoc set_permissions leaves a create/chmod
                // TOCTOU window. See cli/pidfile.rs for rationale.
                super::pidfile::write_pid_file(&pid_path, pid);
            }
            let pid_display = child
                .id()
                .map(|p| p.to_string())
                .unwrap_or_else(|| "?".into());
            eprintln!(
                "  {} {} started (pid: {})",
                "✓".green(),
                worker_name.bold(),
                pid_display
            );
            0
        }
        Err(e) => {
            eprintln!("{} Failed to start binary worker: {}", "error:".red(), e);
            1
        }
    }
}

/// Pick the log directory with the most recently modified, non-empty log file.
/// Returns `None` when no candidate contains any usable log content.
fn file_len(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

async fn read_new_bytes(path: &std::path::Path, offset: u64, is_stderr: bool) -> u64 {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};

    let mut file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(_) => return offset,
    };

    let len = match file.metadata().await {
        Ok(m) => m.len(),
        Err(_) => return offset,
    };

    let offset = if len < offset { 0 } else { offset };

    if len == offset {
        return offset;
    }

    if file.seek(std::io::SeekFrom::Start(offset)).await.is_err() {
        return offset;
    }

    let mut buf = Vec::new();
    if file.read_to_end(&mut buf).await.is_err() {
        return offset;
    }

    let text = String::from_utf8_lossy(&buf);
    let ends_with_newline = text.ends_with('\n');
    let mut lines: Vec<&str> = text.lines().collect();

    let consumed = if ends_with_newline {
        buf.len() as u64
    } else if lines.len() > 1 {
        let last = lines.pop().unwrap();
        buf.len() as u64 - last.len() as u64
    } else {
        0
    };

    for line in &lines {
        if is_stderr {
            eprintln!("{}", line);
        } else {
            println!("{}", line);
        }
    }

    offset + consumed
}

async fn follow_logs(stdout_path: &std::path::Path, stderr_path: &std::path::Path) -> i32 {
    let mut stdout_offset = file_len(stdout_path);
    let mut stderr_offset = file_len(stderr_path);
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    loop {
        tokio::select! {
            _ = &mut ctrl_c => break,
            _ = interval.tick() => {
                stdout_offset = read_new_bytes(stdout_path, stdout_offset, false).await;
                stderr_offset = read_new_bytes(stderr_path, stderr_offset, true).await;
            }
        }
    }
    0
}

async fn follow_single_log(path: &std::path::Path) -> i32 {
    let mut offset = file_len(path);
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    loop {
        tokio::select! {
            _ = &mut ctrl_c => break,
            _ = interval.tick() => {
                offset = read_new_bytes(path, offset, false).await;
            }
        }
    }
    0
}

pub async fn handle_managed_logs(worker_name: &str, follow: bool) -> i32 {
    if let Err(e) = super::registry::validate_worker_name(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }
    let home = dirs::home_dir().unwrap_or_default();

    // Check all possible log locations and prefer the one with the most
    // recently modified, non-empty log files. Shared with the daemon's
    // `worker::logs` trigger so both surfaces read the same directory.
    let candidates = crate::core::logs::candidate_log_dirs(&home, worker_name);
    let unified_logs_dir = candidates[0].clone();
    let logs_dir = crate::core::logs::pick_best_logs_dir(&candidates).unwrap_or(unified_logs_dir);

    let worker_dir = logs_dir.clone();

    let stdout_path = logs_dir.join("stdout.log");
    let stderr_path = logs_dir.join("stderr.log");

    let has_new_logs = stdout_path.exists() || stderr_path.exists();

    if has_new_logs {
        let mut found_content = false;

        // Read stderr.log first: it holds the host vm-boot subprocess's own
        // eprintln! output (e.g. "  Booting VM...") which fires BEFORE the
        // VM enters, so those lines are chronologically the oldest. stdout.log
        // is the VM's --console-output stream, which only starts producing
        // content once the guest is actually running.
        if let Ok(contents) = std::fs::read_to_string(&stderr_path)
            && !contents.is_empty()
        {
            found_content = true;
            let lines: Vec<&str> = contents.lines().collect();
            let start = if lines.len() > 100 {
                lines.len() - 100
            } else {
                0
            };
            for line in &lines[start..] {
                eprintln!("{}", line);
            }
        }

        if let Ok(contents) = std::fs::read_to_string(&stdout_path)
            && !contents.is_empty()
        {
            found_content = true;
            let lines: Vec<&str> = contents.lines().collect();
            let start = if lines.len() > 100 {
                lines.len() - 100
            } else {
                0
            };
            for line in &lines[start..] {
                println!("{}", line);
            }
        }

        if !found_content {
            eprintln!("  No logs available for {}", worker_name.bold());
        }

        if follow {
            return follow_logs(&stdout_path, &stderr_path).await;
        }

        return 0;
    }

    let old_log = worker_dir.join("vm.log");
    match std::fs::read_to_string(&old_log) {
        Ok(contents) => {
            if contents.is_empty() {
                eprintln!("  No logs available for {}", worker_name.bold());
            } else {
                let lines: Vec<&str> = contents.lines().collect();
                let start = if lines.len() > 100 {
                    lines.len() - 100
                } else {
                    0
                };
                for line in &lines[start..] {
                    println!("{}", line);
                }
            }

            if follow {
                return follow_single_log(&old_log).await;
            }

            0
        }
        Err(_) => {
            // No log files anywhere. A known worker that simply hasn't
            // produced logs yet is informational (exit 0, matching the
            // "No logs available" branches above); a name with no config
            // entry and no artifacts is a probable typo (exit 1).
            let known = super::config_file::worker_exists(worker_name)
                || home.join(".iii/managed").join(worker_name).is_dir()
                || home.join(".iii/workers").join(worker_name).exists();
            if known {
                eprintln!("  No logs available for {} yet", worker_name.bold());
                0
            } else {
                eprintln!(
                    "{} No logs found for '{}'. Run `iii worker list` to see known workers.",
                    "error:".red(),
                    worker_name,
                );
                1
            }
        }
    }
}
