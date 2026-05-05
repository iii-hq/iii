// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
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
use super::builtin_defaults::{get_builtin_default, is_any_builtin, resolve_builtin_version};
use super::config_file::ResolvedWorkerType;
use super::lifecycle::build_container_spec;
use super::registry::{
    BinaryWorkerResponse, MANIFEST_PATH, ResolvedWorkerGraph, WorkerInfoResponse,
    fetch_resolved_worker_graph, fetch_worker_info, parse_worker_input,
};
use super::worker_manager::state::WorkerDef;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub use super::local_worker::{handle_local_add, is_local_path, start_local_worker};

/// Fire `GET /download/{name}` for an engine worker so the registry increments
/// its telemetry counters. The endpoint returns 204 (no artifact); errors are
/// logged as warnings and never block the install.
async fn fire_engine_telemetry(name: &str, version: &str) {
    use super::registry::HTTP_CLIENT;

    let api_url =
        std::env::var("III_API_URL").unwrap_or_else(|_| "https://api.workers.iii.dev".to_string());
    let url = format!("{api_url}/download/{name}");

    match HTTP_CLIENT
        .get(&url)
        .query(&[("version", version)])
        .send()
        .await
    {
        Ok(resp) if resp.status().as_u16() == 204 => {}
        Ok(resp) => {
            eprintln!(
                "  {} telemetry for {} returned unexpected status {}",
                "warn:".yellow(),
                name,
                resp.status()
            );
        }
        Err(e) => {
            eprintln!(
                "  {} telemetry for {} failed: {}",
                "warn:".yellow(),
                name,
                e
            );
        }
    }
}

pub async fn handle_binary_add(
    worker_name: &str,
    response: &BinaryWorkerResponse,
    brief: bool,
) -> i32 {
    let target = binary_download::current_target();

    if !brief {
        eprintln!("  {} Resolved to binary v{}", "✓".green(), response.version);
    }

    // If the worker is already running, skip download entirely
    if is_worker_running(worker_name) {
        if !brief {
            eprintln!(
                "\n  {} Worker {} already running, skipping download",
                "✓".green(),
                worker_name.bold(),
            );
        }
        return 0;
    }

    let binary_info = match response.binaries.get(target) {
        Some(info) => info,
        None => {
            eprintln!(
                "{} Platform '{}' is not supported for worker '{}'. Available: {}",
                "error:".red(),
                target,
                worker_name,
                response
                    .binaries
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            return 1;
        }
    };

    if !brief {
        eprintln!("  Downloading {}...", worker_name.bold());
    }
    let install_path =
        match binary_download::download_and_install_binary(worker_name, binary_info).await {
            Ok(path) => path,
            Err(e) => {
                eprintln!("{} {}", "error:".red(), e);
                return 1;
            }
        };

    if !brief {
        eprintln!("  {} Downloaded successfully", "✓".green());
        eprintln!("  {}: {}", "Name".bold(), worker_name);
        eprintln!("  {}: {}", "Version".bold(), response.version);
        eprintln!("  {}: {}", "Platform".bold(), target);
        if let Ok(metadata) = std::fs::metadata(&install_path) {
            eprintln!(
                "  {}: {:.1} MB",
                "Size".bold(),
                metadata.len() as f64 / 1_048_576.0
            );
        }
    }

    let config_yaml = binary_config_yaml(&response.config);

    if let Err(e) = super::config_file::append_worker(worker_name, config_yaml.as_deref()) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }

    if brief {
        eprintln!("        {} {}", "✓".green(), worker_name.bold());
    } else {
        eprintln!(
            "\n  {} Worker {} added to {}",
            "✓".green(),
            worker_name.bold(),
            "config.yaml".dimmed(),
        );

        // The engine's file watcher will detect the config change and
        // reload automatically — no need to start the worker here.
    }
    0
}

fn binary_config_yaml(config: &serde_json::Value) -> Option<String> {
    let config = match config {
        serde_json::Value::Null => return None,
        serde_json::Value::Object(map) if map.is_empty() => return None,
        serde_json::Value::Object(map) => map.get("config").unwrap_or(config),
        _ => config,
    };

    match config {
        serde_json::Value::Null => None,
        serde_json::Value::Object(map) if map.is_empty() => None,
        _ => {
            let yaml = serde_yaml::to_string(config).unwrap_or_default();
            let yaml = yaml
                .strip_prefix("---\n")
                .unwrap_or(&yaml)
                .trim_end()
                .to_string();
            if yaml.is_empty() || yaml == "{}" || yaml == "null" {
                None
            } else {
                Some(yaml)
            }
        }
    }
}

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

struct ProjectOperationLock {
    path: PathBuf,
}

impl ProjectOperationLock {
    fn acquire() -> Result<Self, String> {
        let path = PathBuf::from(".iii-worker.lock");
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                use std::io::Write as _;
                let _ = writeln!(file, "pid={}", std::process::id());
                Ok(Self { path })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(format!(
                "another iii worker operation is active (lock: {}). Wait for it to finish, \
                 or remove the lock if the process crashed.",
                path.display()
            )),
            Err(e) => Err(format!(
                "failed to acquire iii worker operation lock {}: {e}",
                path.display()
            )),
        }
    }
}

impl Drop for ProjectOperationLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

struct WorkerActivationLock {
    path: PathBuf,
}

impl WorkerActivationLock {
    fn acquire(name: &str) -> Result<Self, String> {
        super::registry::validate_worker_name(name)?;
        let dir = binary_download::binary_workers_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("failed to create worker install directory: {e}"))?;
        let path = dir.join(format!(".{name}.lock"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                use std::io::Write as _;
                let _ = writeln!(file, "pid={}", std::process::id());
                Ok(Self { path })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(format!(
                "worker `{name}` is being installed by another process (lock: {}). Wait and rerun \
                 `iii worker sync`.",
                path.display()
            )),
            Err(e) => Err(format!(
                "failed to acquire activation lock for worker `{name}`: {e}"
            )),
        }
    }
}

impl Drop for WorkerActivationLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
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
        let lockfile = match super::lockfile::WorkerLockfile::read_from(lock_path) {
            Ok(lockfile) => lockfile,
            Err(e) => {
                eprintln!("{} {}", "error:".red(), e);
                return 1;
            }
        };

        // Drift check: only active for locks that carry a manifest_hash
        // (i.e. written by Lane A or later). Legacy locks fall through
        // to verify unchanged so we don't regress existing CI pipelines.
        //
        // Each `iii.worker.yaml` state surfaces a distinct error so the
        // user sees what's actually wrong: a *missing* manifest reads
        // as `ManifestMissing`, a *malformed* one as `CorruptManifest`,
        // and a hash mismatch with no structural drift as
        // `LockInconsistent`. Only an actual content change reaches the
        // `Drift` variant — the one with actionable add/remove/change
        // attribution.
        if let Some(stored_hash) = &lockfile.manifest_hash {
            let err: Option<super::sync::SyncError> = match load_cwd_manifest_state() {
                ManifestState::Missing => Some(super::sync::SyncError::ManifestMissing {
                    lock_deps: lockfile.declared_dependencies.clone().unwrap_or_default(),
                }),
                ManifestState::Malformed(reason) => {
                    Some(super::sync::SyncError::CorruptManifest { reason })
                }
                ManifestState::Loaded(manifest_deps) => {
                    let current_hash = super::sync::compute_manifest_hash(&manifest_deps);
                    if current_hash != *stored_hash {
                        let lock_deps = lockfile.declared_dependencies.clone().unwrap_or_default();
                        match super::sync::detect_drift(&manifest_deps, &lock_deps) {
                            Some(report) => Some(super::sync::SyncError::Drift { report }),
                            None => Some(super::sync::SyncError::LockInconsistent),
                        }
                    } else {
                        None
                    }
                }
            };
            if let Some(err) = err {
                // stdout is reserved for worker names; all diagnostic
                // output goes to stderr.
                eprint!("{}", err.render());
                return 1;
            }
        }

        return handle_worker_verify(false).await;
    }

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

    let _operation_lock = match ProjectOperationLock::acquire() {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };

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
                ResolvedWorkerType::Local { .. } => Some((name.clone(), "local-path worker")),
                ResolvedWorkerType::Oci { .. } => Some((name.clone(), "direct OCI worker")),
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

    for (worker_name, worker_path) in local_worker_manifest_paths()? {
        let manifest_path = Path::new(&worker_path).join(super::project::WORKER_MANIFEST);
        let deps = super::project::load_manifest_dependencies(&manifest_path).map_err(|e| {
            format!(
                "{e}. Fix: correct `{}` and rerun `iii worker verify --strict`.",
                manifest_path.display()
            )
        })?;
        for (dependency, range) in deps {
            let locked = lockfile.workers.get(&dependency).ok_or_else(|| {
                format!(
                    "local worker `{worker_name}` declares dependency `{dependency}@{range}` in {}, but `{dependency}` is missing from iii.lock. \
                     Fix: run `iii worker add {}` to resolve and lock declared dependencies.",
                    manifest_path.display(),
                    worker_path
                )
            })?;
            version_satisfies_range(&locked.version, &range).map_err(|e| {
                format!(
                    "local worker `{worker_name}` declares dependency `{dependency}@{range}`, but iii.lock pins version {} ({e}). \
                     Fix: run `iii worker add {}` after confirming the dependency range.",
                    locked.version, worker_path
                )
            })?;
        }
    }

    Ok(())
}

fn local_worker_manifest_paths() -> Result<BTreeMap<String, String>, String> {
    let mut paths = BTreeMap::new();
    for name in super::config_file::list_worker_names_result()? {
        if let ResolvedWorkerType::Local { worker_path } =
            super::config_file::resolve_worker_type(&name)
        {
            paths.insert(name, worker_path);
        }
    }
    Ok(paths)
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
        ResolvedWorkerType::Local { .. } | ResolvedWorkerType::Oci { .. } => false,
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
        let graph = match fetch_resolved_worker_graph(name, Some("latest"), None).await {
            Ok(graph) => graph,
            Err(e) => {
                eprintln!("{} {}", "error:".red(), e);
                fail_count += 1;
                continue;
            }
        };

        let rc = handle_resolved_graph_add(&graph, false).await;
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

fn lockfile_from_graph(
    graph: &ResolvedWorkerGraph,
) -> Result<super::lockfile::WorkerLockfile, String> {
    let mut lock = super::lockfile::WorkerLockfile::default();

    for node in &graph.graph {
        let (worker_type, source) = match node.worker_type.as_str() {
            "engine" => (super::lockfile::LockedWorkerType::Engine, None),
            "binary" => {
                let binaries = node.binaries.as_ref().ok_or_else(|| {
                    format!("resolved binary worker '{}' has no binaries", node.name)
                })?;
                if binaries.is_empty() {
                    return Err(format!(
                        "resolved binary worker '{}' has no binary artifacts",
                        node.name
                    ));
                }
                let artifacts = binaries
                    .iter()
                    .map(|(target, binary)| {
                        binary_download::validate_locked_artifact_url(&binary.url).map_err(
                            |e| {
                                format!(
                                    "resolved binary worker '{}' target '{}' has unreplayable artifact URL: {}",
                                    node.name, target, e
                                )
                            },
                        )?;
                        Ok((
                            target.clone(),
                            super::lockfile::LockedBinaryArtifact {
                                url: binary.url.clone(),
                                sha256: binary.sha256.clone(),
                            },
                        ))
                    })
                    .collect::<Result<_, String>>()?;
                (
                    super::lockfile::LockedWorkerType::Binary,
                    Some(super::lockfile::LockedSource::Binary { artifacts }),
                )
            }
            "image" => (
                super::lockfile::LockedWorkerType::Image,
                Some(super::lockfile::LockedSource::Image {
                    image: node.image.clone().ok_or_else(|| {
                        format!("resolved image worker '{}' has no image", node.name)
                    })?,
                }),
            ),
            other => {
                return Err(format!(
                    "resolved worker '{}' has unsupported type '{}'",
                    node.name, other
                ));
            }
        };

        lock.workers.insert(
            node.name.clone(),
            super::lockfile::LockedWorker {
                version: node.version.clone(),
                worker_type,
                dependencies: node.dependencies.clone().into_iter().collect(),
                source,
            },
        );
    }

    Ok(lock)
}

fn print_resolved_tree(graph: &ResolvedWorkerGraph) {
    eprintln!("\n  Resolved worker graph");
    for node in &graph.graph {
        if node.name == graph.root.name {
            eprintln!("  {}@{}", node.name.bold(), node.version);
            for edge in graph.edges.iter().filter(|edge| edge.from == node.name) {
                let resolved = graph
                    .graph
                    .iter()
                    .find(|node| node.name == edge.to)
                    .map(|node| node.version.as_str())
                    .unwrap_or(edge.range.as_str());
                eprintln!(
                    "  └─ {} ({})",
                    format!("{}@{}", edge.to, resolved).dimmed(),
                    edge.range
                );
            }
        }
    }
}

struct ConfigYamlSnapshot {
    content: Option<String>,
}

impl ConfigYamlSnapshot {
    fn capture() -> Result<Self, String> {
        let path = std::path::Path::new("config.yaml");
        if !path.exists() {
            return Ok(Self { content: None });
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read config.yaml before graph install: {e}"))?;
        Ok(Self {
            content: Some(content),
        })
    }

    fn restore(&self) -> Result<(), String> {
        let path = std::path::Path::new("config.yaml");
        match &self.content {
            Some(content) => std::fs::write(path, content)
                .map_err(|e| format!("failed to restore config.yaml: {e}")),
            None => match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(format!("failed to remove config.yaml: {e}")),
            },
        }
    }

    fn restore_after_failure(&self) {
        if let Err(e) = self.restore() {
            eprintln!("{} {}", "error:".red(), e);
        }
    }
}

fn read_lockfile_or_default(
    path: &std::path::Path,
) -> Result<super::lockfile::WorkerLockfile, String> {
    if path.exists() {
        super::lockfile::WorkerLockfile::read_from(path)
    } else {
        Ok(super::lockfile::WorkerLockfile::default())
    }
}

fn write_engine_lock_entry(worker_name: &str, version: &str) -> Result<(), String> {
    let lock_path = super::lockfile::lockfile_path();
    let mut lockfile = read_lockfile_or_default(&lock_path)?;
    lockfile.workers.insert(
        worker_name.to_string(),
        super::lockfile::LockedWorker {
            version: version.to_string(),
            worker_type: super::lockfile::LockedWorkerType::Engine,
            dependencies: std::collections::BTreeMap::new(),
            source: None,
        },
    );
    lockfile.write_to(&lock_path)
}

fn persist_engine_worker_config_and_lock(
    worker_name: &str,
    version: &str,
    config_yaml: Option<&str>,
) -> Result<(), String> {
    let config_snapshot = ConfigYamlSnapshot::capture()?;
    if let Err(e) = super::config_file::append_worker(worker_name, config_yaml) {
        config_snapshot.restore_after_failure();
        return Err(e);
    }
    if let Err(e) = write_engine_lock_entry(worker_name, version) {
        config_snapshot.restore_after_failure();
        return Err(e);
    }
    Ok(())
}

/// Distinguishes the three states of `iii.worker.yaml` that drift
/// detection cares about. Collapsing `Missing` and `Malformed` into the
/// same fallback (as `load_cwd_manifest_dependencies` does for the
/// `iii worker add` write path) is fine for *populating* a fresh hash,
/// but it produces misleading "drift" output when those states arise
/// during `--frozen`. The `--frozen` path uses this enum directly so
/// each state surfaces with its own error variant.
pub(crate) enum ManifestState {
    Missing,
    Malformed(String),
    Loaded(std::collections::BTreeMap<String, String>),
}

pub(crate) fn load_cwd_manifest_state() -> ManifestState {
    let manifest_path = std::path::Path::new("iii.worker.yaml");
    if !manifest_path.exists() {
        return ManifestState::Missing;
    }
    match super::project::load_manifest_dependencies(manifest_path) {
        Ok(deps) => ManifestState::Loaded(deps),
        Err(e) => ManifestState::Malformed(e),
    }
}

/// Read the cwd `iii.worker.yaml` `dependencies:` block. Returns `None`
/// when the file is absent, empty, or has a null dependencies field.
/// Errors are downgraded to `None` with a stderr warning — missing or
/// malformed root manifests should not block a resolve that otherwise
/// succeeded; drift detection just doesn't light up for that project.
fn load_cwd_manifest_dependencies() -> Option<std::collections::BTreeMap<String, String>> {
    match load_cwd_manifest_state() {
        ManifestState::Loaded(deps) => Some(deps),
        ManifestState::Missing => None,
        ManifestState::Malformed(e) => {
            eprintln!(
                "  {} ignoring iii.worker.yaml for manifest_hash: {e}",
                "warning:".yellow()
            );
            None
        }
    }
}

/// Populate [`super::lockfile::WorkerLockfile::manifest_hash`] and
/// [`super::lockfile::WorkerLockfile::declared_dependencies`] from the
/// current cwd manifest. No-ops when the manifest is absent, leaving both
/// fields at whatever they were before (preserves previous writer's
/// values across incremental `iii worker add` runs).
fn populate_manifest_hash_fields(lockfile: &mut super::lockfile::WorkerLockfile) {
    let Some(deps) = load_cwd_manifest_dependencies() else {
        return;
    };
    lockfile.manifest_hash = Some(super::sync::compute_manifest_hash(&deps));
    lockfile.declared_dependencies = Some(deps);
}

async fn handle_resolved_graph_add(graph: &ResolvedWorkerGraph, brief: bool) -> i32 {
    for node in &graph.graph {
        if let Err(e) = super::registry::validate_worker_name(&node.name) {
            eprintln!(
                "{} invalid resolved worker '{}': {}",
                "error:".red(),
                node.name,
                e
            );
            return 1;
        }
    }

    let graph_lockfile = match lockfile_from_graph(graph).and_then(|lockfile| {
        lockfile.to_yaml()?;
        Ok(lockfile)
    }) {
        Ok(lockfile) => lockfile,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };

    let lock_path = super::lockfile::lockfile_path();
    let mut lockfile = match read_lockfile_or_default(&lock_path) {
        Ok(lockfile) => lockfile,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };

    let config_snapshot = match ConfigYamlSnapshot::capture() {
        Ok(snapshot) => snapshot,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };

    for node in &graph.graph {
        let rc = match node.worker_type.as_str() {
            "engine" => {
                // Engine workers are baked into the iii binary — nothing to download.
                // Fire GET /download/{name} anyway so the registry can count installs.
                let config_yaml = binary_config_yaml(&node.config);
                if let Err(e) =
                    super::config_file::append_worker(&node.name, config_yaml.as_deref())
                {
                    eprintln!("{} {}", "error:".red(), e);
                    config_snapshot.restore_after_failure();
                    return 1;
                }
                fire_engine_telemetry(&node.name, &node.version).await;
                if !brief {
                    eprintln!("  {} {} (engine, built-in)", "✓".green(), node.name.bold());
                }
                0
            }
            "binary" => {
                let response = BinaryWorkerResponse {
                    name: node.name.clone(),
                    version: node.version.clone(),
                    binaries: node.binaries.clone().unwrap_or_default(),
                    config: node.config.clone(),
                };
                handle_binary_add(&node.name, &response, brief).await
            }
            "image" => {
                let Some(image) = node.image.as_deref() else {
                    eprintln!(
                        "{} Resolved image worker '{}' has no image",
                        "error:".red(),
                        node.name
                    );
                    config_snapshot.restore_after_failure();
                    return 1;
                };
                handle_oci_pull_and_add(&node.name, image, brief).await
            }
            other => {
                eprintln!(
                    "{} Resolved worker '{}' has unsupported type '{}'",
                    "error:".red(),
                    node.name,
                    other
                );
                config_snapshot.restore_after_failure();
                return 1;
            }
        };

        if rc != 0 {
            config_snapshot.restore_after_failure();
            return rc;
        }
    }

    for (name, worker) in graph_lockfile.workers {
        lockfile.workers.insert(name, worker);
    }

    // Populate manifest_hash + declared_dependencies from the project's
    // root iii.worker.yaml (if present) so `iii worker sync --frozen` can
    // detect drift on the next run. Projects without a root manifest get
    // `None` for both fields, which preserves legacy behavior.
    //
    // Slice A.1 limitation: only the cwd manifest is scanned. Multi-worker
    // projects with manifests in subdirectories won't get aggregate
    // drift detection until Slice A.2 adds project-wide scanning.
    populate_manifest_hash_fields(&mut lockfile);

    if let Err(e) = lockfile.write_to(&lock_path) {
        eprintln!("{} {}", "error:".red(), e);
        config_snapshot.restore_after_failure();
        return 1;
    }

    if !brief {
        print_resolved_tree(graph);
        eprintln!("  {} Wrote {}", "✓".green(), "iii.lock".dimmed());
    }

    0
}

/// Merge N resolved graphs into a single graph. Nodes are deduped by name.
/// If the same name appears at different versions across graphs, returns an
/// error naming the conflicting dep and both versions — this is the cross-dep
/// version-conflict gate.
pub(crate) fn merge_resolved_graphs(
    graphs: Vec<(String, ResolvedWorkerGraph)>,
) -> Result<ResolvedWorkerGraph, String> {
    if graphs.is_empty() {
        return Err("merge_resolved_graphs: no graphs provided".to_string());
    }

    let mut nodes_by_name: std::collections::BTreeMap<String, super::registry::ResolvedWorker> =
        std::collections::BTreeMap::new();
    let mut edges: Vec<super::registry::ResolvedEdge> = Vec::new();
    let first_root = graphs[0].1.root.clone();

    for (origin, graph) in graphs {
        for node in graph.graph {
            if let Some(existing) = nodes_by_name.get(&node.name) {
                if existing.version != node.version {
                    return Err(format!(
                        "dependency `{name}` resolved to conflicting versions across declared deps: \
                         `{v1}` (from earlier graph) vs `{v2}` (from `{origin}`)",
                        name = node.name,
                        v1 = existing.version,
                        v2 = node.version,
                        origin = origin,
                    ));
                }
                // Same version — skip; first wins.
            } else {
                nodes_by_name.insert(node.name.clone(), node);
            }
        }
        edges.extend(graph.edges);
    }

    Ok(ResolvedWorkerGraph {
        root: first_root,
        target: None,
        graph: nodes_by_name.into_values().collect(),
        edges,
    })
}

/// Resolve every declared manifest dependency against the registry and install
/// the full transitive chain into `config.yaml` + `iii.lock` using the same
/// path that `iii worker add <name>` uses.
///
/// Pass-1: resolve each dep via `fetch_resolved_worker_graph` (serial — fine
/// for ≤3 deps; parallel fan-out is a future optimization).
/// Pass-2: merge all graphs into one synthetic graph (dedupes shared
/// transitive deps, errors on cross-graph version conflicts).
/// Pass-3: single call to `handle_resolved_graph_add`. Its snapshot/rollback
/// boundary covers the whole chain — no partial-install state is possible.
pub(crate) async fn install_manifest_dependencies(
    deps: &std::collections::BTreeMap<String, String>,
    brief: bool,
) -> Result<(), String> {
    if deps.is_empty() {
        return Ok(());
    }

    let mut graphs = Vec::with_capacity(deps.len());
    for (name, range) in deps {
        let graph = match fetch_resolved_worker_graph(name, Some(range.as_str()), None).await {
            Ok(g) => g,
            Err(e) => {
                // If the declared range is a prerelease, preempt the common
                // confusion: the default registry resolver filters to stable
                // versions, so a published prerelease looks "not found."
                let hint = semver::VersionReq::parse(range)
                    .ok()
                    .filter(|req| req.comparators.iter().any(|c| !c.pre.is_empty()))
                    .map(|_| {
                        " (note: the registry filters prereleases by default; \
                         configure the registry to expose prereleases if this \
                         range is intentional)"
                    })
                    .unwrap_or("");
                return Err(format!(
                    "failed to resolve dependency `{name}@{range}`: {e}{hint}"
                ));
            }
        };
        graphs.push((name.clone(), graph));
    }

    let merged = merge_resolved_graphs(graphs)?;

    let rc = handle_resolved_graph_add(&merged, brief).await;
    if rc != 0 {
        return Err(format!(
            "failed to install merged dependency graph (exit {rc}); no partial \
             state written — rerun after fixing the failure",
        ));
    }
    Ok(())
}

pub async fn handle_managed_add(
    image_or_name: &str,
    brief: bool,
    force: bool,
    reset_config: bool,
    wait: bool,
) -> i32 {
    // Local path workers: starts with '.', '/', or '~'
    if super::local_worker::is_local_path(image_or_name) {
        return super::local_worker::handle_local_add(
            image_or_name,
            force,
            reset_config,
            brief,
            wait,
        )
        .await;
    }

    // --force: stop if running, delete artifacts, then proceed with a fresh
    // add. Before the fix this path errored out with "Stop it first" when a
    // running worker was detected — which defeats the point of --force. The
    // whole sequence (stop → clear → add) is what a user means by "force."
    if force {
        let (plain_name, _) = parse_worker_input(image_or_name);

        let is_oci_ref = plain_name.contains('/') || plain_name.contains(':');
        if !is_oci_ref && let Err(e) = super::registry::validate_worker_name(&plain_name) {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }

        if is_worker_running(&plain_name) {
            eprintln!(
                "  {} {} is running, stopping first...",
                "⟳".cyan(),
                plain_name.bold()
            );
            let stop_rc = handle_managed_stop(&plain_name).await;
            if stop_rc != 0 {
                // Don't abort — artifacts will be wiped below anyway, and the
                // most common "failure" is "already stopped between is_worker_running
                // and the signal" which is benign. Surface it so a stuck worker
                // doesn't silently confuse the user.
                eprintln!(
                    "  {} stop exited {} — continuing with force add anyway",
                    "warning:".yellow(),
                    stop_rc
                );
            }
        }

        if is_any_builtin(&plain_name) {
            eprintln!(
                "  {} '{}' is a builtin worker, no artifacts to re-download.",
                "info:".cyan(),
                plain_name,
            );
        } else {
            let freed = delete_worker_artifacts(&plain_name);
            if freed > 0 {
                eprintln!(
                    "  {} Cleared {:.1} MB of artifacts for {}",
                    "✓".green(),
                    freed as f64 / 1_048_576.0,
                    plain_name.bold(),
                );
            }
        }

        if reset_config {
            match super::config_file::remove_worker(&plain_name) {
                Ok(()) => {}
                Err(e) => {
                    tracing::debug!("remove_worker during force: {}", e);
                }
            }
        }
    }

    // Direct OCI reference (contains '/' or ':') — passthrough, skip API
    if image_or_name.contains('/') || image_or_name.contains(':') {
        if !brief {
            eprintln!("  Resolving {}...", image_or_name.bold());
        }
        let name = image_or_name
            .rsplit('/')
            .next()
            .unwrap_or(image_or_name)
            .split(':')
            .next()
            .unwrap_or(image_or_name);
        if !brief {
            eprintln!("  {} Resolved to {}", "✓".green(), image_or_name.dimmed());
        }
        let rc = handle_oci_pull_and_add(name, image_or_name, brief).await;
        return finish_add(name, rc, wait, brief).await;
    }

    // Shorthand name — resolve via API
    let (name, version) = parse_worker_input(image_or_name);

    // Check for engine-builtin workers first (no network needed).
    if let Some(default_yaml) = get_builtin_default(&name) {
        let builtin_version = resolve_builtin_version(version.as_deref());
        let already_exists = super::config_file::worker_exists(&name);
        if let Err(e) = persist_engine_worker_config_and_lock(
            &name,
            builtin_version,
            Some(default_yaml.as_str()),
        ) {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
        if brief {
            if already_exists {
                eprintln!("        {} {} (updated)", "✓".green(), name.bold());
            } else {
                eprintln!("        {} {}", "✓".green(), name.bold());
            }
        } else {
            if already_exists {
                eprintln!(
                    "\n  {} Worker {} updated in {} (merged with builtin defaults)",
                    "✓".green(),
                    name.bold(),
                    "config.yaml".dimmed(),
                );
            } else {
                eprintln!(
                    "\n  {} Worker {} added to {}",
                    "✓".green(),
                    name.bold(),
                    "config.yaml".dimmed(),
                );
            }

            // If the engine is already running, its file watcher will detect
            // the config change and reload automatically. If it isn't, nudge
            // the user to start it (or customize first).
            if !is_engine_running() {
                eprintln!("  Start the engine to run it, or edit config.yaml to customize.");
            }
        }
        // Builtins run in-process with the engine; there is no detached VM
        // or binary to watch. The Phase machinery would loop on Queued until
        // timeout, so skip wait_for_ready for builtins even when wait=true.
        // Fire telemetry so the registry can count this activation.
        fire_engine_telemetry(&name, builtin_version).await;
        return 0;
    }

    if !brief {
        eprintln!("  Resolving {}...", name.bold());
    }

    match fetch_resolved_worker_graph(&name, version.as_deref(), None).await {
        Ok(graph) => {
            let rc = handle_resolved_graph_add(&graph, brief).await;
            return finish_add(&name, rc, wait, brief).await;
        }
        Err(e) if should_fallback_to_legacy_registry_error(&name, &e) => {
            tracing::debug!("falling back to single-worker registry response: {}", e);
        }
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    }

    let response = match fetch_worker_info(&name, version.as_deref()).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 1;
        }
    };

    let rc = match response {
        WorkerInfoResponse::Binary(r) => handle_binary_add(&name, &r, brief).await,
        WorkerInfoResponse::Oci(r) => {
            if !brief {
                eprintln!("  {} Resolved to {}", "✓".green(), r.image_url.dimmed());
            }
            handle_oci_pull_and_add(&r.name, &r.image_url, brief).await
        }
        WorkerInfoResponse::Engine(r) => {
            // Engine workers are built into the iii binary; telemetry was already
            // fired by fetch_worker_info via the 204 response path.
            if let Err(e) = persist_engine_worker_config_and_lock(&r.name, &r.version, None) {
                eprintln!("{} {}", "error:".red(), e);
                return 1;
            }
            if !brief {
                eprintln!(
                    "\n  {} {} v{} (engine, built-in — nothing to download)",
                    "✓".green(),
                    r.name.bold(),
                    r.version
                );
            }
            0
        }
    };
    finish_add(&name, rc, wait, brief).await
}

fn should_fallback_to_legacy_registry_error(name: &str, error: &str) -> bool {
    error.starts_with("Failed to parse worker graph:")
        || error == format!("Worker '{}' not found", name)
        || error.starts_with("Failed to resolve worker graph: HTTP 404")
        || error.starts_with("Failed to resolve worker graph: HTTP 405")
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
               Start:         iii start\n  \
               Then watch:    iii worker status {}",
            "⚠".yellow(),
            worker_name,
        );
        return rc;
    }
    wait_for_ready(worker_name, port).await;
    rc
}

async fn handle_oci_pull_and_add(name: &str, image_ref: &str, brief: bool) -> i32 {
    let adapter = super::worker_manager::create_adapter("libkrun");

    if !brief {
        eprintln!("  Pulling {}...", image_ref.bold());
    }
    let pull_info = match adapter.pull(image_ref).await {
        Ok(info) => info,
        Err(e) => {
            eprintln!("{} Pull failed: {}", "error:".red(), e);
            return 1;
        }
    };

    let manifest: Option<serde_json::Value> =
        match adapter.extract_file(image_ref, MANIFEST_PATH).await {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(yaml_str) => serde_yaml::from_str(&yaml_str).ok(),
                Err(_) => None,
            },
            Err(_) => None,
        };

    if !brief {
        if let Some(ref m) = manifest {
            eprintln!("  {} Image pulled successfully", "✓".green());
            if let Some(v) = m.get("name").and_then(|v| v.as_str()) {
                eprintln!("  {}: {}", "Name".bold(), v);
            }
            if let Some(v) = m.get("version").and_then(|v| v.as_str()) {
                eprintln!("  {}: {}", "Version".bold(), v);
            }
            if let Some(v) = m.get("description").and_then(|v| v.as_str()) {
                eprintln!("  {}: {}", "Description".bold(), v);
            }
            if let Some(size) = pull_info.size_bytes {
                eprintln!("  {}: {:.1} MB", "Size".bold(), size as f64 / 1_048_576.0);
            }
        } else {
            eprintln!("  {} Image pulled (no manifest found)", "✓".green());
            if let Some(size) = pull_info.size_bytes {
                eprintln!("  {}: {:.1} MB", "Size".bold(), size as f64 / 1_048_576.0);
            }
        }
    }

    // Extract OCI env vars from the pulled image rootfs and write as config:
    let rootfs_dir = image_cache_dir(image_ref);
    let oci_env = super::worker_manager::oci::read_oci_env(&rootfs_dir);
    let config_yaml = if oci_env.is_empty() {
        None
    } else {
        // Filter out generic system env vars (PATH, HOME, etc.)
        let filtered: Vec<_> = oci_env
            .iter()
            .filter(|(k, _)| !matches!(k.as_str(), "PATH" | "HOME" | "HOSTNAME" | "LANG" | "TERM"))
            .collect();
        if filtered.is_empty() {
            None
        } else {
            let config_map: serde_json::Map<String, serde_json::Value> = filtered
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect();
            let yaml_str =
                serde_yaml::to_string(&serde_json::Value::Object(config_map)).unwrap_or_default();
            // serde_yaml adds a leading `---\n`, strip it for embedding
            let yaml_str = yaml_str
                .strip_prefix("---\n")
                .unwrap_or(&yaml_str)
                .trim_end();
            if yaml_str.is_empty() {
                None
            } else {
                Some(yaml_str.to_string())
            }
        }
    };

    if let Err(e) =
        super::config_file::append_worker_with_image(name, image_ref, config_yaml.as_deref())
    {
        eprintln!("{} Failed to update config.yaml: {}", "error:".red(), e);
        return 1;
    }
    if brief {
        eprintln!("        {} {}", "✓".green(), name.bold());
    } else {
        eprintln!(
            "\n  {} Worker {} added to {}",
            "✓".green(),
            name.bold(),
            "config.yaml".dimmed(),
        );

        // The engine's file watcher will detect the config change and
        // reload automatically — no need to start the worker here.
    }
    0
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
    if let Err(e) = super::config_file::remove_worker(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }
    if brief {
        eprintln!("        {} {}", "✓".green(), worker_name.bold());
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
    confirm_prompt("  This will remove all downloaded workers and images. Continue? [y/N] ")
}

fn clear_all_workers(skip_confirm: bool) -> i32 {
    let home = dirs::home_dir().unwrap_or_default();
    let workers_dir = home.join(".iii/workers");
    let images_dir = home.join(".iii/images");

    if !workers_dir.exists() && !images_dir.exists() {
        eprintln!("  Nothing to clear.");
        return 0;
    }

    if !skip_confirm && !confirm_clear() {
        eprintln!("  Aborted.");
        return 0;
    }

    let mut skipped: Vec<String> = Vec::new();
    let mut total_freed: u64 = 0;
    let mut worker_count: u32 = 0;
    let mut image_count: u32 = 0;

    // Clear binary workers
    if workers_dir.exists()
        && let Ok(entries) = std::fs::read_dir(&workers_dir)
    {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Skip entries with invalid names (e.g. symlinks with path traversal)
            if super::registry::validate_worker_name(&name).is_err() {
                continue;
            }
            // Verify resolved path stays under workers_dir
            if let Ok(resolved) = entry.path().canonicalize()
                && let Ok(base) = workers_dir.canonicalize()
                && !resolved.starts_with(&base)
            {
                continue;
            }
            if is_worker_running(&name) {
                skipped.push(name);
                continue;
            }
            total_freed += dir_size(&entry.path());
            let _ = std::fs::remove_dir_all(entry.path());
            worker_count += 1;
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
        worker_count,
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
                // Only kill if process is still alive.
                if kill(p, None).is_ok() {
                    tracing::info!(worker = %worker_name, pid, "Killing stale worker process");
                    let _ = kill(p, Signal::SIGTERM);
                    // Brief wait then force-kill.
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let _ = kill(p, Signal::SIGKILL);
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
/// represents (if any). Shared between name discovery and PID lookup so both
/// match against the exact same recognition rules.
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

    // Pattern 2: iii-worker __vm-boot --pid-file <...>/managed/{name}/vm.pid
    if exe_path.file_name().and_then(|s| s.to_str()) == Some("iii-worker")
        && tokens.next() == Some("__vm-boot")
    {
        let rest: Vec<&str> = tokens.collect();
        for i in 0..rest.len().saturating_sub(1) {
            if rest[i] == "--pid-file"
                && let Ok(rel) = std::path::Path::new(rest[i + 1]).strip_prefix(managed_prefix)
                && let Some(name) = rel.iter().next().and_then(|c| c.to_str())
                && !name.is_empty()
            {
                return Some(name.to_string());
            }
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

/// Pure parser used by [`find_worker_pid_from_ps`]. Returns the first PID
/// whose cmdline resolves to `name`. Exposed for testing.
fn find_worker_pid_in_processes(
    processes: &[(u32, String)],
    name: &str,
    workers_prefix: &std::path::Path,
    managed_prefix: &std::path::Path,
) -> Option<u32> {
    processes.iter().find_map(|(pid, cmdline)| {
        match extract_worker_name_from_cmdline(cmdline, workers_prefix, managed_prefix) {
            Some(n) if n == name => Some(*pid),
            _ => None,
        }
    })
}

/// Returns `true` if the worker has a valid PID file and the process is alive.
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
                if kill(Pid::from_raw(pid as i32), None).is_ok() {
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

    // Local-path worker: ~/.iii/managed/{name}/ (same as OCI)
    let managed_dir = home.join(".iii/managed").join(worker_name);
    if managed_dir.is_dir() {
        // Only count if we haven't already freed anything (avoid double-counting with OCI)
        if freed == 0 {
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
    }

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
                 Check logs:    iii worker logs {} -f",
                "⚠".yellow(),
                elapsed.as_secs_f64(),
                worker_name,
                worker_name
            );
        }
    }
}

/// Starts a managed worker, pointing it back at the engine on `port`.
///
/// `port` is the WebSocket port the spawned worker will connect to (used to
/// build `III_ENGINE_URL` for VM-based workers and to probe engine liveness).
/// Callers that don't know any better pass `DEFAULT_PORT`; the engine's
/// auto-spawn path in `registry_worker::ExternalWorkerProcess::spawn` passes
/// the configured `iii-worker-manager` port so non-default manager ports
/// don't silently break connectivity for external workers.
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
        if !is_engine_running() {
            eprintln!(
                "{} '{}' is a builtin served by the iii engine, but the engine isn't running.\n  \
                 Start the engine:  iii start",
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
            let worker_def = WorkerDef::Managed {
                image,
                env,
                resources: None,
            };
            StartOutcome::Exit(start_oci_worker(worker_name, &worker_def, port).await)
        }
        ResolvedWorkerType::Local { worker_path } => {
            if config.is_some() {
                tracing::warn!(
                    worker = %worker_name,
                    "--config ignored for local-source workers"
                );
            }
            StartOutcome::Exit(
                super::local_worker::start_local_worker(worker_name, &worker_path, port).await,
            )
        }
        ResolvedWorkerType::Binary { binary_path } => {
            StartOutcome::Exit(start_binary_worker(worker_name, &binary_path, config).await)
        }
        ResolvedWorkerType::Config => StartOutcome::FallThrough,
    };
    if let StartOutcome::Exit(rc) = local_outcome {
        return finish_start(worker_name, rc, wait, port).await;
    }

    // Not found locally — try remote registry for auto-install
    eprintln!(
        "  Worker '{}' not found locally, checking registry...",
        worker_name
    );
    match fetch_worker_info(worker_name, None).await {
        Ok(WorkerInfoResponse::Binary(response)) => {
            let target = binary_download::current_target();
            let binary_info = match response.binaries.get(target) {
                Some(info) => info,
                None => {
                    eprintln!(
                        "{} Platform '{}' not supported for '{}'. Available: {}",
                        "error:".red(),
                        target,
                        worker_name,
                        response
                            .binaries
                            .keys()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    return 1;
                }
            };

            eprintln!(
                "  Installing {} (binary v{})...",
                worker_name, response.version
            );
            match binary_download::download_and_install_binary(worker_name, binary_info).await {
                Ok(installed_path) => {
                    eprintln!("  {} Installed successfully", "✓".green());
                    let rc = start_binary_worker(worker_name, &installed_path, config).await;
                    return finish_start(worker_name, rc, wait, port).await;
                }
                Err(e) => {
                    eprintln!(
                        "{} Failed to install '{}': {}",
                        "error:".red(),
                        worker_name,
                        e
                    );
                    return 1;
                }
            }
        }
        Ok(WorkerInfoResponse::Oci(response)) => {
            let worker_def = WorkerDef::Managed {
                image: response.image_url,
                env: std::collections::HashMap::new(),
                resources: None,
            };
            let rc = start_oci_worker(worker_name, &worker_def, port).await;
            return finish_start(worker_name, rc, wait, port).await;
        }
        Ok(WorkerInfoResponse::Engine(_)) => {
            if !super::config_file::worker_exists(worker_name) {
                eprintln!(
                    "{} '{}' is an engine builtin but is not configured. Run `iii worker add {}` first.",
                    "error:".red(),
                    worker_name,
                    worker_name,
                );
                return 1;
            }
            if !is_engine_running() {
                eprintln!(
                    "{} '{}' is an engine builtin, but the engine isn't running.\n  \
                     Start the engine:  iii start",
                    "error:".red(),
                    worker_name,
                );
                return 1;
            }
            eprintln!(
                "  {} '{}' is an engine builtin — it starts automatically with `iii start`.",
                "info:".cyan(),
                worker_name
            );
            return 0;
        }
        Err(e) => {
            tracing::warn!("Failed to fetch worker info: {}", e);
        }
    }

    eprintln!(
        "{} Worker '{}' not found locally or in registry. Run `iii worker add {}`.",
        "error:".red(),
        worker_name,
        worker_name
    );
    1
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
    port: u16,
    config: Option<&std::path::Path>,
) -> i32 {
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

    let stdout_file = match std::fs::File::create(logs_dir.join("stdout.log")) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{} Failed to create stdout log: {}", "error:".red(), e);
            return 1;
        }
    };
    let stderr_file = match std::fs::File::create(logs_dir.join("stderr.log")) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{} Failed to create stderr log: {}", "error:".red(), e);
            return 1;
        }
    };

    eprintln!("  Starting {} (binary)...", worker_name.bold());

    let mut cmd = tokio::process::Command::new(binary_path);
    if let Some(cfg_path) = config {
        cmd.arg("--config").arg(cfg_path);
    }
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

pub async fn handle_worker_list() -> i32 {
    let config_names = super::config_file::list_worker_names();

    // Discovery union: on-disk PID files + on-disk managed dirs + live process
    // table (catches workers whose pidfiles were removed but the process kept
    // running -- the actual repro from the user's bug report).
    let disk_names = discover_disk_worker_names();
    let ps_names = discover_running_worker_names_from_ps();
    let ps_set: std::collections::HashSet<String> = ps_names.iter().cloned().collect();
    let config_set: std::collections::HashSet<&str> =
        config_names.iter().map(String::as_str).collect();

    // Orphan = not in current ./config.yaml AND demonstrably alive (either via
    // pidfile signal-0 check or because we just saw it in `ps`). Dead disk-only
    // entries are stale runtime state, not what the user is asking about.
    let candidate_names: std::collections::BTreeSet<String> =
        disk_names.into_iter().chain(ps_names.into_iter()).collect();
    let orphan_names: Vec<String> = candidate_names
        .into_iter()
        .filter(|n| !config_set.contains(n.as_str()))
        .filter(|n| ps_set.contains(n) || is_worker_running(n))
        .collect();

    if config_names.is_empty() && orphan_names.is_empty() {
        eprintln!("  No workers. Use `iii worker add` to get started.");
        return 0;
    }

    let engine_running = is_engine_running();

    eprintln!();
    eprintln!(
        "  {:25} {:10} {}",
        "NAME".bold(),
        "TYPE".bold(),
        "STATUS".bold()
    );
    eprintln!(
        "  {:25} {:10} {}",
        "----".dimmed(),
        "----".dimmed(),
        "------".dimmed()
    );

    for name in &config_names {
        let worker_type = worker_list_type_label(name);

        let running = if is_worker_running(name) {
            "running".green().to_string()
        } else if matches!(worker_type, "config" | "engine") && engine_running {
            "running".green().to_string()
        } else {
            "stopped".dimmed().to_string()
        };

        eprintln!("  {:25} {:10} {}", name, worker_type.dimmed(), running);
    }

    // Orphans: alive on this machine but absent from the current ./config.yaml.
    // The TYPE column is inferred from the on-disk evidence we found the worker
    // through (managed dir vs binary pidfile/exe). Falls back to "?" only when
    // a worker was discovered solely via `ps` and no on-disk artifact remains.
    for name in &orphan_names {
        let home = dirs::home_dir().unwrap_or_default();
        let worker_type = resolve_orphan_type(
            name,
            &home.join(".iii/managed"),
            &home.join(".iii/pids"),
            &home.join(".iii/workers"),
        );
        eprintln!(
            "  {:25} {:10} {}",
            name,
            worker_type.dimmed(),
            "orphan".yellow()
        );
    }

    eprintln!();
    0
}

fn worker_list_type_label(name: &str) -> &'static str {
    worker_list_type_label_from_resolved(name, super::config_file::resolve_worker_type(name))
}

fn worker_list_type_label_from_resolved(name: &str, resolved: ResolvedWorkerType) -> &'static str {
    match resolved {
        ResolvedWorkerType::Local { .. } => "local",
        ResolvedWorkerType::Oci { .. } => "oci",
        ResolvedWorkerType::Binary { .. } => "binary",
        ResolvedWorkerType::Config if is_any_builtin(name) => "engine",
        ResolvedWorkerType::Config => "config",
    }
}

/// Infers the TYPE label for an orphan worker from on-disk evidence alone.
///
/// We can't rebuild the `ResolvedWorkerType` enum without a config entry, but
/// we can tell `managed` (OCI/VM/local-path -- shares a directory shape) from
/// `binary` (single executable + sidecar pidfile) just from where the artifact
/// lives. Returns "?" when only a `ps` match exists and every artifact has been
/// cleaned up under it -- the honest answer.
///
/// Path arguments are injected to keep the function unit-testable against a
/// tempdir without an env override.
fn resolve_orphan_type(
    name: &str,
    managed_dir: &std::path::Path,
    pids_dir: &std::path::Path,
    workers_dir: &std::path::Path,
) -> &'static str {
    if managed_dir.join(name).is_dir() {
        return "managed";
    }
    if pids_dir.join(format!("{}.pid", name)).is_file() || workers_dir.join(name).is_file() {
        return "binary";
    }
    "?"
}

/// Pick the log directory with the most recently modified, non-empty log file.
/// Returns `None` when no candidate contains any usable log content.
fn pick_best_logs_dir(candidates: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    let mut best: Option<(std::path::PathBuf, std::time::SystemTime)> = None;

    for dir in candidates {
        let latest = ["stdout.log", "stderr.log"]
            .iter()
            .map(|f| dir.join(f))
            .filter_map(|p| std::fs::metadata(&p).ok().map(|m| (p, m)))
            .filter(|(_, m)| m.len() > 0)
            .filter_map(|(_, m)| m.modified().ok())
            .max();

        if let Some(modified) = latest
            && best.as_ref().is_none_or(|(_, t)| modified > *t)
        {
            best = Some((dir.clone(), modified));
        }
    }

    best.map(|(dir, _)| dir)
}

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

pub async fn handle_managed_logs(
    worker_name: &str,
    follow: bool,
    _address: &str,
    _port: u16,
) -> i32 {
    if let Err(e) = super::registry::validate_worker_name(worker_name) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }
    let home = dirs::home_dir().unwrap_or_default();

    // Check all possible log locations and prefer the one with the most
    // recently modified, non-empty log files. This avoids picking a stale
    // directory (e.g. ~/.iii/logs/ from a binary worker) over the active
    // one (e.g. ~/.iii/managed/ from a libkrun OCI worker).
    let unified_logs_dir = home.join(".iii/logs").join(worker_name);
    let legacy_managed_dir = home.join(".iii/managed").join(worker_name).join("logs");
    let legacy_binary_dir = home.join(".iii/workers/logs").join(worker_name);

    let logs_dir = pick_best_logs_dir(&[
        unified_logs_dir.clone(),
        legacy_managed_dir,
        legacy_binary_dir,
    ])
    .unwrap_or(unified_logs_dir);

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
            eprintln!("{} No logs found for '{}'", "error:".red(), worker_name);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Mutex;

    static CWD_LOCK: Mutex<()> = Mutex::new(());

    /// Run an async closure with CWD set to a temp dir, then restore.
    /// Uses a drop guard so CWD is restored even if the closure panics.
    async fn in_temp_dir_async<F, Fut>(f: F)
    where
        F: FnOnce(std::path::PathBuf) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        struct CwdGuard(std::path::PathBuf);
        impl Drop for CwdGuard {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }

        let _guard = CWD_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        let dir_path = dir.path().to_path_buf();
        std::env::set_current_dir(&dir_path).unwrap();
        let _cwd_guard = CwdGuard(original);
        f(dir_path).await;
    }

    fn in_temp_dir<F>(f: F)
    where
        F: FnOnce(std::path::PathBuf),
    {
        struct CwdGuard(std::path::PathBuf);
        impl Drop for CwdGuard {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }

        let _guard = CWD_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        let dir_path = dir.path().to_path_buf();
        std::env::set_current_dir(&dir_path).unwrap();
        let _cwd_guard = CwdGuard(original);
        f(dir_path);
    }

    #[test]
    fn binary_config_yaml_omits_empty_registry_config() {
        assert_eq!(binary_config_yaml(&serde_json::json!({})), None);
    }

    #[test]
    fn worker_list_type_label_marks_configured_builtin_as_engine() {
        assert_eq!(
            worker_list_type_label_from_resolved("iii-stream", ResolvedWorkerType::Config),
            "engine"
        );
    }

    #[test]
    fn worker_list_type_label_keeps_non_builtin_config_as_config() {
        assert_eq!(
            worker_list_type_label_from_resolved("custom-config-only", ResolvedWorkerType::Config),
            "config"
        );
    }

    #[test]
    fn worker_list_type_label_preserves_local_oci_and_binary_labels() {
        assert_eq!(
            worker_list_type_label_from_resolved(
                "local-dev",
                ResolvedWorkerType::Local {
                    worker_path: "./worker".to_string(),
                },
            ),
            "local"
        );
        assert_eq!(
            worker_list_type_label_from_resolved(
                "external-image",
                ResolvedWorkerType::Oci {
                    image: "ghcr.io/acme/external:1".to_string(),
                    env: std::collections::HashMap::new(),
                },
            ),
            "oci"
        );
        assert_eq!(
            worker_list_type_label_from_resolved(
                "downloaded-worker",
                ResolvedWorkerType::Binary {
                    binary_path: std::path::PathBuf::from("/tmp/downloaded-worker"),
                },
            ),
            "binary"
        );
    }

    #[test]
    fn active_worker_restore_holds_activation_lock_until_commit() {
        in_temp_dir(|dir| {
            let _env_guard = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let home = dir.join("home");
            std::fs::create_dir_all(&home).unwrap();
            let _home_guard = set_env_var_for_test("HOME", &home);

            let name = "lock-holder";
            let lock = WorkerActivationLock::acquire(name).expect("first acquire");
            let restore = activate_locked_binary(name, "1.0.0", b"binary-bytes", false, lock)
                .expect("activate must succeed")
                .expect("expected Some(restore) for fresh install");

            // Critical: while the restore is alive (i.e., commit/rollback not
            // yet called), a concurrent sync MUST NOT be able to overwrite this
            // worker. If the lock were dropped early, the second acquire would
            // succeed and a later rollback could resurrect a stale backup over
            // the newer install.
            let err = WorkerActivationLock::acquire(name)
                .err()
                .expect("second acquire must fail while restore is alive");
            assert!(
                err.contains("being installed by another process"),
                "expected lock-busy message, got: {err}"
            );

            restore.commit();
            WorkerActivationLock::acquire(name).expect("acquire after commit must succeed");
        });
    }

    #[test]
    fn active_worker_restore_holds_activation_lock_until_rollback() {
        in_temp_dir(|dir| {
            let _env_guard = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let home = dir.join("home");
            std::fs::create_dir_all(&home).unwrap();
            let _home_guard = set_env_var_for_test("HOME", &home);

            let name = "rollback-lock-holder";
            let lock = WorkerActivationLock::acquire(name).unwrap();
            let restore = activate_locked_binary(name, "1.0.0", b"x", false, lock)
                .unwrap()
                .unwrap();

            assert!(WorkerActivationLock::acquire(name).is_err());
            restore.rollback();
            WorkerActivationLock::acquire(name).expect("acquire after rollback must succeed");
        });
    }

    #[test]
    fn binary_config_yaml_returns_none_for_null_json() {
        assert_eq!(binary_config_yaml(&serde_json::Value::Null), None);
    }

    #[test]
    fn binary_config_yaml_returns_none_for_inner_null() {
        let wrapped = serde_json::json!({ "config": null });
        assert_eq!(binary_config_yaml(&wrapped), None);
    }

    use crate::cli::lockfile as cli_lockfile;
    use crate::cli::registry as cli_registry;
    use std::collections::HashMap as StdHashMap;

    #[derive(Clone)]
    struct TestResponse {
        status: u16,
        content_type: &'static str,
        body: Vec<u8>,
    }

    struct EnvVarGuard {
        key: &'static str,
        old: Option<std::ffi::OsString>,
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.old {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    fn set_env_var_for_test(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> EnvVarGuard {
        let old = std::env::var_os(key);
        unsafe { std::env::set_var(key, value) };
        EnvVarGuard { key, old }
    }

    fn binary_archive(binary_name: &str) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let contents = format!("#!/bin/sh\necho {binary_name}\n");
        let mut header = tar::Header::new_gnu();
        header.set_path(binary_name).unwrap();
        header.set_size(contents.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append(&header, contents.as_bytes())
            .expect("append test binary");
        let encoder = archive.into_inner().expect("finish tar archive");
        encoder.finish().expect("finish gzip archive")
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!("{:x}", hasher.finalize())
    }

    fn locked_binary_source(
        target: &str,
        url: &str,
        sha256: String,
    ) -> Option<cli_lockfile::LockedSource> {
        Some(cli_lockfile::LockedSource::Binary {
            artifacts: std::collections::BTreeMap::from([(
                target.to_string(),
                cli_lockfile::LockedBinaryArtifact {
                    url: url.to_string(),
                    sha256,
                },
            )]),
        })
    }

    async fn spawn_static_http_server(
        routes: StdHashMap<String, TestResponse>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        spawn_static_http_server_with_routes(|_| routes).await
    }

    async fn spawn_static_http_server_with_routes<F>(
        build_routes: F,
    ) -> (String, tokio::task::JoinHandle<()>)
    where
        F: FnOnce(&str) -> StdHashMap<String, TestResponse>,
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("test server local addr");
        let base_url = format!("http://{addr}");
        let routes = build_routes(&base_url);
        let handle = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let routes = routes.clone();
                tokio::spawn(async move {
                    let mut buf = [0_u8; 4096];
                    let Ok(n) = stream.read(&mut buf).await else {
                        return;
                    };
                    let request = String::from_utf8_lossy(&buf[..n]);
                    let mut parts = request
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .split_whitespace();
                    let method = parts.next().unwrap_or_default();
                    let path = parts.next().unwrap_or("/");
                    let keyed = format!("{method} {path}");
                    let response = routes.get(&keyed).or_else(|| routes.get(path));

                    let (status, content_type, body) = match response {
                        Some(response) => (
                            response.status,
                            response.content_type,
                            response.body.as_slice(),
                        ),
                        None => (404, "text/plain", b"not found".as_slice()),
                    };
                    let reason = if status == 200 { "OK" } else { "ERROR" };
                    let headers = format!(
                        "HTTP/1.1 {status} {reason}\r\ncontent-length: {}\r\ncontent-type: {content_type}\r\nconnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(headers.as_bytes()).await;
                    let _ = stream.write_all(body).await;
                });
            }
        });

        (base_url, handle)
    }

    fn resolved_binary_worker(
        name: &str,
        version: &str,
        binaries: StdHashMap<String, cli_registry::BinaryInfo>,
    ) -> cli_registry::ResolvedWorker {
        cli_registry::ResolvedWorker {
            name: name.to_string(),
            worker_type: "binary".to_string(),
            version: version.to_string(),
            repo: format!("https://example.com/{name}"),
            config: serde_json::Value::Null,
            binaries: Some(binaries),
            image: None,
            dependencies: StdHashMap::new(),
        }
    }

    fn resolved_image_worker(
        name: &str,
        version: &str,
        image: Option<String>,
    ) -> cli_registry::ResolvedWorker {
        cli_registry::ResolvedWorker {
            name: name.to_string(),
            worker_type: "image".to_string(),
            version: version.to_string(),
            repo: format!("https://example.com/{name}"),
            config: serde_json::Value::Null,
            binaries: None,
            image,
            dependencies: StdHashMap::new(),
        }
    }

    fn resolved_engine_worker(name: &str, version: &str) -> cli_registry::ResolvedWorker {
        cli_registry::ResolvedWorker {
            name: name.to_string(),
            worker_type: "engine".to_string(),
            version: version.to_string(),
            repo: format!("https://example.com/{name}"),
            config: serde_json::Value::Null,
            binaries: None,
            image: None,
            dependencies: StdHashMap::new(),
        }
    }

    fn graph_with(worker: cli_registry::ResolvedWorker) -> cli_registry::ResolvedWorkerGraph {
        cli_registry::ResolvedWorkerGraph {
            root: cli_registry::ResolvedRoot {
                name: worker.name.clone(),
                version: worker.version.clone(),
            },
            target: Some("aarch64-apple-darwin".to_string()),
            graph: vec![worker],
            edges: Vec::new(),
        }
    }

    #[test]
    fn lockfile_from_graph_errors_when_binary_worker_missing_binaries() {
        // A binary worker with `binaries: None` in the resolver response means
        // the registry is inconsistent; surface the worker name so the CLI
        // error message is actionable.
        let mut worker = resolved_binary_worker("hello-worker", "1.0.0", StdHashMap::new());
        worker.binaries = None;

        let err = lockfile_from_graph(&graph_with(worker)).unwrap_err();

        assert!(err.contains("hello-worker"));
        assert!(err.contains("no binaries"));
    }

    #[test]
    fn lockfile_from_graph_records_all_binary_artifacts() {
        let mut binaries = StdHashMap::new();
        binaries.insert(
            "x86_64-unknown-linux-gnu".to_string(),
            cli_registry::BinaryInfo {
                url: "https://workers.iii.dev/linux.tar.gz".to_string(),
                sha256: "b".repeat(64),
            },
        );
        binaries.insert(
            "aarch64-apple-darwin".to_string(),
            cli_registry::BinaryInfo {
                url: "https://workers.iii.dev/darwin.tar.gz".to_string(),
                sha256: "a".repeat(64),
            },
        );
        let worker = resolved_binary_worker("hello-worker", "1.0.0", binaries);

        let lock = lockfile_from_graph(&graph_with(worker)).unwrap();
        let entry = lock.workers.get("hello-worker").expect("entry present");

        match entry.source.as_ref().unwrap() {
            cli_lockfile::LockedSource::Binary { artifacts } => {
                assert_eq!(artifacts.len(), 2);
                assert_eq!(
                    artifacts.get("aarch64-apple-darwin").unwrap().url,
                    "https://workers.iii.dev/darwin.tar.gz"
                );
                assert_eq!(
                    artifacts.get("x86_64-unknown-linux-gnu").unwrap().url,
                    "https://workers.iii.dev/linux.tar.gz"
                );
            }
            other => panic!("expected binary source, got {:?}", other),
        }
    }

    #[test]
    fn lockfile_from_graph_rejects_unreplayable_binary_artifact_url() {
        let mut binaries = StdHashMap::new();
        binaries.insert(
            "aarch64-apple-darwin".to_string(),
            cli_registry::BinaryInfo {
                // Non-HTTPS — still rejected by `validate_locked_artifact_url`.
                url: "http://example.com/h.tar.gz".to_string(),
                sha256: "c".repeat(64),
            },
        );
        let worker = resolved_binary_worker("hello-worker", "1.0.0", binaries);

        let err = lockfile_from_graph(&graph_with(worker)).unwrap_err();

        assert!(err.contains("hello-worker"));
        assert!(err.contains("aarch64-apple-darwin"));
        assert!(err.contains("unreplayable artifact URL"));
    }

    #[test]
    fn lockfile_from_graph_errors_when_image_worker_missing_image_ref() {
        let worker = resolved_image_worker("image-worker", "1.0.0", None);

        let err = lockfile_from_graph(&graph_with(worker)).unwrap_err();

        assert!(err.contains("image-worker"));
        assert!(err.contains("no image"));
    }

    #[test]
    fn lockfile_from_graph_errors_on_unsupported_worker_type() {
        let mut worker = resolved_image_worker(
            "wasm-worker",
            "1.0.0",
            Some("ghcr.io/iii-hq/wasm@sha256:abc".to_string()),
        );
        worker.worker_type = "wasm".to_string();

        let err = lockfile_from_graph(&graph_with(worker)).unwrap_err();

        assert!(err.contains("wasm-worker"));
        assert!(err.contains("wasm"));
    }

    #[test]
    fn lockfile_from_graph_builds_entry_for_binary_worker() {
        let mut binaries = StdHashMap::new();
        binaries.insert(
            "aarch64-apple-darwin".to_string(),
            cli_registry::BinaryInfo {
                url: "https://workers.iii.dev/h.tar.gz".to_string(),
                sha256: "c".repeat(64),
            },
        );
        let mut worker = resolved_binary_worker("hello-worker", "1.0.0", binaries);
        worker
            .dependencies
            .insert("helper".to_string(), "^1.0.0".to_string());

        let lock = lockfile_from_graph(&graph_with(worker)).unwrap();

        let entry = lock.workers.get("hello-worker").expect("entry present");
        assert_eq!(entry.version, "1.0.0");
        assert_eq!(entry.dependencies.get("helper").unwrap(), "^1.0.0");
        assert!(matches!(
            entry.worker_type,
            cli_lockfile::LockedWorkerType::Binary
        ));
        match entry.source.as_ref().unwrap() {
            cli_lockfile::LockedSource::Binary { artifacts } => {
                let artifact = artifacts.get("aarch64-apple-darwin").unwrap();
                assert_eq!(artifact.url, "https://workers.iii.dev/h.tar.gz");
                assert_eq!(artifact.sha256.len(), 64);
            }
            other => panic!("expected binary source, got {:?}", other),
        }
    }

    #[test]
    fn lockfile_from_graph_records_image_type_for_image_worker() {
        let worker = resolved_image_worker(
            "image-worker",
            "1.0.0",
            Some("ghcr.io/iii-hq/image@sha256:abc".to_string()),
        );

        let lock = lockfile_from_graph(&graph_with(worker)).unwrap();

        let entry = lock.workers.get("image-worker").expect("entry present");
        assert!(matches!(
            entry.worker_type,
            cli_lockfile::LockedWorkerType::Image
        ));
    }

    #[test]
    fn lockfile_from_graph_records_engine_type_without_source() {
        let worker = resolved_engine_worker("iii-exec", "1.2.3");

        let lock = lockfile_from_graph(&graph_with(worker)).unwrap();

        let entry = lock.workers.get("iii-exec").expect("entry present");
        assert_eq!(entry.version, "1.2.3");
        assert!(matches!(
            entry.worker_type,
            cli_lockfile::LockedWorkerType::Engine
        ));
        assert!(entry.source.is_none());
    }

    #[test]
    fn frozen_verify_filters_unmanaged_config_workers() {
        in_temp_dir(|_| {
            let mut lock = cli_lockfile::WorkerLockfile::default();
            lock.workers.insert(
                "image-resize".to_string(),
                cli_lockfile::LockedWorker {
                    version: "1.0.0".to_string(),
                    worker_type: cli_lockfile::LockedWorkerType::Binary,
                    dependencies: Default::default(),
                    source: locked_binary_source(
                        "aarch64-apple-darwin",
                        "https://workers.iii.dev/image-resize.tar.gz",
                        "a".repeat(64),
                    ),
                },
            );

            let names = vec![
                "image-resize".to_string(),
                "iii-http".to_string(),
                "local-dev".to_string(),
                "external-image".to_string(),
            ];
            std::fs::write(
                "config.yaml",
                "\
workers:
  - name: image-resize
  - name: iii-http
    config:
      port: 3111
  - name: local-dev
    worker_path: ./worker
  - name: external-image
    image: ghcr.io/acme/external:1
",
            )
            .unwrap();

            let relevant = lockfile_relevant_config_worker_names(&lock, &names);

            assert_eq!(relevant, vec!["image-resize".to_string()]);
        });
    }

    #[test]
    fn frozen_verify_still_flags_registry_like_config_worker_missing_from_lock() {
        in_temp_dir(|_| {
            let lock = cli_lockfile::WorkerLockfile::default();
            std::fs::write(
                "config.yaml",
                "\
workers:
  - name: image-resize
    config:
      width: 200
",
            )
            .unwrap();

            let relevant =
                lockfile_relevant_config_worker_names(&lock, &["image-resize".to_string()]);

            assert_eq!(relevant, vec!["image-resize".to_string()]);
            assert!(lock.verify_config_workers(&relevant).is_err());
        });
    }

    #[tokio::test]
    async fn handle_worker_verify_allows_mixed_locked_and_unmanaged_workers() {
        in_temp_dir_async(|_| async move {
            let mut lock = cli_lockfile::WorkerLockfile::default();
            lock.workers.insert(
                "image-resize".to_string(),
                cli_lockfile::LockedWorker {
                    version: "1.0.0".to_string(),
                    worker_type: cli_lockfile::LockedWorkerType::Binary,
                    dependencies: Default::default(),
                    source: locked_binary_source(
                        binary_download::current_target(),
                        "https://workers.iii.dev/image-resize.tar.gz",
                        "a".repeat(64),
                    ),
                },
            );
            lock.write_to(cli_lockfile::lockfile_path()).unwrap();
            std::fs::write(
                "config.yaml",
                "\
workers:
  - name: image-resize
  - name: iii-http
    config:
      port: 3111
  - name: local-dev
    worker_path: ./worker
  - name: external-image
    image: ghcr.io/acme/external:1
",
            )
            .unwrap();

            let rc = handle_worker_verify(false).await;

            assert_eq!(rc, 0);
        })
        .await;
    }

    #[tokio::test]
    async fn handle_worker_verify_fails_when_config_yaml_is_invalid() {
        in_temp_dir_async(|_| async move {
            cli_lockfile::WorkerLockfile::default()
                .write_to(cli_lockfile::lockfile_path())
                .unwrap();
            std::fs::write(
                "config.yaml",
                "workers:\n  - name: image-resize\n    config: [",
            )
            .unwrap();

            let rc = handle_worker_verify(false).await;

            assert_eq!(rc, 1);
        })
        .await;
    }

    #[tokio::test]
    async fn handle_worker_verify_rejects_binary_lock_missing_current_target_artifact() {
        in_temp_dir_async(|_| async move {
            let current_target = binary_download::current_target();
            let other_target = if current_target == "aarch64-apple-darwin" {
                "x86_64-unknown-linux-gnu"
            } else {
                "aarch64-apple-darwin"
            };
            let mut lock = cli_lockfile::WorkerLockfile::default();
            lock.workers.insert(
                "image-resize".to_string(),
                cli_lockfile::LockedWorker {
                    version: "1.0.0".to_string(),
                    worker_type: cli_lockfile::LockedWorkerType::Binary,
                    dependencies: Default::default(),
                    source: locked_binary_source(
                        other_target,
                        "https://workers.iii.dev/image-resize.tar.gz",
                        "a".repeat(64),
                    ),
                },
            );
            lock.write_to(cli_lockfile::lockfile_path()).unwrap();
            std::fs::write(
                "config.yaml",
                "\
workers:
  - name: image-resize
",
            )
            .unwrap();

            let rc = handle_worker_verify(false).await;

            assert_eq!(rc, 1);
        })
        .await;
    }

    #[test]
    fn worker_update_without_name_selects_roots_not_dependency_entries() {
        let mut lock = cli_lockfile::WorkerLockfile::default();
        lock.workers.insert(
            "root-worker".to_string(),
            cli_lockfile::LockedWorker {
                version: "1.0.0".to_string(),
                worker_type: cli_lockfile::LockedWorkerType::Binary,
                dependencies: [("helper".to_string(), "^1.0.0".to_string())].into(),
                source: locked_binary_source(
                    "aarch64-apple-darwin",
                    "https://workers.iii.dev/root.tar.gz",
                    "a".repeat(64),
                ),
            },
        );
        lock.workers.insert(
            "helper".to_string(),
            cli_lockfile::LockedWorker {
                version: "1.0.0".to_string(),
                worker_type: cli_lockfile::LockedWorkerType::Binary,
                dependencies: Default::default(),
                source: locked_binary_source(
                    "aarch64-apple-darwin",
                    "https://workers.iii.dev/helper.tar.gz",
                    "b".repeat(64),
                ),
            },
        );

        assert_eq!(
            locked_root_worker_names(&lock),
            vec!["root-worker".to_string()]
        );
    }

    #[test]
    fn legacy_registry_fallback_is_limited_to_compatibility_failures() {
        assert!(should_fallback_to_legacy_registry_error(
            "pdfkit",
            "Failed to parse worker graph: missing field `root`"
        ));
        assert!(should_fallback_to_legacy_registry_error(
            "pdfkit",
            "Worker 'pdfkit' not found"
        ));
        assert!(should_fallback_to_legacy_registry_error(
            "pdfkit",
            "Failed to resolve worker graph: HTTP 405 method not allowed"
        ));
        assert!(!should_fallback_to_legacy_registry_error(
            "pdfkit",
            "Failed to resolve worker graph: HTTP 500 internal error"
        ));
    }

    #[tokio::test]
    async fn handle_resolved_graph_add_rejects_invalid_worker_names_before_side_effects() {
        in_temp_dir_async(|_| async move {
            let target = binary_download::current_target();
            let mut binaries = StdHashMap::new();
            binaries.insert(
                target.to_string(),
                cli_registry::BinaryInfo {
                    url: "https://workers.iii.dev/evil.tar.gz".to_string(),
                    sha256: "a".repeat(64),
                },
            );

            let graph = cli_registry::ResolvedWorkerGraph {
                root: cli_registry::ResolvedRoot {
                    name: "../evil".to_string(),
                    version: "1.0.0".to_string(),
                },
                target: None,
                graph: vec![resolved_binary_worker("../evil", "1.0.0", binaries)],
                edges: Vec::new(),
            };

            let rc = handle_resolved_graph_add(&graph, true).await;

            assert_eq!(rc, 1);
            assert!(!std::path::Path::new("config.yaml").exists());
            assert!(!cli_lockfile::lockfile_path().exists());
        })
        .await;
    }

    #[tokio::test]
    async fn handle_resolved_graph_add_rejects_invalid_existing_lockfile_before_side_effects() {
        in_temp_dir_async(|dir| async move {
            let _env_guard = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let home = dir.join("home");
            std::fs::create_dir_all(&home).unwrap();
            let _home_guard = set_env_var_for_test("HOME", &home);

            let original_config = "\
workers:
  - name: existing-worker
";
            std::fs::write("config.yaml", original_config).unwrap();
            std::fs::write(cli_lockfile::lockfile_path(), "version: 2\nworkers: {}\n").unwrap();

            let archive = binary_archive("root-worker");
            let (base_url, server) = spawn_static_http_server(StdHashMap::from([(
                "GET /root-worker.tar.gz".to_string(),
                TestResponse {
                    status: 200,
                    content_type: "application/gzip",
                    body: archive.clone(),
                },
            )]))
            .await;

            let target = binary_download::current_target();
            let graph = cli_registry::ResolvedWorkerGraph {
                root: cli_registry::ResolvedRoot {
                    name: "root-worker".to_string(),
                    version: "1.0.0".to_string(),
                },
                target: Some(target.to_string()),
                graph: vec![resolved_binary_worker(
                    "root-worker",
                    "1.0.0",
                    StdHashMap::from([(
                        target.to_string(),
                        cli_registry::BinaryInfo {
                            url: format!("{base_url}/root-worker.tar.gz"),
                            sha256: sha256_hex(&archive),
                        },
                    )]),
                )],
                edges: Vec::new(),
            };

            let rc = handle_resolved_graph_add(&graph, true).await;

            assert_eq!(rc, 1);
            assert_eq!(
                std::fs::read_to_string("config.yaml").unwrap(),
                original_config
            );
            assert!(
                std::fs::read_to_string(cli_lockfile::lockfile_path())
                    .unwrap()
                    .contains("version: 2")
            );
            assert!(!home.join(".iii/workers/root-worker").exists());
            server.abort();
        })
        .await;
    }

    #[tokio::test]
    async fn handle_resolved_graph_add_restores_config_when_later_node_fails() {
        in_temp_dir_async(|dir| async move {
            let _env_guard = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let home = dir.join("home");
            std::fs::create_dir_all(&home).unwrap();
            let _home_guard = set_env_var_for_test("HOME", &home);

            let original_config = "\
workers:
  - name: existing-worker
    config:
      keep: true
";
            std::fs::write("config.yaml", original_config).unwrap();

            let root_archive = binary_archive("root-worker");
            let (base_url, server) = spawn_static_http_server(StdHashMap::from([(
                "GET /root-worker.tar.gz".to_string(),
                TestResponse {
                    status: 200,
                    content_type: "application/gzip",
                    body: root_archive.clone(),
                },
            )]))
            .await;

            let target = binary_download::current_target();
            let root_binaries = StdHashMap::from([(
                target.to_string(),
                cli_registry::BinaryInfo {
                    url: format!("{base_url}/root-worker.tar.gz"),
                    sha256: sha256_hex(&root_archive),
                },
            )]);
            let broken_binaries = StdHashMap::from([(
                target.to_string(),
                cli_registry::BinaryInfo {
                    url: format!("{base_url}/broken-dep.tar.gz"),
                    sha256: "a".repeat(64),
                },
            )]);

            let mut root = resolved_binary_worker("root-worker", "1.0.0", root_binaries);
            root.dependencies = [("broken-dep".to_string(), "^1.0.0".to_string())].into();
            let graph = cli_registry::ResolvedWorkerGraph {
                root: cli_registry::ResolvedRoot {
                    name: "root-worker".to_string(),
                    version: "1.0.0".to_string(),
                },
                target: Some(target.to_string()),
                graph: vec![
                    root,
                    resolved_binary_worker("broken-dep", "1.0.0", broken_binaries),
                ],
                edges: vec![cli_registry::ResolvedEdge {
                    from: "root-worker".to_string(),
                    to: "broken-dep".to_string(),
                    range: "^1.0.0".to_string(),
                }],
            };

            let rc = handle_resolved_graph_add(&graph, true).await;

            assert_eq!(rc, 1);
            assert_eq!(
                std::fs::read_to_string("config.yaml").unwrap(),
                original_config
            );
            assert!(
                !cli_lockfile::lockfile_path().exists(),
                "failed graph installs must not write iii.lock"
            );
            assert!(
                home.join(".iii/workers/root-worker").exists(),
                "first node should have installed before the later failure"
            );
            server.abort();
        })
        .await;
    }

    #[tokio::test]
    async fn handle_resolved_graph_add_installs_root_and_dependency_graph() {
        in_temp_dir_async(|dir| async move {
            let _env_guard = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let home = dir.join("home");
            std::fs::create_dir_all(&home).unwrap();
            let _home_guard = set_env_var_for_test("HOME", &home);

            let names = ["root-worker", "dep-one", "dep-two", "dep-three"];
            let mut routes = StdHashMap::new();
            let mut archives = StdHashMap::new();
            for name in names {
                let archive = binary_archive(name);
                routes.insert(
                    format!("GET /{name}.tar.gz"),
                    TestResponse {
                        status: 200,
                        content_type: "application/gzip",
                        body: archive.clone(),
                    },
                );
                archives.insert(name.to_string(), archive);
            }
            let (base_url, server) = spawn_static_http_server(routes).await;

            let target = binary_download::current_target();
            let other_target = if target == "aarch64-apple-darwin" {
                "x86_64-unknown-linux-gnu"
            } else {
                "aarch64-apple-darwin"
            };
            let mut graph_nodes = Vec::new();
            for name in names {
                let mut binaries = StdHashMap::new();
                binaries.insert(
                    target.to_string(),
                    cli_registry::BinaryInfo {
                        url: format!("{base_url}/{name}.tar.gz"),
                        sha256: sha256_hex(archives.get(name).unwrap()),
                    },
                );
                binaries.insert(
                    other_target.to_string(),
                    cli_registry::BinaryInfo {
                        url: format!("https://workers.iii.dev/{name}-{other_target}.tar.gz"),
                        sha256: "f".repeat(64),
                    },
                );
                let mut worker = resolved_binary_worker(name, "1.0.0", binaries);
                if name == "root-worker" {
                    worker.dependencies = [
                        ("dep-one".to_string(), "^1.0.0".to_string()),
                        ("dep-two".to_string(), "^1.0.0".to_string()),
                        ("dep-three".to_string(), "^1.0.0".to_string()),
                    ]
                    .into();
                }
                graph_nodes.push(worker);
            }

            let graph = cli_registry::ResolvedWorkerGraph {
                root: cli_registry::ResolvedRoot {
                    name: "root-worker".to_string(),
                    version: "1.0.0".to_string(),
                },
                target: Some(target.to_string()),
                graph: graph_nodes,
                edges: vec![
                    cli_registry::ResolvedEdge {
                        from: "root-worker".to_string(),
                        to: "dep-one".to_string(),
                        range: "^1.0.0".to_string(),
                    },
                    cli_registry::ResolvedEdge {
                        from: "root-worker".to_string(),
                        to: "dep-two".to_string(),
                        range: "^1.0.0".to_string(),
                    },
                    cli_registry::ResolvedEdge {
                        from: "root-worker".to_string(),
                        to: "dep-three".to_string(),
                        range: "^1.0.0".to_string(),
                    },
                ],
            };

            let rc = handle_resolved_graph_add(&graph, true).await;

            assert_eq!(rc, 0);
            let config = std::fs::read_to_string("config.yaml").unwrap();
            let lockfile = cli_lockfile::WorkerLockfile::read_from(cli_lockfile::lockfile_path())
                .expect("lockfile written");
            for name in names {
                assert!(
                    home.join(".iii/workers").join(name).exists(),
                    "{name} binary should be installed"
                );
                assert!(
                    config.contains(&format!("- name: {name}")),
                    "{name} should be added to config.yaml"
                );
                assert!(
                    lockfile.workers.contains_key(name),
                    "{name} should be pinned in iii.lock"
                );
                match lockfile.workers[name].source.as_ref().unwrap() {
                    cli_lockfile::LockedSource::Binary { artifacts } => {
                        assert!(artifacts.contains_key(target));
                        assert!(artifacts.contains_key(other_target));
                    }
                    other => panic!("expected binary source for {name}, got {:?}", other),
                }
            }
            server.abort();
        })
        .await;
    }

    #[tokio::test]
    async fn handle_managed_add_falls_back_to_legacy_download_when_resolve_is_unavailable() {
        in_temp_dir_async(|dir| async move {
            let _env_guard = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let home = dir.join("home");
            std::fs::create_dir_all(&home).unwrap();
            let _home_guard = set_env_var_for_test("HOME", &home);

            let worker_name = "legacy-worker";
            let archive = binary_archive(worker_name);
            let target = binary_download::current_target();
            let archive_sha = sha256_hex(&archive);
            let (base_url, server) = spawn_static_http_server_with_routes(|base_url| {
                let legacy_response = serde_json::json!({
                    "name": worker_name,
                    "type": "binary",
                    "version": "1.0.0",
                    "binaries": {
                        target: {
                            "url": format!("{base_url}/{worker_name}.tar.gz"),
                            "sha256": archive_sha
                        }
                    },
                    "config": {}
                });
                let mut routes = StdHashMap::new();
                routes.insert(
                    "POST /resolve".to_string(),
                    TestResponse {
                        status: 405,
                        content_type: "text/plain",
                        body: b"method not allowed".to_vec(),
                    },
                );
                routes.insert(
                    format!("GET /download/{worker_name}"),
                    TestResponse {
                        status: 200,
                        content_type: "application/json",
                        body: serde_json::to_vec(&legacy_response).unwrap(),
                    },
                );
                routes.insert(
                    format!("GET /{worker_name}.tar.gz"),
                    TestResponse {
                        status: 200,
                        content_type: "application/gzip",
                        body: archive,
                    },
                );
                routes
            })
            .await;
            let _api_guard = set_env_var_for_test("III_API_URL", &base_url);

            let rc = handle_managed_add(worker_name, true, false, false, false).await;

            assert_eq!(rc, 0);
            assert!(home.join(".iii/workers").join(worker_name).exists());
            assert!(
                std::fs::read_to_string("config.yaml")
                    .unwrap()
                    .contains("- name: legacy-worker")
            );
            assert!(
                !cli_lockfile::lockfile_path().exists(),
                "legacy /download fallback should preserve the old no-lockfile behavior"
            );
            server.abort();
        })
        .await;
    }

    #[tokio::test]
    async fn handle_managed_add_legacy_engine_response_persists_config_and_lock() {
        in_temp_dir_async(|_| async move {
            let _env_guard = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let (base_url, server) = spawn_static_http_server(StdHashMap::from([
                (
                    "POST /resolve".to_string(),
                    TestResponse {
                        status: 405,
                        content_type: "text/plain",
                        body: b"method not allowed".to_vec(),
                    },
                ),
                (
                    "GET /download/iii-exec".to_string(),
                    TestResponse {
                        status: 204,
                        content_type: "text/plain",
                        body: Vec::new(),
                    },
                ),
            ]))
            .await;
            let _api_guard = set_env_var_for_test("III_API_URL", &base_url);

            let rc = handle_managed_add("iii-exec", true, false, false, false).await;

            assert_eq!(rc, 0);
            assert!(
                std::fs::read_to_string("config.yaml")
                    .unwrap()
                    .contains("- name: iii-exec")
            );
            let lockfile = cli_lockfile::WorkerLockfile::read_from(cli_lockfile::lockfile_path())
                .expect("lockfile written");
            let worker = lockfile.workers.get("iii-exec").expect("engine pinned");
            assert_eq!(worker.version, "latest");
            assert!(matches!(
                worker.worker_type,
                cli_lockfile::LockedWorkerType::Engine
            ));
            assert!(worker.source.is_none());
            server.abort();
        })
        .await;
    }

    #[tokio::test]
    async fn fire_engine_telemetry_percent_encodes_version_query() {
        let _env_guard = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let _api_guard = set_env_var_for_test("III_API_URL", &base_url);
        let (tx, rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut buf = [0_u8; 4096];
            let n = stream.read(&mut buf).await.expect("read request");
            let request = String::from_utf8_lossy(&buf[..n]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or_default()
                .to_string();
            let _ = tx.send(path);
            let _ = stream
                .write_all(b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\n\r\n")
                .await;
        });

        fire_engine_telemetry("iii-http", "1.2.3+build.5").await;

        let path = rx.await.expect("request captured");
        assert_eq!(path, "/download/iii-http?version=1.2.3%2Bbuild.5");
        server.abort();
    }

    #[tokio::test]
    async fn handle_resolved_graph_add_persists_engine_worker_config_and_lock() {
        in_temp_dir_async(|_| async move {
            let graph = graph_with(resolved_engine_worker("iii-exec", "2.0.0"));

            let rc = handle_resolved_graph_add(&graph, true).await;

            assert_eq!(rc, 0);
            assert!(
                std::fs::read_to_string("config.yaml")
                    .unwrap()
                    .contains("- name: iii-exec")
            );
            let lockfile = cli_lockfile::WorkerLockfile::read_from(cli_lockfile::lockfile_path())
                .expect("lockfile written");
            let worker = lockfile.workers.get("iii-exec").expect("engine pinned");
            assert_eq!(worker.version, "2.0.0");
            assert!(matches!(
                worker.worker_type,
                cli_lockfile::LockedWorkerType::Engine
            ));
            assert!(worker.source.is_none());
        })
        .await;
    }

    #[tokio::test]
    async fn handle_worker_sync_fails_when_lockfile_is_absent() {
        in_temp_dir_async(|_| async move {
            let rc = handle_worker_sync(false).await;
            assert_eq!(rc, 1);
        })
        .await;
    }

    #[tokio::test]
    async fn handle_worker_sync_frozen_delegates_to_verify_and_fails_without_lockfile() {
        in_temp_dir_async(|_| async move {
            let rc = handle_worker_sync(true).await;
            assert_eq!(rc, 1);
        })
        .await;
    }

    #[tokio::test]
    async fn handle_worker_sync_frozen_passes_when_hash_matches() {
        // A fresh lock written by Lane A carries manifest_hash +
        // declared_dependencies. When the cwd manifest agrees, --frozen
        // falls through to verify, which in turn passes because config.yaml
        // is absent and the asymmetric verify design ignores extras.
        in_temp_dir_async(|dir| async move {
            use super::super::lockfile::{
                LockedSource, LockedWorker, LockedWorkerType, WorkerLockfile,
            };
            use super::super::sync::compute_manifest_hash;
            use std::collections::BTreeMap;

            let manifest = r#"name: my-project
dependencies:
  alpha: "^1.0.0"
"#;
            std::fs::write(dir.join("iii.worker.yaml"), manifest).unwrap();

            let declared = BTreeMap::from([("alpha".to_string(), "^1.0.0".to_string())]);
            let mut lock = WorkerLockfile {
                manifest_hash: Some(compute_manifest_hash(&declared)),
                declared_dependencies: Some(declared),
                ..Default::default()
            };
            lock.workers.insert(
                "alpha".to_string(),
                LockedWorker {
                    version: "1.0.0".to_string(),
                    worker_type: LockedWorkerType::Image,
                    dependencies: BTreeMap::new(),
                    source: Some(LockedSource::Image {
                        image: "ghcr.io/iii-hq/alpha@sha256:aaa".to_string(),
                    }),
                },
            );
            lock.write_to(&dir.join("iii.lock")).unwrap();

            let rc = handle_worker_sync(true).await;
            assert_eq!(rc, 0);
        })
        .await;
    }

    #[tokio::test]
    async fn handle_worker_sync_installs_binary_from_lockfile_without_config_mutation() {
        in_temp_dir_async(|dir| async move {
            let _env_guard = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let home = dir.join("home");
            std::fs::create_dir_all(&home).unwrap();
            let _home_guard = set_env_var_for_test("HOME", &home);

            let worker_name = "sync-worker";
            let archive = binary_archive(worker_name);
            let archive_sha = sha256_hex(&archive);
            let (base_url, server) = spawn_static_http_server(StdHashMap::from([(
                format!("GET /{worker_name}.tar.gz"),
                TestResponse {
                    status: 200,
                    content_type: "application/gzip",
                    body: archive,
                },
            )]))
            .await;

            let mut lock = cli_lockfile::WorkerLockfile::default();
            lock.workers.insert(
                worker_name.to_string(),
                cli_lockfile::LockedWorker {
                    version: "1.0.0".to_string(),
                    worker_type: cli_lockfile::LockedWorkerType::Binary,
                    dependencies: Default::default(),
                    source: locked_binary_source(
                        binary_download::current_target(),
                        &format!("{base_url}/{worker_name}.tar.gz"),
                        archive_sha,
                    ),
                },
            );
            lock.write_to(cli_lockfile::lockfile_path()).unwrap();
            let config = "workers:\n  - name: sync-worker\n";
            std::fs::write("config.yaml", config).unwrap();

            let rc = handle_worker_sync(false).await;

            assert_eq!(rc, 0);
            assert!(home.join(".iii/workers").join(worker_name).exists());
            assert_eq!(std::fs::read_to_string("config.yaml").unwrap(), config);
            assert!(
                !std::path::Path::new(".iii-worker.lock").exists(),
                "project operation lock should be removed after sync"
            );
            server.abort();
        })
        .await;
    }

    #[tokio::test]
    async fn handle_worker_sync_frozen_reports_drift_on_added_dep() {
        in_temp_dir_async(|dir| async move {
            use super::super::lockfile::WorkerLockfile;
            use super::super::sync::compute_manifest_hash;
            use std::collections::BTreeMap;

            // Lock was written with only `alpha`, but the manifest now
            // declares `alpha` + `beta`. --frozen must fail and name `beta`.
            let original = BTreeMap::from([("alpha".to_string(), "^1.0.0".to_string())]);
            let lock = WorkerLockfile {
                manifest_hash: Some(compute_manifest_hash(&original)),
                declared_dependencies: Some(original),
                ..Default::default()
            };
            lock.write_to(&dir.join("iii.lock")).unwrap();

            std::fs::write(
                dir.join("iii.worker.yaml"),
                "name: my-project\ndependencies:\n  alpha: \"^1.0.0\"\n  beta: \"^2.0.0\"\n",
            )
            .unwrap();

            let rc = handle_worker_sync(true).await;
            assert_eq!(rc, 1);
        })
        .await;
    }

    #[tokio::test]
    async fn handle_worker_sync_frozen_skips_drift_on_legacy_lock() {
        // Legacy lock (no manifest_hash) must NOT trigger drift detection.
        // It falls through to the existing verify path, which fails because
        // config.yaml listing is broken in a pristine tempdir; but the code
        // path we're testing is that we got to verify, not that sync
        // short-circuited with a drift error.
        in_temp_dir_async(|dir| async move {
            use super::super::lockfile::{
                LockedSource, LockedWorker, LockedWorkerType, WorkerLockfile,
            };
            use std::collections::BTreeMap;

            let mut lock = WorkerLockfile::default();
            lock.workers.insert(
                "alpha".to_string(),
                LockedWorker {
                    version: "1.0.0".to_string(),
                    worker_type: LockedWorkerType::Image,
                    dependencies: BTreeMap::new(),
                    source: Some(LockedSource::Image {
                        image: "ghcr.io/iii-hq/alpha@sha256:aaa".to_string(),
                    }),
                },
            );
            lock.write_to(&dir.join("iii.lock")).unwrap();
            // Manifest declares a dep not in the legacy lock. With no
            // manifest_hash stored, this MUST NOT trigger drift.
            std::fs::write(
                dir.join("iii.worker.yaml"),
                "name: my-project\ndependencies:\n  beta: \"^2.0.0\"\n",
            )
            .unwrap();

            let rc = handle_worker_sync(true).await;
            // Falls through to verify, which passes because config.yaml
            // is absent and verify is asymmetric w.r.t. extras.
            assert_eq!(rc, 0);
        })
        .await;
    }

    #[tokio::test]
    async fn handle_worker_sync_is_idempotent_for_matching_active_binary() {
        in_temp_dir_async(|dir| async move {
            let _env_guard = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let home = dir.join("home");
            std::fs::create_dir_all(&home).unwrap();
            let _home_guard = set_env_var_for_test("HOME", &home);

            let worker_name = "same-worker";
            let archive = binary_archive(worker_name);
            let archive_sha = sha256_hex(&archive);
            let extracted =
                binary_download::extract_binary_from_targz(worker_name, &archive).unwrap();
            let (base_url, server) = spawn_static_http_server(StdHashMap::from([(
                format!("GET /{worker_name}.tar.gz"),
                TestResponse {
                    status: 200,
                    content_type: "application/gzip",
                    body: archive,
                },
            )]))
            .await;

            let worker_dir = home.join(".iii/workers");
            std::fs::create_dir_all(&worker_dir).unwrap();
            let active_path = worker_dir.join(worker_name);
            std::fs::write(&active_path, &extracted).unwrap();

            let mut lock = cli_lockfile::WorkerLockfile::default();
            lock.workers.insert(
                worker_name.to_string(),
                cli_lockfile::LockedWorker {
                    version: "1.0.0".to_string(),
                    worker_type: cli_lockfile::LockedWorkerType::Binary,
                    dependencies: Default::default(),
                    source: locked_binary_source(
                        binary_download::current_target(),
                        &format!("{base_url}/{worker_name}.tar.gz"),
                        archive_sha,
                    ),
                },
            );
            lock.write_to(cli_lockfile::lockfile_path()).unwrap();

            let rc = handle_worker_sync(false).await;

            assert_eq!(rc, 0);
            assert_eq!(std::fs::read(&active_path).unwrap(), extracted);
            server.abort();
        })
        .await;
    }

    #[tokio::test]
    async fn handle_worker_sync_frozen_fails_when_manifest_missing() {
        // Lock has manifest_hash + declared_dependencies but
        // iii.worker.yaml doesn't exist. The user must see a distinct
        // error — NOT a misleading "drift" report — and rc must be 1.
        in_temp_dir_async(|dir| async move {
            use super::super::lockfile::WorkerLockfile;
            use super::super::sync::compute_manifest_hash;
            use std::collections::BTreeMap;

            let declared = BTreeMap::from([("alpha".to_string(), "^1.0.0".to_string())]);
            let lock = WorkerLockfile {
                manifest_hash: Some(compute_manifest_hash(&declared)),
                declared_dependencies: Some(declared),
                ..Default::default()
            };
            lock.write_to(&dir.join("iii.lock")).unwrap();
            // No iii.worker.yaml — that's the scenario.

            let rc = handle_worker_sync(true).await;
            assert_eq!(rc, 1);
        })
        .await;
    }

    #[tokio::test]
    async fn handle_worker_sync_hash_mismatch_preserves_existing_binary() {
        in_temp_dir_async(|dir| async move {
            let _env_guard = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let home = dir.join("home");
            std::fs::create_dir_all(&home).unwrap();
            let _home_guard = set_env_var_for_test("HOME", &home);

            let worker_name = "protected-worker";
            let archive = binary_archive(worker_name);
            let (base_url, server) = spawn_static_http_server(StdHashMap::from([(
                format!("GET /{worker_name}.tar.gz"),
                TestResponse {
                    status: 200,
                    content_type: "application/gzip",
                    body: archive,
                },
            )]))
            .await;

            let worker_dir = home.join(".iii/workers");
            std::fs::create_dir_all(&worker_dir).unwrap();
            let active_path = worker_dir.join(worker_name);
            std::fs::write(&active_path, b"old-good-binary").unwrap();

            let mut lock = cli_lockfile::WorkerLockfile::default();
            lock.workers.insert(
                worker_name.to_string(),
                cli_lockfile::LockedWorker {
                    version: "1.0.0".to_string(),
                    worker_type: cli_lockfile::LockedWorkerType::Binary,
                    dependencies: Default::default(),
                    source: locked_binary_source(
                        binary_download::current_target(),
                        &format!("{base_url}/{worker_name}.tar.gz"),
                        "0".repeat(64),
                    ),
                },
            );
            lock.write_to(cli_lockfile::lockfile_path()).unwrap();

            let rc = handle_worker_sync(false).await;

            assert_eq!(rc, 1);
            assert_eq!(std::fs::read(&active_path).unwrap(), b"old-good-binary");
            server.abort();
        })
        .await;
    }

    #[tokio::test]
    async fn handle_worker_sync_frozen_fails_when_manifest_malformed() {
        // Lock is fine; iii.worker.yaml has bad YAML. The user must see
        // a CorruptManifest error (with the parser reason) — not drift.
        in_temp_dir_async(|dir| async move {
            use super::super::lockfile::WorkerLockfile;
            use super::super::sync::compute_manifest_hash;
            use std::collections::BTreeMap;

            let declared = BTreeMap::from([("alpha".to_string(), "^1.0.0".to_string())]);
            let lock = WorkerLockfile {
                manifest_hash: Some(compute_manifest_hash(&declared)),
                declared_dependencies: Some(declared),
                ..Default::default()
            };
            lock.write_to(&dir.join("iii.lock")).unwrap();
            std::fs::write(
                dir.join("iii.worker.yaml"),
                "name: x\ndependencies: this-is-not-a-mapping\n",
            )
            .unwrap();

            let rc = handle_worker_sync(true).await;
            assert_eq!(rc, 1);
        })
        .await;
    }

    #[tokio::test]
    async fn handle_worker_sync_frozen_rejects_inconsistent_lock_at_read() {
        // A lock with manifest_hash that does NOT match its
        // declared_dependencies must be rejected at read time so the
        // empty-drift-report path is unreachable end-to-end.
        in_temp_dir_async(|dir| async move {
            use super::super::lockfile::MANIFEST_HASH_PREFIX;

            let bogus_hash = format!("{MANIFEST_HASH_PREFIX}{}", "f".repeat(64));
            std::fs::write(
                dir.join("iii.lock"),
                format!(
                    "version: 1\nmanifest_hash: \"{bogus_hash}\"\ndeclared_dependencies:\n  alpha: \"^1.0.0\"\nworkers: {{}}\n",
                ),
            )
            .unwrap();
            std::fs::write(
                dir.join("iii.worker.yaml"),
                "name: x\ndependencies:\n  alpha: \"^1.0.0\"\n",
            )
            .unwrap();

            let rc = handle_worker_sync(true).await;
            assert_eq!(rc, 1);
        })
        .await;
    }

    #[tokio::test]
    async fn handle_worker_sync_rejects_untrusted_lockfile_url() {
        in_temp_dir_async(|dir| async move {
            let _env_guard = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let home = dir.join("home");
            std::fs::create_dir_all(&home).unwrap();
            let _home_guard = set_env_var_for_test("HOME", &home);

            let worker_name = "untrusted-worker";
            let mut lock = cli_lockfile::WorkerLockfile::default();
            lock.workers.insert(
                worker_name.to_string(),
                cli_lockfile::LockedWorker {
                    version: "1.0.0".to_string(),
                    worker_type: cli_lockfile::LockedWorkerType::Binary,
                    dependencies: Default::default(),
                    source: locked_binary_source(
                        binary_download::current_target(),
                        // Non-HTTPS — still rejected by `validate_locked_artifact_url`.
                        "http://example.com/untrusted-worker.tar.gz",
                        "a".repeat(64),
                    ),
                },
            );
            lock.write_to(cli_lockfile::lockfile_path()).unwrap();

            let rc = handle_worker_sync(false).await;

            assert_eq!(rc, 1);
            assert!(!home.join(".iii/workers").join(worker_name).exists());
        })
        .await;
    }

    #[tokio::test]
    async fn handle_worker_verify_fails_when_lockfile_is_absent() {
        in_temp_dir_async(|_| async move {
            let rc = handle_worker_verify(false).await;
            assert_eq!(rc, 1);
        })
        .await;
    }

    #[tokio::test]
    async fn handle_worker_verify_strict_rejects_dependency_range_mismatch() {
        in_temp_dir_async(|_| async move {
            let mut lock = cli_lockfile::WorkerLockfile::default();
            lock.workers.insert(
                "root-worker".to_string(),
                cli_lockfile::LockedWorker {
                    version: "1.0.0".to_string(),
                    worker_type: cli_lockfile::LockedWorkerType::Binary,
                    dependencies: [("helper".to_string(), "^2.0.0".to_string())].into(),
                    source: locked_binary_source(
                        binary_download::current_target(),
                        "https://workers.iii.dev/root-worker.tar.gz",
                        "a".repeat(64),
                    ),
                },
            );
            lock.workers.insert(
                "helper".to_string(),
                cli_lockfile::LockedWorker {
                    version: "1.0.0".to_string(),
                    worker_type: cli_lockfile::LockedWorkerType::Binary,
                    dependencies: Default::default(),
                    source: locked_binary_source(
                        binary_download::current_target(),
                        "https://workers.iii.dev/helper.tar.gz",
                        "b".repeat(64),
                    ),
                },
            );
            lock.write_to(cli_lockfile::lockfile_path()).unwrap();

            let rc = handle_worker_verify(true).await;

            assert_eq!(rc, 1);
        })
        .await;
    }

    #[tokio::test]
    async fn handle_worker_update_rejects_invalid_worker_name_before_touching_disk() {
        in_temp_dir_async(|_| async move {
            // No lockfile exists, but the validation error must fire first
            // so the rc is 1 due to the name check, not the missing file.
            let rc = handle_worker_update(Some("../evil")).await;
            assert_eq!(rc, 1);
        })
        .await;
    }

    #[tokio::test]
    async fn handle_worker_update_fails_when_named_worker_not_in_lockfile() {
        in_temp_dir_async(|_| async move {
            // Write a minimal valid lockfile that does NOT contain "ghost".
            let lock = cli_lockfile::WorkerLockfile::default();
            lock.write_to(cli_lockfile::lockfile_path()).unwrap();

            let rc = handle_worker_update(Some("ghost")).await;
            assert_eq!(rc, 1);
        })
        .await;
    }

    #[tokio::test]
    async fn handle_worker_update_reports_empty_lockfile_without_resolving() {
        in_temp_dir_async(|_| async move {
            cli_lockfile::WorkerLockfile::default()
                .write_to(cli_lockfile::lockfile_path())
                .unwrap();

            let rc = handle_worker_update(None).await;

            assert_eq!(rc, 0);
        })
        .await;
    }

    #[test]
    fn binary_config_yaml_extracts_wrapped_registry_config() {
        let config = serde_json::json!({
            "name": "image-resize",
            "config": {
                "width": 200,
                "strategy": "scale-to-fit"
            }
        });

        let yaml = binary_config_yaml(&config).expect("wrapped config should render");

        assert!(yaml.contains("width: 200"));
        assert!(yaml.contains("strategy: scale-to-fit"));
        assert!(!yaml.contains("name: image-resize"));
    }

    #[test]
    fn binary_config_yaml_accepts_plain_registry_config() {
        let config = serde_json::json!({
            "width": 200,
            "strategy": "scale-to-fit"
        });

        let yaml = binary_config_yaml(&config).expect("plain config should render");

        assert!(yaml.contains("width: 200"));
        assert!(yaml.contains("strategy: scale-to-fit"));
    }

    #[tokio::test]
    async fn read_new_bytes_picks_up_appended_content() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("test.log");
        std::fs::write(&log, "line1\nline2\n").unwrap();

        let initial_len = file_len(&log);
        assert_eq!(initial_len, 12); // "line1\nline2\n"

        // No new bytes → offset unchanged
        let offset = read_new_bytes(&log, initial_len, false).await;
        assert_eq!(offset, initial_len);

        // Append new content
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        write!(f, "line3\nline4\n").unwrap();
        drop(f);

        let offset = read_new_bytes(&log, initial_len, false).await;
        assert_eq!(offset, file_len(&log));
    }

    #[tokio::test]
    async fn read_new_bytes_handles_truncated_file() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("test.log");
        std::fs::write(&log, "aaaa\nbbbb\n").unwrap();
        let old_offset = file_len(&log);

        // Truncate (simulates log rotation)
        std::fs::write(&log, "cc\n").unwrap();

        let offset = read_new_bytes(&log, old_offset, false).await;
        assert_eq!(offset, file_len(&log));
    }

    #[tokio::test]
    async fn read_new_bytes_holds_back_incomplete_line() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("test.log");
        std::fs::write(&log, "").unwrap();

        // Write an incomplete line (no trailing newline)
        std::fs::write(&log, "partial").unwrap();
        let offset = read_new_bytes(&log, 0, false).await;
        assert_eq!(offset, 0, "single incomplete line should be held back");

        // Complete the line and add another incomplete one
        std::fs::write(&log, "partial\nmore").unwrap();
        let offset = read_new_bytes(&log, 0, false).await;
        assert_eq!(
            offset, 8,
            "should consume 'partial\\n' but hold back 'more'"
        );
    }

    #[tokio::test]
    async fn read_new_bytes_missing_file_returns_offset() {
        let offset = read_new_bytes(std::path::Path::new("/no/such/file.log"), 42, false).await;
        assert_eq!(offset, 42);
    }

    #[test]
    fn pick_best_logs_dir_prefers_most_recent() {
        let root = tempfile::tempdir().unwrap();

        // Create two candidate dirs, both with stdout.log
        let stale_dir = root.path().join("stale");
        let fresh_dir = root.path().join("fresh");
        std::fs::create_dir_all(&stale_dir).unwrap();
        std::fs::create_dir_all(&fresh_dir).unwrap();

        std::fs::write(stale_dir.join("stdout.log"), "old content\n").unwrap();
        // Ensure a time gap so the modification times differ
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(fresh_dir.join("stdout.log"), "new content\n").unwrap();

        let result = pick_best_logs_dir(&[stale_dir.clone(), fresh_dir.clone()]).unwrap();
        assert_eq!(result, fresh_dir);
    }

    #[test]
    fn pick_best_logs_dir_skips_empty_files() {
        let root = tempfile::tempdir().unwrap();
        let empty_dir = root.path().join("empty");
        let content_dir = root.path().join("content");
        std::fs::create_dir_all(&empty_dir).unwrap();
        std::fs::create_dir_all(&content_dir).unwrap();

        std::fs::write(empty_dir.join("stdout.log"), "").unwrap();
        std::fs::write(content_dir.join("stdout.log"), "data\n").unwrap();

        let result = pick_best_logs_dir(&[empty_dir.clone(), content_dir.clone()]).unwrap();
        assert_eq!(result, content_dir);
    }

    #[test]
    fn pick_best_logs_dir_returns_none_when_no_content() {
        let root = tempfile::tempdir().unwrap();
        let dir_a = root.path().join("a");
        let dir_b = root.path().join("b");
        std::fs::create_dir_all(&dir_a).unwrap();
        // dir_b doesn't even exist

        std::fs::write(dir_a.join("stdout.log"), "").unwrap();

        assert!(pick_best_logs_dir(&[dir_a, dir_b]).is_none());
    }

    #[tokio::test]
    async fn follow_logs_exits_on_ctrl_c() {
        let dir = tempfile::tempdir().unwrap();
        let stdout_log = dir.path().join("stdout.log");
        let stderr_log = dir.path().join("stderr.log");
        std::fs::write(&stdout_log, "").unwrap();
        std::fs::write(&stderr_log, "").unwrap();

        // Send SIGINT to ourselves after a short delay so follow_logs unblocks
        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            #[cfg(unix)]
            {
                nix::sys::signal::raise(nix::sys::signal::Signal::SIGINT).unwrap();
            }
        });

        let code = follow_logs(&stdout_log, &stderr_log).await;
        assert_eq!(code, 0);
        handle.await.unwrap();
    }

    #[test]
    fn dir_size_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(dir_size(dir.path()), 0);
    }

    #[test]
    fn dir_size_with_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap(); // 5 bytes
        std::fs::write(dir.path().join("b.txt"), "world!").unwrap(); // 6 bytes
        assert_eq!(dir_size(dir.path()), 11);
    }

    #[test]
    fn discover_disk_worker_names_unions_managed_and_pids() {
        let dir = tempfile::tempdir().unwrap();
        let managed = dir.path().join("managed");
        let pids = dir.path().join("pids");
        std::fs::create_dir_all(managed.join("alpha")).unwrap();
        std::fs::create_dir_all(managed.join("beta")).unwrap();
        std::fs::create_dir_all(&pids).unwrap();
        std::fs::write(pids.join("gamma.pid"), "1234").unwrap();
        // Overlap: same name appears in both sources -> deduped.
        std::fs::write(pids.join("alpha.pid"), "5678").unwrap();

        let names = discover_disk_worker_names_in(&managed, &pids);
        assert_eq!(names, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn discover_disk_worker_names_handles_missing_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let names = discover_disk_worker_names_in(
            &dir.path().join("nope-managed"),
            &dir.path().join("nope-pids"),
        );
        assert!(names.is_empty());
    }

    #[test]
    fn discover_disk_worker_names_skips_non_pid_files_and_loose_files() {
        let dir = tempfile::tempdir().unwrap();
        let managed = dir.path().join("managed");
        let pids = dir.path().join("pids");
        std::fs::create_dir_all(managed.join("alpha")).unwrap();
        // Loose file under managed/ is not a worker dir, must be skipped.
        std::fs::write(managed.join("README.md"), "").unwrap();
        std::fs::create_dir_all(&pids).unwrap();
        std::fs::write(pids.join("beta.pid"), "1").unwrap();
        // Non-.pid files under pids/ must be skipped.
        std::fs::write(pids.join("notes.txt"), "").unwrap();
        // Empty-name guard: a bare ".pid" file must not yield "".
        std::fs::write(pids.join(".pid"), "").unwrap();

        let names = discover_disk_worker_names_in(&managed, &pids);
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn discover_running_from_ps_finds_binary_workers() {
        let workers = std::path::PathBuf::from("/home/u/.iii/workers");
        let managed = std::path::PathBuf::from("/home/u/.iii/managed");
        let ps = "\
/home/u/.iii/workers/image-resize\n\
/usr/bin/zsh\n\
/home/u/.iii/workers/another-binary --flag\n";
        let names = discover_running_worker_names_from_ps_output(ps, &workers, &managed);
        assert_eq!(names, vec!["another-binary", "image-resize"]);
    }

    #[test]
    fn discover_running_from_ps_finds_vm_boot_workers() {
        let workers = std::path::PathBuf::from("/home/u/.iii/workers");
        let managed = std::path::PathBuf::from("/home/u/.iii/managed");
        // Real-world cmdline shape from ~/.local/bin/iii-worker __vm-boot.
        let ps = "\
/home/u/.local/bin/iii-worker __vm-boot --rootfs /home/u/.iii/managed/todo-worker-python/rootfs --exec todo-worker --workdir /app --pid-file /home/u/.iii/managed/todo-worker-python/vm.pid --env FOO=bar\n\
/home/u/.local/bin/iii-worker __vm-boot --pid-file /home/u/.iii/managed/postgres/vm.pid --rootfs /home/u/.iii/managed/postgres/rootfs\n";
        let names = discover_running_worker_names_from_ps_output(ps, &workers, &managed);
        assert_eq!(names, vec!["postgres", "todo-worker-python"]);
    }

    #[test]
    fn discover_running_from_ps_dedups_across_patterns() {
        let workers = std::path::PathBuf::from("/h/.iii/workers");
        let managed = std::path::PathBuf::from("/h/.iii/managed");
        // Same name via both binary and vm-boot patterns -> single entry.
        let ps = "\
/h/.iii/workers/dual\n\
/h/.local/bin/iii-worker __vm-boot --pid-file /h/.iii/managed/dual/vm.pid\n";
        let names = discover_running_worker_names_from_ps_output(ps, &workers, &managed);
        assert_eq!(names, vec!["dual"]);
    }

    #[test]
    fn discover_running_from_ps_ignores_unrelated_processes_and_malformed_input() {
        let workers = std::path::PathBuf::from("/h/.iii/workers");
        let managed = std::path::PathBuf::from("/h/.iii/managed");
        let ps = "\
\n\
   \n\
/usr/bin/python\n\
/h/.local/bin/iii-worker __serve\n\
/h/.local/bin/iii-worker __vm-boot --rootfs /h/.iii/managed/x/rootfs\n\
/h/.local/bin/iii-worker __vm-boot --pid-file\n\
/h/.local/bin/iii-worker __vm-boot --pid-file /elsewhere/vm.pid\n";
        // No `--pid-file <path>` matching managed prefix → no orphans found.
        let names = discover_running_worker_names_from_ps_output(ps, &workers, &managed);
        assert!(names.is_empty(), "got unexpected names: {names:?}");
    }

    #[test]
    fn discover_running_from_proc_style_cmdlines() {
        // Simulates Linux /proc/<pid>/cmdline shape: NULs → spaces → joined
        // with newlines exactly the way collect_cmdlines() produces. Verifies
        // the parser is identical regardless of source platform.
        let workers = std::path::PathBuf::from("/h/.iii/workers");
        let managed = std::path::PathBuf::from("/h/.iii/managed");
        let proc_like = [
            "/h/.iii/workers/image-resize",
            "/h/.local/bin/iii-worker __vm-boot --pid-file /h/.iii/managed/todo/vm.pid --rootfs /h/.iii/managed/todo/rootfs",
            "/usr/bin/python3 server.py",
        ]
        .join("\n");
        let names = discover_running_worker_names_from_ps_output(&proc_like, &workers, &managed);
        assert_eq!(names, vec!["image-resize", "todo"]);
    }

    #[test]
    fn find_worker_pid_returns_first_matching_process() {
        let workers = std::path::PathBuf::from("/h/.iii/workers");
        let managed = std::path::PathBuf::from("/h/.iii/managed");
        let processes = vec![
            (12, "/usr/bin/zsh".to_string()),
            (
                42,
                "/h/.local/bin/iii-worker __vm-boot --pid-file /h/.iii/managed/todo/vm.pid"
                    .to_string(),
            ),
            (77, "/h/.iii/workers/image-resize".to_string()),
        ];
        assert_eq!(
            find_worker_pid_in_processes(&processes, "todo", &workers, &managed),
            Some(42)
        );
        assert_eq!(
            find_worker_pid_in_processes(&processes, "image-resize", &workers, &managed),
            Some(77)
        );
        assert_eq!(
            find_worker_pid_in_processes(&processes, "no-such-worker", &workers, &managed),
            None
        );
    }

    #[test]
    fn find_worker_pid_returns_none_for_empty_input() {
        let workers = std::path::PathBuf::from("/h/.iii/workers");
        let managed = std::path::PathBuf::from("/h/.iii/managed");
        assert_eq!(
            find_worker_pid_in_processes(&[], "anything", &workers, &managed),
            None
        );
    }

    #[test]
    fn resolve_orphan_type_managed_takes_priority() {
        let tmp = tempfile::tempdir().unwrap();
        let managed = tmp.path().join("managed");
        let pids = tmp.path().join("pids");
        let workers = tmp.path().join("workers");
        std::fs::create_dir_all(managed.join("dual")).unwrap();
        std::fs::create_dir_all(&pids).unwrap();
        std::fs::write(pids.join("dual.pid"), "1").unwrap();
        // managed/ wins because the directory shape carries more information
        // (rootfs, logs, etc.) than a bare pidfile.
        assert_eq!(
            resolve_orphan_type("dual", &managed, &pids, &workers),
            "managed"
        );
    }

    #[test]
    fn resolve_orphan_type_binary_via_pidfile() {
        let tmp = tempfile::tempdir().unwrap();
        let managed = tmp.path().join("managed");
        let pids = tmp.path().join("pids");
        let workers = tmp.path().join("workers");
        std::fs::create_dir_all(&pids).unwrap();
        std::fs::write(pids.join("img-resize.pid"), "1234").unwrap();
        assert_eq!(
            resolve_orphan_type("img-resize", &managed, &pids, &workers),
            "binary"
        );
    }

    #[test]
    fn resolve_orphan_type_binary_via_workers_executable() {
        let tmp = tempfile::tempdir().unwrap();
        let managed = tmp.path().join("managed");
        let pids = tmp.path().join("pids");
        let workers = tmp.path().join("workers");
        std::fs::create_dir_all(&workers).unwrap();
        std::fs::write(workers.join("img-resize"), b"#!/bin/sh\n").unwrap();
        // No pidfile, only the executable -- still recognisable as binary.
        assert_eq!(
            resolve_orphan_type("img-resize", &managed, &pids, &workers),
            "binary"
        );
    }

    #[test]
    fn resolve_orphan_type_unknown_when_only_ps_evidence() {
        let tmp = tempfile::tempdir().unwrap();
        let managed = tmp.path().join("managed");
        let pids = tmp.path().join("pids");
        let workers = tmp.path().join("workers");
        // Nothing on disk under any of the three roots: a worker that is alive
        // in ps but has had every artifact cleaned up. Honest answer is "?".
        assert_eq!(resolve_orphan_type("ghost", &managed, &pids, &workers), "?");
    }

    #[test]
    fn dir_size_nested() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("nested.txt"), "abc").unwrap(); // 3 bytes
        std::fs::write(dir.path().join("top.txt"), "de").unwrap(); // 2 bytes
        assert_eq!(dir_size(dir.path()), 5);
    }

    #[test]
    fn dir_size_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("does_not_exist");
        assert_eq!(dir_size(&gone), 0);
    }

    #[test]
    fn is_worker_running_no_pid_files() {
        // Worker name that certainly has no PID files on this system
        assert!(!is_worker_running("__iii_test_nonexistent_worker_12345__"));
    }

    #[test]
    fn is_worker_running_stale_pid_file() {
        // Create a fake PID file with a PID that doesn't exist, using tempdir
        let dir = tempfile::tempdir().unwrap();
        let pid_dir = dir.path().join("worker");
        std::fs::create_dir_all(&pid_dir).unwrap();
        let pid_file = pid_dir.join("worker.pid");
        // Use PID 2000000000 which almost certainly doesn't exist
        std::fs::write(&pid_file, "2000000000").unwrap();

        // Read the PID and verify it's considered dead (same logic as is_worker_running)
        let pid_str = std::fs::read_to_string(&pid_file).unwrap();
        let pid: u32 = pid_str.trim().parse().unwrap();
        #[cfg(unix)]
        {
            use nix::sys::signal::kill;
            use nix::unistd::Pid;
            assert!(kill(Pid::from_raw(pid as i32), None).is_err());
        }
        // Tempdir auto-cleans on drop
    }

    #[test]
    fn delete_worker_artifacts_nothing_to_delete() {
        let freed = delete_worker_artifacts("__iii_test_no_artifacts_exist__");
        assert_eq!(freed, 0);
    }

    #[test]
    fn image_cache_dir_consistent() {
        let dir1 = image_cache_dir("ghcr.io/org/worker:1.0");
        let dir2 = image_cache_dir("ghcr.io/org/worker:1.0");
        assert_eq!(dir1, dir2);
        // Different refs produce different dirs
        let dir3 = image_cache_dir("ghcr.io/org/worker:2.0");
        assert_ne!(dir1, dir3);
    }

    #[test]
    fn confirm_clear_returns_false_on_empty_stdin() {
        // confirm_clear reads from stdin — in test context stdin is closed/empty,
        // so read returns 0 bytes which should not match "y"
        // We can't easily call confirm_clear (it blocks on stdin), but we can
        // verify the logic inline:
        let input = "";
        assert!(!input.trim().eq_ignore_ascii_case("y"));
        let input = "n\n";
        assert!(!input.trim().eq_ignore_ascii_case("y"));
        let input = "y\n";
        assert!(input.trim().eq_ignore_ascii_case("y"));
        let input = "Y\n";
        assert!(input.trim().eq_ignore_ascii_case("y"));
        // CR-only line endings (raw/non-canonical terminal mode)
        let input = "y\r";
        assert!(input.trim().eq_ignore_ascii_case("y"));
        let input = "Y\r";
        assert!(input.trim().eq_ignore_ascii_case("y"));
        let input = "y\r\n";
        assert!(input.trim().eq_ignore_ascii_case("y"));
        let input = "\r";
        assert!(!input.trim().eq_ignore_ascii_case("y"));
    }

    #[test]
    fn delete_worker_artifacts_removes_binary_file() {
        // Test the legacy single-file binary path
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("test-worker");
        std::fs::write(&binary, "fake binary content 1234567890").unwrap(); // 30 bytes

        // delete_worker_artifacts operates on ~/.iii/workers/{name}
        // We can't easily redirect it, but we can test dir_size + remove_dir_all directly
        let size_before = dir_size(dir.path());
        assert!(size_before >= 30);

        // Verify the file exists, then remove and check
        assert!(binary.exists());
        std::fs::remove_file(&binary).unwrap();
        assert!(!binary.exists());
        assert_eq!(dir_size(dir.path()), 0);
    }

    #[test]
    fn delete_worker_artifacts_removes_nested_binary_dir() {
        let dir = tempfile::tempdir().unwrap();
        let worker_dir = dir.path().join("my-worker");
        std::fs::create_dir_all(&worker_dir).unwrap();
        std::fs::write(worker_dir.join("binary"), "executable bytes").unwrap();
        std::fs::write(worker_dir.join("worker.pid"), "12345").unwrap();

        let size = dir_size(&worker_dir);
        assert!(size > 0);

        // Simulate what delete_worker_artifacts does for binary dirs
        std::fs::remove_dir_all(&worker_dir).unwrap();
        assert!(!worker_dir.exists());
    }

    #[test]
    fn is_worker_running_invalid_pid_content() {
        // PID file with non-numeric content should return false
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("worker.pid");
        std::fs::write(&pid_file, "not-a-number").unwrap();

        // parse::<u32>() will fail, so the loop continues and returns false
        let content = std::fs::read_to_string(&pid_file).unwrap();
        assert!(content.trim().parse::<u32>().is_err());
    }

    #[test]
    fn is_worker_running_empty_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("worker.pid");
        std::fs::write(&pid_file, "").unwrap();

        let content = std::fs::read_to_string(&pid_file).unwrap();
        assert!(content.trim().parse::<u32>().is_err());
    }

    #[test]
    fn kill_stale_worker_removes_pid_files() {
        // Create fake PID files with a dead PID
        let dir = tempfile::tempdir().unwrap();
        let managed_dir = dir.path().join("managed").join("test-worker");
        std::fs::create_dir_all(&managed_dir).unwrap();
        std::fs::write(managed_dir.join("vm.pid"), "2000000000").unwrap();

        let pids_dir = dir.path().join("pids");
        std::fs::create_dir_all(&pids_dir).unwrap();
        std::fs::write(pids_dir.join("test-worker.pid"), "2000000000").unwrap();

        // Verify files exist
        assert!(managed_dir.join("vm.pid").exists());
        assert!(pids_dir.join("test-worker.pid").exists());

        // kill_stale_worker uses real ~/.iii paths, so we test the logic directly:
        // dead PID → signal-0 fails → no kill attempt → file removed
        for pid_file in [managed_dir.join("vm.pid"), pids_dir.join("test-worker.pid")] {
            if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
                if let Ok(pid) = pid_str.trim().parse::<i32>() {
                    #[cfg(unix)]
                    {
                        use nix::sys::signal::kill;
                        use nix::unistd::Pid;
                        // PID 2000000000 should not be alive
                        assert!(kill(Pid::from_raw(pid), None).is_err());
                    }
                }
                let _ = std::fs::remove_file(&pid_file);
            }
        }

        // Files should be cleaned up
        assert!(!managed_dir.join("vm.pid").exists());
        assert!(!pids_dir.join("test-worker.pid").exists());
    }

    #[tokio::test]
    async fn kill_stale_worker_no_op_when_no_pid_files() {
        let _h = super::super::test_support::lock_home();
        // Should not panic when no PID files exist
        kill_stale_worker("__iii_test_nonexistent_99999__").await;
    }

    #[tokio::test]
    async fn kill_stale_worker_removes_watch_pid_file() {
        let _h = super::super::test_support::lock_home();
        // Writes a fake `watch.pid` with a highly unlikely-to-be-alive
        // PID, then verifies `kill_stale_worker` reaps it from the
        // pid-file list introduced when the source watcher sidecar
        // landed. Uses a unique worker name to avoid collisions with
        // any real workers on the developer's machine.
        let home = dirs::home_dir().unwrap_or_default();
        let worker_name = "__iii_test_watch_pid_cleanup__";
        let managed_dir = home.join(".iii/managed").join(worker_name);
        let _ = std::fs::create_dir_all(&managed_dir);
        let watch_pidfile = managed_dir.join("watch.pid");
        std::fs::write(&watch_pidfile, "2000000000").unwrap();
        assert!(watch_pidfile.exists());

        kill_stale_worker(worker_name).await;

        assert!(
            !watch_pidfile.exists(),
            "watch.pid should be reaped by kill_stale_worker"
        );

        let _ = std::fs::remove_dir_all(&managed_dir);
    }

    #[tokio::test]
    async fn reap_source_watcher_removes_pid_file() {
        let _h = super::super::test_support::lock_home();
        // Exercises the stop-path helper used by `handle_managed_stop`
        // to tear down the watcher sidecar before stopping the VM. A
        // dead PID in watch.pid should still produce a clean remove
        // (signal(0) returns ESRCH, kill_pid_with_grace no-ops, file
        // is unlinked unconditionally).
        let home = dirs::home_dir().unwrap_or_default();
        let worker_name = "__iii_test_reap_watcher__";
        let managed_dir = home.join(".iii/managed").join(worker_name);
        let _ = std::fs::create_dir_all(&managed_dir);
        let watch_pidfile = managed_dir.join("watch.pid");
        std::fs::write(&watch_pidfile, "2000000001").unwrap();
        assert!(watch_pidfile.exists());

        reap_source_watcher(worker_name).await;

        assert!(
            !watch_pidfile.exists(),
            "watch.pid should be removed by reap_source_watcher"
        );

        let _ = std::fs::remove_dir_all(&managed_dir);
    }

    #[tokio::test]
    async fn reap_source_watcher_no_op_when_no_pid_file() {
        let _h = super::super::test_support::lock_home();
        // Idempotent on the cold path — no watch.pid, nothing to do,
        // no panic.
        reap_source_watcher("__iii_test_reap_watcher_nonexistent__").await;
    }

    #[tokio::test]
    async fn reap_source_watcher_handles_garbage_pid_content() {
        let _h = super::super::test_support::lock_home();
        // Parse failure must not prevent file removal.
        let home = dirs::home_dir().unwrap_or_default();
        let worker_name = "__iii_test_reap_watcher_garbage__";
        let managed_dir = home.join(".iii/managed").join(worker_name);
        let _ = std::fs::create_dir_all(&managed_dir);
        let watch_pidfile = managed_dir.join("watch.pid");
        std::fs::write(&watch_pidfile, "not-a-pid").unwrap();

        reap_source_watcher(worker_name).await;

        assert!(!watch_pidfile.exists());
        let _ = std::fs::remove_dir_all(&managed_dir);
    }

    #[tokio::test]
    async fn kill_stale_worker_handles_invalid_pid_content() {
        let _h = super::super::test_support::lock_home();
        // Use real function with a worker name that won't collide
        // The function should handle garbage content gracefully
        let home = dirs::home_dir().unwrap_or_default();
        let pids_dir = home.join(".iii/pids");
        let _ = std::fs::create_dir_all(&pids_dir);
        let pid_file = pids_dir.join("__iii_test_garbage_pid__.pid");
        std::fs::write(&pid_file, "not-a-number").unwrap();

        kill_stale_worker("__iii_test_garbage_pid__").await;

        // File should still be removed even with garbage content
        assert!(!pid_file.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn kill_stale_worker_ignores_symlinked_pidfile() {
        let _h = super::super::test_support::lock_home();
        // A pre-planted symlink at the pidfile location must not be
        // followed: read_pid opens with O_NOFOLLOW and returns None, so
        // we skip the kill. The symlink itself is still removed so
        // subsequent starts aren't jammed up by stale state.
        let home = dirs::home_dir().unwrap_or_default();
        let worker_name = "__iii_test_symlink_pidfile__";
        let managed_dir = home.join(".iii/managed").join(worker_name);
        // Scrub leftover state from an aborted prior run so `symlink`
        // below (which errors EEXIST if the path already exists) and
        // the post-run assertions see a clean slate.
        let _ = std::fs::remove_dir_all(&managed_dir);
        let _ = std::fs::create_dir_all(&managed_dir);

        // Attacker-controlled file we must NOT overwrite or target.
        let sensitive = managed_dir.join("sensitive");
        std::fs::write(&sensitive, "DO-NOT-TOUCH").unwrap();

        let watch_pidfile = managed_dir.join("watch.pid");
        std::os::unix::fs::symlink(&sensitive, &watch_pidfile).unwrap();

        kill_stale_worker(worker_name).await;

        // Symlink removed, target untouched.
        assert!(!watch_pidfile.exists(), "symlink should be cleaned up");
        assert_eq!(
            std::fs::read_to_string(&sensitive).unwrap(),
            "DO-NOT-TOUCH",
            "symlink target must not be read/modified"
        );

        let _ = std::fs::remove_dir_all(&managed_dir);
    }

    #[test]
    fn binary_pid_path_uses_pids_dir() {
        // Verify the PID path for binary workers doesn't conflict with the binary file
        let home = dirs::home_dir().unwrap_or_default();
        let binary_path = home.join(".iii/workers/some-worker");
        let pid_path = home.join(".iii/pids/some-worker.pid");

        // These should be different paths — binary at workers/{name}, PID at pids/{name}.pid
        assert_ne!(binary_path.parent().unwrap(), pid_path.parent().unwrap());
        assert!(pid_path.to_string_lossy().ends_with(".pid"));
    }

    #[test]
    fn image_cache_dir_deterministic_hash() {
        // Same ref always produces same path
        let a = image_cache_dir("ghcr.io/org/worker:1.0");
        let b = image_cache_dir("ghcr.io/org/worker:1.0");
        assert_eq!(a, b);

        // Path ends with a hex string (16 chars for 8 bytes)
        let hash_component = a.file_name().unwrap().to_str().unwrap();
        assert_eq!(hash_component.len(), 16);
        assert!(hash_component.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn image_cache_dir_under_iii_images() {
        let dir = image_cache_dir("test:latest");
        let path_str = dir.to_string_lossy();
        assert!(path_str.contains(".iii/images/") || path_str.contains(".iii\\images\\"));
    }

    #[tokio::test]
    async fn handle_managed_add_routes_local_path() {
        in_temp_dir_async(|dir| async move {
            // Create a minimal project directory with package.json
            let proj = dir.join("test-local-worker");
            std::fs::create_dir_all(&proj).unwrap();
            std::fs::write(proj.join("package.json"), r#"{"name":"test"}"#).unwrap();

            let path_str = proj.to_string_lossy().to_string();
            let exit_code = handle_managed_add(&path_str, false, false, false, false).await;
            assert_eq!(exit_code, 0, "should succeed for valid local path");

            let content = std::fs::read_to_string("config.yaml").unwrap();
            assert!(
                content.contains("worker_path:"),
                "should write worker_path field, got:\n{}",
                content
            );
            assert!(
                !content.contains("image:"),
                "should not have image field, got:\n{}",
                content
            );
        })
        .await;
    }

    #[tokio::test]
    async fn handle_managed_add_local_path_rejects_nonexistent() {
        in_temp_dir_async(|_dir| async move {
            let exit_code =
                handle_managed_add("./nonexistent-path-12345", false, false, false, false).await;
            assert_eq!(exit_code, 1, "should fail for nonexistent local path");
        })
        .await;
    }

    #[tokio::test]
    async fn handle_managed_add_local_path_force_replaces() {
        in_temp_dir_async(|dir| async move {
            // Create project directory
            let proj = dir.join("force-worker");
            std::fs::create_dir_all(&proj).unwrap();
            std::fs::write(proj.join("package.json"), r#"{"name":"force-test"}"#).unwrap();

            let path_str = proj.to_string_lossy().to_string();

            // First add
            let exit_code = handle_managed_add(&path_str, false, false, false, false).await;
            assert_eq!(exit_code, 0);
            assert!(
                std::fs::read_to_string("config.yaml")
                    .unwrap()
                    .contains("worker_path:")
            );

            // Force re-add
            let exit_code = handle_managed_add(&path_str, false, true, false, false).await;
            assert_eq!(exit_code, 0, "force re-add should succeed");

            let content = std::fs::read_to_string("config.yaml").unwrap();
            assert!(
                content.contains("worker_path:"),
                "should still have worker_path after force, got:\n{}",
                content
            );
        })
        .await;
    }

    // ------------------------------------------------------------------
    // install_manifest_dependencies / merge_resolved_graphs
    // ------------------------------------------------------------------

    fn graph_with_nodes(
        root_name: &str,
        root_version: &str,
        nodes: Vec<cli_registry::ResolvedWorker>,
    ) -> cli_registry::ResolvedWorkerGraph {
        cli_registry::ResolvedWorkerGraph {
            root: cli_registry::ResolvedRoot {
                name: root_name.to_string(),
                version: root_version.to_string(),
            },
            target: None,
            graph: nodes,
            edges: Vec::new(),
        }
    }

    #[tokio::test]
    async fn install_manifest_dependencies_empty_is_noop() {
        let deps: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
        let result = super::install_manifest_dependencies(&deps, true).await;
        assert!(result.is_ok(), "empty deps must succeed as noop");
    }

    #[tokio::test]
    async fn install_manifest_dependencies_propagates_resolve_error() {
        let prev = std::env::var("III_API_URL").ok();
        unsafe { std::env::set_var("III_API_URL", "http://127.0.0.1:1") };
        let mut deps = std::collections::BTreeMap::new();
        deps.insert("math-worker".to_string(), "^0.1.0".to_string());
        let result = super::install_manifest_dependencies(&deps, true).await;
        match prev {
            Some(v) => unsafe { std::env::set_var("III_API_URL", v) },
            None => unsafe { std::env::remove_var("III_API_URL") },
        }
        assert!(
            result.is_err(),
            "unreachable registry must surface an error"
        );
        let err = result.unwrap_err();
        assert!(
            !err.contains("filters prereleases"),
            "stable range must not emit the prerelease hint; got: {err}"
        );
    }

    #[tokio::test]
    async fn install_manifest_dependencies_emits_prerelease_hint() {
        let prev = std::env::var("III_API_URL").ok();
        unsafe { std::env::set_var("III_API_URL", "http://127.0.0.1:1") };
        let mut deps = std::collections::BTreeMap::new();
        deps.insert("math-worker".to_string(), "1.0.0-beta.1".to_string());
        let result = super::install_manifest_dependencies(&deps, true).await;
        match prev {
            Some(v) => unsafe { std::env::set_var("III_API_URL", v) },
            None => unsafe { std::env::remove_var("III_API_URL") },
        }
        let err = result.unwrap_err();
        assert!(
            err.contains("filters prereleases"),
            "prerelease range must trigger the registry-filter hint; got: {err}"
        );
    }

    #[test]
    fn merge_graphs_unifies_shared_nodes_at_same_version() {
        let a = graph_with_nodes(
            "a",
            "1.0.0",
            vec![
                resolved_binary_worker("a", "1.0.0", StdHashMap::new()),
                resolved_binary_worker("shared", "1.2.3", StdHashMap::new()),
            ],
        );
        let b = graph_with_nodes(
            "b",
            "1.0.0",
            vec![
                resolved_binary_worker("b", "1.0.0", StdHashMap::new()),
                resolved_binary_worker("shared", "1.2.3", StdHashMap::new()),
            ],
        );
        let merged =
            super::merge_resolved_graphs(vec![("a".to_string(), a), ("b".to_string(), b)]).unwrap();
        let names: std::collections::BTreeSet<_> =
            merged.graph.iter().map(|n| n.name.clone()).collect();
        assert_eq!(
            names,
            ["a", "b", "shared"].iter().map(|s| s.to_string()).collect()
        );
    }

    #[test]
    fn merge_graphs_errors_on_cross_graph_version_mismatch() {
        let a = graph_with_nodes(
            "a",
            "1.0.0",
            vec![
                resolved_binary_worker("a", "1.0.0", StdHashMap::new()),
                resolved_binary_worker("shared", "1.2.3", StdHashMap::new()),
            ],
        );
        let b = graph_with_nodes(
            "b",
            "1.0.0",
            vec![
                resolved_binary_worker("b", "1.0.0", StdHashMap::new()),
                resolved_binary_worker("shared", "2.0.0", StdHashMap::new()),
            ],
        );
        let err = super::merge_resolved_graphs(vec![("a".to_string(), a), ("b".to_string(), b)])
            .unwrap_err();
        assert!(
            err.contains("shared") && err.contains("1.2.3") && err.contains("2.0.0"),
            "error should name the conflicting dep + both versions; got: {err}",
        );
    }
}
