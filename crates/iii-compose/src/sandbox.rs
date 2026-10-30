// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Which containers need the sandbox adapter, the generated container that
//! installs it, the start order that puts the adapter ahead of them, and the
//! VM containers it boots, supervises and destroys through `worker::*`.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    process::ExitStatus,
    time::Duration,
};

use iii_sandbox_contract::{
    self as contract, ADAPTER_INFO_SUFFIX, AdapterInfo, AdapterInfoRequest, CONTRACT_MAJOR,
    ContractVersion, DEFAULT_ADAPTER, SandboxErrorCode, State, WORKER_DESTROY, WORKER_LOGS,
    WORKER_PREPARE, WORKER_START, WORKER_STATUS, WORKER_STOP, WorkerDestroyRequest,
    WorkerDestroyResponse, WorkerLogsRequest, WorkerLogsResponse, WorkerPrepareRequest,
    WorkerPrepareResponse, WorkerStartRequest, WorkerStartResponse, WorkerStatus,
    WorkerStatusRequest, WorkerStopRequest, adapter_function_id, code_of,
};
use iii_sdk::{IIIClient, protocol::TriggerRequest, runtime::IIIConnectionState};
use sha2::{Digest, Sha256};
use tokio::{sync::watch, task::JoinHandle};

use crate::{
    config::{ComposeFile, WorkerSource},
    edit::{self, NewContainer, Source},
    engine::EngineClient,
    error::{ComposeError, Result},
    lifecycle::LifecycleCtx,
    lockfile::PreparedLock,
    logs::LogSink,
    manifest::{StartSpec, VmSpec, read_manifest},
    process::{DEFAULT_STOP_GRACE, Outcome},
    state::{Reconciliation, VmRecord},
};

/// The adapter release range that implements contract major [`CONTRACT_MAJOR`], changed together with it.
pub const ADAPTER_RANGE: &str = "^0.2";

/// Why a project needs the sandbox adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Need {
    /// A registry bundle container, by key.
    VmBundle(String),
    /// A `path://` container whose manifest sets `runtime.base_image`, by key.
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

/// The VM containers in declaration order.
fn vm_needs(file: &ComposeFile) -> Vec<Need> {
    file.containers
        .iter()
        .filter_map(|(key, container)| match &container.worker {
            WorkerSource::Package { .. } => container
                .resolved_package
                .as_ref()
                .is_some_and(|package| package.kind == "bundle")
                .then(|| Need::VmBundle(key.clone())),
            // A broken manifest is `validate_offline`'s to report.
            WorkerSource::Path { dir, .. } => read_manifest(dir)
                .ok()
                .flatten()
                .is_some_and(|manifest| manifest.base_image.is_some())
                .then(|| Need::VmLocal(key.clone())),
        })
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

/// `--frozen` never edits the file, so a need with no declared adapter fails.
/// `file` must be lock-attached, as for [`needs`].
pub(crate) fn require_declared_adapter(file: &ComposeFile, project_namespace: &str) -> Result<()> {
    let adapter = adapter_name(file);
    let needs = needs(file, project_namespace);
    if !needs.is_empty() && adapter_key(file, adapter)?.is_none() {
        return Err(ComposeError::SandboxAdapterNotDeclared {
            adapter: adapter.to_string(),
            needed_by: needed_by(&needs)[0].to_string(),
        });
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

/// Starts every VM container after `adapter_key`. The edge lives in memory
/// only, so an operator's container is never rewritten. An adapter that is
/// itself a VM container is a `DEPENDENCY_CYCLE`.
pub fn wire(file: &mut ComposeFile, adapter_key: &str) -> Result<()> {
    for need in vm_needs(file) {
        let (Need::VmBundle(key) | Need::VmLocal(key)) = need else {
            continue;
        };
        let start_after = &mut file.containers[&key].start_after;
        if !start_after
            .iter()
            .any(|dependency| dependency == adapter_key)
        {
            start_after.push(adapter_key.to_string());
        }
    }
    crate::dag::validate_dependencies(file)
}

/// Whether a VM of `adapter` went with its adapter container: this file
/// declares one, and it is not running here (absent, or exited). The adapter
/// is the VM's lifeline (spec 7.3), so no `worker::stop` can be answered, as
/// [`vm_reconciliation`] assumes for a dead owner. An adapter the file does not
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

/// [`wire`] to this project's adapter container; no adapter, no change.
pub(crate) fn wire_adapter(file: &mut ComposeFile) -> Result<()> {
    match adapter_key(file, adapter_name(file))? {
        Some(key) => wire(file, &key),
        None => Ok(()),
    }
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

/// Adds the adapter a VM needs, re-resolves (`refresh`) or removes
/// (`allow_remove`) the one Compose generated. An operator's adapter, of any
/// key, version or source, is never touched. `file` must be lock-attached.
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
    let container = &file.containers[&key];
    // `compose::add` marks a `path://` request too; only a package is ours.
    if !matches!(container.worker, WorkerSource::Package { .. })
        || !edit::is_generated_container(text, &key)?
    {
        return Ok(None);
    }
    if !needed {
        return Ok(allow_remove.then_some(AdapterChange::Remove(key)));
    }
    if !refresh {
        return Ok(None);
    }
    let mut request = adapter_request(adapter).await?;
    let unchanged = matches!(&request.source, Source::Package { version, .. }
        if version.as_deref() == container.version.as_deref());
    request.key = key;
    Ok((!unchanged).then_some(AdapterChange::Upsert(request)))
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

/// How long [`RemoteVm::stop`] keeps asking an adapter that does not answer;
/// short under test, so a test can wait the bound out.
const STOP_RETRY: Duration = Duration::from_secs(if cfg!(test) { 2 } else { 60 });
/// How long after its first probe [`stop_unanswered_start`] asks again.
const PROBE_AGAIN: Duration = Duration::from_millis(500);
/// `worker::prepare`'s budget: room for an image pull (spec 5.4).
const PREPARE_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// How long a registered adapter may take to register `adapter::info` (spec 7.4);
/// short under test, below STOP_RETRY.
const INFO_RETRY: Duration = Duration::from_secs(if cfg!(test) { 1 } else { 10 });
/// How long a VM start waits for a dropped engine connection to come back: an
/// engine restart's few seconds plus the SDK's 2 s reconnect delay, so a dead
/// engine fails the start near 0.25's ~17 s, not at PREPARE_TIMEOUT; short under test.
const RECONNECT_GRACE: Duration = Duration::from_secs(if cfg!(test) { 5 } else { 10 });
/// `worker::destroy` removes a VM's whole state, rootfs included.
const DESTROY_TIMEOUT: Duration = Duration::from_secs(60);
/// `worker::status` cadence, and the pause between unanswered stops.
const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// A status call's budget, and what a stop call gets beyond its grace.
const CALL_SLACK: Duration = Duration::from_secs(10);

/// A VM container, supervised through its adapter's `worker::*` functions. A
/// task polls `worker::status` every second into a watch, so `poll` never
/// blocks; it ends at an exit, or when this handle drops.
pub struct RemoteVm {
    pub record: VmRecord,
    client: IIIClient,
    token: String,
    outcome: watch::Receiver<Outcome>,
    /// `None` for a VM adopted from an earlier daemon.
    logs: Option<LogSink>,
}

impl RemoteVm {
    /// With `logs`, the poller also keeps the VM's `worker::logs` stream open
    /// into it.
    pub(crate) fn spawn(
        client: IIIClient,
        record: VmRecord,
        token: String,
        logs: Option<LogSink>,
    ) -> Self {
        let (sender, outcome) = watch::channel(Outcome::Running);
        let (poller, vm, auth, sink) =
            (client.clone(), record.clone(), token.clone(), logs.clone());
        tokio::spawn(async move {
            let (mut capture, mut cursor) = (None, None);
            // A boot that fails at once can be `Exited` by the first tick, so
            // its stream opens now; the adapter drains it before closing.
            if let Some(sink) = &sink {
                follow_logs(&poller, &vm, &auth, sink, &mut capture, &mut cursor).await;
            }
            loop {
                let tick = async {
                    tokio::time::sleep(POLL_INTERVAL).await;
                    let mut reply = worker_status(&poller, &vm, &auth).await;
                    if let Some(live) = superseded_by(&reply, &vm) {
                        let _ = worker_stop(&poller, &live, &auth, DEFAULT_STOP_GRACE).await;
                        reply = worker_status(&poller, &vm, &auth).await;
                    }
                    outcome_of(reply)
                };
                tokio::select! {
                    // Every receiver is gone, so the `RemoteVm` is.
                    () = sender.closed() => return,
                    outcome = tick => {
                        let exited = matches!(outcome, Outcome::Exited(_));
                        // A stream down at the exit (a dropped bus) would miss
                        // the VM's last lines, which its next boot truncates:
                        // they are read once more before the exit is published.
                        if exited && let Some(sink) = &sink {
                            follow_logs(&poller, &vm, &auth, sink, &mut capture, &mut cursor)
                                .await;
                            if let Some(draining) = capture.take() {
                                let _ = tokio::time::timeout(CALL_SLACK, draining).await;
                            }
                        }
                        sender.send_replace(outcome);
                        if exited {
                            return;
                        }
                        if let Some(sink) = &sink {
                            follow_logs(&poller, &vm, &auth, sink, &mut capture, &mut cursor)
                                .await;
                        }
                    }
                }
            }
        });
        Self {
            record,
            client,
            token,
            outcome,
            logs,
        }
    }

    pub fn poll(&self) -> Outcome {
        self.outcome.borrow().clone()
    }

    /// Resolves at the first `Exited` the poller sees.
    pub async fn wait(&self) -> ExitStatus {
        let mut outcome = self.outcome.clone();
        let exited = outcome
            .wait_for(|outcome| matches!(outcome, Outcome::Exited(_)))
            .await
            .map(|outcome| outcome.clone());
        match exited {
            Ok(Outcome::Exited(status)) => status,
            _ => vm_exit(None, None),
        }
    }

    /// `worker::stop` with `grace`. Only an answer from the adapter ends it: a
    /// bus error is retried every second until the VM is seen to exit or
    /// [`STOP_RETRY`] passes ([`INFO_RETRY`] of `function_not_found`: an
    /// adapter registers again within it, so one still missing is gone or on
    /// another engine), then this warns and returns, so `down` cannot hang on
    /// an adapter that is gone.
    pub async fn stop(&self, grace: Duration) -> ExitStatus {
        let deadline = tokio::time::Instant::now() + STOP_RETRY;
        let mut missing_since = None;
        loop {
            if let Outcome::Exited(status) = self.poll() {
                return status;
            }
            let mut reply = worker_stop(&self.client, &self.record, &self.token, grace).await;
            if let Some(live) = superseded_by(&reply, &self.record) {
                let _ = worker_stop(&self.client, &live, &self.token, grace).await;
                reply = worker_stop(&self.client, &self.record, &self.token, grace).await;
            }
            let answered = match &reply {
                Ok(_) => true,
                Err(error) => sandbox_code(error).is_some(),
            };
            if answered {
                return match outcome_of(reply) {
                    Outcome::Exited(status) => status,
                    Outcome::Running | Outcome::Unknown => vm_exit(None, None),
                };
            }
            let now = tokio::time::Instant::now();
            let unregistered = matches!(
                &reply,
                Err(iii_sdk::Error::Remote { code, .. }) if code == "function_not_found"
            );
            missing_since = unregistered.then(|| missing_since.unwrap_or(now));
            if now >= deadline || missing_since.is_some_and(|since| now >= since + INFO_RETRY) {
                let VmRecord {
                    adapter,
                    vm_id,
                    instance,
                    ..
                } = &self.record;
                crate::report::daemon_line(
                    &format!(
                        "{adapter}: VM {vm_id} instance {instance} did not answer worker::stop; \
                         it may still be running"
                    ),
                    true,
                );
                return vm_exit(None, None);
            }
            let mut outcome = self.outcome.clone();
            tokio::select! {
                () = tokio::time::sleep(POLL_INTERVAL) => {}
                Ok(_) = outcome.wait_for(|outcome| matches!(outcome, Outcome::Exited(_))) => {}
            }
        }
    }

    pub(crate) fn is_adopted(&self) -> bool {
        self.logs.is_none()
    }
}

/// Keeps one `worker::logs` stream open into `sink`. A stream that closed
/// while the VM runs (an engine restart, a dropped bus, a full log queue) is
/// asked for again from the cursor it reached, kept in memory only; a failed
/// call is retried on the next poll.
async fn follow_logs(
    client: &IIIClient,
    vm: &VmRecord,
    token: &str,
    sink: &LogSink,
    capture: &mut Option<JoinHandle<Option<u64>>>,
    cursor: &mut Option<u64>,
) {
    match capture.take() {
        Some(live) if !live.is_finished() => {
            *capture = Some(live);
            return;
        }
        Some(done) => {
            if let Ok(Some(last)) = done.await {
                *cursor = Some(last);
            }
        }
        None => {}
    }
    let Ok(channel) = iii_sdk::helpers::create_channel(client, None).await else {
        return;
    };
    let request = WorkerLogsRequest {
        auth: token.to_string(),
        vm_id: vm.vm_id.clone(),
        cursor: *cursor,
        follow: true,
        channel: contract::StreamChannelRef {
            channel_id: channel.writer_ref.channel_id,
            access_key: channel.writer_ref.access_key,
            direction: contract::ChannelDirection::Write,
        },
    };
    let reply: std::result::Result<WorkerLogsResponse, _> = call(
        client,
        &vm.adapter,
        &vm.namespace,
        WORKER_LOGS,
        &request,
        CALL_SLACK,
    )
    .await;
    if reply.is_ok() {
        *capture = Some(sink.capture_records(channel.reader));
    }
}

/// Fails the start of `container` unless the adapter speaks this engine's
/// contract major and calls itself `adapter`, the name `adapter_key` was
/// declared as (spec 5.7).
pub(crate) fn check_info(
    info: &AdapterInfo,
    adapter: &str,
    adapter_key: &str,
    container: &str,
) -> Result<()> {
    let ContractVersion { major, minor } = &info.contract;
    if *major != CONTRACT_MAJOR {
        return Err(ComposeError::SpawnFailed {
            container: container.to_string(),
            message: format!(
                "adapter {adapter} implements sandbox contract {major}.{minor}, but this engine \
                 needs major {CONTRACT_MAJOR}; install a {adapter} release that implements \
                 sandbox contract {CONTRACT_MAJOR}.x"
            ),
        });
    }
    if info.adapter != adapter {
        return Err(ComposeError::SandboxAdapterMismatch {
            container: adapter_key.to_string(),
            expected: adapter.to_string(),
            found: info.adapter.clone(),
        });
    }
    Ok(())
}

/// `worker::start` for VM container `key` (spec 5.4, 7.4).
pub(crate) fn start_request(
    token: &str,
    state_dir: &Path,
    engine_url: &str,
    key: &str,
    start: &StartSpec,
    env: BTreeMap<String, String>,
) -> Result<WorkerStartRequest> {
    let failed = |message: String| ComposeError::SpawnFailed {
        container: key.to_string(),
        message,
    };
    let host_path = |path: &Path| {
        path.to_str().map(str::to_string).ok_or_else(|| {
            failed(format!(
                "{} is not UTF-8, so the sandbox adapter cannot be handed it",
                path.display()
            ))
        })
    };
    let source = match start {
        StartSpec::Vm(VmSpec::Bundle {
            package,
            install_dir,
        }) => contract::WorkerSource::Bundle {
            package: package.clone(),
            host_path: host_path(install_dir)?,
        },
        StartSpec::Vm(VmSpec::Local {
            worker_dir,
            run_override,
        }) => contract::WorkerSource::Local {
            host_path: host_path(worker_dir)?,
            run_override: run_override.clone(),
        },
        StartSpec::Shell(_) | StartSpec::Exec { .. } => {
            return Err(failed("not a VM container".to_string()));
        }
    };
    Ok(WorkerStartRequest {
        auth: token.to_string(),
        state_key: state_key(state_dir, key),
        worker_name: key.to_string(),
        source,
        env,
        engine_url: engine_url.to_string(),
    })
}

/// Boots VM container `key` through this project's adapter, in the project
/// namespace: `adapter::info` and its checks, `worker::prepare`, then
/// `worker::start`. An S502 from the start names a live boot of the same
/// state, which is stopped before the one retry.
pub(crate) async fn start_vm(
    ctx: &LifecycleCtx<'_>,
    key: &str,
    start: &StartSpec,
    env: BTreeMap<String, String>,
) -> Result<RemoteVm> {
    let container = ctx
        .file
        .containers
        .get(key)
        .ok_or_else(|| ComposeError::UnknownContainer {
            container: key.to_string(),
        })?;
    let adapter = adapter_name(ctx.file);
    let declared = adapter_key(ctx.file, adapter)?.unwrap_or_else(|| adapter.to_string());
    let (engine, namespace) = (ctx.engine, ctx.project_namespace);
    let failed = |error: iii_sdk::Error| ComposeError::SpawnFailed {
        container: key.to_string(),
        message: spawn_failed_message(&error),
    };

    let info = start_call::<AdapterInfo>(
        engine,
        key,
        adapter,
        namespace,
        ADAPTER_INFO_SUFFIX,
        &AdapterInfoRequest {},
        INFO_RETRY + CALL_SLACK,
        true,
    )
    .await
    .map_err(failed)?;
    check_info(&info, adapter, &declared, key)?;

    let request = start_request(
        ctx.sandbox_token,
        ctx.state_dir,
        ctx.engine_url,
        key,
        start,
        env,
    )?;
    let prepare = WorkerPrepareRequest {
        auth: request.auth.clone(),
        source: request.source.clone(),
    };
    start_call::<WorkerPrepareResponse>(
        engine,
        key,
        adapter,
        namespace,
        WORKER_PREPARE,
        &prepare,
        PREPARE_TIMEOUT,
        false,
    )
    .await
    .map_err(|error| prepare_failed(ctx.file, key, &declared, start, &error))?;

    let started = match worker_start(
        engine,
        adapter,
        namespace,
        &request,
        container.startup_timeout,
        ctx.file.stop_timeout,
    )
    .await
    {
        Ok(started) => started,
        Err(error) => {
            // No answer from the adapter (out of time, or lost with the bus):
            // it may still be booting the VM.
            if sandbox_code(&error).is_none() {
                stop_unanswered_start(ctx, key).await;
            }
            return Err(failed(error));
        }
    };

    Ok(RemoteVm::spawn(
        engine.client().clone(),
        VmRecord {
            adapter: adapter.to_string(),
            namespace: namespace.to_string(),
            vm_id: started.vm_id,
            instance: started.instance,
        },
        ctx.sandbox_token.to_string(),
        Some(ctx.logs.sink(key)),
    ))
}

/// The failure of container `key`'s `worker::prepare`. The adapter reads its
/// host roots when it starts (`layer_adapter_env`), so a local VM outside the
/// compose file's directory, added since, is refused (S501) until it restarts.
fn prepare_failed(
    file: &ComposeFile,
    key: &str,
    adapter_key: &str,
    start: &StartSpec,
    error: &iii_sdk::Error,
) -> ComposeError {
    let mut message = spawn_failed_message(error);
    if let StartSpec::Vm(VmSpec::Local { worker_dir, .. }) = start
        && sandbox_code(error) == Some(SandboxErrorCode::S501)
        && let (Ok(dir), Ok(compose_dir)) = (
            worker_dir.canonicalize(),
            file.path.parent().unwrap_or(Path::new(".")).canonicalize(),
        )
        && !dir.starts_with(compose_dir)
    {
        message.push_str(&format!(
            ". If {} was added after the adapter started, restart the adapter, which \
             stops every VM: iii trigger compose::restart worker={adapter_key}",
            dir.display()
        ));
    }
    ComposeError::SpawnFailed {
        container: key.to_string(),
        message,
    }
}

/// [`start_vm`] until `shutdown` fires, then `None`. Dropping the start does
/// not reach the adapter, which goes on to boot the VM, so that boot is
/// stopped first.
pub(crate) async fn start_vm_until(
    ctx: &LifecycleCtx<'_>,
    key: &str,
    start: &StartSpec,
    env: BTreeMap<String, String>,
    shutdown: Option<&crate::shutdown::ShutdownSignal>,
) -> Option<Result<RemoteVm>> {
    let Some(signal) = shutdown else {
        return Some(start_vm(ctx, key, start, env).await);
    };
    let started = signal.run(start_vm(ctx, key, start, env)).await;
    if started.is_none() {
        stop_unanswered_start(ctx, key).await;
    }
    started
}

/// Stops the boot of VM container `key` that a `worker::start` left without an
/// answer: a VM nobody holds would register as `key`. Asked for instance 0,
/// which no boot is, the adapter names the live one (S503), and stopping that
/// cancels a boot still preparing. Best effort, over a connection that is up.
async fn stop_unanswered_start(ctx: &LifecycleCtx<'_>, key: &str) {
    if ctx.engine.has_exited() || !ctx.engine.is_connected() {
        return;
    }
    let probe = VmRecord {
        adapter: adapter_name(ctx.file).to_string(),
        namespace: ctx.project_namespace.to_string(),
        vm_id: vm_id(&state_key(ctx.state_dir, key)),
        instance: 0,
    };
    let (client, grace) = (ctx.engine.client(), ctx.file.stop_timeout);
    let stop_live = || async {
        let reply = worker_stop(client, &probe, ctx.sandbox_token, grace).await;
        if let Some(live) = superseded_by(&reply, &probe) {
            let _ = worker_stop(client, &live, ctx.sandbox_token, grace).await;
        }
    };
    stop_live().await;
    // The adapter reserves the boot only after admitting its source, so a
    // cancel right after the send can probe first (S504, or an older instance).
    // shortcut: one more probe a moment later; a contract stop by state_key
    // would close the window outright.
    tokio::time::sleep(PROBE_AGAIN).await;
    stop_live().await;
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
/// adapter refuses it while the VM runs, so it goes after the stop. A failure
/// only leaves the state behind, and says where.
pub(crate) async fn destroy_vm(ctx: &LifecycleCtx<'_>, key: &str, adapter: &str, namespace: &str) {
    let state_key = state_key(ctx.state_dir, key);
    let request = WorkerDestroyRequest {
        auth: ctx.sandbox_token.to_string(),
        state_key: state_key.clone(),
    };
    // Registering: an adapter started for this call may not list it yet.
    let destroyed = start_call::<WorkerDestroyResponse>(
        ctx.engine,
        key,
        adapter,
        namespace,
        WORKER_DESTROY,
        &request,
        DESTROY_TIMEOUT,
        true,
    )
    .await;
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

/// Whether `key` is a VM container. `file` must be lock-attached, as for
/// [`needs`].
pub(crate) fn is_vm(file: &ComposeFile, key: &str) -> bool {
    vm_needs(file)
        .iter()
        .any(|need| matches!(need, Need::VmBundle(vm) | Need::VmLocal(vm) if vm == key))
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

/// The adapter's own words for an S-code error, else the bus error as is.
pub(crate) fn spawn_failed_message(error: &iii_sdk::Error) -> String {
    sandbox_payload(error)
        .and_then(|payload| payload["message"].as_str().map(str::to_string))
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
fn sandbox_code(error: &iii_sdk::Error) -> Option<SandboxErrorCode> {
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
fn vm_exit(code: Option<i32>, signal: Option<i32>) -> ExitStatus {
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

/// `worker::start`. An S502 names a live boot of the same state, which is
/// stopped with `grace` before the one retry.
async fn worker_start(
    engine: &EngineClient,
    adapter: &str,
    namespace: &str,
    request: &WorkerStartRequest,
    timeout: Duration,
    grace: Duration,
) -> std::result::Result<WorkerStartResponse, iii_sdk::Error> {
    let start = || {
        start_call(
            engine,
            &request.worker_name,
            adapter,
            namespace,
            WORKER_START,
            request,
            timeout,
            false,
        )
    };
    let mut started = start().await;
    if let Err(error) = &started
        && sandbox_code(error) == Some(SandboxErrorCode::S502)
    {
        // A stop that fails (S502 on a boot still preparing) still gets the retry.
        if let Some(live) = live_instance(error) {
            let live = VmRecord {
                adapter: adapter.to_string(),
                namespace: namespace.to_string(),
                vm_id: live.vm_id,
                instance: live.instance,
            };
            let _ = worker_stop(engine.client(), &live, &request.auth, grace).await;
        }
        started = start().await;
    }
    started
}

async fn worker_stop(
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
async fn call<T: serde::de::DeserializeOwned>(
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
    let request = TriggerRequest {
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
            reply = client.trigger(request) => reply,
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

    use super::*;

    fn parse(dir: &Path, text: &str) -> ComposeFile {
        ComposeFile::parse(text, dir.join("worker-compose.yaml")).unwrap()
    }

    fn manifest(dir: &Path, worker: &str, text: &str) {
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

    #[test]
    fn wire_starts_every_vm_after_the_adapter_in_memory() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = vm_project(dir.path());
        wire(&mut file, "sb").unwrap();
        assert!(file.containers["app"].start_after.contains(&"sb".into()));
        assert!(file.containers["local"].start_after.contains(&"sb".into()));
        assert!(!file.containers["bin"].start_after.contains(&"sb".into()));
        wire(&mut file, "sb").unwrap();
        assert_eq!(file.containers["app"].start_after, vec!["sb".to_string()]);
        assert_eq!(file.start_order().unwrap()[0], "sb");
    }

    #[test]
    fn an_adapter_that_is_a_vm_container_is_a_dependency_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        manifest(
            dir,
            "dev",
            "name: sandbox-libkrun\nruntime: {base_image: python}\nscripts: {start: x}\n",
        );
        let mut file = parse(dir, "containers:\n  dev: {worker: path://./dev}\n");
        assert_eq!(
            wire_adapter(&mut file).unwrap_err().code(),
            "DEPENDENCY_CYCLE"
        );

        // No declared adapter: nothing to order after.
        let mut file = parse(
            dir,
            "engine: {sandbox: {adapter: sandbox-docker}}\ncontainers:\n  dev: {worker: path://./dev}\n",
        );
        wire_adapter(&mut file).unwrap();
        assert!(file.containers["dev"].start_after.is_empty());
    }

    /// A registry whose `/resolve` answers `sandbox-libkrun` 0.2.3 for this
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
                    "version": "0.2.3",
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

    /// `text` with a generated `sandbox-libkrun` 0.2.3 appended.
    fn with_generated_adapter(text: &str) -> String {
        let request = edit::parse_worker("sandbox-libkrun@0.2.3").unwrap();
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
                version: Some("0.2.3".into())
            }
        );
        let asked: serde_json::Value =
            serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
        assert_eq!(asked["version"], ADAPTER_RANGE);
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

    /// A `path://../` VM added while the adapter runs is outside the roots it
    /// read at its start: its S501 says how to get the directory admitted.
    #[test]
    fn a_refused_vm_outside_the_compose_directory_names_the_adapter_restart() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir(&project).unwrap();
        manifest(&project, "inner", "runtime: {base_image: python}\n");
        manifest(
            &dir.path().join("shared"),
            "vm",
            "runtime: {base_image: python}\n",
        );
        let file = parse(&project, "containers:\n  inner: {worker: path://./inner}\n");
        let refused = remote(
            "invocation_failed",
            &wire_message(SandboxErrorCode::S501, "outside the host roots", None),
        );
        let message = |worker_dir: PathBuf| {
            let start = StartSpec::Vm(VmSpec::Local {
                worker_dir,
                run_override: None,
            });
            prepare_failed(&file, "vm", "sb", &start, &refused).to_string()
        };
        let outside = message(project.join("../shared/vm"));
        assert!(
            outside.contains("restart the adapter")
                && outside.ends_with("compose::restart worker=sb"),
            "{outside}"
        );
        assert!(!message(project.join("inner")).contains("restart the adapter"));
    }

    fn remote(code: &str, message: &str) -> iii_sdk::Error {
        iii_sdk::Error::Remote {
            code: code.into(),
            message: message.into(),
            stacktrace: None,
        }
    }

    /// An adapter's S-code error as the SDK sends it: `handler error: {json}`.
    fn wire_message(
        code: SandboxErrorCode,
        message: &str,
        fix: Option<serde_json::Value>,
    ) -> String {
        iii_sdk::Error::Handler(contract::error_payload(code, message, fix, None).to_string())
            .to_string()
    }

    fn status(state: State, exit_code: Option<i32>, signal: Option<i32>) -> WorkerStatus {
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
    fn adapter_info_checks_the_major_then_the_name() {
        let info = |adapter: &str, major| AdapterInfo {
            adapter: adapter.into(),
            adapter_version: "1.0.0".into(),
            contract: ContractVersion { major, minor: 0 },
        };
        assert!(
            check_info(
                &info("sandbox-libkrun", 1),
                "sandbox-libkrun",
                "sandbox-libkrun",
                "app"
            )
            .is_ok()
        );
        let e = check_info(
            &info("sandbox-libkrun", 2),
            "sandbox-libkrun",
            "sandbox-libkrun",
            "app",
        )
        .unwrap_err();
        assert_eq!(e.code(), "SPAWN_FAILED");
        assert_eq!(
            e.to_string(),
            "container 'app' could not start: adapter sandbox-libkrun implements sandbox contract \
             2.0, but this engine needs major 1; install a sandbox-libkrun release that \
             implements sandbox contract 1.x"
        );
        let e = check_info(&info("other", 1), "sandbox-libkrun", "sb", "app").unwrap_err();
        assert_eq!(e.code(), "SANDBOX_ADAPTER_MISMATCH");
        assert_eq!(
            e.to_string(),
            "container 'sb' is the sandbox adapter 'sandbox-libkrun', but adapter::info names \
             'other'. Set engine.sandbox.adapter to 'other' or install 'sandbox-libkrun'"
        );
    }

    #[test]
    fn start_request_follows_spec_7_4() {
        let (dir, env) = (
            Path::new("/srv/app/.iii/compose/shop"),
            BTreeMap::from([("A".to_string(), "1".to_string())]),
        );
        let bundle = StartSpec::Vm(VmSpec::Bundle {
            package: "onboarding".into(),
            install_dir: "/c/onboarding/1.0.0".into(),
        });
        let r = start_request(
            "t",
            dir,
            "ws://127.0.0.1:49134",
            "web",
            &bundle,
            env.clone(),
        )
        .unwrap();
        assert_eq!(
            (r.auth.as_str(), r.state_key.clone(), r.worker_name.as_str()),
            ("t", state_key(dir, "web"), "web")
        );
        // The contract types have no PartialEq: compare their wire form.
        assert_eq!(
            serde_json::to_value(&r.source).unwrap(),
            serde_json::json!({"kind": "bundle", "package": "onboarding", "host_path": "/c/onboarding/1.0.0"})
        );
        assert_eq!(r.env, env);
        assert_eq!(r.engine_url, "ws://127.0.0.1:49134");

        let local = StartSpec::Vm(VmSpec::Local {
            worker_dir: "/srv/app/w".into(),
            run_override: Some("npm start".into()),
        });
        let r = start_request("t", dir, "ws://e", "web", &local, BTreeMap::new()).unwrap();
        assert_eq!(
            serde_json::to_value(&r.source).unwrap(),
            serde_json::json!({"kind": "local", "host_path": "/srv/app/w", "run_override": "npm start"})
        );
        let shell = StartSpec::Shell("npm start".into());
        assert!(start_request("t", dir, "ws://e", "web", &shell, BTreeMap::new()).is_err());
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

    /// A restarted daemon does not ask the adapter about a failed VM, so its
    /// "exited" answer cannot replace the recorded error.
    #[tokio::test]
    async fn a_failed_vm_keeps_its_error_across_a_daemon_restart() {
        use crate::state::{ChildRecord, ChildStatus, DaemonState, Owner, StateStore};

        let (url, mut called) =
            fake_engine_url(|_, _| answered(status(State::Exited, Some(1), None))).await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker-compose.yaml");
        std::fs::write(
            &path,
            "containers:\n  vm:\n    worker: path://.\n    scripts: { run: sleep 60 }\n",
        )
        .unwrap();
        let file = ComposeFile::load(&path).unwrap();
        let namespace = format!("failed-vm-{}", uuid::Uuid::new_v4());
        let store = StateStore::for_project(&namespace, &file.path).unwrap();
        let error = "container 'vm' was not ready after 2s";
        let mut state = DaemonState::new(&file.path, "default");
        state.owner = Some(Owner::current());
        state.containers.insert(
            "vm".into(),
            ChildRecord {
                pid: None,
                birth: None,
                vm: Some(vm_record()),
                status: ChildStatus::Failed,
                started_at: 1,
                last_error: Some(error.into()),
            },
        );
        store.save(&state).unwrap();

        crate::project::Project::open(
            &namespace,
            "default".into(),
            file,
            std::sync::Arc::new(EngineClient::connect(&url, "t", "t")),
            url.clone(),
        )
        .await
        .unwrap();

        let vm = &store.load().unwrap().unwrap().containers["vm"];
        assert_eq!(
            (vm.status, vm.last_error.as_deref()),
            (ChildStatus::Failed, Some(error))
        );
        assert!(drain(&mut called).is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_vm_goes_with_an_adapter_container_that_is_not_running() {
        use crate::lifecycle::{Child, Children};

        let file = parse(
            Path::new("/srv/app"),
            "containers:\n  sb: {worker: package://sandbox-libkrun, version: 1.0.0}\n",
        );
        let mut children = Children::new();
        assert!(adapter_gone(&file, "sandbox-libkrun", &children));

        let mut adapter = tokio::process::Command::new("sleep");
        adapter.arg("30");
        children.insert(
            "sb".into(),
            Child::Process(crate::process::spawn_supervised(adapter).unwrap()),
        );
        assert!(!adapter_gone(&file, "sandbox-libkrun", &children));
        children["sb"].stop(Duration::from_secs(1)).await;
        assert!(adapter_gone(&file, "sandbox-libkrun", &children));

        // Not declared here: its VMs are asked, never presumed gone.
        assert!(!adapter_gone(&file, "sandbox-docker", &Children::new()));
    }

    #[test]
    fn spawn_failed_message_prefers_the_s_code_message() {
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
            "host_path /etc is outside the host roots"
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
    /// it asked from there, then closes.
    async fn fake_engine(
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
    async fn fake_engine_url(
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
                            || function.starts_with("engine::workers::")
                        {
                            continue;
                        }
                        if function.ends_with("::worker::logs") {
                            *asked_from.lock().unwrap() = message["data"]["cursor"].as_u64();
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

    fn answered(result: impl serde::Serialize) -> serde_json::Value {
        serde_json::json!({ "result": result })
    }

    /// An S-code refusal whose `fix` names `instance` of VM `v`.
    fn refused(code: SandboxErrorCode, instance: u64) -> serde_json::Value {
        let fix = serde_json::json!({ "vm_id": "v", "instance": instance });
        let message = wire_message(code, "refused", Some(fix));
        serde_json::json!({ "error": { "code": "invocation_failed", "message": message } })
    }

    fn not_found() -> serde_json::Value {
        serde_json::json!({ "error": { "code": "function_not_found", "message": "gone" } })
    }

    /// Each call so far as (function suffix, the `instance` it names).
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

    fn vm_record() -> VmRecord {
        VmRecord {
            adapter: "sandbox-libkrun".into(),
            namespace: "shop".into(),
            vm_id: "v".into(),
            instance: 1,
        }
    }

    /// The stream opens before the first tick, which already reads `Exited`.
    #[tokio::test]
    async fn a_vm_that_exits_by_the_first_tick_keeps_its_output() {
        let (client, mut called) = fake_engine(|function, _| match function {
            "sandbox-libkrun::worker::logs" => answered(serde_json::json!({})),
            _ => answered(status(State::Exited, Some(1), None)),
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        let store = crate::logs::LogStore::open(dir.path().to_path_buf()).unwrap();
        let vm = RemoteVm::spawn(client, vm_record(), "t".into(), Some(store.sink("web")));
        assert_eq!(vm.wait().await.code(), Some(1));

        // Read once more from its cursor before the exit is published.
        let order: Vec<String> = drain(&mut called).into_iter().map(|(f, _)| f).collect();
        assert_eq!(
            order,
            [
                "engine::channels::create",
                "worker::logs",
                "worker::status",
                "engine::channels::create",
                "worker::logs"
            ]
        );
        assert_eq!(
            crate::logs::tests::entries(&store, "web", 1).await,
            vec![crate::logs::LogEntry {
                stream: crate::logs::LogStream::Stderr,
                message: "no /dev/kvm".into()
            }]
        );
    }

    /// A VM that exits while its log stream is down (a dropped bus) keeps its
    /// last lines: they are read before the exit is published.
    #[tokio::test]
    async fn a_vm_that_exits_while_its_stream_is_down_keeps_its_last_output() {
        let streams = std::sync::atomic::AtomicUsize::new(0);
        let (client, _called) = fake_engine(move |function, _| match function {
            "sandbox-libkrun::worker::logs"
                if streams.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 =>
            {
                not_found()
            }
            "sandbox-libkrun::worker::logs" => answered(serde_json::json!({})),
            _ => answered(status(State::Exited, Some(1), None)),
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        let store = crate::logs::LogStore::open(dir.path().to_path_buf()).unwrap();
        let vm = RemoteVm::spawn(client, vm_record(), "t".into(), Some(store.sink("web")));
        assert_eq!(vm.wait().await.code(), Some(1));
        assert_eq!(
            crate::logs::tests::entries(&store, "web", 1).await,
            vec![crate::logs::LogEntry {
                stream: crate::logs::LogStream::Stderr,
                message: "no /dev/kvm".into()
            }]
        );
    }

    /// S503 on a status: the live boot it names is stopped, the status asked
    /// again, and a second S503 reads as superseded.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_superseded_vm_stops_the_live_boot_then_reads_an_exit_without_code() {
        let (client, mut called) =
            fake_engine(
                |function, payload| match (function, payload["instance"].as_u64()) {
                    ("sandbox-libkrun::worker::status", Some(1)) => {
                        refused(SandboxErrorCode::S503, 2)
                    }
                    ("sandbox-libkrun::worker::stop", Some(2)) => {
                        answered(status(State::Stopped, None, Some(15)))
                    }
                    _ => not_found(),
                },
            )
            .await;
        let vm = RemoteVm::spawn(client, vm_record(), "t".into(), None);
        assert_eq!(vm.wait().await.code(), None);
        let calls = |c: &[(&str, u64)]| -> Vec<(String, Option<u64>)> {
            c.iter().map(|(f, i)| (f.to_string(), Some(*i))).collect()
        };
        assert_eq!(
            drain(&mut called),
            calls(&[
                ("worker::status", 1),
                ("worker::stop", 2),
                ("worker::status", 1)
            ])
        );

        // The same on a stop: the live boot first, then this one again.
        let (client, mut called) =
            fake_engine(
                |function, payload| match (function, payload["instance"].as_u64()) {
                    ("sandbox-libkrun::worker::status", _) => {
                        answered(status(State::Running, None, None))
                    }
                    ("sandbox-libkrun::worker::stop", Some(1)) => {
                        refused(SandboxErrorCode::S503, 2)
                    }
                    ("sandbox-libkrun::worker::stop", Some(2)) => {
                        answered(status(State::Stopped, None, Some(15)))
                    }
                    _ => not_found(),
                },
            )
            .await;
        let vm = RemoteVm::spawn(client, vm_record(), "t".into(), None);
        assert_eq!(vm.stop(Duration::from_secs(1)).await.code(), None);
        let stops: Vec<_> = drain(&mut called)
            .into_iter()
            .filter(|(function, _)| function == "worker::stop")
            .collect();
        assert_eq!(
            stops,
            calls(&[
                ("worker::stop", 1),
                ("worker::stop", 2),
                ("worker::stop", 1)
            ])
        );
    }

    /// S502 on a start: the live boot it names is stopped before the one retry.
    #[tokio::test]
    async fn a_start_that_finds_a_live_boot_stops_it_and_starts_again() {
        let starts = std::sync::atomic::AtomicUsize::new(0);
        let (url, mut called) = fake_engine_url(move |function, _| match function {
            "sandbox-libkrun::worker::start"
                if starts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 =>
            {
                refused(SandboxErrorCode::S502, 1)
            }
            "sandbox-libkrun::worker::start" => {
                answered(serde_json::json!({ "vm_id": "v", "instance": 2 }))
            }
            "sandbox-libkrun::worker::stop" => answered(status(State::Stopped, None, Some(15))),
            _ => not_found(),
        })
        .await;
        let local = StartSpec::Vm(VmSpec::Local {
            worker_dir: "/srv/app/w".into(),
            run_override: None,
        });
        let request = start_request(
            "t",
            Path::new("/srv/app/.iii/compose/shop"),
            "ws://e",
            "web",
            &local,
            BTreeMap::new(),
        )
        .unwrap();
        let started = worker_start(
            &EngineClient::connect(&url, "t", "t"),
            "sandbox-libkrun",
            "shop",
            &request,
            Duration::from_secs(5),
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        assert_eq!((started.vm_id.as_str(), started.instance), ("v", 2));
        assert_eq!(
            drain(&mut called),
            [
                ("worker::start".to_string(), None),
                ("worker::stop".to_string(), Some(1)),
                ("worker::start".to_string(), None)
            ]
        );
    }

    /// An external engine that restarts during a cold `worker::prepare` never
    /// answers it: the start sends it again once the engine is back, and once
    /// the adapter has registered again, so the VM comes up as in 0.25, whose
    /// prepare did not need the bus. With the managed engine exited, the start
    /// ends at once.
    #[tokio::test]
    async fn a_start_sends_a_dropped_call_again_once_the_engine_is_back() {
        let prepares = std::sync::atomic::AtomicUsize::new(0);
        let (url, mut called) = fake_engine_url(move |function, _| match function {
            "sandbox-libkrun::adapter::info" => answered(AdapterInfo {
                adapter: "sandbox-libkrun".into(),
                adapter_version: "1.0.0".into(),
                contract: ContractVersion {
                    major: CONTRACT_MAJOR,
                    minor: 0,
                },
            }),
            "sandbox-libkrun::worker::prepare" => {
                match prepares.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
                    0 | 3 => serde_json::Value::Null,
                    // The adapter has not registered again yet.
                    1 => not_found(),
                    _ => answered(serde_json::json!({})),
                }
            }
            "sandbox-libkrun::worker::start" => {
                answered(serde_json::json!({ "vm_id": "v", "instance": 1 }))
            }
            _ => not_found(),
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
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
        };
        let start = StartSpec::Vm(VmSpec::Local {
            worker_dir: dir.path().join("vm"),
            run_override: None,
        });
        let steps = |called: &mut tokio::sync::mpsc::UnboundedReceiver<_>| {
            drain(called)
                .into_iter()
                .map(|(function, _)| function)
                .filter(|function| {
                    ["adapter::info", "worker::prepare", "worker::start"].contains(&&**function)
                })
                .collect::<Vec<_>>()
        };

        let vm = tokio::time::timeout(
            Duration::from_secs(10),
            start_vm(&ctx, "vm", &start, BTreeMap::new()),
        )
        .await
        .expect("the start must not wait out PREPARE_TIMEOUT");
        assert_eq!(vm.map(|vm| vm.record.instance).ok(), Some(1));
        // The dropped prepare was never answered, so only its re-sends show.
        assert_eq!(
            steps(&mut called),
            [
                "adapter::info",
                "worker::prepare",
                "worker::prepare",
                "worker::start"
            ]
        );

        engine.mark_exited();
        let began = tokio::time::Instant::now();
        let Err(error) = start_vm(&ctx, "vm", &start, BTreeMap::new()).await else {
            panic!("a dropped call to an engine that exited was sent again");
        };
        assert_eq!(error.code(), "SPAWN_FAILED");
        assert!(
            began.elapsed() < Duration::from_secs(1),
            "{:?}",
            began.elapsed()
        );
    }

    /// A `worker::start` whose reply was lost with its connection may still
    /// have booted. Its re-send then meets that boot (S502), which is stopped
    /// before the one retry, so exactly one VM runs.
    #[tokio::test]
    async fn a_re_sent_start_that_meets_the_dropped_boot_leaves_one_vm() {
        // (boots so far, the live instance), as the adapter keeps them.
        let vms = std::sync::Arc::new(std::sync::Mutex::new((0, None)));
        let adapter = vms.clone();
        let (url, _called) = fake_engine_url(move |function, payload| {
            let (boots, live) = &mut *adapter.lock().unwrap();
            match function {
                "sandbox-libkrun::worker::start" => match *live {
                    Some(instance) => refused(SandboxErrorCode::S502, instance),
                    None => {
                        *boots += 1;
                        *live = Some(*boots);
                        if *boots == 1 {
                            return serde_json::Value::Null;
                        }
                        answered(serde_json::json!({ "vm_id": "v", "instance": *boots }))
                    }
                },
                "sandbox-libkrun::worker::stop" => {
                    if *live == payload["instance"].as_u64() {
                        *live = None;
                    }
                    answered(status(State::Stopped, None, Some(15)))
                }
                _ => not_found(),
            }
        })
        .await;
        let local = StartSpec::Vm(VmSpec::Local {
            worker_dir: "/srv/app/w".into(),
            run_override: None,
        });
        let request = start_request(
            "t",
            Path::new("/srv/app/.iii/compose/shop"),
            "ws://e",
            "web",
            &local,
            BTreeMap::new(),
        )
        .unwrap();
        let started = tokio::time::timeout(
            Duration::from_secs(10),
            worker_start(
                &EngineClient::connect(&url, "t", "t"),
                "sandbox-libkrun",
                "shop",
                &request,
                Duration::from_secs(8),
                Duration::from_secs(1),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(started.instance, 2);
        assert_eq!(*vms.lock().unwrap(), (2, Some(2)));
    }

    /// A `worker::start` that outlives its budget, or that a cancel drops, goes
    /// on at the adapter: the boot it left, named by the S503 for instance 0, is
    /// stopped, so no VM Compose does not hold registers as the container. The
    /// first probe lands before the adapter reserved that boot (S504).
    #[tokio::test]
    async fn an_unanswered_start_stops_the_boot_it_left() {
        let probes = std::sync::atomic::AtomicUsize::new(0);
        let (url, mut called) = fake_engine_url(move |function, payload| match function {
            "sandbox-libkrun::adapter::info" => answered(AdapterInfo {
                adapter: "sandbox-libkrun".into(),
                adapter_version: "1.0.0".into(),
                contract: ContractVersion {
                    major: CONTRACT_MAJOR,
                    minor: 0,
                },
            }),
            "sandbox-libkrun::worker::prepare" => answered(serde_json::json!({})),
            // Still building the VM.
            "sandbox-libkrun::worker::start" => serde_json::Value::Bool(true),
            "sandbox-libkrun::worker::stop" => match payload["instance"].as_u64() {
                Some(0)
                    if probes
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                        .is_multiple_of(2) =>
                {
                    refused(SandboxErrorCode::S504, 0)
                }
                Some(0) => refused(SandboxErrorCode::S503, 1),
                _ => answered(status(State::Stopped, None, None)),
            },
            _ => not_found(),
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        let file = parse(
            dir.path(),
            "containers:\n  vm: {worker: path://./vm, startup_timeout: 1s}\n",
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
        };
        let start = StartSpec::Vm(VmSpec::Local {
            worker_dir: dir.path().join("vm"),
            run_override: None,
        });
        let probe = vm_id(&state_key(dir.path(), "vm"));
        let mut stops = || {
            let mut stops = Vec::new();
            while let Ok((function, payload)) = called.try_recv() {
                if function.ends_with("::worker::stop") {
                    stops.push((payload["vm_id"].clone(), payload["instance"].as_u64()));
                }
            }
            stops
        };
        let expected = [
            (serde_json::json!(probe), Some(0)),
            (serde_json::json!(probe), Some(0)),
            (serde_json::json!("v"), Some(1)),
        ];

        let Err(error) = start_vm(&ctx, "vm", &start, BTreeMap::new()).await else {
            panic!("nobody answered the start");
        };
        assert_eq!(error.code(), "SPAWN_FAILED");
        assert_eq!(stops(), expected);

        let signal = shutdown.signal();
        let cancel = async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            shutdown.cancel();
        };
        let (started, ()) = tokio::join!(
            start_vm_until(&ctx, "vm", &start, BTreeMap::new(), Some(&signal)),
            cancel
        );
        assert!(started.is_none());
        assert_eq!(stops(), expected);
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

    /// An adapter that never answers: the stop gives up once STOP_RETRY has
    /// passed, with no code.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_stop_nobody_answers_ends_after_stop_retry() {
        let (client, _called) = fake_engine(
            |_, _| serde_json::json!({ "error": { "code": "timeout", "message": "no reply" } }),
        )
        .await;
        let vm = RemoteVm::spawn(client, vm_record(), "t".into(), None);
        let began = tokio::time::Instant::now();
        assert_eq!(vm.stop(Duration::from_secs(1)).await.code(), None);
        let took = began.elapsed();
        assert!(
            took >= STOP_RETRY && took < STOP_RETRY + POLL_INTERVAL * 2,
            "{took:?}"
        );
    }

    /// An adapter this bus does not have (gone, or answering on another
    /// engine) would register again within INFO_RETRY: the stop gives up
    /// then, not at STOP_RETRY.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_stop_of_an_adapter_not_on_this_bus_ends_after_info_retry() {
        let (client, _called) = fake_engine(|_, _| not_found()).await;
        let vm = RemoteVm::spawn(client, vm_record(), "t".into(), None);
        let began = tokio::time::Instant::now();
        assert_eq!(vm.stop(Duration::from_secs(1)).await.code(), None);
        let took = began.elapsed();
        assert!(took >= INFO_RETRY && took < STOP_RETRY, "{took:?}");
    }

    /// A restarted adapter answers S504 for the VM it lost: the poller reads
    /// an exit (so the restart policy runs), and a stop is answered at once.
    #[cfg(unix)]
    #[tokio::test]
    async fn an_adapter_that_lost_the_vm_ends_it_at_once() {
        let (client, _called) = fake_engine(|_, _| refused(SandboxErrorCode::S504, 1)).await;
        let vm = RemoteVm::spawn(client, vm_record(), "t".into(), None);
        assert_eq!(vm.wait().await.code(), None);

        let (client, _called) = fake_engine(|function, _| match function {
            "sandbox-libkrun::worker::stop" => refused(SandboxErrorCode::S504, 1),
            _ => not_found(),
        })
        .await;
        let vm = RemoteVm::spawn(client, vm_record(), "t".into(), None);
        let began = tokio::time::Instant::now();
        assert_eq!(vm.stop(Duration::from_secs(1)).await.code(), None);
        assert!(began.elapsed() < STOP_RETRY, "{:?}", began.elapsed());
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
}
