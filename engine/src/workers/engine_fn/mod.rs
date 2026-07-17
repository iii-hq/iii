// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, pin::Pin, sync::Arc};

use dashmap::DashMap;
use function_macros::{function, service};
use futures::Future;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest, SessionHandler},
    function::FunctionResult,
    protocol::{ErrorBody, StreamChannelRef, WorkerMetrics},
    trigger::{Trigger, TriggerRegistrator, TriggerType, known_trigger_type_provider},
    worker_connections::{RuntimeWorkerInfo, WorkerConnection, WorkerConnectionTelemetryMeta},
    workers::traits::Worker,
    workers::worker::rbac_session::Session,
};

pub const TRIGGER_FUNCTIONS_AVAILABLE: &str = "engine::functions-available";
pub const TRIGGER_WORKERS_AVAILABLE: &str = "engine::workers-available";

/// Maximum length of `config_summary` strings produced by
/// `engine::registered-triggers::list`.
const CONFIG_SUMMARY_MAX_LEN: usize = 80;

/// Self-reported worker descriptions are untrusted free text rendered by
/// consoles, CLIs, and LLM agents — cap the length and strip control
/// characters at the ingest boundary.
const WORKER_DESCRIPTION_MAX_LEN: usize = 280;

/// True for characters untrusted worker text must never carry onto a
/// console/terminal/LLM display surface: C0/C1/DEL control chars, plus the
/// Unicode bidirectional override/isolate range (U+202A–202E, U+2066–2069 —
/// the Trojan-Source / CVE-2021-42574 class) and the line/paragraph
/// separators U+2028/U+2029.
fn is_unsafe_display_char(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{2028}' | '\u{2029}')
}

fn sanitize_worker_text(raw: String) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !is_unsafe_display_char(*c))
        .take(WORKER_DESCRIPTION_MAX_LEN)
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmptyInput {}

// ── Inputs ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
pub struct CreateChannelInput {
    #[serde(default)]
    pub buffer_size: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
pub struct CreateChannelOutput {
    pub writer: StreamChannelRef,
    pub reader: StreamChannelRef,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
pub struct FunctionsListInput {
    /// Case-insensitive substring matched against `function_id` and
    /// `description`.
    #[serde(default)]
    pub search: Option<String>,
    /// Exact prefix match against `function_id`.
    #[serde(default)]
    pub prefix: Option<String>,
    /// Exact worker-name match (resolved via the engine's worker registry).
    #[serde(default)]
    pub worker: Option<String>,
    /// Exact worker-name match against any of several workers (OR-combined
    /// with `worker` when both are set). Lets a caller list functions owned
    /// by several workers in one call instead of one `worker` filter per
    /// call.
    #[serde(default)]
    pub workers: Option<Vec<String>>,
    /// Include internal engine functions (`engine::*` prefix). Defaults to
    /// false.
    #[serde(default)]
    pub include_internal: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
pub struct FunctionInfoInput {
    /// One function id — returns that function's flat `FunctionDetail`
    /// (the historical single-id shape). Exactly one of `function_id` /
    /// `function_ids` must be set.
    #[serde(default)]
    pub function_id: Option<String>,
    /// Several function ids at once (max 32, deduplicated in request order) —
    /// returns `{ "functions": [...] }` with one entry per id: the
    /// `FunctionDetail`, or `{ "function_id", "error": "forbidden" |
    /// "not_found" }` for an id this caller cannot see. One call replaces N
    /// single-id round-trips.
    #[serde(default)]
    pub function_ids: Option<Vec<String>>,
    /// Optional target namespace, applied to every requested id. When set, each
    /// id resolves *strictly* in this namespace — the same semantics as
    /// `Engine::resolve_function`'s explicit-namespace path — so an id
    /// duplicated across several non-default namespaces becomes addressable. A
    /// miss in this namespace is NOT_FOUND naming where the id does exist, never
    /// a resolution elsewhere. When absent, resolution mirrors invoke routing's
    /// bare-id behavior (`default` wins; a unique non-default id resolves; an id
    /// in several non-default namespaces is ambiguous).
    #[serde(default)]
    pub namespace: Option<String>,
}

/// Hard cap on `function_ids` per call — a batch exists to save round-trips,
/// not to dump the catalog (that is `engine::functions::list`).
const FUNCTION_INFO_BATCH_MAX: usize = 32;

#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
pub struct TriggersListInput {
    /// Case-insensitive substring matched against the trigger-type `id`
    /// and `description`.
    #[serde(default)]
    pub search: Option<String>,
    /// Exact prefix match on the trigger-type `id`.
    #[serde(default)]
    pub prefix: Option<String>,
    /// Worker-name match (first `::` segment of the trigger-type `id`).
    #[serde(default)]
    pub worker: Option<String>,
    /// Include internal engine trigger types (`engine::*` prefix).
    /// Defaults to false.
    #[serde(default)]
    pub include_internal: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TriggerInfoInput {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
pub struct RegisteredTriggersListInput {
    /// Case-insensitive substring matched against `id`, `trigger_type`,
    /// and `function_id`.
    #[serde(default)]
    pub search: Option<String>,
    /// Exact trigger-type id match.
    #[serde(default)]
    pub trigger_type: Option<String>,
    /// Exact function id match.
    #[serde(default)]
    pub function_id: Option<String>,
    /// Worker-name match against the worker owning the target function.
    #[serde(default)]
    pub worker: Option<String>,
    /// Include rows whose target function is internal (`engine::*`).
    /// Defaults to false.
    #[serde(default)]
    pub include_internal: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RegisteredTriggerInfoInput {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
pub struct WorkersListInput {
    /// Case-insensitive substring matched against the worker `name`.
    #[serde(default)]
    pub search: Option<String>,
    /// Exact runtime match (e.g. `node`, `python`, `rust`).
    #[serde(default)]
    pub runtime: Option<String>,
    /// Exact status match (e.g. `connected`, `disconnected`).
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct WorkerInfoInput {
    pub name: String,
    /// Optional target namespace. A worker name is unique only *within* a
    /// namespace, so the same name can identify two different workers once they
    /// register into different namespaces. When set, the lookup resolves
    /// strictly to the worker of that name in this namespace. When absent,
    /// resolution mirrors `engine::functions::info`'s bare-id behavior: a
    /// `default` worker wins, a name unique to one non-default namespace
    /// resolves, and a name that lives in several non-default namespaces at once
    /// is ambiguous — reported as an error naming the candidate namespaces
    /// rather than silently picking one.
    #[serde(default)]
    pub namespace: Option<String>,
}

// ── Outputs ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FunctionSummary {
    pub function_id: String,
    /// Namespace the function is registered in — the `(namespace, function_id)`
    /// key it lives under in the registry. The same `function_id` may appear
    /// once per namespace, so this is what tells two same-named rows apart.
    /// [`crate::protocol::DEFAULT_NAMESPACE`] for workers that declared none.
    pub namespace: String,
    pub worker_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Registration metadata. `metadata.internal == true` marks an
    /// engine-internal handler that the default `engine::functions::list`
    /// hides (pass `include_internal: true` to see it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RegisteredTriggerRef {
    pub id: String,
    pub trigger_type: String,
    pub config: Value,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FunctionDetail {
    pub function_id: String,
    /// Namespace of the registry entry this detail describes. A bare
    /// `function_id` can name a function in more than one namespace, so the
    /// answer states which one it resolved to rather than leaving the caller
    /// to assume.
    pub namespace: String,
    pub worker_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    pub registered_triggers: Vec<RegisteredTriggerRef>,
}

/// One `function_ids` batch entry: the detail, or a per-id marker mirroring
/// the single-id error codes (`forbidden` / `not_found`) — a bad id never
/// fails the whole batch.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum FunctionInfoEntry {
    Detail(Box<FunctionDetail>),
    Unavailable {
        function_id: String,
        /// `"forbidden"` (RBAC-hidden from this session) or `"not_found"`.
        error: String,
    },
}

/// Batch envelope for `engine::functions::info { function_ids: [...] }`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FunctionInfoBatch {
    /// One entry per requested id, in request order.
    pub functions: Vec<FunctionInfoEntry>,
}

/// `engine::functions::info` output: the flat single-id `FunctionDetail`
/// (byte-identical wire shape for existing callers) or the batch envelope.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum FunctionInfoOutput {
    Single(Box<FunctionDetail>),
    Batch(FunctionInfoBatch),
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TriggerTypeSummary {
    pub id: String,
    pub worker_name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TriggerTypeDetail {
    pub id: String,
    pub worker_name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_schema: Option<Value>,
    /// Schema a bound handler must RETURN when the trigger fires (e.g. the HTTP
    /// response envelope). Present only for trigger types with a fixed return
    /// contract; absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<Value>,
    pub instance_count: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RegisteredTriggerSummary {
    pub id: String,
    pub trigger_type: String,
    pub function_id: String,
    pub worker_name: String,
    /// Full trigger config (e.g. `{ "api_path": "...", "http_method": "GET" }`
    /// for HTTP triggers, `{ "topic": "..." }` for events, etc.). Console
    /// list views need this structured payload to render method/path/topic.
    /// `config_summary` is a truncated display string and is kept for
    /// backward compatibility — do not parse it.
    pub config: Value,
    pub config_summary: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RegisteredTriggerDetail {
    pub id: String,
    pub trigger_type: String,
    pub function_id: String,
    pub worker_name: String,
    pub config: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    /// Full trigger-type detail; `None` if the type was unregistered after
    /// the instance was created.
    pub trigger: Option<TriggerTypeDetail>,
    /// Full function detail; `None` if the target function was
    /// unregistered after the instance was created.
    pub function: Option<FunctionDetail>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WorkerSummary {
    pub name: Option<String>,
    /// One-line summary self-reported by the worker via
    /// `engine::workers::register`. `null` when the worker did not provide
    /// one; carried as a shared core field so a parser can walk both the
    /// engine and registry surfaces.
    pub description: Option<String>,
    pub version: Option<String>,
    pub id: String,
    /// Routing namespace the worker declared on `engine::workers::register`,
    /// or [`crate::protocol::DEFAULT_NAMESPACE`] when it declared none. Two
    /// workers may expose the same function ids in different namespaces; this
    /// is what distinguishes them.
    pub namespace: String,
    pub runtime: Option<String>,
    pub os: Option<String>,
    pub status: String,
    pub function_count: usize,
    pub connected_at_ms: u64,
    pub active_invocations: usize,
    pub isolation: Option<String>,
    pub ip_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WorkerDetailEnvelope {
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    pub id: String,
    pub runtime: Option<String>,
    pub os: Option<String>,
    pub status: String,
    pub function_count: usize,
    pub connected_at_ms: u64,
    pub active_invocations: usize,
    pub isolation: Option<String>,
    pub ip_address: Option<String>,
    /// Process id, when running as a managed process. Engine-local extra
    /// (not present on the registry surface).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Whether this worker is an in-process engine module. Engine-local
    /// extra.
    pub internal: bool,
    /// Latest worker metrics snapshot. Engine-local extra.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_metrics: Option<WorkerMetrics>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WorkerInfoOutput {
    pub worker: WorkerDetailEnvelope,
    pub functions: Vec<FunctionSummary>,
    pub trigger_types: Vec<TriggerTypeSummary>,
    pub registered_triggers: Vec<RegisteredTriggerSummary>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FunctionsListResult {
    pub functions: Vec<FunctionSummary>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TriggersListResult {
    pub triggers: Vec<TriggerTypeSummary>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RegisteredTriggersListResult {
    pub registered_triggers: Vec<RegisteredTriggerSummary>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WorkersListResult {
    pub workers: Vec<WorkerSummary>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RegisterWorkerResult {
    pub success: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RegisterWorkerInput {
    /// Caller-supplied identifier of the connecting worker.
    #[serde(rename = "_caller_worker_id")]
    pub worker_id: String,
    /// Worker runtime (e.g. `node`, `python`, `rust`).
    pub runtime: Option<String>,
    /// Worker SDK version reported during the handshake.
    pub version: Option<String>,
    /// Friendly worker name used in dashboards and logs.
    pub name: Option<String>,
    /// One-line summary of what the worker does. Surfaces in
    /// `engine::workers::list` / `engine::workers::info`.
    #[serde(default)]
    pub description: Option<String>,
    /// Worker host operating system.
    pub os: Option<String>,
    /// Telemetry metadata reported by the worker (anonymous device id,
    /// install kind, etc.).
    pub telemetry: Option<WorkerConnectionTelemetryMeta>,
    /// Process id of the worker, when running as a managed process.
    #[serde(default)]
    pub pid: Option<u32>,
    /// Isolation backend used to run the worker (e.g. `vm`, `oci`).
    #[serde(default)]
    pub isolation: Option<String>,
    /// Routing namespace this worker registers its functions into. Absent means
    /// [`crate::protocol::DEFAULT_NAMESPACE`], so workers built against older
    /// SDKs keep registering unchanged.
    #[serde(default)]
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RegisterTriggerInput {
    /// Trigger type to bind (e.g. `cron`, `state`, `stream`, or a custom worker
    /// trigger type).
    pub trigger_type: String,
    /// The function the fire is delivered to. The trigger binds directly to it;
    /// `metadata` is delivered alongside the payload as a distinct argument.
    pub function_id: String,
    /// Trigger-type-specific configuration, passed through verbatim.
    #[serde(default)]
    pub config: Value,
    /// Arbitrary metadata delivered to the target handler as a distinct argument
    /// (not folded into the payload) so a (possibly shared) target function can
    /// recover per-trigger context the engine routing otherwise drops.
    #[serde(default)]
    pub metadata: Option<Value>,
    /// Injected by the engine from the calling worker; scopes the trigger so it
    /// is GC'd when that worker disconnects. Absent for in-process callers.
    #[serde(rename = "_caller_worker_id", default)]
    pub caller_worker_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RegisterTriggerResult {
    /// The id of the registered trigger; pass to `engine::unregister_trigger`.
    pub id: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct UnregisterTriggerInput {
    /// The trigger id returned by `engine::register_trigger`.
    pub id: String,
    /// Optional trigger-type hint; accepted for symmetry with the protocol
    /// message but not required for the registry lookup.
    #[serde(default)]
    pub trigger_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct UnregisterTriggerResult {
    /// Whether a trigger with this id existed and was removed.
    pub removed: bool,
}

// ── Worker ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct EngineFunctionsWorker {
    engine: Arc<Engine>,
    triggers: Arc<DashMap<String, Trigger>>,
}

impl EngineFunctionsWorker {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            triggers: Arc::new(DashMap::new()),
        }
    }

    pub async fn fire_triggers(&self, trigger_type: &str, data: Value) {
        let triggers_to_fire: Vec<Trigger> = self
            .triggers
            .iter()
            .filter(|entry| entry.value().trigger_type == trigger_type)
            .map(|entry| entry.value().clone())
            .collect();

        for trigger in triggers_to_fire {
            let engine = self.engine.clone();
            let function_id = trigger.function_id.clone();
            let namespace = trigger.namespace.clone();
            let metadata = trigger.metadata.clone();
            let data = data.clone();
            tokio::spawn(async move {
                let _ = engine
                    .call_with_metadata_ns(&namespace, &function_id, data, metadata)
                    .await;
            });
        }
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    /// Returns the first `::` segment of `s`, or `s` itself if there is no
    /// separator.
    fn first_segment(s: &str) -> String {
        s.split("::").next().unwrap_or(s).to_string()
    }

    /// Case-insensitive substring match against any of the given haystacks.
    fn matches_search(needle: &str, haystacks: &[&str]) -> bool {
        let lowered = needle.to_lowercase();
        haystacks
            .iter()
            .any(|h| h.to_lowercase().contains(&lowered))
    }

    /// Rank a search hit by match quality: 0 = the needle anchors the id (a
    /// `::` segment equals it, or the id starts with it), 1 = the needle is a
    /// substring of the id, 2 = it only matched the description. Sorting by
    /// this keeps `state::*` above functions that merely mention "state" in
    /// their description — models read search results top-down, and pure
    /// alphabetical order buried the relevant hits under incidental ones.
    fn search_rank(function_id: &str, needle_lowered: &str) -> u8 {
        let id = function_id.to_lowercase();
        if id.starts_with(needle_lowered) || id.split("::").any(|seg| seg == needle_lowered) {
            0
        } else if id.contains(needle_lowered) {
            1
        } else {
            2
        }
    }

    fn config_summary(config: &Value) -> String {
        let raw = serde_json::to_string(config).unwrap_or_else(|_| "{}".to_string());
        if raw.chars().count() <= CONFIG_SUMMARY_MAX_LEN {
            raw
        } else {
            let mut out: String = raw.chars().take(CONFIG_SUMMARY_MAX_LEN).collect();
            out.push('…');
            out
        }
    }

    /// Builds a `(namespace, function_id) -> worker_name` map by scanning both
    /// the WS worker registry and in-process runtime workers. The first entry
    /// wins on collisions so that runtime workers (more stable identifiers)
    /// take precedence.
    ///
    /// Keyed by `(namespace, function_id)` to match `FunctionsRegistry`. A bare
    /// id is not a key: once the same id can be registered once per namespace,
    /// a bare-id map collapses both owners into whichever entry landed first —
    /// and DashMap iteration order makes that nondeterministic, so a listing
    /// could attribute `orders`'s `state::get` to `billing-worker`. That is a
    /// wrong answer, not a missing one.
    async fn function_owner_index(&self) -> HashMap<(String, String), String> {
        let mut map: HashMap<(String, String), String> = HashMap::new();

        for runtime in self.engine.list_runtime_workers() {
            let name = runtime.name.clone();
            for fn_id in runtime.function_ids {
                // In-process workers always register into the default namespace.
                map.entry((crate::protocol::DEFAULT_NAMESPACE.to_string(), fn_id))
                    .or_insert_with(|| name.clone());
            }
        }

        for worker in self.engine.worker_registry.list_workers() {
            let Some(name) = worker.name.clone() else {
                continue;
            };
            // Registry-sourced: this is the copy `engine::workers::register`
            // writes the namespace into, so it is the connection's real one.
            let namespace = self.engine.worker_registry.get_namespace(&worker.id);
            for fn_id in worker.get_function_ids().await {
                map.entry((namespace.clone(), fn_id))
                    .or_insert_with(|| name.clone());
            }
        }

        map
    }

    fn worker_name_for_function_id(
        index: &HashMap<(String, String), String>,
        namespace: &str,
        function_id: &str,
    ) -> String {
        if let Some(name) = index.get(&(namespace.to_string(), function_id.to_string())) {
            return name.clone();
        }
        Self::first_segment(function_id)
    }

    /// The namespace an introspection lookup of a bare `function_id` resolves
    /// to, or `None` when the id names no function this engine can identify.
    ///
    /// Mirrors invoke routing first: a bare id means [`DEFAULT_NAMESPACE`]
    /// (`Engine::resolve_function`), so a `default` entry always wins and
    /// single-namespace engines behave exactly as before. Only when the id is
    /// absent from `default` does this look wider — and then it resolves solely
    /// when the id is unambiguous. An engine whose every worker is namespaced
    /// would otherwise be blind to all of its own contracts.
    ///
    /// A bare id registered in several non-default namespaces is a genuine
    /// ambiguity: this returns `None` rather than guess an owner. Callers
    /// surface that as a not-found naming the candidates, the same way
    /// `resolve_function` does.
    /// The `NOT_FOUND` body for an id [`Self::introspection_namespace`] would
    /// not resolve. When the id *is* registered — just in several non-default
    /// namespaces at once — a bare "not registered" would be a lie, so the
    /// candidates are named, mirroring `Engine::resolve_function`'s hint.
    fn function_not_found(&self, function_id: &str) -> ErrorBody {
        let namespaces = self.engine.functions.namespaces_for(function_id);
        let message = if namespaces.is_empty() {
            format!("Function '{function_id}' is not registered.")
        } else {
            format!(
                "Function '{function_id}' is ambiguous: it is registered in namespace(s): {}.",
                namespaces.join(", ")
            )
        };
        ErrorBody {
            code: "NOT_FOUND".into(),
            message,
            stacktrace: None,
        }
    }

    fn introspection_namespace(&self, function_id: &str) -> Option<String> {
        let namespaces = self.engine.functions.namespaces_for(function_id);
        if namespaces
            .iter()
            .any(|ns| ns == crate::protocol::DEFAULT_NAMESPACE)
        {
            return Some(crate::protocol::DEFAULT_NAMESPACE.to_string());
        }
        match namespaces.as_slice() {
            [only] => Some(only.clone()),
            _ => None,
        }
    }

    /// The namespace an `engine::functions::info` lookup resolves in, given the
    /// caller's optional explicit `namespace`.
    ///
    /// An explicit namespace resolves *strictly* there (mirroring
    /// `Engine::resolve_function`): the id is looked up in that namespace alone,
    /// and a miss returns `None` so the caller reports NOT_FOUND naming where the
    /// id actually exists — never a resolution in some other namespace. Absent an
    /// explicit namespace, this defers to [`Self::introspection_namespace`].
    fn info_namespace(&self, explicit: Option<&str>, function_id: &str) -> Option<String> {
        match explicit {
            Some(ns) => self
                .engine
                .functions
                .get(ns, function_id)
                .map(|_| ns.to_string()),
            None => self.introspection_namespace(function_id),
        }
    }

    /// The namespaces a worker `name` is currently connected in. In-process
    /// runtime workers always live in [`DEFAULT_NAMESPACE`]; WS workers carry
    /// the namespace they declared on `engine::workers::register`. At most one
    /// worker per namespace can hold a given name (the engine rejects duplicate
    /// names within a namespace), so the result is deduplicated.
    fn worker_namespaces_for_name(&self, name: &str) -> Vec<String> {
        let mut namespaces: Vec<String> = Vec::new();
        if self
            .engine
            .list_runtime_workers()
            .iter()
            .any(|w| w.name == name)
        {
            namespaces.push(crate::protocol::DEFAULT_NAMESPACE.to_string());
        }
        for worker in self.engine.worker_registry.list_workers() {
            if worker.name.as_deref() == Some(name) {
                let ns = self.engine.worker_registry.get_namespace(&worker.id);
                if !namespaces.contains(&ns) {
                    namespaces.push(ns);
                }
            }
        }
        namespaces
    }

    /// Resolves which namespace a `engine::workers::info` lookup addresses,
    /// given the caller's optional explicit `namespace`. Mirrors
    /// [`Self::info_namespace`] for functions: an explicit namespace resolves
    /// strictly there (a miss is NOT_FOUND); a bare name prefers `default`,
    /// then resolves when the name is unique to one namespace, and is otherwise
    /// ambiguous — surfaced as an error naming the candidate namespaces so the
    /// engine never silently picks one same-named worker over another.
    fn resolve_worker_namespace(
        &self,
        name: &str,
        explicit: Option<&str>,
    ) -> Result<String, ErrorBody> {
        let candidates = self.worker_namespaces_for_name(name);
        match explicit {
            Some(ns) => {
                if candidates.iter().any(|c| c == ns) {
                    Ok(ns.to_string())
                } else {
                    Err(self.worker_not_found(name, &candidates))
                }
            }
            None => {
                if candidates
                    .iter()
                    .any(|ns| ns == crate::protocol::DEFAULT_NAMESPACE)
                {
                    return Ok(crate::protocol::DEFAULT_NAMESPACE.to_string());
                }
                match candidates.as_slice() {
                    [] => Err(self.worker_not_found(name, &candidates)),
                    [only] => Ok(only.clone()),
                    _ => Err(ErrorBody {
                        code: "NOT_FOUND".into(),
                        message: format!(
                            "Worker '{name}' is ambiguous: it is connected in namespace(s): {}. \
                             Pass `namespace` to disambiguate.",
                            candidates.join(", ")
                        ),
                        stacktrace: None,
                    }),
                }
            }
        }
    }

    /// NOT_FOUND body for a worker lookup that resolved nothing. When the name
    /// *is* connected — just not in the requested namespace — the candidates are
    /// named so the caller can retry, mirroring [`Self::function_not_found`].
    fn worker_not_found(&self, name: &str, candidates: &[String]) -> ErrorBody {
        let message = if candidates.is_empty() {
            format!("Worker '{name}' is not connected.")
        } else {
            format!(
                "Worker '{name}' is not connected in the requested namespace; \
                 it is connected in namespace(s): {}.",
                candidates.join(", ")
            )
        };
        ErrorBody {
            code: "NOT_FOUND".into(),
            message,
            stacktrace: None,
        }
    }

    /// Resolves the worker name that owns a trigger type from an
    /// already-fetched `TriggerType`. Tries, in order: the `worker_id` Uuid
    /// (populated only by WebSocket-connected workers), the known provider
    /// map (used for install guidance and in-process workers), and finally the
    /// first `::` segment of the id.
    ///
    /// Takes the borrowed `TriggerType` instead of an id on purpose: callers
    /// usually sit inside `trigger_types` iteration or a `.get()` guard, and
    /// a by-id variant would re-enter the same DashMap — a re-entrant
    /// same-shard read can deadlock against a queued writer (parking_lot
    /// RwLocks are writer-fair). Only `worker_registry` (a different map) is
    /// touched here.
    fn owner_name_for_trigger_type(&self, tt: &crate::trigger::TriggerType) -> String {
        if let Some(worker_id) = tt.worker_id
            && let Some(name) = self.engine.worker_registry.get_worker_name(&worker_id)
        {
            return name;
        }
        if let Some(name) = known_trigger_type_provider(&tt.id) {
            return name.to_string();
        }
        Self::first_segment(&tt.id)
    }

    fn instance_count_for_type(&self, tt_id: &str) -> usize {
        self.engine
            .trigger_registry
            .triggers
            .iter()
            .filter(|entry| entry.value().trigger_type == tt_id)
            .count()
    }

    /// Triggers bound to `(namespace, function_id)`. The namespace is part of
    /// the key on purpose: the same `function_id` can be registered once per
    /// namespace, so filtering by bare id would splice `analytics`'s triggers
    /// onto the `orders` function detail.
    fn registered_trigger_refs_for_function(
        &self,
        namespace: &str,
        function_id: &str,
    ) -> Vec<RegisteredTriggerRef> {
        self.engine
            .trigger_registry
            .triggers
            .iter()
            .filter(|entry| {
                let t = entry.value();
                t.namespace == namespace && t.function_id == function_id
            })
            .map(|entry| {
                let t = entry.value();
                RegisteredTriggerRef {
                    id: t.id.clone(),
                    trigger_type: t.trigger_type.clone(),
                    config: t.config.clone(),
                }
            })
            .collect()
    }

    // ── Listing primitives ──────────────────────────────────────────────

    async fn list_function_summaries(&self) -> Vec<FunctionSummary> {
        let index = self.function_owner_index().await;
        self.engine
            .functions
            .iter()
            .map(|entry| {
                // The registry key IS the namespace: `functions` is keyed by
                // `(namespace, function_id)`. No worker field can stand in for
                // it — a function is in the namespace it was registered under.
                let namespace = entry.key().0.clone();
                let f = entry.value();
                let worker_name =
                    Self::worker_name_for_function_id(&index, &namespace, &f._function_id);
                FunctionSummary {
                    function_id: f._function_id.clone(),
                    namespace,
                    worker_name,
                    description: f._description.clone(),
                    metadata: f.metadata.clone(),
                }
            })
            .collect()
    }

    fn list_trigger_type_summaries(&self) -> Vec<TriggerTypeSummary> {
        self.engine
            .trigger_registry
            .trigger_types
            .iter()
            .map(|entry| {
                let tt = entry.value();
                TriggerTypeSummary {
                    id: tt.id.clone(),
                    // Guard-safe: we're inside `trigger_types.iter()`.
                    worker_name: self.owner_name_for_trigger_type(tt),
                    description: tt._description.clone(),
                }
            })
            .collect()
    }

    async fn list_registered_trigger_summaries(&self) -> Vec<RegisteredTriggerSummary> {
        let index = self.function_owner_index().await;
        self.engine
            .trigger_registry
            .triggers
            .iter()
            .map(|entry| {
                let t = entry.value();
                RegisteredTriggerSummary {
                    id: t.id.clone(),
                    trigger_type: t.trigger_type.clone(),
                    function_id: t.function_id.clone(),
                    worker_name: Self::worker_name_for_function_id(
                        &index,
                        &t.namespace,
                        &t.function_id,
                    ),
                    config: t.config.clone(),
                    config_summary: Self::config_summary(&t.config),
                }
            })
            .collect()
    }

    async fn list_worker_summaries(&self, filter_worker_id: Option<&str>) -> Vec<WorkerSummary> {
        let mut worker_summaries: Vec<WorkerSummary> = Vec::new();

        for w in self.engine.worker_registry.list_workers() {
            let worker_id = w.id.to_string();

            if let Some(filter_id) = filter_worker_id
                && worker_id != filter_id
            {
                continue;
            }

            if filter_worker_id.is_none() && w.pid.is_none() {
                continue;
            }

            let function_count = w.get_function_ids().await.len();
            let active_invocations = w.invocation_count().await;
            let ip_address = w.session.as_ref().map(|session| session.ip_address.clone());

            worker_summaries.push(WorkerSummary {
                name: w.name.clone(),
                description: w.description.clone(),
                version: w.version.clone(),
                id: worker_id,
                // Read back through the registry rather than off `w`: only the
                // registry's entry is the one `engine::workers::register`
                // writes the namespace into.
                namespace: self.engine.worker_registry.get_namespace(&w.id),
                runtime: w.runtime.clone(),
                os: w.os.clone(),
                status: w.status.as_str().to_string(),
                function_count,
                connected_at_ms: w.connected_at.timestamp_millis() as u64,
                active_invocations,
                isolation: w.isolation.clone(),
                ip_address,
            });
        }

        for runtime_worker in self.engine.list_runtime_workers() {
            if let Some(filter_id) = filter_worker_id
                && runtime_worker.id != filter_id
            {
                continue;
            }

            let functions = runtime_worker.function_ids.clone();
            worker_summaries.push(WorkerSummary {
                name: Some(runtime_worker.name.clone()),
                description: runtime_worker.description.clone(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
                id: runtime_worker.id.clone(),
                // In-process workers never declare a namespace; they always
                // register into the default one.
                namespace: crate::protocol::DEFAULT_NAMESPACE.to_string(),
                runtime: Some("engine".to_string()),
                os: None,
                status: "available".to_string(),
                function_count: functions.len(),
                connected_at_ms: runtime_worker.connected_at.timestamp_millis() as u64,
                active_invocations: 0,
                isolation: Some("in-process".to_string()),
                ip_address: None,
            });
        }

        worker_summaries.sort_by(|a, b| {
            let a_display_name = a.name.as_deref().unwrap_or(a.id.as_str());
            let b_display_name = b.name.as_deref().unwrap_or(b.id.as_str());

            a_display_name
                .cmp(b_display_name)
                .then_with(|| a.id.cmp(&b.id))
        });

        worker_summaries
    }

    // ── Detail builders ─────────────────────────────────────────────────

    /// The per-id RBAC visibility check `functions_info` applies for
    /// session-scoped callers (single and batch paths share it).
    ///
    /// `namespace` must be the one the id actually resolves to. The re-fetch
    /// here is not decorative: `is_function_allowed` reads the function's
    /// `metadata`, and `expose_functions` is an allow-list — a `Metadata`
    /// filter evaluated against `None` matches nothing, so re-fetching from the
    /// wrong namespace silently DENIES a function the session is entitled to.
    fn session_allows_function(
        &self,
        session: &Arc<Session>,
        namespace: &str,
        function_id: &str,
    ) -> bool {
        let function = self.engine.functions.get(namespace, function_id);
        crate::workers::worker::rbac_config::is_function_allowed(
            function_id,
            session.config.rbac.clone(),
            &session.allowed_functions,
            &session.forbidden_functions,
            function.as_ref(),
        )
    }

    async fn build_function_detail(
        &self,
        namespace: &str,
        function_id: &str,
    ) -> Option<FunctionDetail> {
        let index = self.function_owner_index().await;
        self.build_function_detail_indexed(&index, namespace, function_id)
    }

    /// Like [`Self::build_function_detail`] but with the owner index built
    /// once by the caller — a `function_ids` batch would otherwise rebuild
    /// the O(workers × functions) index per id.
    fn build_function_detail_indexed(
        &self,
        index: &HashMap<(String, String), String>,
        namespace: &str,
        function_id: &str,
    ) -> Option<FunctionDetail> {
        let function = self.engine.functions.get(namespace, function_id)?;
        let worker_name = Self::worker_name_for_function_id(index, namespace, function_id);

        Some(FunctionDetail {
            function_id: function_id.to_string(),
            namespace: namespace.to_string(),
            worker_name,
            description: function._description.clone(),
            request_schema: function.request_format.clone(),
            response_schema: function.response_format.clone(),
            metadata: function.metadata.clone(),
            registered_triggers: self.registered_trigger_refs_for_function(namespace, function_id),
        })
    }

    fn build_trigger_type_detail(&self, tt_id: &str) -> Option<TriggerTypeDetail> {
        let tt = self.engine.trigger_registry.trigger_types.get(tt_id)?;
        Some(TriggerTypeDetail {
            id: tt.id.clone(),
            // Guard-safe: we hold the `.get()` guard on this entry.
            worker_name: self.owner_name_for_trigger_type(tt.value()),
            description: tt._description.clone(),
            configuration_schema: tt.trigger_request_format.clone(),
            request_schema: tt.call_request_format.clone(),
            response_schema: tt.call_response_format.clone(),
            instance_count: self.instance_count_for_type(&tt.id),
        })
    }

    async fn build_registered_trigger_detail(&self, id: &str) -> Option<RegisteredTriggerDetail> {
        let trigger = self
            .engine
            .trigger_registry
            .triggers
            .get(id)
            .map(|entry| entry.value().clone())?;

        let index = self.function_owner_index().await;
        let worker_name =
            Self::worker_name_for_function_id(&index, &trigger.namespace, &trigger.function_id);

        let trigger_detail = self.build_trigger_type_detail(&trigger.trigger_type);
        let function_detail = self
            .build_function_detail(&trigger.namespace, &trigger.function_id)
            .await;

        Some(RegisteredTriggerDetail {
            id: trigger.id.clone(),
            trigger_type: trigger.trigger_type.clone(),
            function_id: trigger.function_id.clone(),
            worker_name,
            config: trigger.config.clone(),
            metadata: trigger.metadata.clone(),
            trigger: trigger_detail,
            function: function_detail,
        })
    }

    fn worker_detail_envelope_from_connection(
        &self,
        w: &WorkerConnection,
        function_count: usize,
        active_invocations: usize,
        ip_address: Option<String>,
        latest_metrics: Option<WorkerMetrics>,
    ) -> WorkerDetailEnvelope {
        WorkerDetailEnvelope {
            name: w.name.clone(),
            description: w.description.clone(),
            version: w.version.clone(),
            id: w.id.to_string(),
            runtime: w.runtime.clone(),
            os: w.os.clone(),
            status: w.status.as_str().to_string(),
            function_count,
            connected_at_ms: w.connected_at.timestamp_millis() as u64,
            active_invocations,
            isolation: w.isolation.clone(),
            ip_address,
            pid: w.pid,
            internal: false,
            latest_metrics,
        }
    }

    fn worker_detail_envelope_from_runtime(&self, w: &RuntimeWorkerInfo) -> WorkerDetailEnvelope {
        let functions = w.function_ids.clone();
        WorkerDetailEnvelope {
            name: Some(w.name.clone()),
            description: w.description.clone(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            id: w.id.clone(),
            runtime: Some("engine".to_string()),
            os: None,
            status: "available".to_string(),
            function_count: functions.len(),
            connected_at_ms: w.connected_at.timestamp_millis() as u64,
            active_invocations: 0,
            isolation: Some("in-process".to_string()),
            ip_address: None,
            pid: None,
            internal: w.internal,
            latest_metrics: None,
        }
    }

    /// `worker_namespace` is the already-resolved namespace of the target
    /// worker (see [`Self::resolve_worker_namespace`]). A worker is selected
    /// only if its name AND namespace both match — two workers can share a name
    /// across namespaces, so name alone would pick an arbitrary one.
    async fn build_worker_detail(
        &self,
        name: &str,
        worker_namespace: &str,
    ) -> Option<WorkerInfoOutput> {
        use crate::workers::observability::metrics::get_worker_metrics_from_storage;

        let envelope: WorkerDetailEnvelope;
        let function_ids: Vec<String>;
        // The namespace this worker's functions live in. Its own namespace is
        // the only correct place to look them up: a `default` lookup finds
        // nothing for a namespaced worker and silently reports it as owning no
        // functions at all.
        let worker_namespace = worker_namespace.to_string();

        if let Some(runtime) = self
            .engine
            .list_runtime_workers()
            .into_iter()
            // Runtime workers only ever live in `default`; only match them when
            // that is the namespace being asked for.
            .find(|w| w.name == name && worker_namespace == crate::protocol::DEFAULT_NAMESPACE)
        {
            envelope = self.worker_detail_envelope_from_runtime(&runtime);
            function_ids = runtime.function_ids;
        } else if let Some(worker) =
            self.engine
                .worker_registry
                .list_workers()
                .into_iter()
                .find(|w| {
                    w.name.as_deref() == Some(name)
                        && self.engine.worker_registry.get_namespace(&w.id) == worker_namespace
                })
        {
            let function_count_async = worker.get_function_ids().await;
            let function_count = function_count_async.len();
            let active_invocations = worker.invocation_count().await;
            let ip_address = worker.session.as_ref().map(|s| s.ip_address.clone());
            let latest_metrics = get_worker_metrics_from_storage(&worker.id.to_string());
            envelope = self.worker_detail_envelope_from_connection(
                &worker,
                function_count,
                active_invocations,
                ip_address,
                latest_metrics,
            );
            function_ids = function_count_async;
        } else {
            return None;
        }

        let worker_name = envelope.name.clone();
        let index = self.function_owner_index().await;
        let mut functions: Vec<FunctionSummary> = function_ids
            .into_iter()
            .filter_map(|fn_id| {
                let func = self.engine.functions.get(&worker_namespace, &fn_id)?;
                Some(FunctionSummary {
                    function_id: fn_id.clone(),
                    // Mirrors the lookup directly above: this row *is* the
                    // entry keyed under the worker's own namespace.
                    namespace: worker_namespace.clone(),
                    worker_name: Self::worker_name_for_function_id(
                        &index,
                        &worker_namespace,
                        &fn_id,
                    ),
                    description: func._description.clone(),
                    metadata: func.metadata.clone(),
                })
            })
            .collect();
        functions.sort_by(|a, b| a.function_id.cmp(&b.function_id));

        let mut trigger_types: Vec<TriggerTypeSummary> = self
            .engine
            .trigger_registry
            .trigger_types
            .iter()
            .filter_map(|entry| {
                // Attribute by resolved owner NAME — the same index
                // `engine::triggers::list` uses — so the two surfaces cannot
                // disagree. Id-based attribution diverged whenever several
                // connections shared one worker name (e.g. a stale
                // `iii-worker-ops` daemon reconnecting next to a fresh one):
                // the name resolved to one connection's id while the trigger
                // type stayed pinned to another, and the type silently
                // vanished from `workers::info`.
                //
                // Trust caveat: worker names are self-reported, so a worker
                // that adopts another worker's name will have its trigger
                // types merged into that name's view here — exactly as
                // `triggers::list` already behaves. The engine does not
                // authenticate worker identity today (a connected worker can
                // likewise shadow function ids); fixing that is identity/auth
                // work, not attribution logic.
                let tt = entry.value();
                // Guard-safe resolution: we're inside `trigger_types.iter()`,
                // and the owner is resolved exactly once per entry.
                let owner = self.owner_name_for_trigger_type(tt);
                if worker_name.as_deref() != Some(owner.as_str()) {
                    return None;
                }
                Some(TriggerTypeSummary {
                    id: tt.id.clone(),
                    worker_name: owner,
                    description: tt._description.clone(),
                })
            })
            .collect();
        trigger_types.sort_by(|a, b| a.id.cmp(&b.id));

        let function_id_set: std::collections::HashSet<String> =
            functions.iter().map(|f| f.function_id.clone()).collect();

        let mut registered_triggers: Vec<RegisteredTriggerSummary> = self
            .engine
            .trigger_registry
            .triggers
            .iter()
            // Scope by `(namespace, function_id)`: a trigger bound to the same
            // id in another namespace belongs to a different worker's function,
            // not this one.
            .filter(|entry| {
                let t = entry.value();
                t.namespace == worker_namespace && function_id_set.contains(&t.function_id)
            })
            .map(|entry| {
                let t = entry.value();
                RegisteredTriggerSummary {
                    id: t.id.clone(),
                    trigger_type: t.trigger_type.clone(),
                    function_id: t.function_id.clone(),
                    worker_name: Self::worker_name_for_function_id(
                        &index,
                        &t.namespace,
                        &t.function_id,
                    ),
                    config: t.config.clone(),
                    config_summary: Self::config_summary(&t.config),
                }
            })
            .collect();
        registered_triggers.sort_by(|a, b| a.id.cmp(&b.id));

        Some(WorkerInfoOutput {
            worker: envelope,
            functions,
            trigger_types,
            registered_triggers,
        })
    }

    async fn register_worker_metadata(&self, input: RegisterWorkerInput) {
        let worker_id = match uuid::Uuid::parse_str(&input.worker_id) {
            Ok(id) => id,
            Err(_) => {
                tracing::error!(worker_id = %input.worker_id, "Invalid worker_id format");
                return;
            }
        };

        let runtime = input.runtime.unwrap_or_else(|| "unknown".to_string());

        self.engine.worker_registry.update_worker_metadata(
            &worker_id,
            runtime,
            input.version,
            input.name.and_then(sanitize_worker_text),
            input.description.and_then(sanitize_worker_text),
            input.os,
            input.telemetry,
            input.pid,
            input.isolation,
            input.namespace,
        );
        crate::workers::telemetry::collector::track_worker_registered();
    }
}

impl TriggerRegistrator for EngineFunctionsWorker {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let triggers = self.triggers.clone();
        Box::pin(async move {
            tracing::debug!(
                trigger_id = %trigger.id,
                trigger_type = %trigger.trigger_type,
                function_id = %trigger.function_id,
                "Registering engine trigger"
            );
            triggers.insert(trigger.id.clone(), trigger);
            Ok(())
        })
    }

    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let triggers = self.triggers.clone();
        Box::pin(async move {
            tracing::debug!(trigger_id = %trigger.id, "Unregistering engine trigger");
            triggers.remove(&trigger.id);
            Ok(())
        })
    }
}

#[async_trait::async_trait]
impl Worker for EngineFunctionsWorker {
    fn name(&self) -> &'static str {
        "EngineFunctionsWorker"
    }

    async fn create(
        engine: Arc<Engine>,
        _config: Option<Value>,
    ) -> anyhow::Result<Box<dyn Worker>> {
        Ok(Box::new(EngineFunctionsWorker {
            engine,
            triggers: Arc::new(DashMap::new()),
        }))
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing EngineFunctionsWorker");

        let functions_trigger = TriggerType::new(
            TRIGGER_FUNCTIONS_AVAILABLE,
            "Triggered when functions are registered/unregistered",
            Box::new(self.clone()),
            None,
        );
        let _ = self.engine.register_trigger_type(functions_trigger).await;

        let workers_trigger = TriggerType::new(
            TRIGGER_WORKERS_AVAILABLE,
            "Triggered when workers connect/disconnect",
            Box::new(self.clone()),
            None,
        );
        let _ = self.engine.register_trigger_type(workers_trigger).await;

        Ok(())
    }

    async fn start_background_tasks(
        &self,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
        mut _shutdown_tx: tokio::sync::watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        let engine = self.engine.clone();
        let triggers = self.triggers.clone();
        let worker_module = self.clone();
        let duration_secs = 5u64;

        tokio::spawn(async move {
            let mut current_functions_hash = engine.functions.functions_hash();

            loop {
                tokio::select! {
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(duration_secs)) => {
                        let new_functions_hash = engine.functions.functions_hash();
                        if new_functions_hash != current_functions_hash {
                            tracing::info!("New functions detected, firing functions-available trigger");
                            current_functions_hash = new_functions_hash;

                            let functions = worker_module.list_function_summaries().await;

                            let functions_data = serde_json::json!({
                                "event": "functions_changed",
                                "functions": functions,
                            });

                            let triggers_to_fire: Vec<Trigger> = triggers
                                .iter()
                                .filter(|entry| entry.value().trigger_type == TRIGGER_FUNCTIONS_AVAILABLE)
                                .map(|entry| entry.value().clone())
                                .collect();

                            for trigger in triggers_to_fire {
                                let engine = engine.clone();
                                let function_id = trigger.function_id.clone();
                                let namespace = trigger.namespace.clone();
                                let metadata = trigger.metadata.clone();
                                let data = functions_data.clone();
                                tokio::spawn(async move {
                                    let _ = engine
                                        .call_with_metadata_ns(
                                            &namespace,
                                            &function_id,
                                            data,
                                            metadata,
                                        )
                                        .await;
                                });
                            }
                        }
                    }
                    changed = shutdown_rx.changed() => {
                        if changed.is_err() || *shutdown_rx.borrow() {
                            tracing::info!("EngineFunctionsWorker background tasks shutting down");
                            break;
                        }
                    }
                }
            }
        });

        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

// ── Service block ───────────────────────────────────────────────────────

#[service(name = "engine")]
impl EngineFunctionsWorker {
    #[function(
        id = "engine::channels::create",
        description = "Create a streaming channel pair"
    )]
    pub async fn create_channel(
        &self,
        input: CreateChannelInput,
    ) -> FunctionResult<CreateChannelOutput, ErrorBody> {
        let channel_mgr = self.engine.channel_manager.clone();
        let buffer_size = input.buffer_size.unwrap_or(64).min(1024);
        let (writer_ref, reader_ref) = channel_mgr.create_channel(buffer_size, None);

        FunctionResult::Success(CreateChannelOutput {
            writer: writer_ref,
            reader: reader_ref,
        })
    }

    #[function(
        id = "engine::functions::list",
        description = "List registered functions with their owning worker name."
    )]
    pub async fn functions_list(
        &self,
        input: FunctionsListInput,
        session: Option<Arc<Session>>,
    ) -> FunctionResult<FunctionsListResult, ErrorBody> {
        let mut functions = self.list_function_summaries().await;

        if !input.include_internal.unwrap_or(false) {
            // Hide engine-internal builtins by default, EXCEPT the caller-facing
            // queue ops (`engine::queue::*`: list_topics / topic_stats /
            // dlq_topics / dlq_messages). Those are a public queue/DLQ API a
            // client legitimately needs to discover — keeping them out of the
            // default list left agents unable to find the DLQ-inspection surface.
            // Also hide any handler explicitly tagged `metadata.internal == true`
            // (e.g. `iii-observability::on-config-change`,
            // `iii-http::on-config-change`). These are configuration-trigger
            // fan-out targets, invoked by id — never meant for discovery.
            functions.retain(|f| {
                let is_engine_internal = f.function_id.starts_with("engine::")
                    && !f.function_id.starts_with("engine::queue::");
                let is_tagged_internal = f
                    .metadata
                    .as_ref()
                    .and_then(|m| m.get("internal"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                !is_engine_internal && !is_tagged_internal
            });
        }

        if let Some(prefix) = input.prefix.as_deref() {
            functions.retain(|f| f.function_id.starts_with(prefix));
        }

        if input.worker.is_some() || input.workers.as_ref().is_some_and(|w| !w.is_empty()) {
            functions.retain(|f| {
                input.worker.as_deref() == Some(f.worker_name.as_str())
                    || input
                        .workers
                        .as_ref()
                        .is_some_and(|workers| workers.iter().any(|w| w == &f.worker_name))
            });
        }

        if let Some(search) = input.search.as_deref()
            && !search.is_empty()
        {
            functions.retain(|f| {
                Self::matches_search(
                    search,
                    &[
                        f.function_id.as_str(),
                        f.description.as_deref().unwrap_or(""),
                    ],
                )
            });
        }

        if let Some(session) = &session {
            // `f.namespace` is the row's own registry key, so the re-fetch hits
            // the entry the row was built from and RBAC sees its real metadata.
            functions
                .retain(|f| self.session_allows_function(session, &f.namespace, &f.function_id));
        }

        match input.search.as_deref().filter(|s| !s.is_empty()) {
            // Rank by match quality so id-anchored hits (`state::get`) sort
            // above functions that merely mention the needle in their
            // description; alphabetical within a tier.
            Some(search) => {
                let needle = search.to_lowercase();
                functions.sort_by_cached_key(|f| {
                    (
                        Self::search_rank(&f.function_id, &needle),
                        f.function_id.clone(),
                    )
                });
            }
            None => functions.sort_by(|a, b| a.function_id.cmp(&b.function_id)),
        }

        FunctionResult::Success(FunctionsListResult { functions })
    }

    #[function(
        id = "engine::functions::info",
        description = "Inspect functions: schemas, owner, and registered triggers. Single \
                       `function_id`, or `function_ids: [...]` (max 32) to fetch several \
                       contracts in one call."
    )]
    pub async fn functions_info(
        &self,
        input: FunctionInfoInput,
        session: Option<Arc<Session>>,
    ) -> FunctionResult<FunctionInfoOutput, ErrorBody> {
        let explicit_ns = input.namespace.clone();
        let function_ids = match (input.function_id, input.function_ids) {
            (Some(_), Some(_)) => {
                return FunctionResult::Failure(ErrorBody {
                    code: "INVALID_ARGUMENTS".into(),
                    message: "pass either `function_id` or `function_ids`, not both".into(),
                    stacktrace: None,
                });
            }
            (None, None) => {
                return FunctionResult::Failure(ErrorBody {
                    code: "INVALID_ARGUMENTS".into(),
                    message: "missing field `function_id` (or `function_ids` for a batch)".into(),
                    stacktrace: None,
                });
            }
            // Single id: keep the historical flat shape and error codes.
            (Some(function_id), None) => {
                // Resolve once, then use the same namespace for the RBAC check
                // and the detail — otherwise the two could disagree about which
                // function they are talking about.
                let Some(namespace) = self.info_namespace(explicit_ns.as_deref(), &function_id)
                else {
                    return FunctionResult::Failure(self.function_not_found(&function_id));
                };
                if let Some(session) = &session
                    && !self.session_allows_function(session, &namespace, &function_id)
                {
                    return FunctionResult::Failure(ErrorBody {
                        code: "FORBIDDEN".into(),
                        message: format!(
                            "Function '{function_id}' is not visible to this session."
                        ),
                        stacktrace: None,
                    });
                }
                return match self.build_function_detail(&namespace, &function_id).await {
                    Some(detail) => {
                        FunctionResult::Success(FunctionInfoOutput::Single(Box::new(detail)))
                    }
                    None => FunctionResult::Failure(self.function_not_found(&function_id)),
                };
            }
            (None, Some(function_ids)) => function_ids,
        };

        // Batch: per-id markers mirror the single-id error codes — one bad id
        // never fails the batch.
        let mut ids: Vec<String> = Vec::with_capacity(function_ids.len());
        for id in function_ids {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        if ids.is_empty() {
            return FunctionResult::Failure(ErrorBody {
                code: "INVALID_ARGUMENTS".into(),
                message: "`function_ids` must be a non-empty array of function ids".into(),
                stacktrace: None,
            });
        }
        if ids.len() > FUNCTION_INFO_BATCH_MAX {
            return FunctionResult::Failure(ErrorBody {
                code: "INVALID_ARGUMENTS".into(),
                message: format!("too many function_ids (max {FUNCTION_INFO_BATCH_MAX})"),
                stacktrace: None,
            });
        }

        let index = self.function_owner_index().await;
        let functions = ids
            .into_iter()
            .map(|function_id| {
                // Unresolvable (absent, or ambiguous across several non-default
                // namespaces) is reported per-id, exactly like the single path.
                let Some(namespace) = self.info_namespace(explicit_ns.as_deref(), &function_id)
                else {
                    return FunctionInfoEntry::Unavailable {
                        function_id,
                        error: "not_found".into(),
                    };
                };
                if let Some(session) = &session
                    && !self.session_allows_function(session, &namespace, &function_id)
                {
                    return FunctionInfoEntry::Unavailable {
                        function_id,
                        error: "forbidden".into(),
                    };
                }
                match self.build_function_detail_indexed(&index, &namespace, &function_id) {
                    Some(detail) => FunctionInfoEntry::Detail(Box::new(detail)),
                    None => FunctionInfoEntry::Unavailable {
                        function_id,
                        error: "not_found".into(),
                    },
                }
            })
            .collect();

        FunctionResult::Success(FunctionInfoOutput::Batch(FunctionInfoBatch { functions }))
    }

    #[function(
        id = "engine::triggers::list",
        description = "List trigger types registered with the engine."
    )]
    pub async fn triggers_list(
        &self,
        input: TriggersListInput,
    ) -> FunctionResult<TriggersListResult, ErrorBody> {
        let mut triggers = self.list_trigger_type_summaries();

        if !input.include_internal.unwrap_or(false) {
            triggers.retain(|t| !t.id.starts_with("engine::"));
        }

        if let Some(prefix) = input.prefix.as_deref() {
            triggers.retain(|t| t.id.starts_with(prefix));
        }

        if let Some(worker) = input.worker.as_deref() {
            triggers.retain(|t| t.worker_name == worker);
        }

        if let Some(search) = input.search.as_deref()
            && !search.is_empty()
        {
            triggers
                .retain(|t| Self::matches_search(search, &[t.id.as_str(), t.description.as_str()]));
        }

        triggers.sort_by(|a, b| a.id.cmp(&b.id));

        FunctionResult::Success(TriggersListResult { triggers })
    }

    #[function(
        id = "engine::triggers::info",
        description = "Inspect one trigger type: schemas, owner, and live instance count."
    )]
    pub async fn triggers_info(
        &self,
        input: TriggerInfoInput,
    ) -> FunctionResult<TriggerTypeDetail, ErrorBody> {
        match self.build_trigger_type_detail(&input.id) {
            Some(detail) => FunctionResult::Success(detail),
            None => FunctionResult::Failure(ErrorBody {
                code: "NOT_FOUND".into(),
                message: format!("Trigger type '{}' is not registered.", input.id),
                stacktrace: None,
            }),
        }
    }

    #[function(
        id = "engine::registered-triggers::list",
        description = "List registered trigger instances (subscriber rows)."
    )]
    pub async fn registered_triggers_list(
        &self,
        input: RegisteredTriggersListInput,
    ) -> FunctionResult<RegisteredTriggersListResult, ErrorBody> {
        let mut registered_triggers = self.list_registered_trigger_summaries().await;

        if !input.include_internal.unwrap_or(false) {
            registered_triggers.retain(|t| !t.function_id.starts_with("engine::"));
        }

        if let Some(trigger_type) = input.trigger_type.as_deref() {
            registered_triggers.retain(|t| t.trigger_type == trigger_type);
        }

        if let Some(function_id) = input.function_id.as_deref() {
            registered_triggers.retain(|t| t.function_id == function_id);
        }

        if let Some(worker) = input.worker.as_deref() {
            registered_triggers.retain(|t| t.worker_name == worker);
        }

        if let Some(search) = input.search.as_deref()
            && !search.is_empty()
        {
            registered_triggers.retain(|t| {
                Self::matches_search(
                    search,
                    &[
                        t.id.as_str(),
                        t.trigger_type.as_str(),
                        t.function_id.as_str(),
                    ],
                )
            });
        }

        registered_triggers.sort_by(|a, b| a.id.cmp(&b.id));

        FunctionResult::Success(RegisteredTriggersListResult {
            registered_triggers,
        })
    }

    #[function(
        id = "engine::registered-triggers::info",
        description = "Inspect one registered trigger instance with denormalized trigger and function detail."
    )]
    pub async fn registered_triggers_info(
        &self,
        input: RegisteredTriggerInfoInput,
    ) -> FunctionResult<RegisteredTriggerDetail, ErrorBody> {
        match self.build_registered_trigger_detail(&input.id).await {
            Some(detail) => FunctionResult::Success(detail),
            None => FunctionResult::Failure(ErrorBody {
                code: "NOT_FOUND".into(),
                message: format!("Registered trigger '{}' was not found.", input.id),
                stacktrace: None,
            }),
        }
    }

    #[function(
        id = "engine::workers::list",
        description = "List workers connected to the engine."
    )]
    pub async fn workers_list(
        &self,
        input: WorkersListInput,
    ) -> FunctionResult<WorkersListResult, ErrorBody> {
        let mut workers = self.list_worker_summaries(None).await;

        if let Some(runtime) = input.runtime.as_deref() {
            workers.retain(|w| w.runtime.as_deref() == Some(runtime));
        }

        if let Some(status) = input.status.as_deref() {
            workers.retain(|w| w.status == status);
        }

        if let Some(search) = input.search.as_deref()
            && !search.is_empty()
        {
            workers.retain(|w| Self::matches_search(search, &[w.name.as_deref().unwrap_or("")]));
        }

        FunctionResult::Success(WorkersListResult { workers })
    }

    #[function(
        id = "engine::workers::info",
        description = "Inspect one connected worker's full surface (functions, trigger types, registered triggers)."
    )]
    pub async fn workers_info(
        &self,
        input: WorkerInfoInput,
    ) -> FunctionResult<WorkerInfoOutput, ErrorBody> {
        // Resolve the namespace first: a bare name can identify two different
        // workers once they register into different namespaces, so returning the
        // first match would be arbitrary.
        let namespace = match self.resolve_worker_namespace(&input.name, input.namespace.as_deref())
        {
            Ok(ns) => ns,
            Err(err) => return FunctionResult::Failure(err),
        };
        match self.build_worker_detail(&input.name, &namespace).await {
            Some(detail) => FunctionResult::Success(detail),
            None => FunctionResult::Failure(self.worker_not_found(&input.name, &[])),
        }
    }

    #[function(
        id = "engine::workers::register",
        description = "Register worker metadata"
    )]
    pub async fn register_worker(
        &self,
        input: RegisterWorkerInput,
    ) -> FunctionResult<RegisterWorkerResult, ErrorBody> {
        let worker_id = input.worker_id.clone();
        let namespace = crate::protocol::effective_namespace(&input.namespace).to_string();

        // One live worker name per namespace. Sanitize the name the same way
        // `register_worker_metadata` will store it, so the lease key matches what
        // `cleanup_worker` later releases. A worker that sends no usable name
        // cannot collide, so it skips the claim.
        let worker_name = input.name.clone().and_then(sanitize_worker_text);
        if let Some(worker_name) = &worker_name
            && let Ok(uuid) = uuid::Uuid::parse_str(&worker_id)
            && let Err(conflict) = self.engine.claim_worker_name(&namespace, uuid, worker_name)
        {
            // Fatal, unlike a function-id conflict: reject and close. Return
            // before writing any metadata or draining buffered registrations —
            // the loser leaves no trace.
            self.engine.reject_worker_registration(uuid, conflict).await;
            return FunctionResult::Success(RegisterWorkerResult { success: false });
        }

        self.register_worker_metadata(input).await;

        // This call is the only moment the connection's namespace becomes
        // known. Registrations the worker sent ahead of it are queued on the
        // connection; release them now, into `namespace`.
        if let Ok(uuid) = uuid::Uuid::parse_str(&worker_id)
            && let Some(worker) = self.engine.worker_registry.get_worker(&uuid)
        {
            self.engine
                .resolve_connection_namespace(&worker, &namespace)
                .await;
        }

        let data = serde_json::json!({
            "event": "worker_metadata_updated",
            "worker_id": worker_id,
        });
        self.engine
            .fire_triggers(TRIGGER_WORKERS_AVAILABLE, data)
            .await;

        FunctionResult::Success(RegisterWorkerResult { success: true })
    }

    #[function(
        id = "engine::register_trigger",
        description = "Register a trigger that fires `function_id` directly, with `metadata` delivered to the handler as a distinct argument (not folded into the payload). Returns the trigger id (pass it to engine::unregister_trigger)."
    )]
    pub async fn register_trigger_fn(
        &self,
        input: RegisterTriggerInput,
        session: Option<Arc<Session>>,
    ) -> FunctionResult<RegisterTriggerResult, ErrorBody> {
        // RBAC parity with the `Message::RegisterTrigger` path: honor the
        // session's allowed_trigger_types when a session is present. (The
        // on_trigger_registration hook is Message-path only and intentionally
        // not replicated here — session-less local deployments bypass RBAC.)
        if let Some(session) = &session
            && let Some(allowed) = &session.allowed_trigger_types
            && !allowed.iter().any(|t| t == &input.trigger_type)
        {
            return FunctionResult::Failure(ErrorBody {
                code: "FORBIDDEN".into(),
                message: format!(
                    "Trigger type '{}' is not allowed for this session.",
                    input.trigger_type
                ),
                stacktrace: None,
            });
        }

        let id = uuid::Uuid::new_v4().to_string();

        // Prefix parity with the `Message::RegisterTrigger` path: a prefixed
        // session's functions are stored under `prefix::id`, so the trigger
        // must bind to the prefixed target or firing would miss the real
        // function.
        let mut function_id = input.function_id;
        if let Some(prefix) = session
            .as_ref()
            .and_then(|s| s.function_registration_prefix.as_ref())
        {
            function_id = format!("{prefix}::{function_id}");
        }

        // Bind the trigger straight to the target function. `metadata` rides on
        // the `Trigger` and is delivered to the handler as a distinct argument
        // at fire time (see the trigger-fire paths) — no proxy function, so
        // nothing to leak or GC, and the payload is left untouched.
        //
        // Ownership: deliberately NOT stamped with the caller's worker id.
        // Function-path registrations are durable engine-side state ("when X
        // happens, do Y"), removed only by explicit `engine::unregister_trigger`
        // — a CLI's millisecond-lived connection or an agent proxy's restart
        // must not reap them. A worker's own lifecycle bindings go through the
        // `Message::RegisterTrigger` path, which keeps connection ownership and
        // the disconnect GC.
        let trigger = Trigger {
            id: id.clone(),
            trigger_type: input.trigger_type,
            function_id,
            config: input.config,
            worker_id: None,
            metadata: input.metadata,
            // Function-path (durable) registrations are engine orchestration and
            // resolve their target in the default namespace, matching the
            // `fire_triggers` behavior they predate.
            namespace: crate::protocol::default_namespace(),
        };

        if let Err(err) = self.engine.trigger_registry.register_trigger(trigger).await {
            return FunctionResult::Failure(ErrorBody {
                code: "trigger_registration_failed".into(),
                message: err.to_string(),
                stacktrace: None,
            });
        }

        FunctionResult::Success(RegisterTriggerResult { id })
    }

    #[function(
        id = "engine::unregister_trigger",
        description = "Unregister a trigger by id. Idempotent; reports whether it existed."
    )]
    pub async fn unregister_trigger_fn(
        &self,
        input: UnregisterTriggerInput,
    ) -> FunctionResult<UnregisterTriggerResult, ErrorBody> {
        let removed = match self
            .engine
            .trigger_registry
            .unregister_trigger(input.id.clone(), input.trigger_type)
            .await
        {
            Ok(removed) => removed,
            Err(err) => {
                return FunctionResult::Failure(ErrorBody {
                    code: "unregister_failed".into(),
                    message: err.to_string(),
                    stacktrace: None,
                });
            }
        };

        FunctionResult::Success(UnregisterTriggerResult { removed })
    }
}

crate::register_worker!(
    "iii-engine-functions",
    EngineFunctionsWorker,
    description = "Core engine introspection (engine::*) and platform authoring reference.",
    mandatory
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::function::{Function, HandlerFn};
    use crate::workers::observability::metrics::ensure_default_meter;
    use crate::workers::observability::metrics::{
        StoredDataPoint, StoredMetric, StoredMetricType, StoredNumberDataPoint, get_metric_storage,
        init_metric_storage,
    };
    use serde_json;

    fn setup_engine_and_module() -> (Arc<Engine>, EngineFunctionsWorker) {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let module = EngineFunctionsWorker::new(engine.clone());
        (engine, module)
    }

    fn register_simple_function(engine: &Engine, function_id: &str, description: Option<&str>) {
        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: function_id.to_string(),
                description: description.map(|s| s.to_string()),
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(
                |_input: Value| async move { FunctionResult::Success(None) },
            ),
        );
    }

    fn insert_trigger_instance(
        engine: &Engine,
        id: &str,
        trigger_type: &str,
        function_id: &str,
        config: Value,
    ) {
        engine.trigger_registry.triggers.insert(
            id.to_string(),
            crate::trigger::Trigger {
                id: id.to_string(),
                trigger_type: trigger_type.to_string(),
                function_id: function_id.to_string(),
                config,
                worker_id: None,
                metadata: None,
                namespace: "default".to_string(),
            },
        );
    }

    // ── Input deserialization tests ─────────────────────────────────────

    #[test]
    fn register_worker_input_deserializes_telemetry() {
        let json = serde_json::json!({
            "_caller_worker_id": "550e8400-e29b-41d4-a716-446655440000",
            "runtime": "node",
            "version": "1.0.0",
            "name": "host:123",
            "os": "darwin 25.0",
            "telemetry": {
                "language": "en-US",
                "project_name": "my-project",
                "framework": "express"
            }
        });
        let input: RegisterWorkerInput = serde_json::from_value(json).expect("deserialize");
        assert_eq!(input.worker_id, "550e8400-e29b-41d4-a716-446655440000");
        let telemetry = input.telemetry.expect("telemetry present");
        assert_eq!(telemetry.language.as_deref(), Some("en-US"));
        assert_eq!(telemetry.project_name.as_deref(), Some("my-project"));
        assert_eq!(telemetry.framework.as_deref(), Some("express"));
    }

    #[test]
    fn register_worker_input_accepts_pid() {
        let json = serde_json::json!({
            "_caller_worker_id": "550e8400-e29b-41d4-a716-446655440000",
            "runtime": "node",
            "pid": 9876
        });
        let input: RegisterWorkerInput = serde_json::from_value(json).expect("deserialize");
        assert_eq!(input.pid, Some(9876u32));
    }

    #[test]
    fn register_worker_input_pid_defaults_to_none() {
        let json = serde_json::json!({
            "_caller_worker_id": "550e8400-e29b-41d4-a716-446655440000",
            "runtime": "python"
        });
        let input: RegisterWorkerInput = serde_json::from_value(json).expect("deserialize");
        assert!(input.pid.is_none());
    }

    #[test]
    fn register_worker_input_accepts_isolation() {
        let json = serde_json::json!({
            "_caller_worker_id": "550e8400-e29b-41d4-a716-446655440000",
            "runtime": "rust",
            "isolation": "libkrun"
        });
        let input: RegisterWorkerInput = serde_json::from_value(json).expect("deserialize");
        assert_eq!(input.isolation.as_deref(), Some("libkrun"));
    }

    #[test]
    fn register_worker_input_minimal_deserialization() {
        let json = serde_json::json!({
            "_caller_worker_id": "550e8400-e29b-41d4-a716-446655440000"
        });
        let input: RegisterWorkerInput = serde_json::from_value(json).expect("deserialize");
        assert_eq!(input.worker_id, "550e8400-e29b-41d4-a716-446655440000");
        assert!(input.runtime.is_none());
        assert!(input.version.is_none());
        assert!(input.name.is_none());
        assert!(input.os.is_none());
        assert!(input.telemetry.is_none());
    }

    /// Wire compatibility: a payload from an SDK that predates namespaces has
    /// no `namespace` key and must still deserialize.
    #[test]
    fn register_worker_input_without_namespace_deserializes() {
        let json = serde_json::json!({
            "_caller_worker_id": "550e8400-e29b-41d4-a716-446655440000"
        });
        let input: RegisterWorkerInput = serde_json::from_value(json).expect("deserialize");
        assert!(input.namespace.is_none());
    }

    #[test]
    fn register_worker_input_accepts_namespace() {
        let json = serde_json::json!({
            "_caller_worker_id": "550e8400-e29b-41d4-a716-446655440000",
            "namespace": "orders"
        });
        let input: RegisterWorkerInput = serde_json::from_value(json).expect("deserialize");
        assert_eq!(input.namespace.as_deref(), Some("orders"));
    }

    #[test]
    fn empty_input_deserializes() {
        let json = serde_json::json!({});
        let _input: EmptyInput = serde_json::from_value(json).expect("deserialize");
    }

    #[test]
    fn input_defaults_round_trip() {
        let _: FunctionsListInput = serde_json::from_value(serde_json::json!({})).unwrap();
        let _: TriggersListInput = serde_json::from_value(serde_json::json!({})).unwrap();
        let _: RegisteredTriggersListInput = serde_json::from_value(serde_json::json!({})).unwrap();
        let _: WorkersListInput = serde_json::from_value(serde_json::json!({})).unwrap();
        let info: FunctionInfoInput =
            serde_json::from_value(serde_json::json!({"function_id": "test"})).unwrap();
        assert_eq!(info.function_id.as_deref(), Some("test"));
        assert_eq!(info.function_ids, None);
        let batch: FunctionInfoInput =
            serde_json::from_value(serde_json::json!({"function_ids": ["a::b", "c::d"]})).unwrap();
        assert_eq!(batch.function_id, None);
        assert_eq!(
            batch.function_ids,
            Some(vec!["a::b".to_string(), "c::d".to_string()])
        );
    }

    // ── list_function_summaries tests ───────────────────────────────────

    #[tokio::test]
    async fn list_function_summaries_empty_engine() {
        let (_engine, module) = setup_engine_and_module();
        let functions = module.list_function_summaries().await;
        assert!(functions.is_empty());
    }

    #[tokio::test]
    async fn list_function_summaries_returns_registered_functions() {
        let (engine, module) = setup_engine_and_module();
        register_simple_function(&engine, "test::my_func", Some("A test function"));

        let functions = module.list_function_summaries().await;
        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0].function_id, "test::my_func");
        assert_eq!(functions[0].description.as_deref(), Some("A test function"));
        assert_eq!(functions[0].worker_name, "test"); // fallback to first segment
    }

    #[tokio::test]
    async fn list_function_summaries_resolves_worker_name_via_runtime_worker() {
        let (engine, module) = setup_engine_and_module();
        engine.upsert_runtime_worker(crate::worker_connections::RuntimeWorkerInfo {
            id: "iii-state".to_string(),
            name: "iii-state".to_string(),
            description: None,
            worker_type: "iii-state".to_string(),
            connected_at: chrono::Utc::now(),
            function_ids: vec!["state::get".to_string()],
            internal: false,
        });
        register_simple_function(&engine, "state::get", None);

        let functions = module.list_function_summaries().await;
        let entry = functions
            .iter()
            .find(|f| f.function_id == "state::get")
            .unwrap();
        assert_eq!(entry.worker_name, "iii-state");
    }

    // ── list_registered_trigger_summaries tests ─────────────────────────

    #[tokio::test]
    async fn list_registered_trigger_summaries_empty() {
        let (_engine, module) = setup_engine_and_module();
        let triggers = module.list_registered_trigger_summaries().await;
        assert!(triggers.is_empty());
    }

    #[tokio::test]
    async fn list_registered_trigger_summaries_returns_registered_triggers() {
        let (engine, module) = setup_engine_and_module();
        insert_trigger_instance(
            &engine,
            "trig-1",
            "cron",
            "test::handler",
            serde_json::json!({"interval": 5}),
        );

        let summaries = module.list_registered_trigger_summaries().await;
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "trig-1");
        assert_eq!(summaries[0].trigger_type, "cron");
        assert_eq!(summaries[0].function_id, "test::handler");
        assert_eq!(summaries[0].worker_name, "test"); // fallback first segment
        assert!(summaries[0].config_summary.contains("interval"));
    }

    #[tokio::test]
    async fn list_registered_trigger_summaries_truncates_long_config() {
        let (engine, module) = setup_engine_and_module();
        let huge = "x".repeat(200);
        insert_trigger_instance(
            &engine,
            "trig-long",
            "cron",
            "test::handler",
            serde_json::json!({"data": huge}),
        );

        let summaries = module.list_registered_trigger_summaries().await;
        assert_eq!(summaries.len(), 1);
        let summary = &summaries[0].config_summary;
        let char_count = summary.chars().count();
        assert!(
            char_count <= CONFIG_SUMMARY_MAX_LEN + 1,
            "got {} chars",
            char_count
        );
        assert!(summary.ends_with('…'));
    }

    // ── list_worker_summaries tests ─────────────────────────────────────

    #[tokio::test]
    async fn list_worker_summaries_empty() {
        let (_engine, module) = setup_engine_and_module();
        let workers = module.list_worker_summaries(None).await;
        assert!(workers.is_empty());
    }

    #[tokio::test]
    async fn list_worker_summaries_with_filter_no_match() {
        let (_engine, module) = setup_engine_and_module();
        let workers = module
            .list_worker_summaries(Some("nonexistent-worker-id"))
            .await;
        assert!(workers.is_empty());
    }

    #[tokio::test]
    async fn list_worker_summaries_hides_workers_without_pid() {
        let (engine, module) = setup_engine_and_module();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let worker = crate::worker_connections::WorkerConnection::new(tx);
        let worker_id = worker.id.to_string();
        engine.worker_registry.register_worker(worker);

        let all = module.list_worker_summaries(None).await;
        assert!(all.is_empty());

        let direct = module.list_worker_summaries(Some(&worker_id)).await;
        assert_eq!(direct.len(), 1);
    }

    #[tokio::test]
    async fn list_worker_summaries_includes_runtime_worker_snapshots() {
        let (engine, module) = setup_engine_and_module();

        engine.upsert_runtime_worker(crate::worker_connections::RuntimeWorkerInfo {
            id: "iii-state".to_string(),
            name: "iii-state".to_string(),
            description: Some("Distributed key-value state management.".to_string()),
            worker_type: "iii-state".to_string(),
            connected_at: chrono::Utc::now(),
            function_ids: vec!["state::get".to_string(), "state::set".to_string()],
            internal: false,
        });

        let workers = module.list_worker_summaries(None).await;
        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].id, "iii-state");
        assert_eq!(workers[0].name.as_deref(), Some("iii-state"));
        assert_eq!(workers[0].runtime.as_deref(), Some("engine"));
        assert_eq!(workers[0].isolation.as_deref(), Some("in-process"));
        assert_eq!(workers[0].status, "available");
        assert_eq!(workers[0].function_count, 2);
        assert_eq!(
            workers[0].description.as_deref(),
            Some("Distributed key-value state management.")
        );
    }

    #[tokio::test]
    async fn list_worker_summaries_hides_pidless_socket_but_shows_runtime_snapshot() {
        let (engine, module) = setup_engine_and_module();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let socket_worker = crate::worker_connections::WorkerConnection::new(tx);
        engine.worker_registry.register_worker(socket_worker);

        engine.upsert_runtime_worker(crate::worker_connections::RuntimeWorkerInfo {
            id: "iii-stream".to_string(),
            name: "iii-stream".to_string(),
            description: None,
            worker_type: "iii-stream".to_string(),
            connected_at: chrono::Utc::now(),
            function_ids: vec!["stream::list".to_string()],
            internal: false,
        });

        let workers = module.list_worker_summaries(None).await;
        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].id, "iii-stream");
    }

    // ── register_worker_metadata tests ──────────────────────────────────

    #[tokio::test]
    async fn register_worker_metadata_invalid_uuid() {
        let (_engine, module) = setup_engine_and_module();
        let input = RegisterWorkerInput {
            worker_id: "not-a-valid-uuid".to_string(),
            runtime: Some("node".to_string()),
            version: Some("1.0".to_string()),
            name: Some("test-worker".to_string()),
            description: None,
            os: Some("linux".to_string()),
            telemetry: None,
            pid: None,
            isolation: None,
            namespace: None,
        };
        module.register_worker_metadata(input).await;
    }

    #[tokio::test]
    async fn register_worker_metadata_defaults_runtime_to_unknown() {
        let (engine, module) = setup_engine_and_module();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let worker = crate::worker_connections::WorkerConnection::new(tx);
        let worker_id = worker.id.to_string();
        engine.worker_registry.register_worker(worker);

        let input = RegisterWorkerInput {
            worker_id: worker_id.clone(),
            runtime: None,
            version: Some("2.0".to_string()),
            name: Some("my-worker".to_string()),
            description: None,
            os: Some("darwin".to_string()),
            telemetry: None,
            pid: None,
            isolation: Some("libkrun".to_string()),
            namespace: None,
        };

        module.register_worker_metadata(input).await;

        let workers = module.list_worker_summaries(Some(&worker_id)).await;
        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].runtime.as_deref(), Some("unknown"));
        assert_eq!(workers[0].version.as_deref(), Some("2.0"));
        assert_eq!(workers[0].name.as_deref(), Some("my-worker"));
        assert_eq!(workers[0].os.as_deref(), Some("darwin"));
        assert_eq!(workers[0].isolation.as_deref(), Some("libkrun"));
    }

    fn register_worker_input_with_namespace(
        worker_id: &str,
        namespace: Option<String>,
    ) -> RegisterWorkerInput {
        RegisterWorkerInput {
            worker_id: worker_id.to_string(),
            runtime: Some("node".to_string()),
            version: None,
            name: None,
            description: None,
            os: None,
            telemetry: None,
            pid: None,
            isolation: None,
            namespace,
        }
    }

    /// End-to-end wire compatibility: a worker that sends no namespace over
    /// `engine::workers::register` lands in `DEFAULT_NAMESPACE` on the
    /// connection.
    #[tokio::test]
    async fn register_worker_metadata_without_namespace_lands_in_default() {
        let (engine, module) = setup_engine_and_module();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let worker = crate::worker_connections::WorkerConnection::new(tx);
        let worker_uuid = worker.id;
        engine.worker_registry.register_worker(worker);

        module
            .register_worker_metadata(register_worker_input_with_namespace(
                &worker_uuid.to_string(),
                None,
            ))
            .await;

        assert_eq!(
            engine.worker_registry.get_namespace(&worker_uuid),
            crate::protocol::DEFAULT_NAMESPACE
        );
    }

    /// The namespace a worker declares reaches its connection, which is what
    /// every registration and cleanup path resolves against.
    #[tokio::test]
    async fn register_worker_metadata_stores_declared_namespace() {
        let (engine, module) = setup_engine_and_module();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let worker = crate::worker_connections::WorkerConnection::new(tx);
        let worker_uuid = worker.id;
        engine.worker_registry.register_worker(worker);

        module
            .register_worker_metadata(register_worker_input_with_namespace(
                &worker_uuid.to_string(),
                Some("orders".to_string()),
            ))
            .await;

        assert_eq!(engine.worker_registry.get_namespace(&worker_uuid), "orders");
    }

    /// End-to-end: an SDK that flushes `RegisterFunction` before
    /// `engine::workers::register` still lands its functions in the namespace
    /// it declares.
    #[tokio::test]
    async fn register_worker_drains_buffered_registrations_into_its_namespace() {
        let (engine, module) = setup_engine_and_module();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let worker = crate::worker_connections::WorkerConnection::new(tx);
        let worker_uuid = worker.id;
        engine.worker_registry.register_worker(worker.clone());
        engine.begin_namespace_resolution(&worker);

        engine
            .router_msg(
                &worker,
                &crate::protocol::Message::RegisterFunction {
                    id: "orders::create".to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                    invocation: None,
                },
            )
            .await
            .expect("buffer register");
        assert!(engine.functions.get("orders", "orders::create").is_none());

        module
            .register_worker(register_worker_input_with_namespace(
                &worker_uuid.to_string(),
                Some("orders".to_string()),
            ))
            .await;

        assert!(engine.functions.get("orders", "orders::create").is_some());
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "orders::create")
                .is_none()
        );
    }

    /// A worker that sends `engine::workers::register` with no namespace
    /// drains into `DEFAULT_NAMESPACE`.
    #[tokio::test]
    async fn register_worker_without_a_namespace_drains_into_default() {
        let (engine, module) = setup_engine_and_module();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let worker = crate::worker_connections::WorkerConnection::new(tx);
        let worker_uuid = worker.id;
        engine.worker_registry.register_worker(worker.clone());
        engine.begin_namespace_resolution(&worker);

        engine
            .router_msg(
                &worker,
                &crate::protocol::Message::RegisterFunction {
                    id: "legacy::ping".to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                    invocation: None,
                },
            )
            .await
            .expect("buffer register");

        module
            .register_worker(register_worker_input_with_namespace(
                &worker_uuid.to_string(),
                None,
            ))
            .await;

        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "legacy::ping")
                .is_some()
        );
    }

    // ── TriggerRegistrator implementation tests ─────────────────────────

    #[tokio::test]
    async fn trigger_registrator_register_and_unregister() {
        let (_engine, module) = setup_engine_and_module();

        let trigger = crate::trigger::Trigger {
            id: "eng-trig-1".to_string(),
            trigger_type: TRIGGER_FUNCTIONS_AVAILABLE.to_string(),
            function_id: "test::on_functions_changed".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
            metadata: None,
            namespace: "default".to_string(),
        };

        let result = module.register_trigger(trigger.clone()).await;
        assert!(result.is_ok());
        assert_eq!(module.triggers.len(), 1);
        assert!(module.triggers.contains_key("eng-trig-1"));

        let result = module.unregister_trigger(trigger).await;
        assert!(result.is_ok());
        assert!(module.triggers.is_empty());
    }

    #[tokio::test]
    async fn trigger_registrator_register_multiple() {
        let (_engine, module) = setup_engine_and_module();

        for i in 0..3 {
            let trigger = crate::trigger::Trigger {
                id: format!("eng-trig-{}", i),
                trigger_type: TRIGGER_WORKERS_AVAILABLE.to_string(),
                function_id: format!("test::handler_{}", i),
                config: serde_json::json!({}),
                worker_id: None,
                metadata: None,
                namespace: "default".to_string(),
            };
            module.register_trigger(trigger).await.unwrap();
        }

        assert_eq!(module.triggers.len(), 3);
    }

    // ── fire_triggers tests ─────────────────────────────────────────────

    #[tokio::test]
    async fn fire_triggers_no_matching_triggers() {
        let (_engine, module) = setup_engine_and_module();
        let trigger = crate::trigger::Trigger {
            id: "trig-1".to_string(),
            trigger_type: TRIGGER_FUNCTIONS_AVAILABLE.to_string(),
            function_id: "test::handler".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
            metadata: None,
            namespace: "default".to_string(),
        };
        module.register_trigger(trigger).await.unwrap();

        module
            .fire_triggers(
                TRIGGER_WORKERS_AVAILABLE,
                serde_json::json!({"event": "test"}),
            )
            .await;
    }

    #[tokio::test]
    async fn fire_triggers_with_matching_trigger() {
        let (engine, module) = setup_engine_and_module();

        let (tx, rx) = tokio::sync::oneshot::channel::<Value>();
        let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
        let tx_clone = tx.clone();

        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::on_workers".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(move |input: Value| {
                let tx = tx_clone.clone();
                async move {
                    if let Some(sender) = tx.lock().unwrap().take() {
                        let _ = sender.send(input);
                    }
                    FunctionResult::Success(None)
                }
            }),
        );

        let trigger = crate::trigger::Trigger {
            id: "trig-workers".to_string(),
            trigger_type: TRIGGER_WORKERS_AVAILABLE.to_string(),
            function_id: "test::on_workers".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
            metadata: None,
            namespace: "default".to_string(),
        };
        module.register_trigger(trigger).await.unwrap();

        let data = serde_json::json!({"event": "worker_connected", "worker_id": "w1"});
        module
            .fire_triggers(TRIGGER_WORKERS_AVAILABLE, data.clone())
            .await;

        let received = tokio::time::timeout(std::time::Duration::from_secs(2), rx)
            .await
            .expect("timed out waiting for trigger fire")
            .expect("channel closed");

        assert_eq!(received, data);
    }

    /// BUG 1 on the third fire path: `EngineFunctionsWorker::fire_triggers`
    /// keeps its own trigger map and dispatches the target. It must resolve in
    /// the trigger's namespace, not `default` — otherwise a namespaced worker's
    /// `on_functions_available` callback silently never runs. `test::on_fns`
    /// exists in both `orders` ("from-orders") and `default` ("from-default");
    /// the trigger bound in `orders` must fire "from-orders". Reverting either
    /// dispatch site to `DEFAULT_NAMESPACE` turns this RED.
    #[tokio::test]
    async fn engine_functions_worker_fires_trigger_in_its_namespace() {
        use std::sync::Mutex as StdMutex;

        let (engine, module) = setup_engine_and_module();

        let fired: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
        for (ns, tag) in [("orders", "from-orders"), ("default", "from-default")] {
            let fired = fired.clone();
            engine.register_function_handler_ns(
                ns,
                crate::engine::RegisterFunctionRequest {
                    function_id: "test::on_fns".to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                },
                crate::engine::Handler::new(move |_input: Value| {
                    let fired = fired.clone();
                    async move {
                        fired.lock().unwrap().push(tag.to_string());
                        FunctionResult::Success(None)
                    }
                }),
            );
        }

        module
            .register_trigger(crate::trigger::Trigger {
                id: "trig-fns-orders".to_string(),
                trigger_type: TRIGGER_FUNCTIONS_AVAILABLE.to_string(),
                function_id: "test::on_fns".to_string(),
                config: serde_json::json!({}),
                worker_id: None,
                metadata: None,
                namespace: "orders".to_string(),
            })
            .await
            .unwrap();

        module
            .fire_triggers(
                TRIGGER_FUNCTIONS_AVAILABLE,
                serde_json::json!({"event": "fns"}),
            )
            .await;

        // fire_triggers spawns the dispatch; give it a moment to run.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let fired = fired.lock().unwrap().clone();
        assert_eq!(
            fired,
            vec!["from-orders".to_string()],
            "the orders-bound functions-available trigger must fire the orders target; got: {fired:?}"
        );
    }

    // ── Serialization tests ─────────────────────────────────────────────

    #[test]
    fn function_summary_serializes() {
        let summary = FunctionSummary {
            function_id: "my::func".to_string(),
            namespace: crate::protocol::DEFAULT_NAMESPACE.to_string(),
            worker_name: "my".to_string(),
            description: Some("desc".to_string()),
            metadata: None,
        };
        let json = serde_json::to_value(&summary).expect("serialize");
        assert_eq!(json["function_id"], "my::func");
        assert_eq!(json["worker_name"], "my");
        assert_eq!(json["description"], "desc");
    }

    #[test]
    fn function_summary_omits_null_description() {
        let summary = FunctionSummary {
            function_id: "my::func".to_string(),
            namespace: crate::protocol::DEFAULT_NAMESPACE.to_string(),
            worker_name: "my".to_string(),
            description: None,
            metadata: None,
        };
        let json = serde_json::to_value(&summary).expect("serialize");
        assert!(json.get("description").is_none());
    }

    #[test]
    fn function_summary_metadata_skips_when_none_surfaces_when_some() {
        // None metadata is omitted from the wire (skip_serializing_if).
        let without = FunctionSummary {
            function_id: "my::func".to_string(),
            namespace: crate::protocol::DEFAULT_NAMESPACE.to_string(),
            worker_name: "my".to_string(),
            description: None,
            metadata: None,
        };
        let json = serde_json::to_value(&without).expect("serialize");
        assert!(
            json.get("metadata").is_none(),
            "None metadata must be omitted, not serialized as null"
        );

        // Some metadata is surfaced so callers can distinguish internal handlers.
        let with = FunctionSummary {
            function_id: "my::on-config-change".to_string(),
            namespace: crate::protocol::DEFAULT_NAMESPACE.to_string(),
            worker_name: "my".to_string(),
            description: None,
            metadata: Some(serde_json::json!({ "internal": true })),
        };
        let json = serde_json::to_value(&with).expect("serialize");
        assert_eq!(json["metadata"]["internal"], true);
    }

    #[test]
    fn registered_trigger_summary_serializes() {
        let summary = RegisteredTriggerSummary {
            id: "t-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "fn::handler".to_string(),
            worker_name: "fn".to_string(),
            config: serde_json::json!({ "x": 1 }),
            config_summary: "{\"x\":1}".to_string(),
        };
        let json = serde_json::to_value(&summary).expect("serialize");
        assert_eq!(json["id"], "t-1");
        assert_eq!(json["trigger_type"], "cron");
        assert_eq!(json["function_id"], "fn::handler");
        assert_eq!(json["worker_name"], "fn");
        assert_eq!(json["config"], serde_json::json!({ "x": 1 }));
        assert_eq!(json["config_summary"], "{\"x\":1}");
    }

    #[test]
    fn trigger_type_detail_uses_request_schema_alias() {
        let detail = TriggerTypeDetail {
            id: "http".to_string(),
            worker_name: "iii-http".to_string(),
            description: "HTTP trigger".to_string(),
            configuration_schema: Some(serde_json::json!({"type": "object"})),
            request_schema: Some(serde_json::json!({"type": "object"})),
            response_schema: None,
            instance_count: 2,
        };
        let json = serde_json::to_value(&detail).expect("serialize");
        assert!(json.get("configuration_schema").is_some());
        assert!(json.get("request_schema").is_some());
        assert!(json.get("response_schema").is_none());
        assert!(json.get("call_request_format").is_none());
        assert!(json.get("trigger_request_format").is_none());
        assert_eq!(json["instance_count"], 2);
    }

    #[test]
    fn worker_summary_serializes_with_description_null() {
        let summary = WorkerSummary {
            name: Some("agent-memory".to_string()),
            description: None,
            version: Some("0.4.0".to_string()),
            id: "w-abc".to_string(),
            namespace: crate::protocol::DEFAULT_NAMESPACE.to_string(),
            runtime: Some("rust".to_string()),
            os: Some("darwin".to_string()),
            status: "connected".to_string(),
            function_count: 9,
            connected_at_ms: 1_715_520_000_000,
            active_invocations: 0,
            isolation: None,
            ip_address: None,
        };
        let json = serde_json::to_value(&summary).expect("serialize");
        assert!(json.get("description").is_some());
        assert!(json["description"].is_null());
        assert_eq!(json["name"], "agent-memory");
        assert_eq!(json["function_count"], 9);
    }

    // ── Lifecycle tests ─────────────────────────────────────────────────

    #[tokio::test]
    async fn initialize_registers_engine_trigger_types() {
        let (engine, module) = setup_engine_and_module();
        module.initialize().await.unwrap();
        assert!(
            engine
                .trigger_registry
                .trigger_types
                .contains_key(TRIGGER_FUNCTIONS_AVAILABLE)
        );
        assert!(
            engine
                .trigger_registry
                .trigger_types
                .contains_key(TRIGGER_WORKERS_AVAILABLE)
        );
    }

    #[tokio::test]
    async fn start_background_tasks_shutdown_is_clean() {
        let (_engine, module) = setup_engine_and_module();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        module
            .start_background_tasks(shutdown_rx, shutdown_tx.clone())
            .await
            .unwrap();
        let _ = shutdown_tx.send(true);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    // ── create_channel test ─────────────────────────────────────────────

    #[tokio::test]
    async fn create_channel_returns_channel_refs() {
        let (engine, module) = setup_engine_and_module();
        let result = module
            .create_channel(CreateChannelInput {
                buffer_size: Some(2048),
            })
            .await;

        match result {
            FunctionResult::Success(output) => {
                assert!(
                    engine
                        .channel_manager
                        .get_channel(&output.writer.channel_id, &output.writer.access_key)
                        .is_some()
                );
                assert_eq!(output.writer.channel_id, output.reader.channel_id);
            }
            _ => panic!("expected create_channel success"),
        }
    }

    // ── functions_list service tests ────────────────────────────────────

    #[tokio::test]
    async fn functions_list_search_ranks_id_hits_above_description_hits() {
        let (engine, module) = setup_engine_and_module();

        // Alphabetically, coder::move sorts first — but it only matches
        // "state" via its description and must rank LAST.
        register_simple_function(&engine, "coder::move", Some("Move files; preserves state."));
        register_simple_function(&engine, "restate::apply", None); // id substring
        register_simple_function(&engine, "state::get", None); // id segment
        register_simple_function(&engine, "unrelated::fn", None); // no match

        let result = module
            .functions_list(
                FunctionsListInput {
                    search: Some("state".to_string()),
                    ..Default::default()
                },
                None,
            )
            .await;
        match result {
            FunctionResult::Success(result) => {
                let ids: Vec<&str> = result
                    .functions
                    .iter()
                    .map(|f| f.function_id.as_str())
                    .collect();
                assert_eq!(ids, vec!["state::get", "restate::apply", "coder::move"]);
            }
            _ => panic!("expected functions_list success"),
        }
    }

    #[tokio::test]
    async fn functions_list_filters_internal_by_default() {
        let (engine, module) = setup_engine_and_module();

        for function_id in ["engine::internal", "user::visible"] {
            register_simple_function(&engine, function_id, None);
        }

        let filtered = module
            .functions_list(
                FunctionsListInput {
                    include_internal: None,
                    ..Default::default()
                },
                None,
            )
            .await;
        match filtered {
            FunctionResult::Success(result) => {
                assert_eq!(result.functions.len(), 1);
                assert_eq!(result.functions[0].function_id, "user::visible");
            }
            _ => panic!("expected functions_list success"),
        }

        let all = module
            .functions_list(
                FunctionsListInput {
                    include_internal: Some(true),
                    ..Default::default()
                },
                None,
            )
            .await;
        match all {
            FunctionResult::Success(result) => {
                assert_eq!(result.functions.len(), 2);
            }
            _ => panic!("expected functions_list success"),
        }
    }

    #[tokio::test]
    async fn functions_list_filters_metadata_internal_by_default() {
        let (engine, module) = setup_engine_and_module();

        // A non-engine:: handler tagged internal (mirrors the config-change
        // handlers iii-observability::on-config-change / iii-http::on-config-change).
        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "worker::on-config-change".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: Some(serde_json::json!({ "internal": true })),
            },
            crate::engine::Handler::new(
                |_input: Value| async move { FunctionResult::Success(None) },
            ),
        );
        register_simple_function(&engine, "user::visible", None);

        let filtered = module
            .functions_list(
                FunctionsListInput {
                    include_internal: None,
                    ..Default::default()
                },
                None,
            )
            .await;
        match filtered {
            FunctionResult::Success(result) => {
                let ids: Vec<&str> = result
                    .functions
                    .iter()
                    .map(|f| f.function_id.as_str())
                    .collect();
                assert!(
                    !ids.contains(&"worker::on-config-change"),
                    "internal-tagged handler must be hidden by default: {ids:?}"
                );
                assert!(ids.contains(&"user::visible"));
            }
            _ => panic!("expected functions_list success"),
        }

        let all = module
            .functions_list(
                FunctionsListInput {
                    include_internal: Some(true),
                    ..Default::default()
                },
                None,
            )
            .await;
        match all {
            FunctionResult::Success(result) => {
                let ids: Vec<&str> = result
                    .functions
                    .iter()
                    .map(|f| f.function_id.as_str())
                    .collect();
                assert!(
                    ids.contains(&"worker::on-config-change"),
                    "include_internal must surface the internal handler: {ids:?}"
                );
            }
            _ => panic!("expected functions_list success"),
        }
    }

    #[tokio::test]
    async fn functions_list_applies_prefix_and_search() {
        let (engine, module) = setup_engine_and_module();
        register_simple_function(&engine, "alpha::one", Some("first"));
        register_simple_function(&engine, "alpha::two", Some("second"));
        register_simple_function(&engine, "beta::one", Some("first beta"));

        let by_prefix = module
            .functions_list(
                FunctionsListInput {
                    prefix: Some("alpha::".to_string()),
                    ..Default::default()
                },
                None,
            )
            .await;
        match by_prefix {
            FunctionResult::Success(result) => {
                assert_eq!(result.functions.len(), 2);
                assert!(
                    result
                        .functions
                        .iter()
                        .all(|f| f.function_id.starts_with("alpha::"))
                );
            }
            _ => panic!("expected success"),
        }

        let by_search = module
            .functions_list(
                FunctionsListInput {
                    search: Some("two".to_string()),
                    ..Default::default()
                },
                None,
            )
            .await;
        match by_search {
            FunctionResult::Success(result) => {
                assert_eq!(result.functions.len(), 1);
                assert_eq!(result.functions[0].function_id, "alpha::two");
            }
            _ => panic!("expected success"),
        }
    }

    #[tokio::test]
    async fn functions_list_filters_by_worker_name() {
        let (engine, module) = setup_engine_and_module();
        engine.upsert_runtime_worker(crate::worker_connections::RuntimeWorkerInfo {
            id: "iii-state".to_string(),
            name: "iii-state".to_string(),
            description: None,
            worker_type: "iii-state".to_string(),
            connected_at: chrono::Utc::now(),
            function_ids: vec!["state::get".to_string()],
            internal: false,
        });
        register_simple_function(&engine, "state::get", None);
        register_simple_function(&engine, "user::other", None);

        let filtered = module
            .functions_list(
                FunctionsListInput {
                    worker: Some("iii-state".to_string()),
                    ..Default::default()
                },
                None,
            )
            .await;
        match filtered {
            FunctionResult::Success(result) => {
                assert_eq!(result.functions.len(), 1);
                assert_eq!(result.functions[0].function_id, "state::get");
                assert_eq!(result.functions[0].worker_name, "iii-state");
            }
            _ => panic!("expected success"),
        }
    }

    #[tokio::test]
    async fn functions_list_filters_by_multiple_worker_names() {
        let (engine, module) = setup_engine_and_module();
        for (worker, function_id) in [
            ("iii-state", "state::get"),
            ("iii-queue", "queue::push"),
            ("iii-http", "http::fetch"),
        ] {
            engine.upsert_runtime_worker(crate::worker_connections::RuntimeWorkerInfo {
                id: worker.to_string(),
                name: worker.to_string(),
                description: None,
                worker_type: worker.to_string(),
                connected_at: chrono::Utc::now(),
                function_ids: vec![function_id.to_string()],
                internal: false,
            });
            register_simple_function(&engine, function_id, None);
        }

        let filtered = module
            .functions_list(
                FunctionsListInput {
                    workers: Some(vec!["iii-state".to_string(), "iii-http".to_string()]),
                    ..Default::default()
                },
                None,
            )
            .await;
        match filtered {
            FunctionResult::Success(result) => {
                let ids: Vec<&str> = result
                    .functions
                    .iter()
                    .map(|f| f.function_id.as_str())
                    .collect();
                assert_eq!(ids, vec!["http::fetch", "state::get"]);
            }
            _ => panic!("expected success"),
        }
    }

    // ── functions_info service tests ────────────────────────────────────

    #[tokio::test]
    async fn functions_info_returns_full_detail() {
        let (engine, module) = setup_engine_and_module();
        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::detailed".to_string(),
                description: Some("a detailed function".to_string()),
                request_format: Some(serde_json::json!({"type": "object"})),
                response_format: Some(serde_json::json!({"type": "string"})),
                metadata: Some(serde_json::json!({"v": 1})),
            },
            crate::engine::Handler::new(
                |_input: Value| async move { FunctionResult::Success(None) },
            ),
        );

        insert_trigger_instance(
            &engine,
            "trig-d",
            "cron",
            "test::detailed",
            serde_json::json!({"interval": 5}),
        );

        let result = module
            .functions_info(
                FunctionInfoInput {
                    function_id: Some("test::detailed".to_string()),
                    function_ids: None,
                    namespace: None,
                },
                None,
            )
            .await;

        match result {
            FunctionResult::Success(FunctionInfoOutput::Single(detail)) => {
                assert_eq!(detail.function_id, "test::detailed");
                assert_eq!(detail.description.as_deref(), Some("a detailed function"));
                assert_eq!(
                    detail.request_schema,
                    Some(serde_json::json!({"type": "object"}))
                );
                assert_eq!(
                    detail.response_schema,
                    Some(serde_json::json!({"type": "string"}))
                );
                assert_eq!(detail.metadata, Some(serde_json::json!({"v": 1})));
                assert_eq!(detail.registered_triggers.len(), 1);
                assert_eq!(detail.registered_triggers[0].id, "trig-d");
                assert_eq!(detail.registered_triggers[0].trigger_type, "cron");
            }
            _ => panic!("expected single functions_info success"),
        }
    }

    #[tokio::test]
    async fn functions_info_not_found_returns_failure() {
        let (_engine, module) = setup_engine_and_module();
        let result = module
            .functions_info(
                FunctionInfoInput {
                    function_id: Some("does::not::exist".to_string()),
                    function_ids: None,
                    namespace: None,
                },
                None,
            )
            .await;
        match result {
            FunctionResult::Failure(err) => {
                assert_eq!(err.code, "NOT_FOUND");
            }
            _ => panic!("expected NOT_FOUND failure"),
        }
    }

    #[tokio::test]
    async fn functions_info_batch_mixes_details_and_markers() {
        let (engine, module) = setup_engine_and_module();
        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::batched".to_string(),
                description: Some("a batched function".to_string()),
                request_format: Some(serde_json::json!({"type": "object"})),
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(
                |_input: Value| async move { FunctionResult::Success(None) },
            ),
        );

        let result = module
            .functions_info(
                FunctionInfoInput {
                    function_id: None,
                    // Duplicate id collapses; unknown id becomes a marker.
                    function_ids: Some(vec![
                        "test::batched".to_string(),
                        "does::not::exist".to_string(),
                        "test::batched".to_string(),
                    ]),
                    namespace: None,
                },
                None,
            )
            .await;

        let FunctionResult::Success(FunctionInfoOutput::Batch(batch)) = result else {
            panic!("expected batch functions_info success");
        };
        assert_eq!(batch.functions.len(), 2);
        match &batch.functions[0] {
            FunctionInfoEntry::Detail(detail) => assert_eq!(detail.function_id, "test::batched"),
            other => panic!("expected detail entry, got {other:?}"),
        }
        match &batch.functions[1] {
            FunctionInfoEntry::Unavailable { function_id, error } => {
                assert_eq!(function_id, "does::not::exist");
                assert_eq!(error, "not_found");
            }
            other => panic!("expected not_found marker, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn functions_info_rejects_both_and_neither_id_forms() {
        let (_engine, module) = setup_engine_and_module();
        for input in [
            FunctionInfoInput {
                function_id: Some("a::b".to_string()),
                function_ids: Some(vec!["a::b".to_string()]),
                namespace: None,
            },
            FunctionInfoInput {
                function_id: None,
                function_ids: None,
                namespace: None,
            },
            FunctionInfoInput {
                function_id: None,
                function_ids: Some(vec![]),
                namespace: None,
            },
            FunctionInfoInput {
                function_id: None,
                function_ids: Some((0..33).map(|i| format!("f::{i}")).collect()),
                namespace: None,
            },
        ] {
            match module.functions_info(input, None).await {
                FunctionResult::Failure(err) => assert_eq!(err.code, "INVALID_ARGUMENTS"),
                _ => panic!("expected INVALID_ARGUMENTS failure"),
            }
        }
    }

    // ── triggers_list / triggers_info ───────────────────────────────────

    #[tokio::test]
    async fn triggers_list_returns_trigger_types_not_instances() {
        let (engine, module) = setup_engine_and_module();
        engine.trigger_registry.trigger_types.insert(
            "user-type".to_string(),
            crate::trigger::TriggerType::new(
                "user-type",
                "User type",
                Box::new(module.clone()),
                None,
            ),
        );
        engine.trigger_registry.trigger_types.insert(
            "engine::internal-type".to_string(),
            crate::trigger::TriggerType::new(
                "engine::internal-type",
                "Internal",
                Box::new(module.clone()),
                None,
            ),
        );

        let filtered = module.triggers_list(TriggersListInput::default()).await;
        match filtered {
            FunctionResult::Success(result) => {
                assert_eq!(result.triggers.len(), 1);
                assert_eq!(result.triggers[0].id, "user-type");
            }
            _ => panic!("expected success"),
        }

        let all = module
            .triggers_list(TriggersListInput {
                include_internal: Some(true),
                ..Default::default()
            })
            .await;
        match all {
            FunctionResult::Success(result) => {
                assert!(
                    result
                        .triggers
                        .iter()
                        .any(|t| t.id == "engine::internal-type")
                );
                assert!(result.triggers.iter().any(|t| t.id == "user-type"));
            }
            _ => panic!("expected success"),
        }
    }

    #[tokio::test]
    async fn triggers_info_returns_schemas_and_instance_count() {
        let (engine, module) = setup_engine_and_module();
        // Use a builtin trigger type id so default schemas are populated.
        let tt = crate::trigger::TriggerType::new(
            "cron",
            "Cron trigger",
            Box::new(module.clone()),
            None,
        );
        engine
            .trigger_registry
            .register_trigger_type(tt)
            .await
            .unwrap();

        insert_trigger_instance(
            &engine,
            "trig-a",
            "cron",
            "test::handler_a",
            serde_json::json!({"expression": "* * * * * *"}),
        );
        insert_trigger_instance(
            &engine,
            "trig-b",
            "cron",
            "test::handler_b",
            serde_json::json!({"expression": "* * * * * *"}),
        );

        let result = module
            .triggers_info(TriggerInfoInput {
                id: "cron".to_string(),
            })
            .await;
        match result {
            FunctionResult::Success(detail) => {
                assert_eq!(detail.id, "cron");
                assert_eq!(detail.instance_count, 2);
                assert!(detail.configuration_schema.is_some());
                assert!(detail.request_schema.is_some());
                assert!(detail.response_schema.is_none());
            }
            _ => panic!("expected triggers_info success"),
        }
    }

    #[tokio::test]
    async fn triggers_info_returns_http_response_schema() {
        let (engine, module) = setup_engine_and_module();
        let tt = crate::trigger::TriggerType::new(
            "http",
            "HTTP trigger",
            Box::new(module.clone()),
            None,
        );
        engine
            .trigger_registry
            .register_trigger_type(tt)
            .await
            .unwrap();

        let result = module
            .triggers_info(TriggerInfoInput {
                id: "http".to_string(),
            })
            .await;
        match result {
            FunctionResult::Success(detail) => {
                let schema = detail
                    .response_schema
                    .clone()
                    .expect("http trigger type exposes response_schema");
                let properties = schema
                    .get("properties")
                    .and_then(Value::as_object)
                    .expect("response_schema has a properties object");
                for key in ["status_code", "headers", "body"] {
                    assert!(
                        properties.contains_key(key),
                        "expected property '{key}', got: {properties:?}"
                    );
                }
                // Pin optionality: the worker treats every field as optional, so
                // none may be in the schema's `required` array.
                if let Some(required) = schema.get("required").and_then(Value::as_array) {
                    for key in ["status_code", "headers", "body"] {
                        assert!(
                            !required.iter().any(|v| v.as_str() == Some(key)),
                            "'{key}' must not be required, got required: {required:?}"
                        );
                    }
                }

                let json = serde_json::to_value(&detail).expect("serialize");
                let serialized_response_schema = json
                    .get("response_schema")
                    .expect("serialized response_schema present");
                let serialized_properties = serialized_response_schema
                    .get("properties")
                    .and_then(Value::as_object)
                    .expect("serialized response_schema has a properties object");
                for key in ["status_code", "headers", "body"] {
                    assert!(
                        serialized_properties.contains_key(key),
                        "expected serialized property '{key}', got: {serialized_properties:?}"
                    );
                }
                // Pin optionality on the serialized form too.
                if let Some(required) = serialized_response_schema
                    .get("required")
                    .and_then(Value::as_array)
                {
                    for key in ["status_code", "headers", "body"] {
                        assert!(
                            !required.iter().any(|v| v.as_str() == Some(key)),
                            "'{key}' must not be required in serialized schema, got required: {required:?}"
                        );
                    }
                }
            }
            _ => panic!("expected triggers_info success"),
        }
    }

    #[tokio::test]
    async fn triggers_info_not_found_returns_failure() {
        let (_engine, module) = setup_engine_and_module();
        let result = module
            .triggers_info(TriggerInfoInput {
                id: "nope".to_string(),
            })
            .await;
        assert!(matches!(result, FunctionResult::Failure(_)));
    }

    // ── registered_triggers_list / info ─────────────────────────────────

    #[tokio::test]
    async fn registered_triggers_list_filters_internal_by_default() {
        let (engine, module) = setup_engine_and_module();
        insert_trigger_instance(
            &engine,
            "internal-row",
            "cron",
            "engine::internal",
            serde_json::json!({}),
        );
        insert_trigger_instance(
            &engine,
            "user-row",
            "cron",
            "user::handler",
            serde_json::json!({}),
        );

        let filtered = module
            .registered_triggers_list(RegisteredTriggersListInput::default())
            .await;
        match filtered {
            FunctionResult::Success(result) => {
                assert_eq!(result.registered_triggers.len(), 1);
                assert_eq!(result.registered_triggers[0].id, "user-row");
            }
            _ => panic!("expected success"),
        }

        let all = module
            .registered_triggers_list(RegisteredTriggersListInput {
                include_internal: Some(true),
                ..Default::default()
            })
            .await;
        match all {
            FunctionResult::Success(result) => {
                assert_eq!(result.registered_triggers.len(), 2);
            }
            _ => panic!("expected success"),
        }
    }

    #[tokio::test]
    async fn registered_triggers_list_filters_by_trigger_type_and_function_id() {
        let (engine, module) = setup_engine_and_module();
        insert_trigger_instance(&engine, "a", "cron", "user::a", serde_json::json!({}));
        insert_trigger_instance(&engine, "b", "http", "user::b", serde_json::json!({}));

        let by_type = module
            .registered_triggers_list(RegisteredTriggersListInput {
                trigger_type: Some("http".to_string()),
                ..Default::default()
            })
            .await;
        match by_type {
            FunctionResult::Success(result) => {
                assert_eq!(result.registered_triggers.len(), 1);
                assert_eq!(result.registered_triggers[0].id, "b");
            }
            _ => panic!("expected success"),
        }

        let by_function = module
            .registered_triggers_list(RegisteredTriggersListInput {
                function_id: Some("user::a".to_string()),
                ..Default::default()
            })
            .await;
        match by_function {
            FunctionResult::Success(result) => {
                assert_eq!(result.registered_triggers.len(), 1);
                assert_eq!(result.registered_triggers[0].id, "a");
            }
            _ => panic!("expected success"),
        }
    }

    #[tokio::test]
    async fn registered_triggers_info_returns_composite_with_function_and_trigger() {
        let (engine, module) = setup_engine_and_module();
        register_simple_function(&engine, "user::target", Some("target function"));
        let tt = crate::trigger::TriggerType::new("cron", "Cron", Box::new(module.clone()), None);
        engine
            .trigger_registry
            .register_trigger_type(tt)
            .await
            .unwrap();
        insert_trigger_instance(
            &engine,
            "compose-1",
            "cron",
            "user::target",
            serde_json::json!({"expression": "0 0 * * * *"}),
        );

        let result = module
            .registered_triggers_info(RegisteredTriggerInfoInput {
                id: "compose-1".to_string(),
            })
            .await;
        match result {
            FunctionResult::Success(detail) => {
                assert_eq!(detail.id, "compose-1");
                assert_eq!(detail.trigger_type, "cron");
                assert_eq!(detail.function_id, "user::target");
                assert!(detail.trigger.is_some());
                assert!(detail.function.is_some());
                let f = detail.function.unwrap();
                assert_eq!(f.function_id, "user::target");
                assert_eq!(f.description.as_deref(), Some("target function"));
            }
            _ => panic!("expected success"),
        }
    }

    #[tokio::test]
    async fn registered_triggers_info_returns_null_for_dangling_pointers() {
        let (engine, module) = setup_engine_and_module();
        insert_trigger_instance(
            &engine,
            "dangling",
            "unknown-type",
            "missing::function",
            serde_json::json!({}),
        );

        let result = module
            .registered_triggers_info(RegisteredTriggerInfoInput {
                id: "dangling".to_string(),
            })
            .await;
        match result {
            FunctionResult::Success(detail) => {
                assert!(detail.trigger.is_none());
                assert!(detail.function.is_none());
            }
            _ => panic!("expected success"),
        }
    }

    // ── workers_list / workers_info ─────────────────────────────────────

    #[tokio::test]
    async fn workers_list_returns_runtime_workers_with_filters() {
        let (engine, module) = setup_engine_and_module();
        engine.upsert_runtime_worker(crate::worker_connections::RuntimeWorkerInfo {
            id: "iii-state".to_string(),
            name: "iii-state".to_string(),
            description: None,
            worker_type: "iii-state".to_string(),
            connected_at: chrono::Utc::now(),
            function_ids: vec!["state::get".to_string()],
            internal: false,
        });
        engine.upsert_runtime_worker(crate::worker_connections::RuntimeWorkerInfo {
            id: "iii-queue".to_string(),
            name: "iii-queue".to_string(),
            description: None,
            worker_type: "iii-queue".to_string(),
            connected_at: chrono::Utc::now(),
            function_ids: vec![],
            internal: false,
        });

        let all = module.workers_list(WorkersListInput::default()).await;
        match all {
            FunctionResult::Success(result) => {
                assert_eq!(result.workers.len(), 2);
            }
            _ => panic!("expected success"),
        }

        let by_search = module
            .workers_list(WorkersListInput {
                search: Some("queue".to_string()),
                ..Default::default()
            })
            .await;
        match by_search {
            FunctionResult::Success(result) => {
                assert_eq!(result.workers.len(), 1);
                assert_eq!(result.workers[0].name.as_deref(), Some("iii-queue"));
            }
            _ => panic!("expected success"),
        }

        let by_runtime = module
            .workers_list(WorkersListInput {
                runtime: Some("engine".to_string()),
                ..Default::default()
            })
            .await;
        match by_runtime {
            FunctionResult::Success(result) => {
                assert_eq!(result.workers.len(), 2);
            }
            _ => panic!("expected success"),
        }
    }

    /// In-process (runtime) workers never declare a namespace — they register
    /// into `default`. Their `engine::workers::list` row must say so rather
    /// than carry a blank or a foreign namespace.
    #[tokio::test]
    async fn workers_list_reports_default_namespace_for_runtime_workers() {
        let (engine, module) = setup_engine_and_module();
        engine.upsert_runtime_worker(crate::worker_connections::RuntimeWorkerInfo {
            id: "iii-state".to_string(),
            name: "iii-state".to_string(),
            description: None,
            worker_type: "iii-state".to_string(),
            connected_at: chrono::Utc::now(),
            function_ids: vec!["state::get".to_string()],
            internal: false,
        });

        match module.workers_list(WorkersListInput::default()).await {
            FunctionResult::Success(result) => {
                let worker = result
                    .workers
                    .iter()
                    .find(|w| w.name.as_deref() == Some("iii-state"))
                    .expect("runtime worker must be listed");
                assert_eq!(worker.namespace, crate::protocol::DEFAULT_NAMESPACE);
            }
            _ => panic!("expected success"),
        }
    }

    /// `engine::workers::info` denormalizes the worker's functions as
    /// `FunctionSummary` rows; those must report the namespace they were looked
    /// up in, consistently with `engine::functions::list`.
    #[tokio::test]
    async fn workers_info_function_rows_report_their_namespace() {
        let (engine, module) = setup_engine_and_module();
        engine.upsert_runtime_worker(crate::worker_connections::RuntimeWorkerInfo {
            id: "iii-state".to_string(),
            name: "iii-state".to_string(),
            description: None,
            worker_type: "iii-state".to_string(),
            connected_at: chrono::Utc::now(),
            function_ids: vec!["state::get".to_string()],
            internal: false,
        });
        register_simple_function(&engine, "state::get", None);

        match module
            .workers_info(WorkerInfoInput {
                name: "iii-state".to_string(),
                namespace: None,
            })
            .await
        {
            FunctionResult::Success(result) => {
                let function = result
                    .functions
                    .iter()
                    .find(|f| f.function_id == "state::get")
                    .expect("state::get must be listed");
                assert_eq!(function.namespace, crate::protocol::DEFAULT_NAMESPACE);
            }
            _ => panic!("expected success"),
        }
    }

    #[tokio::test]
    async fn workers_info_returns_full_surface() {
        let (engine, module) = setup_engine_and_module();
        engine.upsert_runtime_worker(crate::worker_connections::RuntimeWorkerInfo {
            id: "iii-state".to_string(),
            name: "iii-state".to_string(),
            description: None,
            worker_type: "iii-state".to_string(),
            connected_at: chrono::Utc::now(),
            function_ids: vec!["state::get".to_string(), "state::set".to_string()],
            internal: false,
        });
        register_simple_function(&engine, "state::get", Some("get state"));
        register_simple_function(&engine, "state::set", Some("set state"));
        insert_trigger_instance(
            &engine,
            "trg-state",
            "cron",
            "state::get",
            serde_json::json!({}),
        );

        let result = module
            .workers_info(WorkerInfoInput {
                name: "iii-state".to_string(),
                namespace: None,
            })
            .await;
        match result {
            FunctionResult::Success(detail) => {
                assert_eq!(detail.worker.name.as_deref(), Some("iii-state"));
                assert_eq!(detail.functions.len(), 2);
                assert_eq!(detail.registered_triggers.len(), 1);
                assert_eq!(detail.registered_triggers[0].id, "trg-state");
                assert!(detail.worker.internal == false);
            }
            _ => panic!("expected success"),
        }
    }

    #[tokio::test]
    async fn workers_info_attributes_ws_trigger_types_like_triggers_list() {
        let (engine, module) = setup_engine_and_module();

        // The name resolves to a runtime entry first…
        engine.upsert_runtime_worker(crate::worker_connections::RuntimeWorkerInfo {
            id: "ops-worker".to_string(),
            name: "ops-worker".to_string(),
            description: None,
            worker_type: "ops-worker".to_string(),
            connected_at: chrono::Utc::now(),
            function_ids: vec![],
            internal: false,
        });

        // …while the trigger type is pinned to a same-named WS connection's
        // Uuid (the stale-daemon / duplicate-name scenario). Id-based
        // attribution dropped the type here; name-based attribution — the
        // index `engine::triggers::list` uses — must keep it.
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let conn = crate::worker_connections::WorkerConnection::new(tx);
        let conn_id = conn.id;
        engine.worker_registry.register_worker(conn);
        engine.worker_registry.update_worker_metadata(
            &conn_id,
            "rust".to_string(),
            Some("1.0.0".to_string()),
            Some("ops-worker".to_string()),
            None,
            Some("linux".to_string()),
            None,
            None,
            None,
            None,
        );
        let tt = crate::trigger::TriggerType::new(
            "worker",
            "Worker lifecycle events",
            Box::new(module.clone()),
            Some(conn_id),
        );
        engine
            .trigger_registry
            .register_trigger_type(tt)
            .await
            .unwrap();

        let result = module
            .workers_info(WorkerInfoInput {
                name: "ops-worker".to_string(),
                namespace: None,
            })
            .await;
        match result {
            FunctionResult::Success(detail) => {
                let ids: Vec<&str> = detail.trigger_types.iter().map(|t| t.id.as_str()).collect();
                assert_eq!(
                    ids,
                    vec!["worker"],
                    "workers::info must attribute WS-registered trigger types by \
                     owner name, matching engine::triggers::list"
                );
            }
            _ => panic!("expected success"),
        }
    }

    #[test]
    fn sanitize_worker_text_strips_controls_and_caps_length() {
        assert_eq!(
            sanitize_worker_text("ok\u{1b}[31m text\u{7f}".to_string()),
            Some("ok[31m text".to_string()),
            "ESC/C0/C1 control chars are stripped, printable remainder kept"
        );
        assert_eq!(sanitize_worker_text("  \u{1b}\u{0007} ".to_string()), None);
        let long = "x".repeat(WORKER_DESCRIPTION_MAX_LEN + 50);
        assert_eq!(
            sanitize_worker_text(long).map(|s| s.chars().count()),
            Some(WORKER_DESCRIPTION_MAX_LEN)
        );
        // Cap is by char, not byte: a multibyte input must truncate on a char
        // boundary (no panic, valid UTF-8) and yield exactly MAX chars.
        let multibyte = "\u{1f600}".repeat(WORKER_DESCRIPTION_MAX_LEN + 10);
        let out = sanitize_worker_text(multibyte).unwrap();
        assert_eq!(out.chars().count(), WORKER_DESCRIPTION_MAX_LEN);
        // Trojan-Source bidi overrides/isolates are stripped (CVE-2021-42574).
        assert_eq!(
            sanitize_worker_text("a\u{202e}b\u{2066}c".to_string()),
            Some("abc".to_string())
        );
    }

    #[tokio::test]
    async fn workers_info_excludes_trigger_types_owned_by_other_workers() {
        let (engine, module) = setup_engine_and_module();

        engine.upsert_runtime_worker(crate::worker_connections::RuntimeWorkerInfo {
            id: "ops-worker".to_string(),
            name: "ops-worker".to_string(),
            description: None,
            worker_type: "ops-worker".to_string(),
            connected_at: chrono::Utc::now(),
            function_ids: vec![],
            internal: false,
        });

        // A trigger type pinned to a DIFFERENT worker name must never be
        // attributed to the queried worker (guards against an over-broad
        // attribution regression — e.g. matching everything).
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let conn = crate::worker_connections::WorkerConnection::new(tx);
        let conn_id = conn.id;
        engine.worker_registry.register_worker(conn);
        engine.worker_registry.update_worker_metadata(
            &conn_id,
            "rust".to_string(),
            Some("1.0.0".to_string()),
            Some("other-worker".to_string()),
            None,
            Some("linux".to_string()),
            None,
            None,
            None,
            None,
        );
        let tt = crate::trigger::TriggerType::new(
            "foreign",
            "Owned by other-worker",
            Box::new(module.clone()),
            Some(conn_id),
        );
        engine
            .trigger_registry
            .register_trigger_type(tt)
            .await
            .unwrap();

        let result = module
            .workers_info(WorkerInfoInput {
                name: "ops-worker".to_string(),
                namespace: None,
            })
            .await;
        match result {
            FunctionResult::Success(detail) => {
                assert!(
                    detail.trigger_types.iter().all(|t| t.id != "foreign"),
                    "workers::info must not attribute another worker's trigger type"
                );
            }
            _ => panic!("expected success"),
        }
    }

    #[tokio::test]
    async fn workers_info_not_found_returns_failure() {
        let (_engine, module) = setup_engine_and_module();
        let result = module
            .workers_info(WorkerInfoInput {
                name: "nope".to_string(),
                namespace: None,
            })
            .await;
        assert!(matches!(result, FunctionResult::Failure(_)));
    }

    #[tokio::test]
    async fn workers_list_returns_registered_worker_with_metrics_when_filtered() {
        let (engine, module) = setup_engine_and_module();

        init_metric_storage(Some(128), Some(3600));
        if let Some(storage) = get_metric_storage() {
            storage.clear();
        }

        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let worker = crate::worker_connections::WorkerConnection::new(tx);
        let worker_id = worker.id.to_string();
        worker.include_function_id("user::visible").await;
        // Give the worker a name + pid so it appears in the unfiltered list.
        engine.worker_registry.register_worker(worker);
        engine.worker_registry.update_worker_metadata(
            &uuid::Uuid::parse_str(&worker_id).unwrap(),
            "node".to_string(),
            Some("1.0.0".to_string()),
            Some("named-worker".to_string()),
            None,
            Some("linux".to_string()),
            None,
            Some(4242),
            None,
            None,
        );

        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        if let Some(storage) = get_metric_storage() {
            storage.add_metrics(vec![StoredMetric {
                name: "iii.worker.cpu.percent".to_string(),
                description: "cpu".to_string(),
                unit: "%".to_string(),
                metric_type: StoredMetricType::Gauge,
                data_points: vec![StoredDataPoint::Number(StoredNumberDataPoint {
                    value: 42.0,
                    attributes: vec![("worker.id".to_string(), worker_id.clone())],
                    timestamp_unix_nano: now_ns,
                })],
                service_name: "svc".to_string(),
                timestamp_unix_nano: now_ns,
                instrumentation_scope_name: None,
                instrumentation_scope_version: None,
            }]);
        }

        let result = module
            .workers_list(WorkersListInput {
                search: Some("named".to_string()),
                ..Default::default()
            })
            .await;
        match result {
            FunctionResult::Success(result) => {
                assert_eq!(result.workers.len(), 1);
                assert_eq!(result.workers[0].name.as_deref(), Some("named-worker"));
                assert_eq!(result.workers[0].function_count, 1);
            }
            _ => panic!("expected workers_list success"),
        }

        // workers::info should also surface latest_metrics for this worker.
        let info_result = module
            .workers_info(WorkerInfoInput {
                name: "named-worker".to_string(),
                namespace: None,
            })
            .await;
        match info_result {
            FunctionResult::Success(detail) => {
                assert_eq!(detail.worker.id, worker_id);
                assert_eq!(detail.worker.pid, Some(4242));
                assert!(detail.worker.latest_metrics.is_some());
            }
            _ => panic!("expected workers_info success"),
        }
    }

    // ── register_worker service ─────────────────────────────────────────

    #[tokio::test]
    async fn register_worker_service_fires_worker_trigger() {
        let (engine, module) = setup_engine_and_module();

        let (tx, rx) = tokio::sync::oneshot::channel::<Value>();
        let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
        let tx_clone = tx.clone();
        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::worker_listener".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(move |input: Value| {
                let tx = tx_clone.clone();
                async move {
                    if let Some(sender) = tx.lock().unwrap().take() {
                        let _ = sender.send(input);
                    }
                    FunctionResult::Success(None)
                }
            }),
        );

        engine.trigger_registry.triggers.insert(
            "worker-trigger".to_string(),
            crate::trigger::Trigger {
                id: "worker-trigger".to_string(),
                trigger_type: TRIGGER_WORKERS_AVAILABLE.to_string(),
                function_id: "test::worker_listener".to_string(),
                config: serde_json::json!({}),
                worker_id: None,
                metadata: None,
                namespace: "default".to_string(),
            },
        );

        let worker =
            crate::worker_connections::WorkerConnection::new(tokio::sync::mpsc::channel(1).0);
        let worker_id = worker.id.to_string();
        engine.worker_registry.register_worker(worker);

        let result = module
            .register_worker(RegisterWorkerInput {
                worker_id: worker_id.clone(),
                runtime: Some("node".to_string()),
                version: Some("1.0.0".to_string()),
                name: Some("my-worker".to_string()),
                description: None,
                os: Some("linux".to_string()),
                telemetry: None,
                pid: None,
                isolation: None,
                namespace: None,
            })
            .await;
        assert!(matches!(
            result,
            FunctionResult::Success(RegisterWorkerResult { success: true })
        ));

        let payload = tokio::time::timeout(std::time::Duration::from_secs(2), rx)
            .await
            .expect("timed out waiting for worker trigger")
            .expect("worker trigger channel closed");
        assert_eq!(payload["event"], "worker_metadata_updated");
        assert_eq!(payload["worker_id"], worker_id);
    }

    #[tokio::test]
    async fn destroy_returns_ok() {
        let (_engine, module) = setup_engine_and_module();
        module.destroy().await.unwrap();
    }

    // ── engine::register_trigger / engine::unregister_trigger ────────────

    /// Registers `test-type` (with the module itself as registrator) so
    /// `register_trigger_fn` can bind against it.
    async fn register_test_trigger_type(engine: &Engine, module: &EngineFunctionsWorker) {
        engine
            .trigger_registry
            .register_trigger_type(crate::trigger::TriggerType::new(
                "test-type",
                "Test trigger type",
                Box::new(module.clone()),
                None,
            ))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn register_trigger_fn_binds_directly_and_delivers_metadata_as_arg() {
        let (engine, module) = setup_engine_and_module();
        register_test_trigger_type(&engine, &module).await;

        // Target function records the payload AND the metadata sidecar it
        // receives as a distinct argument.
        let recorded_input: Arc<std::sync::Mutex<Option<Value>>> =
            Arc::new(std::sync::Mutex::new(None));
        let recorded_meta: Arc<std::sync::Mutex<Option<Value>>> =
            Arc::new(std::sync::Mutex::new(None));
        let ri = recorded_input.clone();
        let rm = recorded_meta.clone();
        let handler: Arc<HandlerFn> = Arc::new(move |_inv, input, _session, metadata| {
            let ri = ri.clone();
            let rm = rm.clone();
            Box::pin(async move {
                *ri.lock().unwrap() = Some(input);
                *rm.lock().unwrap() = metadata;
                FunctionResult::Success(None)
            })
        });
        engine.functions.register_function(
            "test::target".to_string(),
            Function {
                handler,
                _function_id: "test::target".to_string(),
                _description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
        );

        let wid = uuid::Uuid::new_v4();
        let meta = serde_json::json!({ "session_id": "s_1", "subscription_id": "sub_1" });
        let id = match module
            .register_trigger_fn(
                RegisterTriggerInput {
                    trigger_type: "test-type".to_string(),
                    function_id: "test::target".to_string(),
                    config: serde_json::json!({}),
                    metadata: Some(meta.clone()),
                    caller_worker_id: Some(wid.to_string()),
                },
                None,
            )
            .await
        {
            FunctionResult::Success(r) => r.id,
            _ => panic!("expected register success"),
        };

        // The trigger binds DIRECTLY to the target — no proxy function exists —
        // and carries the metadata. Function-path registrations are durable:
        // the caller's connection is deliberately NOT recorded as owner, so
        // the caller's disconnect can never reap the binding.
        let trig = engine
            .trigger_registry
            .triggers
            .get(&id)
            .expect("trigger registered");
        assert_eq!(trig.function_id, "test::target");
        assert_eq!(trig.worker_id, None);
        assert_eq!(trig.metadata, Some(meta.clone()));

        // Fire the way a registrator does: metadata travels explicitly, NOT
        // folded into the payload.
        let event = serde_json::json!({ "event_type": "set", "key": "k", "value": 1 });
        engine
            .call_with_metadata(&trig.function_id, event.clone(), trig.metadata.clone())
            .await
            .expect("fire ok");

        let got_input = recorded_input.lock().unwrap().clone().expect("invoked");
        let got_meta = recorded_meta.lock().unwrap().clone();
        // Payload is untouched — no `__metadata` smuggled in.
        assert_eq!(got_input, event);
        assert!(got_input.get("__metadata").is_none());
        // Metadata arrived as the separate argument.
        assert_eq!(got_meta, Some(meta));
    }

    #[tokio::test]
    async fn unregister_trigger_fn_removes_trigger_and_is_idempotent() {
        let (engine, module) = setup_engine_and_module();
        register_test_trigger_type(&engine, &module).await;
        register_simple_function(&engine, "test::target", None);

        let id = match module
            .register_trigger_fn(
                RegisterTriggerInput {
                    trigger_type: "test-type".to_string(),
                    function_id: "test::target".to_string(),
                    config: serde_json::json!({}),
                    metadata: Some(serde_json::json!({ "session_id": "s" })),
                    caller_worker_id: None,
                },
                None,
            )
            .await
        {
            FunctionResult::Success(r) => r.id,
            _ => panic!("expected register success"),
        };
        assert!(engine.trigger_registry.triggers.contains_key(&id));

        // First unregister removes the trigger.
        match module
            .unregister_trigger_fn(UnregisterTriggerInput {
                id: id.clone(),
                trigger_type: None,
            })
            .await
        {
            FunctionResult::Success(r) => assert!(r.removed),
            _ => panic!("expected unregister success"),
        }
        assert!(!engine.trigger_registry.triggers.contains_key(&id));

        // Second unregister is a no-op reporting removed: false.
        match module
            .unregister_trigger_fn(UnregisterTriggerInput {
                id,
                trigger_type: None,
            })
            .await
        {
            FunctionResult::Success(r) => assert!(!r.removed),
            _ => panic!("expected idempotent unregister success"),
        }
    }

    /// Builds a session whose `function_registration_prefix` is set, mirroring
    /// the message-path prefix tests in `engine::mod`.
    fn session_with_prefix(
        engine: Arc<Engine>,
        prefix: &str,
    ) -> Arc<crate::workers::worker::rbac_session::Session> {
        use crate::workers::worker::{WorkerManagerConfig, rbac_session::Session};
        Arc::new(Session {
            engine,
            config: Arc::new(WorkerManagerConfig::default()),
            ip_address: "127.0.0.1".to_string(),
            session_id: uuid::Uuid::new_v4(),
            allowed_functions: vec![],
            forbidden_functions: vec![],
            allowed_trigger_types: None,
            allow_function_registration: true,
            allow_trigger_type_registration: true,
            context: serde_json::json!({}),
            function_registration_prefix: Some(prefix.to_string()),
        })
    }

    /// A prefixed session registers its functions under `prefix::id`, so a
    /// trigger bound via `engine::register_trigger` must target the prefixed
    /// function — parity with the `Message::RegisterTrigger` path. Before the
    /// fix the raw id was stored and firing missed the real function.
    #[tokio::test]
    async fn register_trigger_fn_applies_session_function_prefix() {
        let (engine, module) = setup_engine_and_module();
        register_test_trigger_type(&engine, &module).await;
        register_simple_function(&engine, "pfx::test::target", None);

        let session = session_with_prefix(engine.clone(), "pfx");
        let id = match module
            .register_trigger_fn(
                RegisterTriggerInput {
                    trigger_type: "test-type".to_string(),
                    function_id: "test::target".to_string(),
                    config: serde_json::json!({}),
                    metadata: None,
                    caller_worker_id: None,
                },
                Some(session),
            )
            .await
        {
            FunctionResult::Success(r) => r.id,
            _ => panic!("expected register success"),
        };

        let trig = engine
            .trigger_registry
            .triggers
            .get(&id)
            .expect("trigger registered");
        assert_eq!(
            trig.function_id, "pfx::test::target",
            "trigger must bind to the session-prefixed function id"
        );
    }

    #[tokio::test]
    async fn register_trigger_fn_unknown_type_defers_and_activates_later() {
        let (engine, module) = setup_engine_and_module();
        register_simple_function(&engine, "test::target", None);

        let result = module
            .register_trigger_fn(
                RegisterTriggerInput {
                    trigger_type: "test-type".to_string(),
                    function_id: "test::target".to_string(),
                    config: serde_json::json!({}),
                    metadata: None,
                    caller_worker_id: None,
                },
                None,
            )
            .await;
        // Registering ahead of the trigger type's provider succeeds and
        // returns a usable id: the intent is parked, not live.
        let id = match result {
            FunctionResult::Success(r) => r.id,
            _ => panic!("expected deferred register success"),
        };
        assert!(engine.trigger_registry.triggers.is_empty());
        assert!(engine.trigger_registry.pending_triggers.contains_key(&id));

        // Once the type registers, the parked intent goes live.
        register_test_trigger_type(&engine, &module).await;
        assert!(engine.trigger_registry.pending_triggers.is_empty());
        assert!(engine.trigger_registry.triggers.contains_key(&id));
    }
}
