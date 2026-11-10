// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The explicit VM mode: which containers run in a VM (`scripts.run: iii sandbox run`) and need
//! the sandbox adapter, the generated container that installs it, the rewrite of the containers
//! 0.26 ran in a VM (spec 7.3), and what Compose still asks the adapter about a VM that
//! `iii sandbox run` boots: whether it runs, its stop and its destroy. Compose boots no VM itself.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    process::ExitStatus,
    time::Duration,
};

use iii_sandbox_contract::{
    self as contract, DEFAULT_ADAPTER, SandboxErrorCode, State, WORKER_DESTROY, WORKER_STATUS,
    WORKER_STOP, WorkerDestroyRequest, WorkerDestroyResponse, WorkerStartResponse, WorkerStatus,
    WorkerStatusRequest, WorkerStopRequest, adapter_function_id, code_of,
};
use iii_sdk::{IIIClient, protocol::CallRequest, runtime::IIIConnectionState};
use sha2::{Digest, Sha256};

use crate::{
    config::{ComposeFile, Container, WorkerSource},
    edit::{self, NewContainer, Source},
    engine::EngineClient,
    error::{ComposeError, Result},
    lifecycle::LifecycleCtx,
    lockfile::PreparedLock,
    manifest::read_manifest,
    process::Outcome,
    sandbox_cli,
    state::{Reconciliation, VmRecord},
};

/// The adapter release range that implements contract major [`contract::CONTRACT_MAJOR`] with
/// `worker.build` (spec 6.6), changed together with it.
pub const ADAPTER_RANGE: &str = "^0.4";

/// The `scripts.run` that boots a container's VM.
pub const SANDBOX_RUN: &str = "iii sandbox run";
/// The `scripts.build` that builds a container's VM image.
pub const SANDBOX_BUILD: &str = "iii sandbox build";

/// Whether `command` is `iii sandbox run`, with or without arguments.
pub fn is_sandbox_run(command: &str) -> bool {
    command
        .split_whitespace()
        .take(3)
        .eq(SANDBOX_RUN.split_whitespace())
}

/// Whether `command` is `iii sandbox build`, with or without arguments.
pub fn is_sandbox_build(command: &str) -> bool {
    command
        .split_whitespace()
        .take(3)
        .eq(SANDBOX_BUILD.split_whitespace())
}

/// Why a project needs the sandbox adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Need {
    /// A registry bundle container that runs in a VM, or that the migration will put in one.
    VmBundle(String),
    /// A `path://` container that runs in a VM, or that the migration will put in one.
    VmLocal(String),
    /// The file sets `engine.sandbox`, bound to its own namespace.
    Explicit,
}

/// The VM containers in declaration order, then `engine.sandbox` when it is
/// bound to `project_namespace`, the one the project runs in (`--namespace`
/// wins over the file's). Only a lock-attached package is known to be a
/// bundle, so call it after `lockfile::attach` or `prepare`.
pub fn needs(file: &ComposeFile, project_namespace: &str) -> Vec<Need> {
    let mut needs = vm_needs(file);
    // A binding to another namespace is served there: an adapter installed
    // here would never be reached.
    if file
        .engine
        .as_ref()
        .and_then(|engine| engine.sandbox.as_ref())
        .is_some_and(|sandbox| {
            sandbox
                .namespace
                .as_deref()
                .is_none_or(|namespace| namespace == project_namespace)
        })
    {
        needs.push(Need::Explicit);
    }
    needs
}

/// The VM containers in declaration order: those whose `scripts.run` is `iii sandbox run`, and
/// those [`migration_due`] will rewrite into it (spec 7.2).
fn vm_needs(file: &ComposeFile) -> Vec<Need> {
    file.containers
        .iter()
        .filter(|(_, container)| runs_in_vm(container) || migration_due(file, container))
        .map(|(key, container)| match container.worker {
            WorkerSource::Package { .. } => Need::VmBundle(key.clone()),
            WorkerSource::Path { .. } => Need::VmLocal(key.clone()),
        })
        .collect()
}

/// Whether `container`'s `scripts.run` is `iii sandbox run`: to Compose a process, which boots
/// the container's VM through the adapter.
pub(crate) fn runs_in_vm(container: &Container) -> bool {
    container.scripts.run.as_deref().is_some_and(is_sandbox_run)
}

/// [`runs_in_vm`] for container `key` of `file`.
pub(crate) fn runs_own_vm(file: &ComposeFile, key: &str) -> bool {
    file.containers.get(key).is_some_and(runs_in_vm)
}

/// Whether `container` is a registry bundle. Only a lock-attached package is known to be one.
pub(crate) fn is_bundle(container: &Container) -> bool {
    matches!(container.worker, WorkerSource::Package { .. })
        && container
            .resolved_package
            .as_ref()
            .is_some_and(|package| package.kind == "bundle")
}

/// Whether `container` is a binary package. Only a lock-attached package is known to be one.
fn is_binary(container: &Container) -> bool {
    matches!(container.worker, WorkerSource::Package { .. })
        && container
            .resolved_package
            .as_ref()
            .is_some_and(|package| package.kind != "bundle")
}

/// Whether `container` is a `path://` worker whose manifest sets `runtime.base_image`. A broken
/// manifest is `validate_offline`'s to report.
fn has_base_image(container: &Container) -> bool {
    container.worker_dir().is_some_and(|dir| {
        read_manifest(dir)
            .ok()
            .flatten()
            .is_some_and(|manifest| manifest.base_image.is_some())
    })
}

/// Whether `container` is one that 0.26 ran in a VM and the file does not yet place anywhere
/// (spec 7.3): a bundle without `scripts.run`, a `path://` with `runtime.base_image` and no
/// `scripts.build`, or an `iii sandbox run` without `scripts.build` or the adapter in
/// `start_after`. Keyed on `scripts` alone, so a rewritten container is never due again.
pub(crate) fn migration_due(file: &ComposeFile, container: &Container) -> bool {
    if is_bundle(container) && container.scripts.run.is_none() {
        return true;
    }
    if container.scripts.build.is_none() && has_base_image(container) {
        return true;
    }
    runs_in_vm(container)
        && (container.scripts.build.is_none()
            || !adapter_key(file, adapter_name(file))
                .ok()
                .flatten()
                .is_some_and(|adapter| container.start_after.contains(&adapter)))
}

/// Every container [`migration_due`], in declaration order. `file` must be lock-attached.
pub fn due_migrations(file: &ComposeFile) -> Vec<String> {
    file.containers
        .iter()
        .filter(|(_, container)| migration_due(file, container))
        .map(|(key, _)| key.clone())
        .collect()
}

/// The VM container keys in `needs`, or `engine.sandbox` when the section is
/// the only need: what a message names as needing the adapter.
pub(crate) fn needed_by(needs: &[Need]) -> Vec<&str> {
    let keys: Vec<&str> = needs
        .iter()
        .filter_map(|need| match need {
            Need::VmBundle(key) | Need::VmLocal(key) => Some(key.as_str()),
            Need::Explicit => None,
        })
        .collect();
    if keys.is_empty() {
        vec!["engine.sandbox"]
    } else {
        keys
    }
}

/// `--frozen` never edits the file. A container to rewrite fails first (`VM_SCRIPTS_MISSING`,
/// whose `up` without `--frozen` adds a missing adapter too), then a need with no declared
/// adapter, then an adapter outside [`ADAPTER_RANGE`] in a project with a VM container (spec 7.2,
/// 7.3). `file` must be lock-attached, as for [`needs`].
pub(crate) fn require_declared_adapter(file: &ComposeFile, project_namespace: &str) -> Result<()> {
    let due = due_migrations(file);
    if !due.is_empty() {
        return Err(ComposeError::VmScriptsMissing {
            containers: due.join(", "),
        });
    }
    let adapter = adapter_name(file);
    let needs = needs(file, project_namespace);
    if needs.is_empty() {
        return Ok(());
    }
    let Some(key) = adapter_key(file, adapter)? else {
        return Err(ComposeError::SandboxAdapterNotDeclared {
            adapter: adapter.to_string(),
            needed_by: needed_by(&needs)[0].to_string(),
        });
    };
    if !vm_needs(file).is_empty() {
        refuse_adapter_out_of_range(file, &key, None)?;
    }
    Ok(())
}

/// `engine.sandbox.adapter`, else [`DEFAULT_ADAPTER`]. An attached project has
/// no `engine:`, so its VMs go through the default adapter.
pub fn adapter_name(file: &ComposeFile) -> &str {
    file.engine
        .as_ref()
        .and_then(|engine| engine.sandbox.as_ref())
        .map_or(DEFAULT_ADAPTER, |sandbox| sandbox.adapter.as_str())
}

/// The first container that runs `adapter`, known by its package or manifest
/// name, never by its key: the key only becomes `III_WORKER_NAME`.
pub fn adapter_key(file: &ComposeFile, adapter: &str) -> Result<Option<String>> {
    for (key, container) in &file.containers {
        let name = match &container.worker {
            WorkerSource::Package { reference } => Some(match &container.resolved_package {
                Some(package) => package
                    .alias_of
                    .clone()
                    .unwrap_or_else(|| package.name.clone()),
                None => crate::registry::split_reference(reference).1,
            }),
            WorkerSource::Path { dir, .. } => {
                read_manifest(dir)?.and_then(|manifest| manifest.name)
            }
        };
        if name.as_deref() == Some(adapter) {
            return Ok(Some(key.clone()));
        }
    }
    Ok(None)
}

/// The release adapter container `key` runs: what the lock selected, else an exact declared
/// version. `None` for a `path://` adapter, which has no release to check.
fn adapter_version(
    file: &ComposeFile,
    prepared: Option<&PreparedLock>,
    key: &str,
) -> Option<String> {
    let container = file.containers.get(key)?;
    if !matches!(container.worker, WorkerSource::Package { .. }) {
        return None;
    }
    prepared
        .and_then(|prepared| prepared.resolved_version(key).map(str::to_string))
        .or_else(|| {
            container
                .resolved_package
                .as_ref()
                .map(|package| package.version.clone())
        })
        .or_else(|| {
            container
                .version
                .clone()
                .filter(|version| semver::Version::parse(version).is_ok())
        })
}

/// Whether `version` is in [`ADAPTER_RANGE`]. A version that does not parse is not.
fn in_adapter_range(version: &str) -> bool {
    let range = semver::VersionReq::parse(ADAPTER_RANGE).expect("ADAPTER_RANGE parses");
    semver::Version::parse(version).is_ok_and(|version| range.matches(&version))
}

/// `SANDBOX_ADAPTER_TOO_OLD` for adapter container `key` when the release it runs is outside
/// [`ADAPTER_RANGE`].
fn refuse_adapter_out_of_range(
    file: &ComposeFile,
    key: &str,
    prepared: Option<&PreparedLock>,
) -> Result<()> {
    match adapter_version(file, prepared, key) {
        Some(version) if !in_adapter_range(&version) => Err(ComposeError::SandboxAdapterTooOld {
            container: key.to_string(),
            version,
            range: ADAPTER_RANGE.to_string(),
        }),
        _ => Ok(()),
    }
}

/// The rules of spec 4.2, on a lock-attached file: a package whose kind is not known yet is
/// checked for what does not depend on it. A missing `scripts.build`, adapter in `start_after`
/// or bundle `scripts.run` is [`migration_due`], not refused.
pub(crate) fn validate_scripts(file: &ComposeFile) -> Result<()> {
    for (key, container) in &file.containers {
        let scripts = &container.scripts;
        if is_binary(container) && (scripts.run.is_some() || scripts.build.is_some()) {
            return Err(ComposeError::RunNotAllowedForPackage {
                container: key.clone(),
            });
        }
        if !runs_in_vm(container) {
            if scripts.build.as_deref().is_some_and(is_sandbox_build) {
                return Err(ComposeError::SandboxBuildWithoutRun {
                    container: key.clone(),
                });
            }
            continue;
        }
        if cfg!(windows) {
            return Err(ComposeError::SandboxUnsupportedPlatform {
                container: key.clone(),
            });
        }
        let vm_worker = match container.worker {
            WorkerSource::Package { .. } => {
                container.resolved_package.is_none() || is_bundle(container)
            }
            WorkerSource::Path { .. } => has_base_image(container),
        };
        if !vm_worker {
            return Err(ComposeError::SandboxRunNotAVmWorker {
                container: key.clone(),
            });
        }
        if let Some(build) = &scripts.build
            && !is_sandbox_build(build)
        {
            return Err(ComposeError::SandboxRunWithoutBuild {
                container: key.clone(),
                build: build.clone(),
            });
        }
    }
    Ok(())
}

/// Whether a VM of `adapter` went with its adapter container: this file
/// declares one, and it is not running here (absent, or exited). An adapter the file does not
/// declare is never presumed gone.
pub(crate) fn adapter_gone(
    file: &ComposeFile,
    adapter: &str,
    children: &crate::lifecycle::Children,
) -> bool {
    adapter_key(file, adapter)
        .ok()
        .flatten()
        .is_some_and(|key| {
            children
                .get(&key)
                .is_none_or(|child| matches!(child.poll(), Outcome::Exited(_)))
        })
}

/// `package://<adapter>` at the newest release in [`ADAPTER_RANGE`], pinned to
/// that exact version.
pub(crate) async fn adapter_request(adapter: &str) -> Result<NewContainer> {
    let mut request = edit::parse_worker(adapter)?;
    if let Source::Package { reference, version } = &mut request.source {
        let node = crate::registry::resolve_node(&request.key, reference, ADAPTER_RANGE).await?;
        *version = Some(node.version);
    }
    Ok(request)
}

/// What the sandbox step does to the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AdapterChange {
    Upsert(NewContainer),
    Remove(String),
}

/// Whether adapter container `key` is the one Compose generated, which it may refresh or remove.
fn is_generated_adapter(text: &str, file: &ComposeFile, key: &str) -> Result<bool> {
    if !matches!(file.containers[key].worker, WorkerSource::Package { .. }) {
        return Ok(false);
    }
    let graphs = crate::lockfile::graphs(&file.path)?;
    Ok(crate::daemon::generated_dependencies(text, file, &graphs)?.contains(key))
}

/// Adds the adapter a VM needs, re-resolves (`refresh`) or removes
/// (`allow_remove`) the one Compose generated. An operator's adapter, of any
/// key, version or source, written by hand or asked for by name, is never
/// touched. `file` must be lock-attached.
pub(crate) async fn adapter_change(
    text: &str,
    file: &ComposeFile,
    project_namespace: &str,
    refresh: bool,
    allow_remove: bool,
) -> Result<Option<AdapterChange>> {
    let adapter = adapter_name(file);
    let needed = !needs(file, project_namespace).is_empty();
    let Some(key) = adapter_key(file, adapter)? else {
        if !needed {
            return Ok(None);
        }
        refuse_key_taken(file, adapter)?;
        return Ok(Some(AdapterChange::Upsert(adapter_request(adapter).await?)));
    };
    if !is_generated_adapter(text, file, &key)? {
        return Ok(None);
    }
    if !needed {
        return Ok(allow_remove.then_some(AdapterChange::Remove(key)));
    }
    if !refresh {
        return Ok(None);
    }
    let container = &file.containers[&key];
    let mut request = adapter_request(adapter).await?;
    let unchanged = matches!(&request.source, Source::Package { version, .. }
        if version.as_deref() == container.version.as_deref());
    request.key = key;
    Ok((!unchanged).then_some(AdapterChange::Upsert(request)))
}

/// What [`migrate`] changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Migration {
    /// The containers rewritten to the explicit VM mode.
    pub containers: Vec<String>,
    /// The generated adapter it added or updated, when it did.
    pub adapter: Option<AdapterChange>,
}

/// Spec 7.2 and 7.3, run before [`apply_adapter_change`]: updates a generated adapter outside
/// [`ADAPTER_RANGE`] to the newest release in range (7.2), then rewrites the containers 0.26 ran
/// in a VM (7.3). Also adds the adapter container when a container it rewrites needs one and
/// none is declared, as [`adapter_change`] would; [`apply_adapter_change`] keeps re-resolving
/// (`refresh`) and removing (`allow_remove`) it. Edits `edited`, `file` and `prepared` in place
/// and logs each rewrite; writes nothing and acquires nothing: when `adapter` is `Some`, the
/// caller runs `PreparedLock::install`, then writes the file and the lock, as after
/// [`apply_adapter_change`]. `None` when nothing changed. Not called under `--frozen`: there
/// [`require_declared_adapter`] checks for a due migration first and refuses it with
/// `VM_SCRIPTS_MISSING`, whose `up` without `--frozen` also adds a missing adapter; only then
/// does it refuse a missing adapter (`SANDBOX_ADAPTER_NOT_DECLARED`) or an out-of-range one
/// (`SANDBOX_ADAPTER_TOO_OLD`). A rewrite that breaks a rule of spec 4.2 is refused with that
/// rule's code, leaving all three as they were.
pub(crate) async fn migrate(
    path: &Path,
    edited: &mut String,
    file: &mut ComposeFile,
    project_namespace: &str,
    prepared: &mut PreparedLock,
) -> Result<Option<Migration>> {
    rewrite_due(
        path,
        edited,
        file,
        project_namespace,
        prepared,
        &BTreeSet::new(),
    )
    .await
}

/// [`migrate`], logging no rewrite of the containers in `quiet`: `compose::add` writes the
/// explicit mode of the containers it adds the same way, and they were never migrated.
pub(crate) async fn rewrite_due(
    path: &Path,
    edited: &mut String,
    file: &mut ComposeFile,
    project_namespace: &str,
    prepared: &mut PreparedLock,
    quiet: &BTreeSet<String>,
) -> Result<Option<Migration>> {
    // What runs in a VM decides, whatever namespace `engine.sandbox` binds: a project with only
    // ephemeral sandboxes keeps its adapter (spec 7.2).
    let _ = project_namespace;
    if vm_needs(file).is_empty() {
        return Ok(None);
    }
    let due = due_migrations(file);
    let adapter = adapter_name(file).to_string();
    let declared = adapter_key(file, &adapter)?;
    let mut update = None;
    match &declared {
        None => {
            refuse_key_taken(file, &adapter)?;
            update = Some((None, adapter_request(&adapter).await?));
        }
        Some(key) => {
            if let Some(version) = adapter_version(file, Some(prepared), key)
                && !in_adapter_range(&version)
            {
                if !is_generated_adapter(edited, file, key)? {
                    return Err(ComposeError::SandboxAdapterTooOld {
                        container: key.clone(),
                        version,
                        range: ADAPTER_RANGE.to_string(),
                    });
                }
                let mut request = adapter_request(&adapter).await?;
                request.key.clone_from(key);
                update = Some((Some(version), request));
            }
        }
    }
    let adapter_key = match (&update, declared) {
        (Some((_, request)), _) => request.key.clone(),
        (None, Some(key)) => key,
        (None, None) => unreachable!("a missing adapter is always added"),
    };

    let mut text = edited.clone();
    if let Some((_, request)) = &update {
        match edit::upsert_container(&text, request)? {
            edit::Outcome::Added(next) | edit::Outcome::Replaced { text: next, .. } => text = next,
            edit::Outcome::Unchanged => {}
        }
    }
    let mut rewritten = Vec::new();
    for key in &due {
        if let Some(rewrite) = edit::explicit_vm_mode(&text, key, &adapter_key)? {
            text = rewrite.text;
            rewritten.push((key.clone(), rewrite.run, rewrite.wrapped));
        }
    }
    if update.is_none() && rewritten.is_empty() {
        return Ok(None);
    }

    let mut next = ComposeFile::parse(&text, path)?;
    prepared.attach_to(&mut next);
    validate_scripts(&next)?;
    crate::dag::validate_dependencies(&next)?;
    *edited = text;
    *file = next;
    if let Some((_, request)) = &update
        && let Source::Package {
            version: Some(version),
            ..
        } = &request.source
    {
        reprepare(
            path,
            edited,
            file,
            prepared,
            &BTreeSet::from([request.key.clone()]),
            &BTreeMap::from([(request.key.clone(), version.clone())]),
        )
        .await?;
    }

    if let Some((Some(from), request)) = &update
        && let Source::Package {
            version: Some(to), ..
        } = &request.source
    {
        crate::report::daemon_line(
            &format!(
                "{}: sandbox adapter {from} -> {to}, the first release in {ADAPTER_RANGE} that \
                 builds VM images",
                request.key
            ),
            false,
        );
    }
    for (key, run, wrapped) in &rewritten {
        if quiet.contains(key) {
            continue;
        }
        crate::report::daemon_line(
            &format!(
                "{key}: migrated to the explicit VM mode (scripts.build: {SANDBOX_BUILD}, \
                 scripts.run: {run})"
            ),
            false,
        );
        if *wrapped {
            crate::report::daemon_line(
                &format!(
                    "{key}: scripts.run now runs inside the VM (iii sandbox run --command); to \
                     keep it on the host, set scripts.build (\"\" when there is nothing to build)"
                ),
                true,
            );
        }
    }
    Ok(Some(Migration {
        containers: rewritten.into_iter().map(|(key, ..)| key).collect(),
        adapter: update.map(|(_, request)| AdapterChange::Upsert(request)),
    }))
}

/// The adapter's container would go under the key `adapter`. A container
/// already there that is not the adapter is the operator's, and their
/// declaration wins (spec 7.2): refused, never rewritten.
fn refuse_key_taken(file: &ComposeFile, adapter: &str) -> Result<()> {
    if !file.containers.contains_key(adapter) {
        return Ok(());
    }
    Err(ComposeError::InvalidWorkerSpec {
        spec: adapter.to_string(),
        reason: format!(
            "container '{adapter}' does not run the sandbox adapter '{adapter}' (its package or \
             manifest name differs), so Compose will not install the adapter over it. Give that \
             worker the name '{adapter}', rename the container, or set engine.sandbox.adapter to \
             the adapter it runs"
        ),
    })
}

/// `compose::add iii-sandbox`: the engine's router is built in, so the request
/// means its adapter.
pub(crate) fn is_iii_sandbox(worker: &NewContainer) -> bool {
    worker.key == "iii-sandbox" && matches!(worker.source, Source::Package { .. })
}

/// What `compose::add iii-sandbox` asks for: the declared adapter as it is
/// declared, else a new one.
pub(crate) async fn iii_sandbox_request(file: &ComposeFile) -> Result<NewContainer> {
    let adapter = adapter_name(file);
    let Some(key) = adapter_key(file, adapter)? else {
        refuse_key_taken(file, adapter)?;
        return adapter_request(adapter).await;
    };
    let container = &file.containers[&key];
    let mut request = edit::parse_worker(&match &container.worker {
        WorkerSource::Package { reference } => format!("package://{reference}"),
        WorkerSource::Path { declared, .. } => declared.clone(),
    })?;
    request.key = key;
    if let Source::Package { version, .. } = &mut request.source {
        version.clone_from(&container.version);
    }
    Ok(request)
}

/// Re-reads `edited` into `current` and prepares the lock again: each `force`d entry at its
/// `selected` version, every other at the version `prepared` chose, so nothing else resolves
/// differently.
async fn reprepare(
    path: &Path,
    edited: &str,
    current: &mut ComposeFile,
    prepared: &mut PreparedLock,
    force: &BTreeSet<String>,
    selected: &BTreeMap<String, String>,
) -> Result<()> {
    *current = ComposeFile::parse(edited, path)?;
    let mut versions = selected.clone();
    for key in current.containers.keys() {
        if let Some(version) = prepared.resolved_version(key) {
            versions
                .entry(key.clone())
                .or_insert_with(|| version.to_string());
        }
    }
    *prepared = crate::lockfile::prepare_metadata_with_versions(current, force, &versions).await?;
    Ok(())
}

/// Applies [`adapter_change`] to `edited` and `current`, then prepares the lock
/// again: the adapter at its exact version, every other entry at the version
/// the first pass chose, so nothing else resolves differently.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn apply_adapter_change(
    path: &Path,
    edited: &mut String,
    current: &mut ComposeFile,
    project_namespace: &str,
    prepared: &mut PreparedLock,
    force: &mut BTreeSet<String>,
    selected: &mut BTreeMap<String, String>,
    refresh: bool,
    allow_remove: bool,
) -> Result<Option<AdapterChange>> {
    let Some(change) =
        adapter_change(edited, current, project_namespace, refresh, allow_remove).await?
    else {
        return Ok(None);
    };
    let next = match &change {
        AdapterChange::Upsert(request) => {
            if let Source::Package {
                version: Some(version),
                ..
            } = &request.source
            {
                force.insert(request.key.clone());
                selected.insert(request.key.clone(), version.clone());
            }
            match edit::upsert_container(edited, request)? {
                edit::Outcome::Added(text) | edit::Outcome::Replaced { text, .. } => Some(text),
                edit::Outcome::Unchanged => None,
            }
        }
        AdapterChange::Remove(key) => edit::remove_container(edited, key)?,
    };
    if let Some(next) = next {
        *edited = next;
    }
    reprepare(path, edited, current, prepared, force, selected).await?;
    Ok(Some(change))
}

/// What only the adapter container gets, over its own environment: the worker
/// token, the host directories a guest may mount from (the compose file's, the
/// package cache, and each `vm_dirs`), and `III_DISABLE_TRACE_PAYLOADS` so iii-sdk records
/// no handler input (`auth`, the guest `env`, `channel.access_key`) as the
/// `iii.invocation.input` span event.
pub(crate) fn adapter_env(
    token: &str,
    compose_file: &Path,
    package_cache: &Path,
    vm_dirs: &[&Path],
) -> BTreeMap<String, String> {
    let compose_dir = compose_file.parent().unwrap_or(Path::new(""));
    // A root that cannot be joined (a path separator in it) is dropped alone and
    // loudly, so the others survive; the adapter then refuses host_path
    // mounts under the dropped one (S501).
    let roots: Vec<&Path> = [compose_dir, package_cache]
        .into_iter()
        .chain(vm_dirs.iter().copied())
        .filter(|root| {
            let joinable = std::env::join_paths([root]).is_ok();
            if !joinable {
                crate::report::daemon_line(
                    &format!(
                        "sandbox: {} contains a path separator, so it is not a host root for \
                         the adapter; host_path mounts under it will be refused",
                        root.display()
                    ),
                    true,
                );
            }
            joinable
        })
        .collect();
    let roots = std::env::join_paths(roots).unwrap_or_default();
    BTreeMap::from([
        ("III_SANDBOX_WORKER_TOKEN".to_string(), token.to_string()),
        (
            "III_SANDBOX_HOST_ROOTS".to_string(),
            roots.to_string_lossy().into_owned(),
        ),
        ("III_DISABLE_TRACE_PAYLOADS".to_string(), "1".to_string()),
    ])
}

/// Lays [`adapter_env`] over `user_env` when `key` is this project's adapter
/// container, and only then: the token never reaches a container that runs
/// user code. It goes after the operator's own variables, so an operator
/// `III_DISABLE_TRACE_PAYLOADS=0` cannot undo it.
pub(crate) fn layer_adapter_env(
    file: &ComposeFile,
    key: &str,
    token: &str,
    package_cache: &Path,
    user_env: &mut BTreeMap<String, String>,
) -> Result<()> {
    if adapter_key(file, adapter_name(file))?.as_deref() == Some(key) {
        // `path://../w` puts a VM's own directory outside the compose file's.
        // shortcut: read when the adapter starts, so such a VM added while it
        // runs is refused (S501) until the adapter restarts.
        let vm_dirs: Vec<&Path> = vm_needs(file)
            .into_iter()
            .filter_map(|need| match need {
                Need::VmLocal(vm) => file.containers[&vm].worker_dir(),
                Need::VmBundle(_) | Need::Explicit => None,
            })
            .collect();
        user_env.extend(adapter_env(token, &file.path, package_cache, &vm_dirs));
    }
    Ok(())
}

/// `path` as the UTF-8 the adapter is handed, or the container's start fails.
fn utf8_path(key: &str, path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| ComposeError::SpawnFailed {
            container: key.to_string(),
            message: format!(
                "{} is not UTF-8, so the sandbox adapter cannot be handed it",
                path.display()
            ),
        })
}

/// Lays `III_PACKAGE_DIR` and `III_PACKAGE_NAME` over `user_env` when container `key` is a
/// registry bundle, in a VM or on the host (spec 7.5): its `.workers/<container>/`, and its
/// manifest name, which for an alias is the package it stands for.
pub(crate) fn layer_package_env(
    file: &ComposeFile,
    key: &str,
    user_env: &mut BTreeMap<String, String>,
) -> Result<()> {
    let Some(container) = file.containers.get(key) else {
        return Ok(());
    };
    let WorkerSource::Package { reference } = &container.worker else {
        return Ok(());
    };
    if !is_bundle(container) && !runs_in_vm(container) {
        return Ok(());
    }
    let name = match &container.resolved_package {
        Some(package) => package
            .alias_of
            .clone()
            .unwrap_or_else(|| package.name.clone()),
        None => crate::registry::split_reference(reference).1,
    };
    let compose_file = crate::spawn::canonical_compose_file(&file.path);
    let compose_dir = compose_file.parent().unwrap_or(Path::new("."));
    user_env.extend([
        (
            sandbox_cli::PACKAGE_DIR_ENV.to_string(),
            utf8_path(key, &crate::build_record::package_dir(compose_dir, key))?,
        ),
        (sandbox_cli::PACKAGE_NAME_ENV.to_string(), name),
    ]);
    Ok(())
}

/// Variables that are inputs of `iii sandbox run` and `iii sandbox build` themselves, which never
/// reach the guest (spec 8.3, 9.4).
fn host_only(name: &str) -> bool {
    name == "III_URL"
        || name.starts_with("III_SANDBOX_")
        || [
            sandbox_cli::BUILD_DIR_ENV,
            sandbox_cli::BUILD_TIMEOUT_ENV,
            sandbox_cli::BUILD_COMPOSE_FILE_ENV,
            sandbox_cli::PACKAGE_DIR_ENV,
            sandbox_cli::PACKAGE_NAME_ENV,
        ]
        .contains(&name)
}

/// Lays what `iii sandbox run` and `iii sandbox build` read over `user_env` when container `key`
/// [`runs_in_vm`], and only then (spec 8.3). The token only opens this container's VM: the
/// project's stays with the adapter.
pub(crate) fn layer_own_vm_env(
    file: &ComposeFile,
    key: &str,
    project_token: &str,
    state_dir: &Path,
    project_namespace: &str,
    user_env: &mut BTreeMap<String, String>,
) -> Result<()> {
    let Some(container) = file
        .containers
        .get(key)
        .filter(|container| runs_in_vm(container))
    else {
        return Ok(());
    };
    let source = match container.worker_dir() {
        Some(worker_dir) => (
            sandbox_cli::WORKER_DIR_ENV.to_string(),
            utf8_path(key, worker_dir)?,
        ),
        None => {
            layer_package_env(file, key, user_env)?;
            (
                sandbox_cli::PACKAGE_DIR_ENV.to_string(),
                user_env[sandbox_cli::PACKAGE_DIR_ENV].clone(),
            )
        }
    };
    // The guest gets its own variables and what it reads of the reserved contract, such as the
    // namespace readiness watches; the adapter gives it its engine URL.
    let worker_env_keys: Vec<&str> = user_env
        .keys()
        .map(String::as_str)
        .chain(crate::spawn::RESERVED_ENV)
        .filter(|name| !host_only(name))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let worker_env_keys = worker_env_keys.join(",");
    // `iii` in the command is this binary. A PATH that cannot lead with its directory is left as
    // it is.
    // shortcut: a user-set PATH reaches the guest with this host directory first, which matches
    // nothing there; hand the guest the user's own value if that ever matters.
    let path = user_env
        .get("PATH")
        .map(std::ffi::OsString::from)
        .or_else(|| std::env::var_os("PATH"))
        .unwrap_or_default();
    let path = std::env::current_exe()
        .ok()
        .and_then(|binary| Some(binary.parent()?.to_path_buf()))
        .and_then(|lead| {
            std::env::join_paths(std::iter::once(lead).chain(std::env::split_paths(&path))).ok()
        })
        .and_then(|path| path.into_string().ok());
    if let Some(path) = path {
        user_env.insert("PATH".to_string(), path);
    }
    let compose_file = crate::spawn::canonical_compose_file(&file.path);
    let compose_dir = compose_file.parent().unwrap_or(Path::new("."));
    let state_key = state_key(state_dir, key);
    user_env.extend([
        (
            sandbox_cli::TOKEN_ENV.to_string(),
            contract::scoped_auth_token(project_token, &state_key),
        ),
        (sandbox_cli::STATE_KEY_ENV.to_string(), state_key),
        (
            sandbox_cli::ADAPTER_ENV.to_string(),
            adapter_name(file).to_string(),
        ),
        (
            sandbox_cli::ADAPTER_NAMESPACE_ENV.to_string(),
            project_namespace.to_string(),
        ),
        source,
        (
            sandbox_cli::WORKER_ENV_KEYS_ENV.to_string(),
            worker_env_keys,
        ),
        (
            sandbox_cli::STOP_TIMEOUT_ENV.to_string(),
            file.stop_timeout.as_millis().to_string(),
        ),
        (
            sandbox_cli::BUILD_DIR_ENV.to_string(),
            utf8_path(key, &crate::build_record::build_dir(compose_dir, key))?,
        ),
        (
            sandbox_cli::BUILD_TIMEOUT_ENV.to_string(),
            container.scripts.build_timeout.as_millis().to_string(),
        ),
        (
            sandbox_cli::BUILD_COMPOSE_FILE_ENV.to_string(),
            utf8_path(key, &compose_file)?,
        ),
    ]);
    Ok(())
}

/// The VM's identity at the adapter: hex SHA-256 over the project's state
/// directory, a NUL, then the container key.
pub fn state_key(state_dir: &Path, key: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(state_dir.as_os_str().as_encoded_bytes());
    digest.update([0]);
    digest.update(key.as_bytes());
    hex::encode(digest.finalize())
}

/// The `vm_id` the adapter gives `state_key`: the first 32 hex chars of its
/// SHA-256. shortcut: the contract calls `vm_id` opaque and this is
/// sandbox-libkrun's derivation, so another adapter answers S504 to it; add a
/// call by `state_key` to the contract if one ships.
fn vm_id(state_key: &str) -> String {
    hex::encode(Sha256::digest(state_key))[..32].to_string()
}

/// How long a registered adapter may take to register `adapter::info` (spec 7.4);
/// short under test, below STOP_RETRY.
const INFO_RETRY: Duration = Duration::from_secs(if cfg!(test) { 1 } else { 10 });
/// How long a VM start waits for a dropped engine connection to come back: an
/// engine restart's few seconds plus the SDK's 2 s reconnect delay, so a dead
/// engine fails the start near 0.25's ~17 s, not at PREPARE_TIMEOUT; short under test.
const RECONNECT_GRACE: Duration = Duration::from_secs(if cfg!(test) { 5 } else { 10 });
/// `worker::destroy` removes a VM's whole state, rootfs included.
const DESTROY_TIMEOUT: Duration = Duration::from_secs(60);
/// How long `worker::destroy` is asked again while the adapter aborts a stopped build of the same
/// state (S509); short under test.
const BUILD_ABORT_GRACE: Duration = Duration::from_secs(if cfg!(test) { 2 } else { 15 });
/// The pause between those asks.
const BUILD_ABORT_RETRY: Duration = Duration::from_millis(if cfg!(test) { 100 } else { 500 });
/// A status call's budget, and what a stop call gets beyond its grace.
const CALL_SLACK: Duration = Duration::from_secs(10);

/// VM container `key` at instance 0, which no boot is: the adapter answers with the one it holds.
fn boot_probe(ctx: &LifecycleCtx<'_>, key: &str) -> VmRecord {
    VmRecord {
        adapter: adapter_name(ctx.file).to_string(),
        namespace: ctx.project_namespace.to_string(),
        vm_id: vm_id(&state_key(ctx.state_dir, key)),
        instance: 0,
    }
}

/// The boot of container `key`, one that [`runs_own_vm`], that is running at its adapter: the VM
/// outlived the `iii sandbox run` that ran it, and the next one reattaches. Its guest is not a
/// stranger holding the worker's name. A boot still preparing cannot hold that name yet.
pub(crate) async fn own_vm_running(ctx: &LifecycleCtx<'_>, key: &str) -> Option<VmRecord> {
    if !runs_own_vm(ctx.file, key) || ctx.engine.has_exited() {
        return None;
    }
    let (client, token) = (ctx.engine.client(), ctx.sandbox_token);
    let probe = boot_probe(ctx, key);
    let latest = superseded_by(&worker_status(client, &probe, token).await, &probe)?;
    let status = worker_status(client, &latest, token).await.ok()?;
    matches!(status.state, State::Running).then_some(latest)
}

/// Stops what [`own_vm_running`] finds, for a start that must boot the VM anew or a `down`, and
/// says whether it found one. One that does not stop keeps the name, which the start then refuses.
pub(crate) async fn stop_own_vm(ctx: &LifecycleCtx<'_>, key: &str) -> bool {
    let Some(running) = own_vm_running(ctx, key).await else {
        return false;
    };
    let _ = worker_stop(
        ctx.engine.client(),
        &running,
        ctx.sandbox_token,
        ctx.file.stop_timeout,
    )
    .await;
    true
}

/// [`call`] for a step of `container`'s start (or its destroy), which an engine
/// restart must not fail: 0.25 prepared without the bus. A call whose connection drops, or that
/// begins after a drop before the engine is back, is sent again once it is,
/// unless the managed engine exited, all within `timeout`; an engine not back
/// within [`RECONNECT_GRACE`] fails the step with the `NotConnected`, so the
/// restart policy decides. Only the first connection, as at daemon start, gets
/// the whole `timeout`. The adapter registers its functions after its worker
/// name, and again on its own reconnect, so `function_not_found` is asked again
/// for [`INFO_RETRY`] after a drop, or from the first call with `registering`.
/// Each step is idempotent; a re-sent `worker::start` that meets the boot of
/// the dropped one gets S502, which [`worker_start`] stops.
#[allow(clippy::too_many_arguments)]
async fn start_call<T: serde::de::DeserializeOwned>(
    engine: &EngineClient,
    container: &str,
    adapter: &str,
    namespace: &str,
    suffix: &str,
    request: &impl serde::Serialize,
    timeout: Duration,
    mut registering: bool,
) -> std::result::Result<T, iii_sdk::Error> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut missing_since = None;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        let client = engine.client();
        let reply = match client.get_connection_state() {
            IIIConnectionState::Connected | IIIConnectionState::Reconnecting => {
                send(client, adapter, namespace, suffix, request, left).await
            }
            _ => call(client, adapter, namespace, suffix, request, left).await,
        };
        let now = tokio::time::Instant::now();
        match &reply {
            Err(iii_sdk::Error::NotConnected) if now < deadline && !engine.has_exited() => {
                let wait = RECONNECT_GRACE.min(deadline - now);
                crate::report::daemon_line(
                    &reconnect_line(container, &adapter_function_id(adapter, suffix), wait),
                    true,
                );
                if tokio::time::timeout(wait, connection(client, true))
                    .await
                    .is_err()
                {
                    return reply;
                }
                (registering, missing_since) = (true, None);
            }
            Err(iii_sdk::Error::Remote { code, .. })
                if registering
                    && code == "function_not_found"
                    && now < deadline
                    && now < *missing_since.get_or_insert(now) + INFO_RETRY =>
            {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            _ => return reply,
        }
    }
}

/// What [`start_call`] prints while `container` waits up to `wait` for the
/// engine to come back.
fn reconnect_line(container: &str, function: &str, wait: Duration) -> String {
    format!(
        "{container}: no engine connection for {function}; waiting up to {}s for it to come back",
        wait.as_millis().div_ceil(1000)
    )
}

/// `worker::destroy` for VM container `key`'s state, by its `state_key`. The
/// adapter refuses it while the VM runs, so it goes after the stop, and once more
/// after stopping a VM the refusal names: one whose `iii sandbox run` died is up
/// until its lease ends it, and nothing asks again then. A build of the same state that the
/// adapter is still aborting refuses it too (S509): the caller stops the build first, and the
/// destroy is asked again for [`BUILD_ABORT_GRACE`]. A failure only leaves the state behind, and
/// says where.
pub(crate) async fn destroy_vm(ctx: &LifecycleCtx<'_>, key: &str, adapter: &str, namespace: &str) {
    let state_key = state_key(ctx.state_dir, key);
    let request = WorkerDestroyRequest {
        auth: ctx.sandbox_token.to_string(),
        state_key: state_key.clone(),
    };
    // Registering: an adapter started for this call may not list it yet.
    let destroy = || {
        start_call::<WorkerDestroyResponse>(
            ctx.engine,
            key,
            adapter,
            namespace,
            WORKER_DESTROY,
            &request,
            DESTROY_TIMEOUT,
            true,
        )
    };
    let aborting_until = tokio::time::Instant::now() + BUILD_ABORT_GRACE;
    let mut stopped_boot = false;
    let mut destroyed = destroy().await;
    while let Err(error) = &destroyed {
        if sandbox_code(error) == Some(SandboxErrorCode::S509)
            && tokio::time::Instant::now() < aborting_until
        {
            tokio::time::sleep(BUILD_ABORT_RETRY).await;
        } else if !stopped_boot
            && stop_busy_boot(
                ctx.engine,
                adapter,
                namespace,
                error,
                ctx.sandbox_token,
                ctx.file.stop_timeout,
            )
            .await
        {
            stopped_boot = true;
        } else {
            break;
        }
        destroyed = destroy().await;
    }
    if let Err(error) = destroyed {
        let error = ComposeError::EngineCallFailed {
            function: adapter_function_id(adapter, WORKER_DESTROY),
            message: spawn_failed_message(&error),
        };
        let vm_id = vm_id(&state_key);
        crate::report::daemon_line(
            &format!(
                "{key}: could not destroy VM {vm_id} through {adapter}: {error}; its state \
                 stays in ~/.iii/sandbox/workers/{vm_id}"
            ),
            true,
        );
    }
}

/// Stops the VM a 0.26 daemon left running for container `key` (its `vm` record in
/// `state.json`), before the container's first build (spec 7.3, step 2), and forgets it. One the
/// adapter no longer holds is already stopped. A stop nobody answers fails the start, and the
/// next one asks again. `worker::destroy` is never called: the VM's data stays.
pub(crate) async fn stop_legacy_vm(ctx: &LifecycleCtx<'_>, key: &str) -> Result<()> {
    let Some(vm) = ctx.legacy_vm(key) else {
        return Ok(());
    };
    let (client, token) = (ctx.engine.client(), ctx.sandbox_token);
    let mut reply = worker_stop(client, &vm, token, ctx.file.stop_timeout).await;
    if let Some(live) = superseded_by(&reply, &vm) {
        reply = worker_stop(client, &live, token, ctx.file.stop_timeout).await;
    }
    if let Err(error) = &reply
        && !matches!(
            sandbox_code(error),
            Some(SandboxErrorCode::S503 | SandboxErrorCode::S504)
        )
    {
        return Err(ComposeError::SpawnFailed {
            container: key.to_string(),
            message: format!(
                "could not stop VM {} instance {} that a 0.26 daemon left running: {}",
                vm.vm_id,
                vm.instance,
                spawn_failed_message(error)
            ),
        });
    }
    ctx.forget_legacy_vm(key);
    crate::report::daemon_line(
        &format!(
            "{key}: stopped VM {} instance {}, which a 0.26 daemon left running",
            vm.vm_id, vm.instance
        ),
        false,
    );
    Ok(())
}

/// What a reopened project does with a recorded VM. With its owner gone, the
/// adapter's lifeline took it; otherwise `worker::status` decides, and a reply
/// that says nothing adopts it for the poller to settle.
pub(crate) fn vm_reconciliation(
    owner_alive: bool,
    status: std::result::Result<WorkerStatus, iii_sdk::Error>,
) -> Reconciliation {
    if !owner_alive {
        return Reconciliation::Stopped;
    }
    match outcome_of(status) {
        Outcome::Running | Outcome::Unknown => Reconciliation::Adopt,
        Outcome::Exited(_) => Reconciliation::Gone,
    }
}

/// An S-code error as `S501: <the adapter's words> (<docs_url>)`, else the
/// bus error as is.
pub(crate) fn spawn_failed_message(error: &iii_sdk::Error) -> String {
    sandbox_payload(error)
        .and_then(|payload| {
            let line = format!(
                "{}: {}",
                payload["code"].as_str()?,
                payload["message"].as_str()?
            );
            Some(match payload["docs_url"].as_str() {
                Some(docs) => format!("{line} ({docs})"),
                None => line,
            })
        })
        .unwrap_or_else(|| error.to_string())
}

/// What one `worker::status` (or `worker::stop`) reply says about the VM. A
/// bus error says nothing (`Unknown`); a VM superseded by another boot (S503
/// after its retry) or gone from the adapter (S504) has exited with no code.
pub(crate) fn outcome_of(status: std::result::Result<WorkerStatus, iii_sdk::Error>) -> Outcome {
    match status {
        Ok(status) => match status.state {
            State::Preparing | State::Running => Outcome::Running,
            State::Unknown => Outcome::Unknown,
            State::Exited | State::Stopped => {
                Outcome::Exited(vm_exit(status.exit_code, status.signal))
            }
        },
        Err(error) => match sandbox_code(&error) {
            Some(SandboxErrorCode::S503 | SandboxErrorCode::S504) => {
                Outcome::Exited(vm_exit(None, None))
            }
            _ => Outcome::Unknown,
        },
    }
}

/// The live VM an S502 or S503 names in its `fix`.
pub(crate) fn live_instance(error: &iii_sdk::Error) -> Option<WorkerStartResponse> {
    if !matches!(
        sandbox_code(error)?,
        SandboxErrorCode::S502 | SandboxErrorCode::S503
    ) {
        return None;
    }
    serde_json::from_value(sandbox_payload(error)?.get("fix")?.clone()).ok()
}

/// The S-code an adapter answered with; `None` for anything else on the bus.
pub(crate) fn sandbox_code(error: &iii_sdk::Error) -> Option<SandboxErrorCode> {
    match error {
        iii_sdk::Error::Remote { message, .. } => code_of(message),
        _ => None,
    }
}

/// An S-code error's whole payload, read from the first `{` as [`code_of`]
/// does: the SDK sends it as `handler error: {json}`.
fn sandbox_payload(error: &iii_sdk::Error) -> Option<serde_json::Value> {
    let iii_sdk::Error::Remote { message, .. } = error else {
        return None;
    };
    code_of(message)?;
    serde_json::from_str(&message[message.find('{')?..]).ok()
}

/// On S503 another boot of `vm` is live: the one to stop before asking again,
/// so a restart never starts a second VM beside it.
fn superseded_by(
    reply: &std::result::Result<WorkerStatus, iii_sdk::Error>,
    vm: &VmRecord,
) -> Option<VmRecord> {
    let error = reply.as_ref().err()?;
    if sandbox_code(error) != Some(SandboxErrorCode::S503) {
        return None;
    }
    let live = live_instance(error)?;
    Some(VmRecord {
        vm_id: live.vm_id,
        instance: live.instance,
        ..vm.clone()
    })
}

/// A VM's end as a process status, so the restart policy reads it unchanged.
/// No code (a stop, a lost VM) reads as SIGKILL, so `code()` is `None`.
pub(crate) fn vm_exit(code: Option<i32>, signal: Option<i32>) -> ExitStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(match (code, signal) {
            (Some(code), _) => (code & 0xff) << 8,
            (None, Some(signal)) if (1..0x7f).contains(&signal) => signal,
            (None, _) => 9,
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        let _ = signal;
        ExitStatus::from_raw(code.unwrap_or(1) as u32)
    }
}

pub(crate) async fn worker_status(
    client: &IIIClient,
    vm: &VmRecord,
    token: &str,
) -> std::result::Result<WorkerStatus, iii_sdk::Error> {
    let request = WorkerStatusRequest {
        auth: token.to_string(),
        vm_id: vm.vm_id.clone(),
        instance: vm.instance,
    };
    call(
        client,
        &vm.adapter,
        &vm.namespace,
        WORKER_STATUS,
        &request,
        CALL_SLACK,
    )
    .await
}

/// Whether `error` is an S502, whose live boot this has then stopped with `grace` for the one
/// retry of what it refused. A stop that fails (S502 on a boot still preparing) still gets that.
async fn stop_busy_boot(
    engine: &EngineClient,
    adapter: &str,
    namespace: &str,
    error: &iii_sdk::Error,
    token: &str,
    grace: Duration,
) -> bool {
    if sandbox_code(error) != Some(SandboxErrorCode::S502) {
        return false;
    }
    if let Some(live) = live_instance(error) {
        let live = VmRecord {
            adapter: adapter.to_string(),
            namespace: namespace.to_string(),
            vm_id: live.vm_id,
            instance: live.instance,
        };
        let _ = worker_stop(engine.client(), &live, token, grace).await;
    }
    true
}

pub(crate) async fn worker_stop(
    client: &IIIClient,
    vm: &VmRecord,
    token: &str,
    grace: Duration,
) -> std::result::Result<WorkerStatus, iii_sdk::Error> {
    let request = WorkerStopRequest {
        auth: token.to_string(),
        vm_id: vm.vm_id.clone(),
        instance: vm.instance,
        grace_ms: grace.as_millis() as u64,
    };
    call(
        client,
        &vm.adapter,
        &vm.namespace,
        WORKER_STOP,
        &request,
        grace + CALL_SLACK,
    )
    .await
}

/// `<adapter>::<suffix>` in the adapter's namespace. A reply that is not a `T`
/// is a `Serde` error, and a call whose connection drops ends at once as
/// `NotConnected`; `outcome_of` reads both as `Unknown`.
pub(crate) async fn call<T: serde::de::DeserializeOwned>(
    client: &IIIClient,
    adapter: &str,
    namespace: &str,
    suffix: &str,
    request: &impl serde::Serialize,
    timeout: Duration,
) -> std::result::Result<T, iii_sdk::Error> {
    // The SDK keeps a call pending across a reconnect until its timeout, and
    // an engine that died or restarted never answers it. So the call goes out
    // once connected, and that connection dropping ends it.
    let sent = async {
        connection(client, true).await;
        send(client, adapter, namespace, suffix, request, timeout).await
    };
    tokio::time::timeout(timeout, sent)
        .await
        .unwrap_or(Err(iii_sdk::Error::Timeout))
}

/// [`call`] without its wait for the connection: one that is down when this
/// begins, or drops, ends it at once as `NotConnected`.
async fn send<T: serde::de::DeserializeOwned>(
    client: &IIIClient,
    adapter: &str,
    namespace: &str,
    suffix: &str,
    request: &impl serde::Serialize,
    timeout: Duration,
) -> std::result::Result<T, iii_sdk::Error> {
    let request = CallRequest {
        function_id: adapter_function_id(adapter, suffix),
        payload: serde_json::to_value(request)?,
        action: None,
        timeout_ms: Some(timeout.as_millis() as u64),
    }
    .namespace(namespace);
    let sent = async {
        tokio::select! {
            // First, so a call that begins disconnected is never queued in the
            // SDK, which would send it on the next connection.
            biased;
            () = connection(client, false) => Err(iii_sdk::Error::NotConnected),
            reply = client.call(request) => reply,
        }
    };
    let reply = tokio::time::timeout(timeout, sent)
        .await
        .unwrap_or(Err(iii_sdk::Error::Timeout))?;
    Ok(serde_json::from_value(reply)?)
}

/// Resolves once `client`'s connection is up, or down. A dropped connection
/// stays down for the SDK's 2 s retry delay, so the poll cannot miss one.
async fn connection(client: &IIIClient, up: bool) {
    while (client.get_connection_state() == IIIConnectionState::Connected) != up {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;

    use iii_sandbox_contract::{WORKER_PREPARE, WORKER_START, WorkerPrepareResponse};

    use super::*;

    /// `worker::prepare`'s budget in the 0.26 boot: room for an image pull.
    const PREPARE_TIMEOUT: Duration = Duration::from_secs(20 * 60);

    fn wire_message(
        code: SandboxErrorCode,
        message: &str,
        fix: Option<serde_json::Value>,
    ) -> String {
        iii_sdk::Error::Handler(contract::error_payload(code, message, fix, None).to_string())
            .to_string()
    }

    fn drain(
        called: &mut tokio::sync::mpsc::UnboundedReceiver<(String, serde_json::Value)>,
    ) -> Vec<(String, Option<u64>)> {
        let mut calls = Vec::new();
        while let Ok((function, payload)) = called.try_recv() {
            let suffix = function.trim_start_matches("sandbox-libkrun::").to_string();
            calls.push((suffix, payload["instance"].as_u64()));
        }
        calls
    }

    fn parse(dir: &Path, text: &str) -> ComposeFile {
        ComposeFile::parse(text, dir.join("worker-compose.yaml")).unwrap()
    }

    pub(crate) fn manifest(dir: &Path, worker: &str, text: &str) {
        std::fs::create_dir_all(dir.join(worker)).unwrap();
        std::fs::write(dir.join(worker).join("iii.worker.yaml"), text).unwrap();
    }

    fn package(fields: &str) -> crate::registry::ResolvedPackage {
        serde_yaml::from_str(&format!("{{version: 1.0.0, artifacts: {{}}, {fields}}}")).unwrap()
    }

    fn vm_project(dir: &Path) -> ComposeFile {
        manifest(
            dir,
            "local",
            "runtime: {base_image: python}\nscripts: {start: run}\n",
        );
        let mut file = parse(
            dir,
            "engine: {sandbox: {}}\ncontainers:\n  sb: {worker: package://sandbox-libkrun, version: 1.0.0}\n  app: {worker: package://app, version: 1.0.0}\n  bin: {worker: package://bin, version: 1.0.0}\n  local: {worker: path://./local}\n",
        );
        file.containers["app"].resolved_package = Some(package("name: app, type: bundle"));
        file.containers["bin"].resolved_package = Some(package("name: bin, type: binary"));
        file
    }

    #[test]
    fn needs_lists_bundles_base_images_and_the_section() {
        let dir = tempfile::tempdir().unwrap();
        let file = vm_project(dir.path());
        assert_eq!(
            needs(&file, "default"),
            vec![
                Need::VmBundle("app".into()),
                Need::VmLocal("local".into()),
                Need::Explicit
            ]
        );
    }

    #[test]
    fn a_binding_to_another_namespace_needs_no_adapter_here() {
        let dir = tempfile::tempdir().unwrap();
        let explicit = |declared: &str, sandbox: &str, project_namespace: &str| {
            let file = parse(
                dir.path(),
                &format!(
                    "{declared}engine: {{sandbox: {sandbox}}}\ncontainers:\n  app: {{worker: package://app, version: 1.0.0}}\n"
                ),
            );
            needs(&file, project_namespace).contains(&Need::Explicit)
        };
        assert!(explicit("namespace: shop\n", "{}", "shop"));
        assert!(explicit("namespace: shop\n", "{namespace: shop}", "shop"));
        assert!(!explicit("namespace: shop\n", "{namespace: other}", "shop"));
        // `--namespace orders` runs the project, and the router, in `orders`.
        assert!(explicit("", "{namespace: orders}", "orders"));
        assert!(!explicit("", "{namespace: orders}", "default"));
        assert!(!explicit(
            "namespace: orders\n",
            "{namespace: orders}",
            "shop"
        ));

        // A VM still needs the adapter, in the project namespace.
        let mut file = vm_project(dir.path());
        file.engine
            .as_mut()
            .unwrap()
            .sandbox
            .as_mut()
            .unwrap()
            .namespace = Some("other".into());
        assert_eq!(
            needs(&file, "default"),
            vec![Need::VmBundle("app".into()), Need::VmLocal("local".into())]
        );
    }

    #[test]
    fn the_adapter_is_found_by_manifest_name_not_by_key() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        manifest(dir, "other", "name: other\n");
        manifest(dir, "dev", "name: sandbox-libkrun\n");
        let adapter = |text: &str| adapter_key(&parse(dir, text), "sandbox-libkrun").unwrap();
        assert_eq!(
            adapter("containers:\n  sb: {worker: package://sandbox-libkrun, version: 1.0.0}\n"),
            Some("sb".into())
        );
        assert_eq!(
            adapter("containers:\n  sandbox-libkrun: {worker: path://./other}\n"),
            None
        );
        assert_eq!(
            adapter("containers:\n  dev: {worker: path://./dev}\n"),
            Some("dev".into())
        );
        // A resolved alias is known by the package it stands for.
        let mut file = parse(
            dir,
            "containers:\n  sb: {worker: package://my-sandbox, version: 1.0.0}\n",
        );
        file.containers["sb"].resolved_package = Some(package(
            "name: my-sandbox, alias_of: sandbox-libkrun, type: binary",
        ));
        assert_eq!(
            adapter_key(&file, "sandbox-libkrun").unwrap(),
            Some("sb".into())
        );
    }

    /// A registry whose `/resolve` answers `sandbox-libkrun` 0.4.0 for this
    /// host, and serves `archive` as its artifact.
    pub(crate) async fn adapter_registry(archive: &[u8]) -> wiremock::MockServer {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers};
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/resolve"))
            .and(matchers::body_partial_json(
                serde_json::json!({ "worker": "sandbox-libkrun" }),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "graph": [{
                    "name": "sandbox-libkrun",
                    "version": "0.4.0",
                    "type": "binary",
                    "binaries": { (crate::registry::host_targets()[0]): {
                        "sha256": hex::encode(Sha256::digest(archive)),
                        "url": format!("{}/artifact", server.uri()),
                    }},
                }]
            })))
            .mount(&server)
            .await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/artifact"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(archive.to_vec()))
            .mount(&server)
            .await;
        server
    }

    /// `api` beside the adapter `sb`, with `fields` in its declaration.
    fn own_vm_project(dir: &Path, manifest_text: &str, fields: &str) -> ComposeFile {
        manifest(dir, "api", manifest_text);
        parse(
            dir,
            &format!(
                "containers:\n  sb: {{worker: package://sandbox-libkrun, version: 1.0.0}}\n  api: {{worker: path://./api, {fields}}}\n"
            ),
        )
    }

    const VM_MANIFEST: &str = "runtime: {base_image: python}\nscripts: {start: run}\n";
    const OWN_VM: &str = "start_after: [sb], scripts: {run: iii sandbox run}";

    /// To Compose it is a plain process, whatever its manifest says, that still needs the
    /// adapter.
    #[test]
    fn a_container_that_runs_its_own_vm_is_a_process_that_needs_the_adapter() {
        let dir = tempfile::tempdir().unwrap();
        for manifest_text in [VM_MANIFEST, "scripts: {start: run}\n"] {
            let file = own_vm_project(dir.path(), manifest_text, OWN_VM);
            assert!(runs_own_vm(&file, "api"));
            assert_eq!(needs(&file, "default"), vec![Need::VmLocal("api".into())]);
            assert_eq!(
                crate::manifest::resolve_start("api", &file.containers["api"]).unwrap(),
                crate::manifest::StartSpec::Shell("iii sandbox run".into())
            );
        }
    }

    #[tokio::test]
    async fn a_generated_adapter_stays_for_a_container_that_runs_its_own_vm() {
        let dir = tempfile::tempdir().unwrap();
        manifest(dir.path(), "api", "scripts: {start: run}\n");
        let text = with_generated_adapter(
            "containers:\n  api: {worker: path://./api, scripts: {run: iii sandbox run}}\n",
        )
        .replace(
            "{worker: path://./api,",
            "{worker: path://./api, start_after: [sandbox-libkrun],",
        );
        let file = parse(dir.path(), &text);
        assert_eq!(
            adapter_change(&text, &file, "default", false, true)
                .await
                .unwrap(),
            None
        );
    }

    #[test]
    fn the_directory_of_a_container_that_runs_its_own_vm_is_a_host_root() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir(&project).unwrap();
        manifest(&dir.path().join("shared"), "api", "scripts: {start: run}\n");
        let file = parse(
            &project,
            &format!(
                "containers:\n  sb: {{worker: package://sandbox-libkrun, version: 1.0.0}}\n  api: {{worker: path://../shared/api, {OWN_VM}}}\n"
            ),
        );
        let mut env = BTreeMap::new();
        layer_adapter_env(&file, "sb", "t0k", Path::new("/pkg"), &mut env).unwrap();
        let roots: Vec<PathBuf> = std::env::split_paths(&env["III_SANDBOX_HOST_ROOTS"]).collect();
        assert_eq!(roots.len(), 3, "{roots:?}");
        assert_eq!(
            roots[2].canonicalize().unwrap(),
            dir.path().join("shared/api").canonicalize().unwrap()
        );
    }

    /// `text` with a generated `sandbox-libkrun` 0.4.0 appended.
    fn with_generated_adapter(text: &str) -> String {
        let request = edit::parse_worker("sandbox-libkrun@0.4.0").unwrap();
        let edit::Outcome::Added(text) = edit::upsert_container(text, &request).unwrap() else {
            panic!("the adapter is new");
        };
        text
    }

    #[tokio::test]
    async fn the_adapter_resolves_within_the_contract_major_and_pins_the_exact_release() {
        let server = adapter_registry(b"").await;
        let request = crate::registry::TEST_REGISTRY
            .scope(server.uri(), adapter_request("sandbox-libkrun"))
            .await
            .unwrap();
        assert_eq!(
            request.source,
            edit::Source::Package {
                reference: "api.workers.iii.dev/sandbox-libkrun".into(),
                version: Some("0.4.0".into())
            }
        );
        let asked: serde_json::Value =
            serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
        assert_eq!(asked["version"], ADAPTER_RANGE);
        assert_eq!(ADAPTER_RANGE, "^0.4");
        let edit::Outcome::Added(text) =
            edit::upsert_container("containers: {}\n", &request).unwrap()
        else {
            panic!("the adapter is new");
        };
        assert!(edit::is_generated_container(&text, "sandbox-libkrun").unwrap());
    }

    #[tokio::test]
    async fn a_generated_adapter_goes_with_its_last_vm() {
        let dir = tempfile::tempdir().unwrap();
        let text = with_generated_adapter("containers: {}\n");
        let file = parse(dir.path(), &text);
        assert_eq!(
            adapter_change(&text, &file, "default", false, true)
                .await
                .unwrap(),
            Some(AdapterChange::Remove("sandbox-libkrun".into()))
        );
        // up, build and add never remove it, so up leaves the file as it is.
        assert_eq!(
            adapter_change(&text, &file, "default", false, false)
                .await
                .unwrap(),
            None
        );
    }

    /// `compose::add` marks the worker asked for like one it generated; only the
    /// lock's graph root tells them apart.
    #[tokio::test]
    async fn an_adapter_asked_for_by_name_is_never_refreshed_or_removed() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        let server = adapter_registry(b"").await;
        let request = edit::parse_worker("sandbox-libkrun@0.2.0").unwrap();
        let edit::Outcome::Added(text) =
            edit::upsert_container("containers: {}\n", &request).unwrap()
        else {
            panic!("the adapter is new");
        };
        let mut file = parse(dir, &text);
        let mut lock = crate::registry::TEST_REGISTRY
            .scope(
                server.uri(),
                crate::lockfile::prepare_metadata(&mut file, &BTreeSet::new()),
            )
            .await
            .unwrap();
        lock.replace_graph(
            "sandbox-libkrun",
            BTreeSet::from(["sandbox-libkrun".to_string()]),
        );
        lock.write_if_changed().unwrap();

        // The last VM is gone: it stays.
        assert_eq!(
            adapter_change(&text, &file, "default", false, true)
                .await
                .unwrap(),
            None
        );
        // A VM still needs it: an update leaves it at the operator's 0.2.0.
        manifest(dir, "local", "runtime: {base_image: python}\n");
        let with_vm = format!("{text}  local: {{worker: path://./local}}\n");
        assert_eq!(
            crate::registry::TEST_REGISTRY
                .scope(
                    server.uri(),
                    adapter_change(&with_vm, &parse(dir, &with_vm), "default", true, true),
                )
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn an_operator_adapter_is_never_added_twice_refreshed_or_removed() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        // No mock: any registry call fails the test instead of leaving the host.
        let registry = wiremock::MockServer::start().await;
        let change = |text: String, bundle: bool| {
            let mut file = parse(dir, &text);
            if bundle {
                file.containers["app"].resolved_package = Some(package("name: app, type: bundle"));
            }
            crate::registry::TEST_REGISTRY.scope(registry.uri(), async move {
                adapter_change(&text, &file, "default", true, true).await
            })
        };
        let pinned = "containers:\n  sb: {worker: package://sandbox-libkrun, version: 1.0.0}\n";
        let app = "  app: {worker: package://app, version: 1.0.0}\n";
        assert_eq!(change(format!("{pinned}{app}"), true).await.unwrap(), None);
        assert_eq!(change(pinned.to_string(), false).await.unwrap(), None);

        manifest(dir, "dev", "name: sandbox-libkrun\n");
        let checkout = "containers:\n  sandbox-libkrun: {worker: path://./dev}\n";
        assert_eq!(
            change(format!("{checkout}{app}"), true).await.unwrap(),
            None
        );
        // A comment on the key reads like the bare key.
        let commented = "containers:  # workers\n  sandbox-libkrun: {worker: path://./dev}\n";
        assert_eq!(
            change(format!("{commented}{app}"), true).await.unwrap(),
            None
        );
        let commented = pinned.replace("containers:", "containers: # workers");
        assert_eq!(
            change(format!("{commented}{app}"), true).await.unwrap(),
            None
        );

        // Keyed like the adapter but another worker: refused, never rewritten.
        manifest(dir, "other", "name: other\n");
        for taken in [
            "containers:\n  sandbox-libkrun: {worker: package://acme/sandbox-fork, version: 1.0.0}\n",
            "containers:\n  sandbox-libkrun: {worker: path://./other}\n",
        ] {
            let error = change(format!("{taken}{app}"), true).await.unwrap_err();
            assert_eq!(error.code(), "INVALID_WORKER_SPEC");
            assert!(
                error
                    .to_string()
                    .contains("container 'sandbox-libkrun' does not run the sandbox adapter"),
                "{error}"
            );
            // Without a VM the key is the operator's business.
            assert_eq!(change(taken.to_string(), false).await.unwrap(), None);
        }
    }

    #[test]
    fn compose_add_iii_sandbox_names_the_adapter() {
        assert!(is_iii_sandbox(&edit::parse_worker("iii-sandbox").unwrap()));
        assert!(!is_iii_sandbox(
            &edit::parse_worker("./iii-sandbox").unwrap()
        ));
    }

    #[test]
    fn adapter_env_carries_the_token_and_both_roots() {
        let env = adapter_env(
            "t0k",
            Path::new("/srv/app/worker-compose.yaml"),
            Path::new("/home/u/.iii/compose/packages"),
            &[],
        );
        assert_eq!(env["III_SANDBOX_WORKER_TOKEN"], "t0k");
        // Keeps auth, env and channel.access_key out of the trace store.
        assert_eq!(env["III_DISABLE_TRACE_PAYLOADS"], "1");
        assert_eq!(
            std::env::split_paths(&env["III_SANDBOX_HOST_ROOTS"]).collect::<Vec<_>>(),
            vec![
                PathBuf::from("/srv/app"),
                PathBuf::from("/home/u/.iii/compose/packages")
            ]
        );
    }

    // ':' separates PATH entries on Unix only.
    #[cfg(unix)]
    #[test]
    fn a_root_that_cannot_be_joined_is_dropped_alone() {
        let env = adapter_env(
            "t0k",
            Path::new("/srv/a:b/worker-compose.yaml"),
            Path::new("/home/u/.iii/compose/packages"),
            &[],
        );
        assert_eq!(
            std::env::split_paths(&env["III_SANDBOX_HOST_ROOTS"]).collect::<Vec<_>>(),
            vec![PathBuf::from("/home/u/.iii/compose/packages")]
        );
        assert_eq!(env["III_SANDBOX_WORKER_TOKEN"], "t0k");
    }

    #[test]
    fn only_the_adapter_container_gets_the_token() {
        let file = parse(
            Path::new("/srv/app"),
            "engine: {sandbox: {}}\ncontainers:\n  sb: {worker: package://sandbox-libkrun, version: 1.0.0}\n  app: {worker: package://app, version: 1.0.0}\n",
        );
        let layer = |key: &str| {
            let mut env = BTreeMap::from([
                ("III_DISABLE_TRACE_PAYLOADS".to_string(), "0".to_string()),
                ("KEEP".to_string(), "me".to_string()),
            ]);
            layer_adapter_env(&file, key, "t0k", Path::new("/pkg"), &mut env).unwrap();
            env
        };

        let adapter = layer("sb");
        assert_eq!(adapter["III_SANDBOX_WORKER_TOKEN"], "t0k");
        assert_eq!(adapter["III_DISABLE_TRACE_PAYLOADS"], "1");
        assert_eq!(adapter["KEEP"], "me");

        let other = layer("app");
        assert!(!other.contains_key("III_SANDBOX_WORKER_TOKEN"));
        assert!(!other.contains_key("III_SANDBOX_HOST_ROOTS"));
        assert_eq!(other["III_DISABLE_TRACE_PAYLOADS"], "0");
    }

    /// `path://../shared/vm` is a supported spelling: the adapter must admit
    /// that VM's directory, not only the compose file's.
    #[test]
    fn a_local_vm_outside_the_compose_directory_is_a_host_root() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir(&project).unwrap();
        manifest(
            &dir.path().join("shared"),
            "vm",
            "runtime: {base_image: python}\nscripts: {start: run}\n",
        );
        let file = parse(
            &project,
            "containers:\n  sb: {worker: package://sandbox-libkrun, version: 1.0.0}\n  vm: {worker: path://../shared/vm}\n",
        );
        let mut env = BTreeMap::new();
        layer_adapter_env(&file, "sb", "t0k", Path::new("/pkg"), &mut env).unwrap();
        let roots: Vec<PathBuf> = std::env::split_paths(&env["III_SANDBOX_HOST_ROOTS"]).collect();
        assert_eq!(roots[..2], [project, PathBuf::from("/pkg")]);
        assert_eq!(roots.len(), 3, "{roots:?}");
        assert_eq!(
            roots[2].canonicalize().unwrap(),
            dir.path().join("shared/vm").canonicalize().unwrap()
        );
    }

    fn remote(code: &str, message: &str) -> iii_sdk::Error {
        iii_sdk::Error::Remote {
            code: code.into(),
            message: message.into(),
            stacktrace: None,
        }
    }

    pub(crate) fn status(
        state: State,
        exit_code: Option<i32>,
        signal: Option<i32>,
    ) -> WorkerStatus {
        WorkerStatus {
            vm_id: "v".into(),
            instance: 1,
            state,
            exit_code,
            signal,
            started_at_ms: 0,
            ended_at_ms: None,
        }
    }

    #[test]
    fn vm_recovery_follows_the_owner_and_the_status() {
        assert_eq!(
            vm_reconciliation(true, Ok(status(State::Running, None, None))),
            Reconciliation::Adopt
        );
        // Unknown: the poller settles it.
        assert_eq!(
            vm_reconciliation(true, Err(iii_sdk::Error::Timeout)),
            Reconciliation::Adopt
        );
        assert_eq!(
            vm_reconciliation(true, Ok(status(State::Exited, Some(0), None))),
            Reconciliation::Gone
        );
        assert_eq!(
            vm_reconciliation(
                true,
                Err(remote(
                    "invocation_failed",
                    &wire_message(SandboxErrorCode::S504, "no vm", None)
                ))
            ),
            Reconciliation::Gone
        );
        // The adapter's lifeline took it.
        assert_eq!(
            vm_reconciliation(false, Ok(status(State::Running, None, None))),
            Reconciliation::Stopped
        );
    }

    #[test]
    fn spawn_failed_message_keeps_the_s_code_and_its_docs() {
        let e = remote(
            "invocation_failed",
            &wire_message(
                SandboxErrorCode::S501,
                "host_path /etc is outside the host roots",
                None,
            ),
        );
        assert_eq!(
            spawn_failed_message(&e),
            format!(
                "S501: host_path /etc is outside the host roots ({}S501)",
                contract::DOCS_BASE
            )
        );
        assert_eq!(
            spawn_failed_message(&remote("function_not_found", "gone")),
            remote("function_not_found", "gone").to_string()
        );
    }

    #[cfg(unix)]
    #[test]
    fn bus_errors_are_unknown_and_s504_is_an_exit_without_code() {
        assert_eq!(outcome_of(Err(iii_sdk::Error::Timeout)), Outcome::Unknown);
        assert_eq!(
            outcome_of(Err(remote("function_not_found", "gone"))),
            Outcome::Unknown
        );
        let Outcome::Exited(s) = outcome_of(Err(remote(
            "invocation_failed",
            &wire_message(SandboxErrorCode::S504, "no vm", None),
        ))) else {
            panic!()
        };
        assert_eq!(s.code(), None);
        let stale = remote(
            "invocation_failed",
            &wire_message(
                SandboxErrorCode::S503,
                "stale",
                Some(serde_json::json!({"vm_id": "v", "instance": 3})),
            ),
        );
        assert_eq!(
            live_instance(&stale).map(|l| (l.vm_id, l.instance)),
            Some(("v".to_string(), 3))
        );
        // S503 after the retry: superseded.
        let Outcome::Exited(s) = outcome_of(Err(stale)) else {
            panic!()
        };
        assert_eq!(s.code(), None);
        assert!(live_instance(&iii_sdk::Error::Timeout).is_none());
        assert_eq!(
            outcome_of(Ok(status(State::Preparing, None, None))),
            Outcome::Running
        );
        assert_eq!(
            outcome_of(Ok(status(State::Running, None, None))),
            Outcome::Running
        );
        assert_eq!(
            outcome_of(Ok(status(State::Unknown, None, None))),
            Outcome::Unknown
        );
        let Outcome::Exited(s) = outcome_of(Ok(status(State::Exited, Some(3), None))) else {
            panic!()
        };
        assert_eq!(s.code(), Some(3));
        let Outcome::Exited(s) = outcome_of(Ok(status(State::Stopped, None, Some(15)))) else {
            panic!()
        };
        assert_eq!(s.code(), None);
    }

    /// An engine stand-in on a local port. It answers `engine::channels::create`
    /// itself, and every other call with `answer`'s reply fields (`result` or
    /// `error`); `true` never answers, a `null` reply drops the connection
    /// instead, and `false` also refuses every later connection, as an engine
    /// that stays dead. Each answered or unanswered call but the SDK's own
    /// registration goes to the receiver as (function id, payload). A channel
    /// socket sends one line at cursor 12, none when the `worker::logs` before
    /// it asked from there or from the end, then closes.
    pub(crate) async fn fake_engine(
        answer: impl Fn(&str, &serde_json::Value) -> serde_json::Value + Send + Sync + 'static,
    ) -> (
        IIIClient,
        tokio::sync::mpsc::UnboundedReceiver<(String, serde_json::Value)>,
    ) {
        let (url, called) = fake_engine_url(answer).await;
        let client = iii_sdk::register_worker(&url, iii_sdk::InitOptions::default());
        (client, called)
    }

    /// [`fake_engine`]'s address, for an [`EngineClient`].
    pub(crate) async fn fake_engine_url(
        answer: impl Fn(&str, &serde_json::Value) -> serde_json::Value + Send + Sync + 'static,
    ) -> (
        String,
        tokio::sync::mpsc::UnboundedReceiver<(String, serde_json::Value)>,
    ) {
        use futures::{SinkExt, StreamExt};
        use serde_json::json;
        use tokio_tungstenite::tungstenite::{
            Message,
            handshake::server::{Request, Response},
        };

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let answer = std::sync::Arc::new(answer);
        let (calls, called) = tokio::sync::mpsc::unbounded_channel();
        let dead = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let asked_from = std::sync::Arc::new(std::sync::Mutex::new(None));
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                // Kept listening, so no other test's engine can take the port.
                if dead.load(std::sync::atomic::Ordering::SeqCst) {
                    continue;
                }
                let (calls, answer, dead, asked_from) = (
                    calls.clone(),
                    answer.clone(),
                    dead.clone(),
                    asked_from.clone(),
                );
                tokio::spawn(async move {
                    let mut path = String::new();
                    // tungstenite fixes the callback's error type.
                    #[allow(clippy::result_large_err)]
                    let accept = |request: &Request, response: Response| {
                        path = request.uri().path().to_string();
                        Ok(response)
                    };
                    let mut socket = tokio_tungstenite::accept_hdr_async(stream, accept)
                        .await
                        .unwrap();
                    if path.starts_with("/ws/channels/") {
                        let line =
                            r#"{"stream":"stderr","line":"no /dev/kvm","ts_ms":1,"cursor":12}"#;
                        let cursor = *asked_from.lock().unwrap();
                        if cursor.is_none_or(|cursor: u64| cursor < 12) {
                            socket.send(Message::Text(line.into())).await.unwrap();
                        }
                        socket.close(None).await.unwrap();
                        return;
                    }
                    while let Some(Ok(frame)) = socket.next().await {
                        let Ok(message) =
                            serde_json::from_slice::<serde_json::Value>(&frame.into_data())
                        else {
                            continue;
                        };
                        let function = message["function_id"].as_str().unwrap_or_default();
                        if message["type"] != "invokefunction"
                            || (function.starts_with("engine::workers::")
                                && function != "engine::workers::list")
                        {
                            continue;
                        }
                        if function.ends_with("::worker::logs") {
                            // From the end of a log whose last record is 12: nothing to send.
                            *asked_from.lock().unwrap() = if message["data"]["from_end"] == true {
                                Some(12)
                            } else {
                                message["data"]["cursor"].as_u64()
                            };
                        }
                        let end = |direction| json!({"channel_id": "c", "access_key": "k", "direction": direction});
                        let mut reply = match function {
                            "engine::channels::create" => {
                                json!({"result": {"writer": end("write"), "reader": end("read")}})
                            }
                            _ => answer(function, &message["data"]),
                        };
                        if reply == json!(false) {
                            dead.store(true, std::sync::atomic::Ordering::SeqCst);
                            return;
                        }
                        if reply.is_null() {
                            return;
                        }
                        if reply == json!(true) {
                            let _ = calls.send((function.to_string(), message["data"].clone()));
                            continue;
                        }
                        reply["type"] = json!("invocationresult");
                        reply["invocation_id"] = message["invocation_id"].clone();
                        reply["function_id"] = json!(function);
                        let _ = calls.send((function.to_string(), message["data"].clone()));
                        let _ = socket.send(Message::Text(reply.to_string().into())).await;
                    }
                });
            }
        });
        (format!("ws://{address}"), called)
    }

    pub(crate) fn answered(result: impl serde::Serialize) -> serde_json::Value {
        serde_json::json!({ "result": result })
    }

    /// An S-code refusal whose `fix` names `instance` of VM `v`.
    pub(crate) fn refused(code: SandboxErrorCode, instance: u64) -> serde_json::Value {
        let fix = serde_json::json!({ "vm_id": "v", "instance": instance });
        let message = wire_message(code, "refused", Some(fix));
        serde_json::json!({ "error": { "code": "invocation_failed", "message": message } })
    }

    pub(crate) fn not_found() -> serde_json::Value {
        serde_json::json!({ "error": { "code": "function_not_found", "message": "gone" } })
    }

    /// A removed VM container's state goes by its `state_key`, whatever its
    /// record holds: none for a first boot that failed, no record at all in a
    /// new daemon.
    #[tokio::test]
    async fn a_removed_vm_destroys_its_state_without_a_vm_record() {
        use crate::state::{ChildRecord, ChildStatus};

        let (url, mut called) = fake_engine_url(|function, _| match function {
            "sandbox-libkrun::worker::destroy" => {
                answered(serde_json::json!({ "destroyed": true }))
            }
            _ => not_found(),
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        manifest(
            dir.path(),
            "vm",
            "runtime: {base_image: python}\nscripts: {start: run}\n",
        );
        let file = parse(dir.path(), "containers:\n  vm: {worker: path://./vm}\n");
        let engine = EngineClient::connect(&url, "t", "t");
        let (shutdown, post_runs, diagnostics) = Default::default();
        let logs = crate::logs::LogStore::open(dir.path().join("logs")).unwrap();
        let ctx = LifecycleCtx {
            shutdown: &shutdown,
            file: &file,
            engine: &engine,
            post_runs: &post_runs,
            compose_namespace: "t",
            project_namespace: "shop",
            engine_url: &url,
            package_cache: dir.path(),
            logs: &logs,
            state_dir: dir.path(),
            sandbox_token: "t",
            empty_env_diagnostics: &diagnostics,
            legacy_vms: &Default::default(),
        };
        let failed = ChildRecord {
            pid: None,
            birth: None,
            vm: None,
            status: ChildStatus::Failed,
            started_at: 1,
            last_error: Some("container 'vm' was not ready after 60s".into()),
        };
        for mut records in [
            BTreeMap::from([("vm".to_string(), failed)]),
            BTreeMap::new(),
        ] {
            let removed = crate::lifecycle::remove_one(
                &ctx,
                &mut crate::lifecycle::Children::new(),
                &mut records,
                "vm",
                "rm".into(),
            )
            .await;
            assert_eq!(removed.status, crate::lifecycle::OpStatus::Ok);
            assert!(records.is_empty());
            let mut destroyed = Vec::new();
            while let Ok((function, payload)) = called.try_recv() {
                if function == "sandbox-libkrun::worker::destroy" {
                    destroyed.push(payload["state_key"].clone());
                }
            }
            assert_eq!(destroyed, [serde_json::json!(state_key(dir.path(), "vm"))]);
        }
    }

    /// A plain VM or process holding its name is not asked about: only a container that runs
    /// its own VM has one that may outlive what ran it.
    #[tokio::test]
    async fn only_a_container_that_runs_its_own_vm_is_asked_about() {
        let (url, mut called) = fake_engine_url(|function, _| match function {
            "sandbox-libkrun::worker::status" => answered(status(State::Running, None, None)),
            _ => not_found(),
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        manifest(
            dir.path(),
            "vm",
            "runtime: {base_image: python}\nscripts: {start: run}\n",
        );
        manifest(dir.path(), "plain", "scripts: {start: run}\n");
        let file = parse(
            dir.path(),
            "containers:\n  vm: {worker: path://./vm}\n  plain: {worker: path://./plain}\n",
        );
        let engine = EngineClient::connect(&url, "t", "t");
        let (shutdown, post_runs, diagnostics) = Default::default();
        let logs = crate::logs::LogStore::open(dir.path().join("logs")).unwrap();
        let ctx = LifecycleCtx {
            shutdown: &shutdown,
            file: &file,
            engine: &engine,
            post_runs: &post_runs,
            compose_namespace: "t",
            project_namespace: "shop",
            engine_url: &url,
            package_cache: dir.path(),
            logs: &logs,
            state_dir: dir.path(),
            sandbox_token: "t",
            empty_env_diagnostics: &diagnostics,
            legacy_vms: &Default::default(),
        };
        for key in ["vm", "plain"] {
            assert!(own_vm_running(&ctx, key).await.is_none());
            stop_own_vm(&ctx, key).await;
        }
        assert_eq!(drain(&mut called), []);
    }

    /// A VM the command that ran it left behind refuses the destroy until its lease stops
    /// it, and nothing would ask again then. So it is stopped now, and asked once more; a
    /// second refusal only leaves the state behind.
    #[tokio::test]
    async fn a_destroy_refused_for_a_live_vm_stops_it_and_asks_once_more() {
        for refusals in [1, 2] {
            let destroys = std::sync::atomic::AtomicUsize::new(0);
            let (url, mut called) = fake_engine_url(move |function, _| match function {
                "sandbox-libkrun::worker::destroy"
                    if destroys.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < refusals =>
                {
                    refused(SandboxErrorCode::S502, 7)
                }
                "sandbox-libkrun::worker::destroy" => {
                    answered(serde_json::json!({ "destroyed": true }))
                }
                "sandbox-libkrun::worker::stop" => answered(status(State::Stopped, None, None)),
                _ => not_found(),
            })
            .await;
            let dir = tempfile::tempdir().unwrap();
            manifest(
                dir.path(),
                "vm",
                "runtime: {base_image: python}\nscripts: {start: run}\n",
            );
            let file = parse(dir.path(), "containers:\n  vm: {worker: path://./vm}\n");
            let engine = EngineClient::connect(&url, "t", "t");
            let (shutdown, post_runs, diagnostics) = Default::default();
            let logs = crate::logs::LogStore::open(dir.path().join("logs")).unwrap();
            let ctx = LifecycleCtx {
                shutdown: &shutdown,
                file: &file,
                engine: &engine,
                post_runs: &post_runs,
                compose_namespace: "t",
                project_namespace: "shop",
                engine_url: &url,
                package_cache: dir.path(),
                logs: &logs,
                state_dir: dir.path(),
                sandbox_token: "project-token",
                empty_env_diagnostics: &diagnostics,
                legacy_vms: &Default::default(),
            };

            destroy_vm(&ctx, "vm", "sandbox-libkrun", "shop").await;

            let mut calls = Vec::new();
            while let Ok((function, payload)) = called.try_recv() {
                let auth = payload["auth"].as_str().unwrap().to_string();
                calls.push((function, payload["instance"].as_u64(), auth));
            }
            let call = |suffix: &str, instance| {
                (
                    format!("sandbox-libkrun::{suffix}"),
                    instance,
                    "project-token".to_string(),
                )
            };
            assert_eq!(
                calls,
                [
                    call("worker::destroy", None),
                    call("worker::stop", Some(7)),
                    call("worker::destroy", None),
                ]
            );
        }
    }

    /// An engine that dies or restarts with a call in flight never answers it:
    /// the call ends with its connection, as a bus error that status and stop
    /// read as `Unknown` and a start retries, not after PREPARE_TIMEOUT. The
    /// retry goes out once the engine is back.
    #[tokio::test]
    async fn a_call_ends_when_its_connection_drops() {
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let (client, _called) = fake_engine(move |_, _| {
            match calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
                0 => serde_json::Value::Null,
                _ => answered(serde_json::json!({})),
            }
        })
        .await;
        let request = serde_json::json!({});
        let prepare = || {
            tokio::time::timeout(
                Duration::from_secs(5),
                call::<WorkerPrepareResponse>(
                    &client,
                    "sandbox-libkrun",
                    "shop",
                    WORKER_PREPARE,
                    &request,
                    PREPARE_TIMEOUT,
                ),
            )
        };
        let reply = prepare()
            .await
            .expect("the call must end when its connection drops");
        let Err(error) = reply else {
            panic!("nobody answered the call");
        };
        assert!(matches!(error, iii_sdk::Error::NotConnected), "{error:?}");
        assert_eq!(outcome_of(Err(error)), Outcome::Unknown);

        assert!(prepare().await.expect("the engine is back").is_ok());
    }

    /// An external engine that stays dead never sets `has_exited`: a start's
    /// dropped call waits RECONNECT_GRACE for it, then fails as before the
    /// re-send, so the restart policy decides well before PREPARE_TIMEOUT. So
    /// does a step that begins once the connection is already down.
    #[tokio::test]
    async fn a_start_call_whose_engine_stays_dead_fails_after_the_grace() {
        let (url, _called) = fake_engine_url(|_, _| serde_json::Value::Bool(false)).await;
        let engine = EngineClient::connect(&url, "t", "t");
        let request = serde_json::json!({});
        // The second step begins with the connection already down.
        for suffix in [WORKER_PREPARE, WORKER_START] {
            let began = tokio::time::Instant::now();
            let reply = tokio::time::timeout(
                RECONNECT_GRACE + Duration::from_secs(2),
                start_call::<WorkerPrepareResponse>(
                    &engine,
                    "vm",
                    "sandbox-libkrun",
                    "shop",
                    suffix,
                    &request,
                    PREPARE_TIMEOUT,
                    false,
                ),
            )
            .await
            .expect("the start must not wait out PREPARE_TIMEOUT");
            assert!(
                matches!(reply, Err(iii_sdk::Error::NotConnected)),
                "{reply:?}"
            );
            assert!(began.elapsed() >= RECONNECT_GRACE, "{:?}", began.elapsed());
            assert!(!engine.is_connected());
        }
    }

    /// The wait line names the container and the wait it has left, not more.
    #[test]
    fn the_reconnect_line_names_the_container_and_the_wait_left() {
        assert_eq!(
            reconnect_line(
                "web",
                "sandbox-libkrun::worker::start",
                Duration::from_millis(3200)
            ),
            "web: no engine connection for sandbox-libkrun::worker::start; waiting up to 4s \
             for it to come back"
        );
    }

    #[test]
    fn state_key_is_stable_per_state_dir_and_container() {
        let key = state_key(Path::new("/srv/app/.iii/compose/shop"), "web");
        assert_eq!(key.len(), 64);
        assert!(key.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(
            key,
            state_key(Path::new("/srv/app/.iii/compose/shop"), "web")
        );
        assert_ne!(
            key,
            state_key(Path::new("/srv/app/.iii/compose/shop"), "api")
        );
        assert_ne!(
            key,
            state_key(Path::new("/srv/other/.iii/compose/shop"), "web")
        );
    }

    /// A 0.26 project: the operator's adapter, a base-image worker with no scripts, one with a
    /// command of its own, the 0.26 opt-in, one moved to the host, and a plain worker.
    const PROJECT_026: &str = "# my project\ncontainers:\n  # the adapter, declared by hand\n  sb:\n    worker: path://./sb\n  # a VM, as 0.26 ran it\n  link:\n    worker: path://./link\n  dev:\n    worker: path://./dev\n    scripts:\n      run: python src/dev.py   # hot reload\n  opt:\n    worker: path://./opt\n    start_after: [sb]\n    scripts: {run: iii sandbox run}\n  host:\n    worker: path://./host\n    scripts:\n      build: \"\"\n      run: node index.js\n  plain:\n    worker: path://./plain\n";

    /// [`PROJECT_026`] after its migration: only the three VM containers changed.
    const PROJECT_MIGRATED: &str = "# my project\ncontainers:\n  # the adapter, declared by hand\n  sb:\n    worker: path://./sb\n  # a VM, as 0.26 ran it\n  link:\n    worker: path://./link\n    start_after: [sb]\n    scripts:\n      build: iii sandbox build\n      run: iii sandbox run\n  dev:\n    worker: path://./dev\n    start_after: [sb]\n    scripts:\n      build: iii sandbox build\n      run: iii sandbox run --command 'python src/dev.py'\n  opt:\n    worker: path://./opt\n    start_after: [sb]\n    scripts:\n      run: iii sandbox run\n      build: iii sandbox build\n  host:\n    worker: path://./host\n    scripts:\n      build: \"\"\n      run: node index.js\n  plain:\n    worker: path://./plain\n";

    /// The workers of [`PROJECT_026`] on disk.
    fn project_026(dir: &Path) -> PathBuf {
        manifest(dir, "sb", "name: sandbox-libkrun\nscripts: {start: run}\n");
        for worker in ["link", "dev", "opt", "host"] {
            manifest(dir, worker, VM_MANIFEST);
        }
        manifest(dir, "plain", "scripts: {start: run}\n");
        let path = dir.join("worker-compose.yaml");
        std::fs::write(&path, PROJECT_026).unwrap();
        path
    }

    /// `path`'s file, lock-attached, with its prepared lock and its text.
    async fn prepared(path: &Path) -> (String, ComposeFile, PreparedLock) {
        let text = std::fs::read_to_string(path).unwrap();
        let mut file = ComposeFile::load(path).unwrap();
        let prepared = crate::lockfile::prepare_metadata(&mut file, &BTreeSet::new())
            .await
            .unwrap();
        (text, file, prepared)
    }

    /// Spec 7.3: per container, keyed on `scripts`, comments kept, and a second run changes
    /// nothing; the lock keeps its format.
    #[tokio::test]
    async fn a_026_project_is_rewritten_container_by_container_and_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = project_026(dir.path());
        let (mut edited, mut file, mut lock) = prepared(&path).await;
        assert_eq!(due_migrations(&file), ["link", "dev", "opt"]);

        let migration = migrate(&path, &mut edited, &mut file, "default", &mut lock)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(migration.containers, ["link", "dev", "opt"]);
        assert_eq!(migration.adapter, None);
        assert_eq!(edited, PROJECT_MIGRATED);
        assert_eq!(
            file.containers["dev"].scripts.run.as_deref(),
            Some("iii sandbox run --command 'python src/dev.py'")
        );
        assert_eq!(due_migrations(&file), Vec::<String>::new());
        assert_eq!(
            migrate(&path, &mut edited, &mut file, "default", &mut lock)
                .await
                .unwrap(),
            None
        );
        assert_eq!(edited, PROJECT_MIGRATED);
        // Nothing was written: the caller writes.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), PROJECT_026);
        lock.write_if_changed().unwrap();
        let written = std::fs::read_to_string(crate::lockfile::lock_path(&path)).unwrap();
        assert!(written.starts_with("version: 1\n"), "{written}");

        // A file migrated, read again with its lock, is not due either.
        std::fs::write(&path, PROJECT_MIGRATED).unwrap();
        let (mut edited, mut file, mut lock) = prepared(&path).await;
        assert_eq!(
            migrate(&path, &mut edited, &mut file, "default", &mut lock)
                .await
                .unwrap(),
            None
        );
    }

    /// A lock for `containers`, each `(key, name, version, kind)`, as Compose writes one.
    fn write_lock(path: &Path, containers: &[(&str, &str, &str, &str)]) {
        let entries: String = containers
            .iter()
            .map(|(key, name, version, kind)| {
                format!(
                    "  {key}:\n    worker: package://api.workers.iii.dev/{name}\n    requested: {version}\n    resolved:\n      name: {name}\n      version: {version}\n      type: {kind}\n      artifacts:\n        {target}:\n          url: https://example.test/{name}\n          sha256: \"{sha}\"\n",
                    target = crate::registry::host_targets()[0],
                    sha = "0".repeat(64)
                )
            })
            .collect();
        std::fs::write(
            crate::lockfile::lock_path(path),
            format!("version: 1\ncontainers:\n{entries}"),
        )
        .unwrap();
    }

    /// A bundle without `scripts.run` moves into the VM; one with a host command stays where it
    /// is, with or without its lock; a rewrite that breaks a rule of spec 4.2 writes nothing.
    #[tokio::test]
    async fn a_bundle_is_rewritten_unless_it_runs_on_the_host() {
        let dir = tempfile::tempdir().unwrap();
        manifest(
            dir.path(),
            "sb",
            "name: sandbox-libkrun\nscripts: {start: run}\n",
        );
        let path = dir.path().join("worker-compose.yaml");
        let text = "containers:\n  sb: {worker: path://./sb}\n  onboarding:\n    worker: package://api.workers.iii.dev/onboarding\n    version: 0.2.0\n  web:\n    worker: package://api.workers.iii.dev/web\n    version: 1.0.0\n    scripts: {run: node server.js}\n";
        std::fs::write(&path, text).unwrap();
        write_lock(
            &path,
            &[
                ("onboarding", "onboarding", "0.2.0", "bundle"),
                ("web", "web", "1.0.0", "bundle"),
            ],
        );
        let (mut edited, mut file, mut lock) = prepared(&path).await;

        let migration = migrate(&path, &mut edited, &mut file, "default", &mut lock)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(migration.containers, ["onboarding"]);
        assert_eq!(
            edited,
            "containers:\n  sb: {worker: path://./sb}\n  onboarding:\n    worker: package://api.workers.iii.dev/onboarding\n    version: 0.2.0\n    start_after: [sb]\n    scripts:\n      build: iii sandbox build\n      run: iii sandbox run\n  web:\n    worker: package://api.workers.iii.dev/web\n    version: 1.0.0\n    scripts: {run: node server.js}\n"
        );
        // Without its lock a package's kind is unknown, so nothing is due.
        std::fs::remove_file(crate::lockfile::lock_path(&path)).unwrap();
        let mut bare = ComposeFile::load(&path).unwrap();
        crate::lockfile::attach(&mut bare).unwrap();
        assert_eq!(due_migrations(&bare), Vec::<String>::new());

        let refused = "containers:\n  sb: {worker: path://./sb}\n  onboarding:\n    worker: package://api.workers.iii.dev/onboarding\n    version: 0.2.0\n    scripts: {build: npm ci}\n";
        std::fs::write(&path, refused).unwrap();
        write_lock(&path, &[("onboarding", "onboarding", "0.2.0", "bundle")]);
        let (mut edited, mut file, mut lock) = prepared(&path).await;
        let error = migrate(&path, &mut edited, &mut file, "default", &mut lock)
            .await
            .unwrap_err();
        assert_eq!(error.code(), "SANDBOX_RUN_WITHOUT_BUILD");
        assert_eq!(edited, refused);
        assert!(file.containers["onboarding"].scripts.run.is_none());
    }

    /// `text` with a generated `sandbox-libkrun` at `version` appended, locked at it.
    fn with_locked_generated_adapter(path: &Path, text: &str, version: &str) -> String {
        let request = edit::parse_worker(&format!("sandbox-libkrun@{version}")).unwrap();
        let edit::Outcome::Added(text) = edit::upsert_container(text, &request).unwrap() else {
            panic!("the adapter is new");
        };
        std::fs::write(path, &text).unwrap();
        write_lock(
            path,
            &[("sandbox-libkrun", "sandbox-libkrun", version, "binary")],
        );
        text
    }

    /// Spec 7.2: a generated 0.3.x adapter moves to the newest release in range before the
    /// rewrite, in a project with a VM container; one with ephemeral sandboxes only keeps it.
    #[tokio::test]
    async fn a_generated_adapter_too_old_for_a_vm_is_updated_with_its_lock() {
        let server = adapter_registry(b"").await;
        let dir = tempfile::tempdir().unwrap();
        manifest(dir.path(), "link", VM_MANIFEST);
        let path = dir.path().join("worker-compose.yaml");
        with_locked_generated_adapter(
            &path,
            "containers:\n  link: {worker: path://./link}\n",
            "0.3.1",
        );
        let (mut edited, mut file, mut lock) = prepared(&path).await;
        assert_eq!(lock.resolved_version("sandbox-libkrun"), Some("0.3.1"));

        let migration = crate::registry::TEST_REGISTRY
            .scope(
                server.uri(),
                migrate(&path, &mut edited, &mut file, "default", &mut lock),
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(migration.containers, ["link"]);
        assert!(matches!(
            migration.adapter,
            Some(AdapterChange::Upsert(ref request)) if request.key == "sandbox-libkrun"
        ));
        assert_eq!(lock.resolved_version("sandbox-libkrun"), Some("0.4.0"));
        assert_eq!(
            file.containers["sandbox-libkrun"].version.as_deref(),
            Some("0.4.0")
        );
        assert!(edited.contains("version: \"0.4.0\""), "{edited}");
        assert!(edit::is_generated_container(&edited, "sandbox-libkrun").unwrap());
        assert_eq!(file.containers["link"].start_after, ["sandbox-libkrun"]);

        // Ephemeral sandboxes only: 0.3.x serves them, and nothing is resolved.
        let text = with_locked_generated_adapter(
            &path,
            "engine: {sandbox: {}}\ncontainers:\n  plain: {worker: path://./plain}\n",
            "0.3.1",
        );
        manifest(dir.path(), "plain", "scripts: {start: run}\n");
        let (mut edited, mut file, mut lock) = prepared(&path).await;
        assert_eq!(
            migrate(&path, &mut edited, &mut file, "default", &mut lock)
                .await
                .unwrap(),
            None
        );
        assert_eq!(edited, text);
    }

    /// An adapter the operator declared is theirs to move: refused, the file untouched.
    #[tokio::test]
    async fn an_operator_adapter_too_old_for_a_vm_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        manifest(dir.path(), "link", VM_MANIFEST);
        let path = dir.path().join("worker-compose.yaml");
        let text = "containers:\n  sb: {worker: package://api.workers.iii.dev/sandbox-libkrun, version: 0.3.1}\n  link: {worker: path://./link}\n";
        std::fs::write(&path, text).unwrap();
        write_lock(&path, &[("sb", "sandbox-libkrun", "0.3.1", "binary")]);
        let (mut edited, mut file, mut lock) = prepared(&path).await;

        let error = migrate(&path, &mut edited, &mut file, "default", &mut lock)
            .await
            .unwrap_err();

        assert_eq!(error.code(), "SANDBOX_ADAPTER_TOO_OLD");
        assert!(
            error
                .to_string()
                .contains("iii call compose::update worker=sb@latest"),
            "{error}"
        );
        assert_eq!(edited, text);
    }

    /// `--frozen` edits nothing: a due migration first, then a missing adapter, then one out of
    /// range.
    #[test]
    fn frozen_refuses_a_due_migration_before_the_adapter_checks() {
        let dir = tempfile::tempdir().unwrap();
        manifest(dir.path(), "link", VM_MANIFEST);
        let due = parse(dir.path(), "containers:\n  link: {worker: path://./link}\n");
        let error = require_declared_adapter(&due, "default").unwrap_err();
        assert_eq!(error.code(), "VM_SCRIPTS_MISSING");
        assert!(error.to_string().starts_with("link: "), "{error}");
        assert!(error.to_string().contains("iii compose --up"), "{error}");

        let explicit = "containers:\n  link:\n    worker: path://./link\n    start_after: [sb]\n    scripts: {build: iii sandbox build, run: iii sandbox run}\n";
        let missing = parse(
            dir.path(),
            &explicit.replace("start_after: [sb]", "start_after: []"),
        );
        // Without an adapter its `start_after` cannot name one: due, and refused as such.
        assert_eq!(
            require_declared_adapter(&missing, "default")
                .unwrap_err()
                .code(),
            "VM_SCRIPTS_MISSING"
        );
        let ephemeral = parse(
            dir.path(),
            "engine: {sandbox: {}}\ncontainers:\n  plain: {worker: path://./link, scripts: {build: \"\", run: x}}\n",
        );
        assert_eq!(
            require_declared_adapter(&ephemeral, "default")
                .unwrap_err()
                .code(),
            "SANDBOX_ADAPTER_NOT_DECLARED"
        );
        let old = parse(
            dir.path(),
            &format!("{explicit}  sb: {{worker: package://sandbox-libkrun, version: 0.3.1}}\n"),
        );
        assert_eq!(
            require_declared_adapter(&old, "default")
                .unwrap_err()
                .code(),
            "SANDBOX_ADAPTER_TOO_OLD"
        );
        let current = parse(
            dir.path(),
            &format!("{explicit}  sb: {{worker: package://sandbox-libkrun, version: 0.4.0}}\n"),
        );
        require_declared_adapter(&current, "default").unwrap();
    }

    /// Spec 4.2, a code per rule.
    #[test]
    fn the_rules_of_the_explicit_mode_have_their_codes() {
        let dir = tempfile::tempdir().unwrap();
        manifest(dir.path(), "vm", VM_MANIFEST);
        manifest(dir.path(), "host", "scripts: {start: run}\n");
        let check = |containers: &str| {
            let mut file = parse(dir.path(), &format!("containers:\n{containers}"));
            for (key, kind) in [("bin", "binary"), ("app", "bundle")] {
                if let Some(container) = file.containers.get_mut(key) {
                    container.resolved_package =
                        Some(package(&format!("name: {key}, type: {kind}")));
                }
            }
            validate_scripts(&file).err().map(|error| error.code())
        };
        let vm = "start_after: [sb], scripts: {build: iii sandbox build, run: iii sandbox run}";
        let sb = "  sb: {worker: package://sandbox-libkrun, version: 0.4.0}\n";
        for (containers, code) in [
            (format!("{sb}  vm: {{worker: path://./vm, {vm}}}\n"), None),
            (
                format!("{sb}  app: {{worker: package://app, version: 1.0.0, {vm}}}\n"),
                None,
            ),
            (
                format!("{sb}  host: {{worker: path://./host, {vm}}}\n"),
                Some("SANDBOX_RUN_NOT_A_VM_WORKER"),
            ),
            (
                format!("{sb}  bin: {{worker: package://bin, version: 1.0.0, {vm}}}\n"),
                Some("RUN_NOT_ALLOWED_FOR_PACKAGE"),
            ),
            (
                "  bin: {worker: package://bin, version: 1.0.0, scripts: {build: make}}\n".into(),
                Some("RUN_NOT_ALLOWED_FOR_PACKAGE"),
            ),
            (
                "  vm: {worker: path://./vm, scripts: {build: iii sandbox build, run: node .}}\n"
                    .into(),
                Some("SANDBOX_BUILD_WITHOUT_RUN"),
            ),
            (
                format!("{sb}  vm: {{worker: path://./vm, start_after: [sb], scripts: {{build: npm ci, run: iii sandbox run}}}}\n"),
                Some("SANDBOX_RUN_WITHOUT_BUILD"),
            ),
            (
                format!("{sb}  vm: {{worker: path://./vm, start_after: [sb], scripts: {{build: '', run: iii sandbox run}}}}\n"),
                Some("SANDBOX_RUN_WITHOUT_BUILD"),
            ),
            // On the host, with nothing to build or a build of its own.
            (
                "  vm: {worker: path://./vm, scripts: {build: '', run: node .}}\n".into(),
                None,
            ),
            (
                "  app: {worker: package://app, version: 1.0.0, scripts: {build: npm ci, run: node .}}\n".into(),
                None,
            ),
        ] {
            assert_eq!(check(&containers), code, "{containers}");
        }
    }

    /// `destroy` answers S509 while the adapter aborts a stopped build of the same state: it is
    /// asked again, and a refusal that outlasts the grace only leaves the state behind.
    #[tokio::test]
    async fn a_destroy_refused_for_a_running_build_is_asked_again() {
        for (refusals, destroyed) in [(2_usize, true), (usize::MAX, false)] {
            let destroys = std::sync::atomic::AtomicUsize::new(0);
            let (url, mut called) = fake_engine_url(move |function, _| match function {
                "sandbox-libkrun::worker::destroy" => {
                    if destroys.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < refusals {
                        refused(SandboxErrorCode::S509, 0)
                    } else {
                        answered(json_destroyed())
                    }
                }
                _ => not_found(),
            })
            .await;
            let dir = tempfile::tempdir().unwrap();
            manifest(dir.path(), "vm", VM_MANIFEST);
            let file = parse(
                dir.path(),
                "containers:\n  vm: {worker: path://./vm, scripts: {build: iii sandbox build, run: iii sandbox run}}\n",
            );
            let engine = EngineClient::connect(&url, "t", "shop");
            while !engine.is_connected() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let (shutdown, post_runs, diagnostics) = Default::default();
            let logs = crate::logs::LogStore::open(dir.path().join("logs")).unwrap();
            let ctx = LifecycleCtx {
                shutdown: &shutdown,
                file: &file,
                engine: &engine,
                post_runs: &post_runs,
                compose_namespace: "t",
                project_namespace: "shop",
                engine_url: &url,
                package_cache: dir.path(),
                logs: &logs,
                state_dir: dir.path(),
                sandbox_token: "t",
                empty_env_diagnostics: &diagnostics,
                legacy_vms: &Default::default(),
            };
            let began = tokio::time::Instant::now();
            destroy_vm(&ctx, "vm", "sandbox-libkrun", "shop").await;
            let calls: Vec<String> = drain(&mut called)
                .into_iter()
                .map(|(call, _)| call)
                .collect();
            if destroyed {
                assert_eq!(calls, ["worker::destroy"; 3]);
            } else {
                assert!(calls.len() > 3, "{calls:?}");
                assert!(
                    began.elapsed() >= BUILD_ABORT_GRACE,
                    "{:?}",
                    began.elapsed()
                );
            }
        }
    }

    fn json_destroyed() -> serde_json::Value {
        serde_json::json!({ "destroyed": true })
    }
}
