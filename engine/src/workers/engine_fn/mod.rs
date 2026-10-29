// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, sync::Arc};

use function_macros::{function, service};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest, SessionHandler},
    function::FunctionResult,
    observer::FunctionOptions,
    protocol::{ErrorBody, StreamChannelRef, WorkerMetrics},
    worker_connections::{RuntimeWorkerInfo, WorkerConnection, WorkerConnectionTelemetryMeta},
    workers::traits::Worker,
    workers::worker::rbac_session::Session,
};

/// Window after the first registry change during which further changes are
/// folded into the same `engine::on-functions-changed` event.
const FUNCTIONS_CHANGED_COALESCE: std::time::Duration = std::time::Duration::from_millis(100);

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
    /// Injected by the engine from the calling worker, which owns the channel:
    /// its disconnect releases the ends nobody has attached yet. Absent for
    /// in-process callers, whose channels are left to the TTL sweeper.
    #[serde(rename = "_caller_worker_id", default)]
    pub caller_worker_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
pub struct CreateChannelOutput {
    pub writer: StreamChannelRef,
    pub reader: StreamChannelRef,
}

/// Read-only observability builtins that stay in the default
/// `engine::functions::list` alongside `engine::queue::*`. The `*::clear`
/// siblings and the rest of `engine::` remain `include_internal`-only.
const DISCOVERABLE_ENGINE_FUNCTIONS: [&str; 5] = [
    "engine::traces::list",
    "engine::traces::spans",
    "engine::traces::tree",
    "engine::traces::group_by",
    "engine::logs::list",
];

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
    #[serde(flatten)]
    pub observe: FunctionObserveInfo,
}

/// What a function row says about observing the function (decision 6): the
/// registration's options, the defaults left out, and how many observers it
/// has.
#[derive(Debug, Clone, Default, PartialEq, Serialize, JsonSchema)]
pub struct FunctionObserveInfo {
    /// JSON Schema observers' `args` are validated against; absent when the
    /// function takes no args.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args_format: Option<Value>,
    /// `true`: only the worker that hosts the function calls it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub private: bool,
    /// `false`: nobody may observe the function.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observable: Option<bool>,
    /// Live observers of the function, in any state but `disposed`.
    pub observer_count: usize,
}

impl FunctionObserveInfo {
    fn new(options: FunctionOptions, observer_count: usize) -> Self {
        Self {
            args_format: options.args_format,
            private: options.private,
            observable: (!options.observable).then_some(false),
            observer_count,
        }
    }
}

impl FunctionSummary {
    fn into_notification_value(self) -> Value {
        let mut fields = serde_json::Map::new();
        fields.insert("function_id".to_string(), Value::String(self.function_id));
        fields.insert("namespace".to_string(), Value::String(self.namespace));
        fields.insert("worker_name".to_string(), Value::String(self.worker_name));
        if let Some(description) = self.description {
            fields.insert("description".to_string(), Value::String(description));
        }
        if let Some(metadata) = self.metadata {
            fields.insert("metadata".to_string(), metadata);
        }
        if let Some(args_format) = self.observe.args_format {
            fields.insert("args_format".to_string(), args_format);
        }
        if self.observe.private {
            fields.insert("private".to_string(), Value::Bool(true));
        }
        if let Some(observable) = self.observe.observable {
            fields.insert("observable".to_string(), Value::Bool(observable));
        }
        fields.insert(
            "observer_count".to_string(),
            Value::from(self.observe.observer_count),
        );
        Value::Object(fields)
    }
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
    #[serde(flatten)]
    pub observe: FunctionObserveInfo,
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
    /// Routing namespace this worker resolved to. A worker name is unique only
    /// *within* a namespace, so the same name can identify two different
    /// workers across namespaces; this is what tells them apart.
    pub namespace: String,
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
    /// Ids of the observers this worker created (see
    /// `engine::observers::get`).
    pub observers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FunctionsListResult {
    pub functions: Vec<FunctionSummary>,
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
    /// Protocol features the SDK supports (`observe.v1`, `observe.v2`,
    /// `local-dispatch.v1`, `large-messages.v1`). Absent from SDKs older
    /// than 0.25. Read leniently: a malformed list declares nothing instead
    /// of failing the registration.
    #[serde(default, deserialize_with = "crate::protocol::deserialize_features")]
    pub features: Vec<String>,
}

// ── Worker ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct EngineFunctionsWorker {
    engine: Arc<Engine>,
}

impl EngineFunctionsWorker {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self { engine }
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
                "Function '{function_id}' is ambiguous: it is registered in namespace(s): {}. \
                 Pass `namespace` to select one.",
                namespaces.join(", ")
            )
        };
        ErrorBody {
            code: "NOT_FOUND".into(),
            message,
            stacktrace: None,
        }
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
    /// ambiguity: this returns `None` rather than guess an owner.
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
                .with_function(ns, function_id, |function| function.map(|_| ns.to_string())),
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
                let observe = self.observe_info(&namespace, &f._function_id);
                FunctionSummary {
                    function_id: f._function_id.clone(),
                    namespace,
                    worker_name,
                    description: f._description.clone(),
                    metadata: f.metadata.clone(),
                    observe,
                }
            })
            .collect()
    }

    /// What a function row says about observing `(namespace, function_id)`.
    fn observe_info(&self, namespace: &str, function_id: &str) -> FunctionObserveInfo {
        let observers = &self.engine.observers;
        FunctionObserveInfo::new(
            observers.options(namespace, function_id),
            observers.observer_count(namespace, function_id),
        )
    }

    /// The `functions_changed` event of `engine::on-functions-changed`.
    async fn functions_changed_payload(&self) -> Value {
        let functions = self
            .list_function_summaries()
            .await
            .into_iter()
            .map(FunctionSummary::into_notification_value)
            .collect();
        // Move the snapshot metadata into the event instead of serializing a
        // borrowed snapshot into a second owned tree.
        Value::Object(serde_json::Map::from_iter([
            (
                "event".to_string(),
                Value::String("functions_changed".to_string()),
            ),
            ("functions".to_string(), Value::Array(functions)),
        ]))
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
        self.engine
            .functions
            .with_function(namespace, function_id, |function| {
                crate::workers::worker::rbac_config::is_function_allowed_ref(
                    function_id,
                    namespace,
                    session.config.rbac.as_ref(),
                    &session.namespaces,
                    &session.allowed_functions,
                    &session.forbidden_functions,
                    function,
                )
            })
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
            description: function._description,
            request_schema: function.request_format,
            response_schema: function.response_format,
            metadata: function.metadata,
            observe: self.observe_info(namespace, function_id),
        })
    }

    fn worker_detail_envelope_from_connection(
        &self,
        w: &WorkerConnection,
        namespace: String,
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
            namespace,
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

    fn worker_detail_envelope_from_runtime(
        &self,
        w: &RuntimeWorkerInfo,
        namespace: String,
    ) -> WorkerDetailEnvelope {
        let functions = w.function_ids.clone();
        WorkerDetailEnvelope {
            name: Some(w.name.clone()),
            description: w.description.clone(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            id: w.id.clone(),
            namespace,
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
        // The connection id of the resolved worker, when it is a WebSocket
        // worker. `None` for in-process runtime workers (which have no
        // connection Uuid, and create no observers). This is what
        // disambiguates same-named workers in different namespaces when
        // listing the observers they created below.
        let resolved_worker_id: Option<uuid::Uuid>;
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
            envelope = self.worker_detail_envelope_from_runtime(&runtime, worker_namespace.clone());
            function_ids = runtime.function_ids;
            resolved_worker_id = None;
        } else {
            let worker = self
                .engine
                .worker_registry
                .list_workers()
                .into_iter()
                .find(|w| {
                    w.name.as_deref() == Some(name)
                        && self.engine.worker_registry.get_namespace(&w.id) == worker_namespace
                })?;
            let function_count_async = worker.get_function_ids().await;
            let function_count = function_count_async.len();
            let active_invocations = worker.invocation_count().await;
            let ip_address = worker.session.as_ref().map(|s| s.ip_address.clone());
            let latest_metrics = get_worker_metrics_from_storage(&worker.id.to_string());
            resolved_worker_id = Some(worker.id);
            envelope = self.worker_detail_envelope_from_connection(
                &worker,
                worker_namespace.clone(),
                function_count,
                active_invocations,
                ip_address,
                latest_metrics,
            );
            function_ids = function_count_async;
        }

        let index = self.function_owner_index().await;
        let mut functions: Vec<FunctionSummary> = function_ids
            .into_iter()
            .filter_map(|fn_id| {
                let (description, metadata) =
                    self.engine
                        .functions
                        .with_function(&worker_namespace, &fn_id, |function| {
                            function.map(|function| {
                                (function._description.clone(), function.metadata.clone())
                            })
                        })?;
                let worker_name =
                    Self::worker_name_for_function_id(&index, &worker_namespace, &fn_id);
                let observe = self.observe_info(&worker_namespace, &fn_id);
                Some(FunctionSummary {
                    function_id: fn_id,
                    // Mirrors the lookup directly above: this row *is* the
                    // entry keyed under the worker's own namespace.
                    namespace: worker_namespace.clone(),
                    worker_name,
                    description,
                    metadata,
                    observe,
                })
            })
            .collect();
        functions.sort_by(|a, b| a.function_id.cmp(&b.function_id));

        let observers = resolved_worker_id
            .map(|connection| self.engine.observers.created_by(connection))
            .unwrap_or_default();

        Some(WorkerInfoOutput {
            worker: envelope,
            functions,
            observers,
        })
    }

    /// Returns whether this call fixed the connection's protocol features.
    async fn register_worker_metadata(&self, input: RegisterWorkerInput) -> bool {
        let worker_id = match uuid::Uuid::parse_str(&input.worker_id) {
            Ok(id) => id,
            Err(_) => {
                tracing::error!(worker_id = %input.worker_id, "Invalid worker_id format");
                return false;
            }
        };

        let runtime = input.runtime.unwrap_or_else(|| "unknown".to_string());

        // Before the buffered registrations drain (the caller drains right
        // after this returns): what the engine sends for them depends on
        // whether the connection speaks observe. Also before
        // `update_worker_metadata` below, which sizes the connection's receive
        // limit by these features.
        let features_fixed = !input.features.is_empty()
            && self
                .engine
                .worker_registry
                .set_worker_features(&worker_id, &input.features);
        if !input.features.is_empty() && !features_fixed {
            tracing::debug!(
                worker_id = %worker_id,
                "features already fixed for this connection; ignoring a later declaration"
            );
        }

        // A CLI command connects only long enough to do its work, so it cannot
        // wait for a heartbeat to report itself. It marks the name it registers
        // with, and the engine counts it for the next heartbeat instead.
        if let Some(command) = input.name.as_deref().and_then(|name| {
            name.strip_prefix(crate::workers::telemetry::collector::CLI_WORKER_NAME_PREFIX)
        }) {
            crate::workers::telemetry::collector::track_cli_command(command);
        }

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
        features_fixed
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
        Ok(Box::new(EngineFunctionsWorker { engine }))
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing EngineFunctionsWorker");

        crate::observer::events::register_engine_event_sources(&self.engine);

        // Call `engine::on-functions-changed` from the registry itself, with no
        // polling task. A burst of changes (a worker registering its whole
        // surface on connect) is coalesced into one event: the first change
        // schedules a one-shot send, later ones ride along until it runs.
        // Weak: the registry lives inside the engine, so a strong handle here
        // would keep the engine alive forever.
        let engine = Arc::downgrade(&self.engine);
        // Captured here (initialize always runs inside the runtime) rather than
        // looked up per change: the registry's register/remove are synchronous
        // and public, so a change can come from a thread with no current
        // runtime, and it must still produce an event.
        let runtime = tokio::runtime::Handle::current();
        // Whether a send is scheduled, and the deepest observe depth among the
        // changes it carries: a change made inside an activation (an
        // in-process registration, or one a worker made while handling it) is
        // part of that chain, and so is the event
        // `engine::on-functions-changed` observers get for it. One lock, so a
        // change's depth goes with the event that carries the change.
        let burst = Arc::new(std::sync::Mutex::new((false, 0u32)));
        self.engine.functions.set_change_listener(Arc::new(
            move |_change: crate::function::FunctionChange,
                  _namespace: &str,
                  _function_id: &str| {
                {
                    let mut burst = burst.lock().unwrap_or_else(|e| e.into_inner());
                    burst.1 = burst.1.max(crate::observer::InvocationCtx::current_depth());
                    if std::mem::replace(&mut burst.0, true) {
                        return;
                    }
                }
                let engine = engine.clone();
                let burst = burst.clone();
                runtime.spawn(async move {
                    tokio::time::sleep(FUNCTIONS_CHANGED_COALESCE).await;
                    // Clear before snapshotting: a change landing from here on
                    // schedules its own event, so none is lost.
                    let depth = {
                        let mut burst = burst.lock().unwrap_or_else(|e| e.into_inner());
                        burst.0 = false;
                        std::mem::take(&mut burst.1)
                    };
                    let Some(engine) = engine.upgrade() else {
                        return;
                    };
                    // Checked after coalescing, so an observer added during the
                    // delay still gets the change.
                    if !engine.observers.is_observed(
                        crate::protocol::DEFAULT_NAMESPACE,
                        crate::observer::events::ON_FUNCTIONS_CHANGED,
                    ) {
                        return;
                    }
                    let payload = EngineFunctionsWorker::new(engine.clone())
                        .functions_changed_payload()
                        .await;
                    engine.observers.emit_with(
                        crate::protocol::DEFAULT_NAMESPACE,
                        crate::observer::events::ON_FUNCTIONS_CHANGED,
                        crate::observer::Emission {
                            depth: Some(depth),
                            traceparent: None,
                        },
                        || Some(payload),
                    );
                });
            },
        ));

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
        let owner = input
            .caller_worker_id
            .as_deref()
            .and_then(|id| uuid::Uuid::parse_str(id).ok());
        let (writer_ref, reader_ref) = channel_mgr.create_channel(buffer_size, owner);

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
            // dlq_topics / dlq_messages) and the read side of observability
            // (`DISCOVERABLE_ENGINE_FUNCTIONS`). Those are public APIs a client
            // legitimately needs to discover — keeping them out of the default
            // list left agents unable to find the DLQ-inspection surface, and
            // shelling out to the CLI to read traces `functions::info` would
            // happily describe.
            // Also hide any handler explicitly tagged `metadata.internal == true`
            // (e.g. `iii-observability::ui-content`, `iii-http::on-config-change`).
            // These are targets the engine and its workers invoke by id — never
            // meant for discovery.
            functions.retain(|f| {
                let is_engine_internal = f.function_id.starts_with("engine::")
                    && !f.function_id.starts_with("engine::queue::")
                    && !DISCOVERABLE_ENGINE_FUNCTIONS.contains(&f.function_id.as_str());
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
        description = "Inspect functions: schemas, owner, args_format, private, observable and \
                       observer_count. Single `function_id`, or `function_ids: [...]` (max 32) \
                       to fetch several contracts in one call."
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
        description = "Inspect one connected worker's full surface (functions, and the ids of the observers it created)."
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
        let mut input = input;
        let worker_id = input.worker_id.clone();

        // Refused rather than read as absent. This is the namespace the whole
        // connection inherits -- its lease, its functions, and everything it
        // calls or binds afterwards -- so getting it wrong here is not a local
        // mistake.
        if crate::protocol::is_blank_namespace(&input.namespace) {
            return FunctionResult::Failure(ErrorBody {
                code: crate::protocol::INVALID_NAMESPACE.into(),
                message: format!(
                    "namespace is empty: worker '{}' declared {:?}. Give it a name, or omit \
                     the field to register in '{}'.",
                    input.name.as_deref().unwrap_or("<unnamed>"),
                    input.namespace.as_deref().unwrap_or_default(),
                    crate::protocol::DEFAULT_NAMESPACE
                ),
                stacktrace: None,
            });
        }

        let declared = crate::protocol::effective_namespace(&input.namespace).to_string();

        // The grace timer fixes a connection to `default` when no
        // `workers::register` arrives in time. If it already fired, the buffered
        // functions have drained into that fixed namespace — so a *late*
        // `workers::register` declaring a different namespace cannot be honored
        // without splitting state (functions in one namespace, lease + metadata
        // in another). Bind the lease, metadata, and drain to the namespace the
        // connection was actually fixed to, and warn on a mismatch. While the
        // namespace is still `Pending` (the normal, in-time case) this is `None`
        // and the declared namespace is used verbatim.
        let already_fixed = uuid::Uuid::parse_str(&worker_id)
            .ok()
            .and_then(|uuid| self.engine.already_fixed_namespace(&uuid));
        let namespace_was_fixed = already_fixed.is_some();
        let namespace = match already_fixed {
            Some(fixed) if fixed != declared => {
                tracing::warn!(
                    worker_id = %worker_id,
                    declared = %declared,
                    fixed = %fixed,
                    "late engine::workers::register: connection namespace already fixed (grace timer); \
                     honoring the fixed namespace so lease, metadata, and functions stay in one namespace"
                );
                fixed
            }
            Some(fixed) => fixed,
            None => declared,
        };
        // Keep the stored metadata's namespace consistent with the lease above.
        input.namespace = Some(namespace.clone());

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
            // the loser leaves no trace. Cancel the grace timer first so it can
            // never fire after the reject and drain this worker's buffered
            // registrations into `default`.
            self.engine.cancel_namespace_grace_timer(&uuid);
            self.engine.reject_worker_registration(uuid, conflict).await;
            return FunctionResult::Success(RegisterWorkerResult { success: false });
        }

        let features_fixed = self.register_worker_metadata(input).await;

        // Registrations that drained before this connection declared its
        // features were handled as a legacy connection's; the observer
        // registry catches up on them. The grace timer may have fixed the
        // namespace (and drained) while this call ran, so it is read again
        // now that the features are stored: from here on a drain announces
        // its registrations itself, and one announced twice is sent once.
        let fixed_now = uuid::Uuid::parse_str(&worker_id)
            .ok()
            .and_then(|uuid| self.engine.already_fixed_namespace(&uuid));
        if features_fixed
            && (namespace_was_fixed || fixed_now.is_some())
            && let Ok(uuid) = uuid::Uuid::parse_str(&worker_id)
            && let Some(worker) = self.engine.worker_registry.get_worker(&uuid)
        {
            let fixed = fixed_now.unwrap_or_else(|| namespace.clone());
            self.engine
                .observers
                .features_declared_late(&self.engine, &worker, &fixed)
                .await;
        }

        // This call is the only moment the connection's namespace becomes
        // known. Registrations the worker sent ahead of it are queued on the
        // connection; release them now, into `namespace`.
        if let Ok(uuid) = uuid::Uuid::parse_str(&worker_id)
            && let Some(worker) = self.engine.worker_registry.get_worker(&uuid)
        {
            self.engine
                .resolve_connection_namespace(&worker, &namespace)
                .await;
            // Namespace is fixed by this in-time announce; the grace timer is no
            // longer needed.
            self.engine.cancel_namespace_grace_timer(&uuid);
        }

        self.engine.observers.emit(
            crate::protocol::DEFAULT_NAMESPACE,
            crate::observer::events::ON_WORKERS_CHANGED,
            || {
                Some(serde_json::json!({
                    "event": "worker_metadata_updated",
                    "worker_id": worker_id,
                }))
            },
        );

        FunctionResult::Success(RegisterWorkerResult { success: true })
    }

    #[function(
        id = "engine::observers::list",
        description = "List observers (iii.observe): function, args, filters, target, observing worker, state, reason, fires and drops by reason. Only observers of functions the caller may invoke are listed."
    )]
    pub async fn observers_list(
        &self,
        input: crate::observer::builtins::ListObserversInput,
        session: Option<Arc<Session>>,
    ) -> FunctionResult<crate::observer::builtins::ObserversListResult, ErrorBody> {
        FunctionResult::Success(self.engine.observers.list_infos(
            &self.engine,
            session.as_ref(),
            &input,
        ))
    }

    #[function(
        id = "engine::observers::get",
        description = "Get one observer by id. OBSERVER_NOT_FOUND when it does not exist or its function is not visible to the caller."
    )]
    pub async fn observers_get(
        &self,
        input: crate::observer::builtins::ObserverIdInput,
        session: Option<Arc<Session>>,
    ) -> FunctionResult<crate::observer::builtins::ObserverInfo, ErrorBody> {
        match self
            .engine
            .observers
            .get_info(&self.engine, session.as_ref(), &input.id)
        {
            Ok(info) => FunctionResult::Success(info),
            Err(err) => FunctionResult::Failure(err),
        }
    }

    #[function(
        id = "engine::observers::dispose",
        description = "Dispose an observer by id. Allowed for the worker that created it, or for a caller no RBAC restricts."
    )]
    pub async fn observers_dispose(
        &self,
        input: crate::observer::builtins::ObserverIdInput,
        session: Option<Arc<Session>>,
    ) -> FunctionResult<crate::observer::builtins::DisposeObserverResult, ErrorBody> {
        match self
            .engine
            .observers
            .dispose_by_caller(&self.engine, session.as_ref(), &input)
        {
            Ok(result) => FunctionResult::Success(result),
            Err(err) => FunctionResult::Failure(err),
        }
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
    fn register_worker_input_reads_features() {
        let json = serde_json::json!({
            "_caller_worker_id": "550e8400-e29b-41d4-a716-446655440000",
            "features": ["observe.v1", "local-dispatch.v1"]
        });
        let input: RegisterWorkerInput = serde_json::from_value(json).expect("deserialize");
        assert_eq!(input.features, vec!["observe.v1", "local-dispatch.v1"]);
    }

    /// A pre-0.25 SDK sends no features; a malformed list costs the worker its
    /// features, never its registration.
    #[test]
    fn register_worker_input_without_or_with_malformed_features_still_deserializes() {
        for features in [
            None,
            Some(serde_json::json!("observe.v1")),
            Some(serde_json::json!(7)),
        ] {
            let mut json = serde_json::json!({
                "_caller_worker_id": "550e8400-e29b-41d4-a716-446655440000",
                "namespace": "orders"
            });
            if let Some(features) = features {
                json["features"] = features;
            }
            let input: RegisterWorkerInput = serde_json::from_value(json).expect("deserialize");
            assert!(input.features.is_empty());
            assert_eq!(input.namespace.as_deref(), Some("orders"));
        }
    }

    #[test]
    fn empty_input_deserializes() {
        let json = serde_json::json!({});
        let _input: EmptyInput = serde_json::from_value(json).expect("deserialize");
    }

    #[test]
    fn input_defaults_round_trip() {
        let _: FunctionsListInput = serde_json::from_value(serde_json::json!({})).unwrap();
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
            features: Vec::new(),
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
            features: Vec::new(),
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

    /// The features ride on the registry's copy of the connection, but every
    /// clone shares them: `router_msg` holds the connect-time snapshot.
    #[tokio::test]
    async fn register_worker_metadata_records_the_declared_features() {
        let (engine, module) = setup_engine_and_module();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let worker = crate::worker_connections::WorkerConnection::new(tx);
        let snapshot = worker.clone();
        engine.worker_registry.register_worker(worker);
        assert!(!snapshot.features.observe_v1());

        let mut input = register_worker_input_with_namespace(&snapshot.id.to_string(), None);
        input.features = vec![
            "observe.v1".to_string(),
            "local-dispatch.v1".to_string(),
            "telepathy.v9".to_string(),
        ];
        module.register_worker_metadata(input).await;

        assert!(snapshot.features.observe_v1());
        assert!(snapshot.features.local_dispatch_v1());
        assert_eq!(
            snapshot.features.declared(),
            vec!["observe.v1", "local-dispatch.v1", "telepathy.v9"],
            "unknown features are kept as declared"
        );
    }

    #[tokio::test]
    async fn register_worker_metadata_without_features_leaves_the_connection_legacy() {
        let (engine, module) = setup_engine_and_module();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let worker = crate::worker_connections::WorkerConnection::new(tx);
        let snapshot = worker.clone();
        engine.worker_registry.register_worker(worker);

        module
            .register_worker_metadata(register_worker_input_with_namespace(
                &snapshot.id.to_string(),
                None,
            ))
            .await;

        assert!(!snapshot.features.observe_v1());
        assert!(!snapshot.features.local_dispatch_v1());
        assert!(snapshot.features.declared().is_empty());
    }

    /// A Rust worker reports its own crate version: the `large-messages.v1`
    /// stored by the same call sets the 64 MiB receive limit. Features stay
    /// fixed, so a later call without them keeps it.
    #[tokio::test]
    async fn register_worker_metadata_sizes_the_receive_limit_by_the_declared_features() {
        use std::sync::atomic::Ordering;

        let (engine, module) = setup_engine_and_module();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let worker = crate::worker_connections::WorkerConnection::new(tx);
        let snapshot = worker.clone();
        engine.worker_registry.register_worker(worker);

        let mut input = register_worker_input_with_namespace(&snapshot.id.to_string(), None);
        input.runtime = Some("rust".to_string());
        input.version = Some("0.3.0".to_string());
        input.features = vec![
            "observe.v1".to_string(),
            "local-dispatch.v1".to_string(),
            "large-messages.v1".to_string(),
        ];
        module.register_worker_metadata(input.clone()).await;
        assert_eq!(
            snapshot.receive_message_limit.load(Ordering::Relaxed),
            crate::workers::worker::MAX_JSON_MESSAGE_BYTES
        );
        assert!(
            !snapshot
                .receive_limit_requires_upgrade
                .load(Ordering::Relaxed)
        );

        input.features = Vec::new();
        module.register_worker_metadata(input).await;
        assert_eq!(
            snapshot.receive_message_limit.load(Ordering::Relaxed),
            crate::workers::worker::MAX_JSON_MESSAGE_BYTES,
            "the features stay fixed"
        );
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
            features: Vec::new(),
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
                    args_format: None,
                    private: false,
                    observable: true,
                    observe: None,
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

    /// A *late* `engine::workers::register` — arriving after the grace timer
    /// already fixed the connection to `default` and drained its buffered
    /// functions there — must not split state. The declared namespace is ignored
    /// in favor of the already-fixed one, so functions, the name lease, and the
    /// stored metadata namespace all stay in `default`.
    #[tokio::test(start_paused = true)]
    async fn late_register_worker_honors_the_grace_fixed_namespace() {
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
                    args_format: None,
                    private: false,
                    observable: true,
                    observe: None,
                },
            )
            .await
            .expect("buffer register");

        // Grace expires: the connection is fixed to `default` and the buffered
        // function drains there. Paused time auto-advances once nothing else is
        // runnable, so this resumes strictly after the grace drain completes.
        tokio::time::sleep(
            crate::engine::REGISTRATION_NAMESPACE_GRACE + std::time::Duration::from_secs(1),
        )
        .await;
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "orders::create")
                .is_some(),
            "grace drain must land the function in default"
        );

        // Late announce declaring a *different* namespace, with a name so the
        // lease is exercised.
        let mut input = register_worker_input_with_namespace(
            &worker_uuid.to_string(),
            Some("orders".to_string()),
        );
        input.name = Some("state".to_string());
        module.register_worker(input).await;

        // No split: the stored metadata namespace is the grace-fixed `default`,
        // not the late-declared `orders`.
        assert_eq!(
            engine.worker_registry.get_namespace(&worker_uuid),
            crate::protocol::DEFAULT_NAMESPACE,
            "metadata namespace must follow the grace-fixed namespace, not the late declaration"
        );

        // The name lease sits in `default`, never in `orders`: a fresh worker can
        // still claim `state` in `orders`, but not in `default`.
        let other = uuid::Uuid::new_v4();
        assert!(
            engine.claim_worker_name("orders", other, "state").is_ok(),
            "no lease should have leaked into the declared `orders` namespace"
        );
        let other2 = uuid::Uuid::new_v4();
        assert!(
            engine
                .claim_worker_name(crate::protocol::DEFAULT_NAMESPACE, other2, "state")
                .is_err(),
            "the name must be leased in the grace-fixed `default` namespace"
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
                    args_format: None,
                    private: false,
                    observable: true,
                    observe: None,
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

    // ── Serialization tests ─────────────────────────────────────────────

    #[test]
    fn function_summary_serializes() {
        let summary = FunctionSummary {
            function_id: "my::func".to_string(),
            namespace: crate::protocol::DEFAULT_NAMESPACE.to_string(),
            worker_name: "my".to_string(),
            description: Some("desc".to_string()),
            metadata: None,
            observe: FunctionObserveInfo::default(),
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
            observe: FunctionObserveInfo::default(),
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
            observe: FunctionObserveInfo::default(),
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
            observe: FunctionObserveInfo::default(),
        };
        let json = serde_json::to_value(&with).expect("serialize");
        assert_eq!(json["metadata"]["internal"], true);
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

    /// `engine::on-functions-changed` is called from the registry itself (no
    /// polling task) with `functions_changed` and the full list, and folds a
    /// burst of registrations into one call.
    #[tokio::test]
    async fn functions_changed_fires_on_change_with_full_list() {
        let (engine, module) = setup_engine_and_module();
        module.initialize().await.unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::on_fns".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(move |input: Value| {
                let tx = tx.clone();
                async move {
                    let _ = tx.send(input);
                    FunctionResult::Success(None)
                }
            }),
        );
        observe_source(
            &engine,
            crate::observer::events::ON_FUNCTIONS_CHANGED,
            "test::on_fns",
        );

        fn ids(event: &Value) -> Vec<String> {
            event["functions"]
                .as_array()
                .expect("functions array")
                .iter()
                .filter_map(|f| f["function_id"].as_str().map(str::to_string))
                .collect()
        }
        // The handler's own registration also schedules an event; wait for
        // the one whose list matches `want`.
        async fn next_matching(
            rx: &mut tokio::sync::mpsc::UnboundedReceiver<Value>,
            want: impl Fn(&[String]) -> bool,
        ) -> Value {
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                loop {
                    let event = rx.recv().await.expect("channel closed");
                    if want(&ids(&event)) {
                        return event;
                    }
                }
            })
            .await
            .expect("engine::on-functions-changed must be called")
        }

        for id in ["burst::a", "burst::b", "burst::c"] {
            register_simple_function(&engine, id, None);
        }
        let registered = next_matching(&mut rx, |ids| {
            ["burst::a", "burst::b", "burst::c"]
                .iter()
                .all(|id| ids.iter().any(|i| i == id))
        })
        .await;
        assert_eq!(registered["event"], "functions_changed");
        // The burst was folded into that single event.
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(300), rx.recv())
                .await
                .is_err(),
            "a burst of registrations must yield a single event"
        );

        engine
            .functions
            .remove(crate::protocol::DEFAULT_NAMESPACE, "burst::b");
        let removed = next_matching(&mut rx, |ids| !ids.iter().any(|i| i == "burst::b")).await;
        assert_eq!(removed["event"], "functions_changed");
    }

    /// A registry change made from a thread with no current Tokio runtime
    /// (register/remove are synchronous and public) must still produce an
    /// `engine::on-functions-changed` event: the listener spawns on the handle
    /// captured at `initialize`, not on a per-change lookup.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn functions_changed_fires_for_changes_outside_the_runtime() {
        let (engine, module) = setup_engine_and_module();
        module.initialize().await.unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::on_fns".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(move |input: Value| {
                let tx = tx.clone();
                async move {
                    let _ = tx.send(input);
                    FunctionResult::Success(None)
                }
            }),
        );
        observe_source(
            &engine,
            crate::observer::events::ON_FUNCTIONS_CHANGED,
            "test::on_fns",
        );

        let off_runtime = engine.clone();
        std::thread::spawn(move || {
            assert!(
                tokio::runtime::Handle::try_current().is_err(),
                "the change must come from a thread without a runtime"
            );
            register_simple_function(&off_runtime, "offrt::fn", None);
        })
        .join()
        .unwrap();

        let event = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let event = rx.recv().await.expect("channel closed");
                let has = event["functions"]
                    .as_array()
                    .is_some_and(|fs| fs.iter().any(|f| f["function_id"] == "offrt::fn"));
                if has {
                    return event;
                }
            }
        })
        .await
        .expect("a change made off the runtime must still call engine::on-functions-changed");
        assert_eq!(event["event"], "functions_changed");
    }

    // ── create_channel test ─────────────────────────────────────────────

    #[tokio::test]
    async fn create_channel_returns_channel_refs() {
        let (engine, module) = setup_engine_and_module();
        let result = module
            .create_channel(CreateChannelInput {
                buffer_size: Some(2048),
                ..Default::default()
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
    async fn functions_list_keeps_read_only_observability_discoverable() {
        let (engine, module) = setup_engine_and_module();

        for function_id in [
            "engine::traces::list",
            "engine::traces::tree",
            "engine::logs::list",
            "engine::traces::clear",
            "engine::logs::clear",
            "engine::functions::list",
        ] {
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
                let mut ids: Vec<&str> = result
                    .functions
                    .iter()
                    .map(|f| f.function_id.as_str())
                    .collect();
                ids.sort_unstable();
                assert_eq!(
                    ids,
                    vec![
                        "engine::logs::list",
                        "engine::traces::list",
                        "engine::traces::tree"
                    ],
                    "read side visible, clear + plumbing hidden"
                );
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

        engine.observers.set_options(
            crate::protocol::DEFAULT_NAMESPACE,
            "test::detailed",
            FunctionOptions {
                args_format: Some(serde_json::json!({ "type": "object" })),
                private: true,
                observable: true,
            },
        );
        register_simple_function(&engine, "test::sink", None);
        observe_source(&engine, "test::detailed", "test::sink");

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
                assert_eq!(
                    detail.observe,
                    FunctionObserveInfo {
                        args_format: Some(serde_json::json!({ "type": "object" })),
                        private: true,
                        observable: None,
                        observer_count: 1,
                    }
                );
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
                assert!(
                    detail.observers.is_empty(),
                    "an in-process worker observes nothing"
                );
                assert!(detail.worker.internal == false);
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

    /// Worker-list fixtures mutate the same global metrics store as OTLP
    /// ingestion tests; serialize the clear/write/read sequence with them.
    #[tokio::test]
    #[serial_test::serial]
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
    async fn register_worker_service_calls_on_workers_changed() {
        let (engine, module) = setup_engine_and_module();
        module.initialize().await.unwrap();

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

        observe_source(
            &engine,
            crate::observer::events::ON_WORKERS_CHANGED,
            "test::worker_listener",
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
                features: Vec::new(),
            })
            .await;
        assert!(matches!(
            result,
            FunctionResult::Success(RegisterWorkerResult { success: true })
        ));

        let payload = tokio::time::timeout(std::time::Duration::from_secs(2), rx)
            .await
            .expect("timed out waiting for engine::on-workers-changed")
            .expect("listener channel closed");
        assert_eq!(payload["event"], "worker_metadata_updated");
        assert_eq!(payload["worker_id"], worker_id);
    }

    #[tokio::test]
    async fn destroy_returns_ok() {
        let (_engine, module) = setup_engine_and_module();
        module.destroy().await.unwrap();
    }

    /// Function rows carry the registration's options (the defaults left
    /// out) and the observer count, in `engine::functions::list` and in the
    /// `engine::on-functions-changed` event; `engine::workers::info` lists
    /// the ids of the observers a worker created.
    #[tokio::test]
    async fn function_rows_carry_options_and_observer_counts() {
        let (engine, module) = setup_engine_and_module();
        register_simple_function(&engine, "plain::fn", None);
        engine.observers.set_options(
            crate::protocol::DEFAULT_NAMESPACE,
            "vault::read",
            FunctionOptions {
                args_format: None,
                private: false,
                observable: false,
            },
        );
        register_simple_function(&engine, "vault::read", None);

        let rows = module.list_function_summaries().await;
        let row = |id: &str| {
            let row = rows.iter().find(|row| row.function_id == id).unwrap();
            serde_json::to_value(row).unwrap()
        };
        let plain = row("plain::fn");
        assert_eq!(plain["observer_count"], 0);
        for key in ["args_format", "private", "observable"] {
            assert!(plain.get(key).is_none(), "{key}: {plain}");
        }
        assert_eq!(row("vault::read")["observable"], false);

        let event = module.functions_changed_payload().await;
        let vault = event["functions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["function_id"] == "vault::read")
            .unwrap();
        assert_eq!(vault["observable"], false);
        assert_eq!(vault["observer_count"], 0);
    }

    /// `engine::workers::info` lists the ids of the observers the worker
    /// created.
    #[tokio::test]
    async fn workers_info_lists_the_observers_a_worker_created() {
        let (engine, module) = setup_engine_and_module();
        register_simple_function(&engine, "orders::create", None);
        register_simple_function(&engine, "alerts::big-order", None);
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        let worker = crate::worker_connections::WorkerConnection::new(tx);
        let worker_id = worker.id.to_string();
        engine.worker_registry.register_worker(worker.clone());
        let mut registration = register_worker_input_with_namespace(&worker_id, None);
        registration.name = Some("orders-worker".to_string());
        module.register_worker_metadata(registration).await;
        engine
            .observers
            .observe(
                &engine,
                &worker,
                crate::observer::events::test_support::request(
                    "obs-big-order",
                    "orders::create",
                    "alerts::big-order",
                    None,
                ),
            )
            .await;

        match module
            .workers_info(WorkerInfoInput {
                name: "orders-worker".to_string(),
                namespace: None,
            })
            .await
        {
            FunctionResult::Success(detail) => {
                assert_eq!(detail.observers, vec!["obs-big-order".to_string()]);
            }
            _ => panic!("expected workers_info success"),
        }
    }

    /// An engine-owned observer of `source` delivering its input to the
    /// in-process function `target`.
    fn observe_source(engine: &Arc<Engine>, source: &str, target: &str) {
        let default = crate::protocol::DEFAULT_NAMESPACE.to_string();
        engine
            .observers
            .observe_as_engine(crate::observer::EngineObserver {
                id: format!("test-observer:{target}"),
                function: (default.clone(), source.to_string()),
                args: serde_json::json!({}),
                filters: None,
                target: (default, target.to_string()),
                deliver: crate::protocol::Deliver::Input,
                metadata: None,
            })
            .expect("observe the source");
    }
}
