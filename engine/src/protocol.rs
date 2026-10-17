// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::invocation::{auth::HttpAuthConfig, method::HttpMethod};

/// Namespace used when a message carries no explicit `namespace`.
pub const DEFAULT_NAMESPACE: &str = "default";

/// Returns the effective namespace for an optional wire-level namespace,
/// falling back to [`DEFAULT_NAMESPACE`] when absent.
pub fn effective_namespace(ns: &Option<String>) -> &str {
    ns.as_deref().unwrap_or(DEFAULT_NAMESPACE)
}

/// [`DEFAULT_NAMESPACE`] as an owned `String`. Used as a `#[serde(default)]`
/// for owned namespace fields so payloads that predate the field decode to
/// `default` rather than failing.
pub fn default_namespace() -> String {
    DEFAULT_NAMESPACE.to_string()
}

/// [`Message::RegistrationRejected`] code: another live worker already runs
/// under this name in this namespace.
///
/// Defined, but **not emitted yet** — deferred to Task 9.5, after the SDKs learn
/// to handle `RegistrationRejected`. Rejecting a worker means closing its
/// connection, and today's SDKs would simply reconnect and be rejected again,
/// forever. See `.superpowers/sdd/task-4-report.md` for the `hostname:pid`
/// naming problem this code has to solve before it can ship.
pub const WORKER_NAMESPACE_CONFLICT: &str = "WORKER_NAMESPACE_CONFLICT";

/// [`Message::RegistrationRejected`] code: another live worker in this
/// namespace already exports this function id. Only that one registration is
/// refused; the connection stays open.
pub const FUNCTION_NAMESPACE_CONFLICT: &str = "FUNCTION_NAMESPACE_CONFLICT";

/// [`Message::RegistrationRejected`] code: the worker declared a namespace its
/// session is not scoped to. Fatal: the engine closes the connection. SDKs
/// announce themselves with a fire-and-forget `engine::workers::register`, so
/// an invocation error alone would go unread and leave the worker serving from
/// `default`.
pub const NAMESPACE_FORBIDDEN: &str = "NAMESPACE_FORBIDDEN";

/// A namespace field was present and held nothing but whitespace.
pub const INVALID_NAMESPACE: &str = "INVALID_NAMESPACE";

/// Whether a namespace field was named and left empty.
///
/// Absent and blank are opposite intents. Absent asks for [`DEFAULT_NAMESPACE`];
/// blank names a namespace and gives nothing to name it with. Reading blank as
/// absent puts the worker in `default` — somewhere it never asked for, where
/// every call and trigger it makes then follows it, and the one place an
/// operator cannot see by reading the declaration. Filing it under the empty
/// string is worse still: a namespace nobody can address or type.
///
/// The SDKs refuse this at construction, each in its own way. The engine
/// refuses it again because it is the only party every client goes through, and
/// because an SDK that forgets (or a hand-rolled client) must not be able to
/// create a namespace with no name.
pub fn is_blank_namespace(ns: &Option<String>) -> bool {
    ns.as_deref().is_some_and(|ns| ns.trim().is_empty())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpInvocationRef {
    pub url: String,
    #[serde(default = "default_http_method")]
    pub method: HttpMethod,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub auth: Option<HttpAuthConfig>,
}

fn default_http_method() -> HttpMethod {
    HttpMethod::Post
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TriggerAction {
    Enqueue { queue: String },
    Void,
}

/// Reads an optional field leniently: `null` or a value that does not decode
/// as `T` reads as absent (logged at debug) instead of failing the whole
/// message. For the fields observe adds to existing messages, where a peer
/// getting one wrong must not lose the call it rides on.
fn deserialize_lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = Value::deserialize(deserializer)?;
    if value.is_null() {
        return Ok(None);
    }
    match serde_json::from_value(value) {
        Ok(decoded) => Ok(Some(decoded)),
        Err(err) => {
            tracing::debug!(error = %err, "ignoring a malformed optional field");
            Ok(None)
        }
    }
}

/// Reads `null` as the field's default, like an absent field: the observe
/// fixtures' rule that every optional field also accepts `null`.
fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Option::unwrap_or_default)
}

/// Deserialize a present field as `Some(value)` even when the value is `null`.
/// Pair with `#[serde(default)]` so an absent field still reads as `None`.
fn deserialize_present<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Value::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Message {
    /// Worker→engine. Every SDK has always sent this when a provider retires a
    /// trigger type; until 0.25 no engine decoded it and the frame was dropped
    /// with a decode warning. The engine removes the type only when the sending
    /// connection is its provider.
    UnregisterTriggerType {
        id: String,
        /// Namespace the provider was registered in. Absent means the sending
        /// connection's namespace, mirroring `RegisterTriggerType`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
    },
    RegisterTriggerType {
        id: String,
        description: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        trigger_request_format: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        call_request_format: Option<Value>,
        /// Namespace this provider serves. Absent means the registering
        /// connection's namespace, which is what every SDK wants and what
        /// keeps two projects providing the same type id from overwriting each
        /// other. In-process engine providers register in
        /// [`DEFAULT_NAMESPACE`], where the resolution in
        /// [`crate::trigger::TriggerRegistry`] falls back to find them.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
    },
    RegisterTrigger {
        id: String,
        trigger_type: String,
        function_id: String,
        config: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
        /// Namespace the trigger's target function resolves in. Optional and
        /// additive: absent means the engine's default namespace, independent
        /// of the registering connection's namespace. Older peers that never
        /// send it stay wire-compatible.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        /// Namespace to find `trigger_type`'s provider in.
        ///
        /// Absent is not "default": it asks the engine to resolve, taking the
        /// registering connection's namespace first and [`DEFAULT_NAMESPACE`]
        /// second. That is what lets a project ship its own provider for a
        /// type id the engine also provides, while every worker that has not
        /// been migrated keeps reaching the engine's one without saying so.
        /// Naming a namespace here is strict: that namespace or nothing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trigger_namespace: Option<String>,
    },
    TriggerRegistrationResult {
        id: String,
        trigger_type: String,
        function_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<ErrorBody>,
    },
    UnregisterTrigger {
        id: String,
        #[serde(default)]
        trigger_type: Option<String>,
    },
    RegisterFunction {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_format: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response_format: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        invocation: Option<HttpInvocationRef>,
        /// How observers of this function are delivered (protocol.md §2.2).
        /// Absent means `delivery: auto`, observable, no args. The SDK-local
        /// callbacks never travel. Boxed like `Observe.source`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        observe: Option<Box<ObserveDescriptor>>,
    },
    UnregisterFunction {
        id: String,
    },
    InvokeFunction {
        /// Absent on a fire-and-forget (`void`) call. The engine always sends
        /// one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        invocation_id: Option<Uuid>,
        function_id: String,
        data: Value,
        /// W3C trace-context traceparent header for distributed tracing
        #[serde(skip_serializing_if = "Option::is_none")]
        traceparent: Option<String>,
        /// W3C baggage header for cross-cutting context propagation
        #[serde(skip_serializing_if = "Option::is_none")]
        baggage: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        action: Option<TriggerAction>,
        /// Per-invocation metadata sidecar, delivered to the target handler as a
        /// distinct argument (not folded into `data`). Optional and additive:
        /// omitted when absent, so older peers that don't send/expect it stay
        /// wire-compatible.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
        /// Target namespace for routing. Optional and additive: absent means
        /// [`DEFAULT_NAMESPACE`], so older peers that don't send it stay
        /// wire-compatible.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        /// Who made the call (protocol.md §2.4). Worker→engine: the handler
        /// the call was made from, asserted by the SDK. Engine→worker: also
        /// `worker_id` / `worker_name`, stamped from the calling connection.
        /// Boxed so the variant stays small on the hot path.
        ///
        /// The three observe fields are read leniently: a malformed value is
        /// treated as absent rather than failing the whole call.
        #[serde(
            default,
            deserialize_with = "deserialize_lenient",
            skip_serializing_if = "Option::is_none"
        )]
        caller: Option<Box<Caller>>,
        /// Depth of the causal chain in observe hops. Absent means 0.
        #[serde(
            default,
            deserialize_with = "deserialize_lenient",
            skip_serializing_if = "Option::is_none"
        )]
        observe_depth: Option<u32>,
        /// Engine→worker: present on every observer activation.
        /// Worker→engine: marks an owner delivery (`obs.call`).
        #[serde(
            default,
            deserialize_with = "deserialize_lenient",
            skip_serializing_if = "Option::is_none"
        )]
        activation: Option<Activation>,
    },
    InvocationResult {
        invocation_id: Uuid,
        function_id: String,
        /// The executor's return value. `"result": null` (returned null) and
        /// an absent field (returned nothing) are different answers, and the
        /// caller distinguishes them: `state::get` on a miss is `null`.
        #[serde(
            default,
            deserialize_with = "deserialize_present",
            skip_serializing_if = "Option::is_none"
        )]
        result: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<ErrorBody>,
        /// W3C trace-context traceparent header for distributed tracing
        #[serde(skip_serializing_if = "Option::is_none")]
        traceparent: Option<String>,
        /// W3C baggage header for cross-cutting context propagation
        #[serde(skip_serializing_if = "Option::is_none")]
        baggage: Option<String>,
    },
    RegisterService {
        id: String,
        #[serde(default)]
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_service_id: Option<String>,
    },
    Ping,
    Pong,
    /// Worker→engine, sent as the first message of a reconnect, before the
    /// registration replay: `previous_worker_id` and `reattach_token` are
    /// the values the engine handed this worker via `WorkerRegistered` on
    /// its previous connection. The engine retires that previous connection
    /// (same teardown as a disconnect) so the replay lands on a clean slate
    /// instead of racing the old connection's cleanup. The token is
    /// required: worker ids are publicly discoverable, the token was only
    /// ever sent over the previous connection's own socket. Old engines
    /// warn-log and ignore unknown message types, so this is version-skew
    /// safe.
    Reattach {
        previous_worker_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reattach_token: Option<String>,
    },
    WorkerRegistered {
        worker_id: String,
        /// Secret the worker must present in `Reattach` to retire this
        /// connection after a reconnect. Optional on the wire so older
        /// SDKs/engines interop cleanly.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reattach_token: Option<String>,
        /// Protocol features this engine supports ([`ENGINE_FEATURES`]).
        /// Absent from engines older than 0.25.
        #[serde(
            default,
            deserialize_with = "deserialize_features",
            skip_serializing_if = "Vec::is_empty"
        )]
        features: Vec<String>,
    },
    RegistrationRejected {
        code: String,
        namespace: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        worker_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        function_id: Option<String>,
        owner_worker_id: String,
    },

    // ── observe (protocol.md §2). Sent by the engine only to connections that
    // declared `observe.v1`; decoded from anyone. ──
    /// Observing worker → engine: create, or idempotently replay, an observer
    /// owned by the sending connection.
    Observe {
        /// Client-generated, stable across replays.
        id: String,
        /// The left side of `iii.observe`: what is observed. Boxed: a rare
        /// control message should not size every `Message`.
        source: Box<ObserveSource>,
        /// The right side: the function each activation invokes.
        target: ObserveTarget,
        /// The activation's metadata sidecar. Never shown to the owner.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_fires: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires_in_ms: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_in_flight: Option<u32>,
    },
    /// Observing worker → engine: dispose an observer it owns.
    Unobserve {
        id: String,
    },
    /// Engine → observing worker: the reply to `observe` (also on replay).
    ObserveResult {
        id: String,
        state: ObserverState,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<ErrorBody>,
        #[serde(
            default,
            deserialize_with = "deserialize_null_default",
            skip_serializing_if = "Vec::is_empty"
        )]
        warnings: Vec<String>,
    },
    /// Engine → observing worker: every lifecycle transition.
    ObserverStateChanged {
        id: String,
        state: ObserverState,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<ErrorBody>,
        #[serde(default)]
        fires: u64,
    },
    /// Engine → owner: the `RegisterFunction` success ack, with the full current
    /// observer set. Sent once per registration (and after a re-registration).
    FunctionRegistered {
        id: String,
        #[serde(default = "default_namespace")]
        namespace: String,
        revision: u64,
        /// Whether the SDK may run this function in-process.
        #[serde(default)]
        local_dispatch: bool,
        #[serde(default)]
        observers: Vec<ObserverView>,
    },
    /// Engine → owner: one observer attached to a function the connection
    /// hosts. Awaits `observerack`.
    ObserverAdded {
        function_id: String,
        #[serde(default = "default_namespace")]
        namespace: String,
        revision: u64,
        observer: ObserverView,
    },
    /// Engine → owner: one observer detached.
    ObserverRemoved {
        function_id: String,
        #[serde(default = "default_namespace")]
        namespace: String,
        revision: u64,
        id: String,
    },
    /// Owner → engine: acknowledges one added observer, or rejects it with
    /// `error` (also later, for an active observer: a late rejection).
    ObserverAck {
        function_id: String,
        /// Echoes `observeradded.namespace`. Absent means the sending
        /// connection's namespace.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        id: String,
        revision: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<ErrorBody>,
    },
    /// Owner → engine: matched local invocations of auto functions and
    /// `obs.emit` events of owner functions, batched.
    InvocationsObserved {
        /// Decoded one by one: an item a newer or buggy SDK got wrong is set
        /// aside on its own instead of failing the whole batch.
        #[serde(default)]
        items: ObservedItems,
    },
    /// Owner → engine: periodic local invocation counts and SDK-side drops,
    /// sent only when non-zero.
    LocalInvocationStats {
        window_ms: u64,
        #[serde(default, deserialize_with = "deserialize_null_default")]
        functions: Vec<LocalFunctionStats>,
        #[serde(default, deserialize_with = "deserialize_null_default")]
        dropped_reports: Vec<DroppedReportStats>,
    },
}

// ─────────────────────────────── observe ───────────────────────────────────

/// Engine feature: observers, `functionregistered` and the observe messages.
pub const FEATURE_OBSERVE_V1: &str = "observe.v1";
/// Engine feature: SDKs may run their own functions in-process once the engine
/// confirms them with `functionregistered { local_dispatch: true }`.
pub const FEATURE_LOCAL_DISPATCH_V1: &str = "local-dispatch.v1";
/// What this engine advertises in [`Message::WorkerRegistered`].
pub const ENGINE_FEATURES: [&str; 2] = [FEATURE_OBSERVE_V1, FEATURE_LOCAL_DISPATCH_V1];

/// [`ENGINE_FEATURES`] as owned strings, for the wire.
pub fn engine_features() -> Vec<String> {
    ENGINE_FEATURES.iter().map(|f| (*f).to_string()).collect()
}

/// Reads a `features` list leniently: the string elements of an array, and
/// nothing for any other value. A worker's feature list rides on
/// `engine::workers::register`, the call that also fixes the connection's
/// namespace, so a malformed list must cost the worker its features, not its
/// registration.
pub fn deserialize_features<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(match Value::deserialize(deserializer)? {
        Value::Array(items) => items
            .into_iter()
            .filter_map(|item| match item {
                Value::String(feature) => Some(feature),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    })
}

/// Error codes, state reasons and drop reasons of observe, exactly as in
/// protocol.md §4. These strings are the wire contract.
pub mod observe_codes {
    // creation and registration
    pub const UNSUPPORTED: &str = "UNSUPPORTED";
    pub const INVALID_ARGS: &str = "INVALID_ARGS";
    pub const INVALID_OBSERVE_DESCRIPTOR: &str = "INVALID_OBSERVE_DESCRIPTOR";
    pub const INVALID_FILTER: &str = "INVALID_FILTER";
    pub const INVALID_FILTER_PATH: &str = "INVALID_FILTER_PATH";
    pub const NOT_OBSERVABLE: &str = "NOT_OBSERVABLE";
    pub const FORBIDDEN: &str = "FORBIDDEN";
    pub const SELF_TARGET: &str = "SELF_TARGET";
    pub const LIMIT_EXCEEDED: &str = "LIMIT_EXCEEDED";
    pub const OBSERVER_ID_CONFLICT: &str = "OBSERVER_ID_CONFLICT";
    pub const OWNER_TIMEOUT: &str = "OWNER_TIMEOUT";
    /// An owner's `onObserverAdded` threw a plain error.
    pub const OBSERVE_REJECTED: &str = "OBSERVE_REJECTED";

    // owner delivery (protocol.md §3.5)
    pub const OBSERVER_NOT_FOUND: &str = "OBSERVER_NOT_FOUND";
    pub const NOT_FUNCTION_OWNER: &str = "NOT_FUNCTION_OWNER";
    pub const NOT_OWNER_DELIVERED: &str = "NOT_OWNER_DELIVERED";
    pub const OBSERVER_NOT_ACTIVE: &str = "OBSERVER_NOT_ACTIVE";
    pub const TARGET_MISMATCH: &str = "TARGET_MISMATCH";
    pub const OVERFLOW: &str = "OVERFLOW";
    pub const LOOP_GUARD: &str = "LOOP_GUARD";
    /// SDK-side: `obs.call` on a payload the observer's filters reject.
    pub const FILTERS_NOT_MATCHED: &str = "FILTERS_NOT_MATCHED";

    // state reasons
    pub const OWNER_UNAVAILABLE: &str = "OWNER_UNAVAILABLE";
    pub const TARGET_UNAVAILABLE: &str = "TARGET_UNAVAILABLE";
    pub const OBSERVER_DISCONNECTED: &str = "OBSERVER_DISCONNECTED";
    pub const EXPIRED: &str = "EXPIRED";
    pub const MAX_FIRES: &str = "MAX_FIRES";
    pub const UNOBSERVED: &str = "UNOBSERVED";

    // drop reasons (also LOOP_GUARD, MAX_FIRES, OVERFLOW above)
    pub const INACTIVE: &str = "INACTIVE";
    pub const REPORTER_NOT_OWNER: &str = "REPORTER_NOT_OWNER";
    /// SDK-side, reported through `localinvocationstats`.
    pub const BUFFER_FULL: &str = "BUFFER_FULL";
    /// SDK-side, reported through `localinvocationstats`.
    pub const TOO_LARGE: &str = "TOO_LARGE";
}

/// Declares a wire enum carried as a lowercase string, with an
/// `Unknown(String)` catch-all: a value added by a newer peer decodes instead of
/// failing the whole frame (a `capture` array, an observer `state`), and
/// re-encodes unchanged. Whoever acts on the value decides what an unknown one
/// means.
macro_rules! wire_string_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $wire:literal, )+
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub enum $name {
            $( $(#[$vmeta])* $variant, )+
            /// A value this engine does not know, kept verbatim.
            Unknown(String),
        }

        impl $name {
            pub fn as_str(&self) -> &str {
                match self {
                    $( Self::$variant => $wire, )+
                    Self::Unknown(other) => other,
                }
            }

            pub fn is_known(&self) -> bool {
                !matches!(self, Self::Unknown(_))
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                match value {
                    $( $wire => Self::$variant, )+
                    other => Self::Unknown(other.to_string()),
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = String::deserialize(deserializer)?;
                Ok(Self::from(value.as_str()))
            }
        }
    };
}

wire_string_enum! {
    /// How a function's observers are delivered (README §4.2).
    #[derive(Default)]
    pub enum Delivery {
        /// The function's invocations are the events; every observer is
        /// offered every invocation.
        #[default]
        Auto => "auto",
        /// The owner routes events to observers by their args
        /// (`obs.emit` / `obs.call`).
        Owner => "owner",
    }
}

wire_string_enum! {
    /// What an activation ships to the target.
    pub enum Capture {
        Input => "input",
        Output => "output",
        Error => "error",
        Caller => "caller",
        Metadata => "metadata",
    }
}

/// CONTRACT D1: `["input", "output", "caller"]`.
pub fn default_capture() -> Vec<Capture> {
    vec![Capture::Input, Capture::Output, Capture::Caller]
}

wire_string_enum! {
    /// Which outcomes of an auto function's invocation activate an observer.
    /// Defaults to `success` (CONTRACT D1).
    #[derive(Default)]
    pub enum On {
        #[default]
        Success => "success",
        Error => "error",
        Complete => "complete",
    }
}

wire_string_enum! {
    /// What the target's first argument is. Defaults to `envelope`
    /// (CONTRACT D1).
    #[derive(Default)]
    pub enum Deliver {
        /// The `Observed` envelope.
        #[default]
        Envelope => "envelope",
        /// The observed invocation's input alone.
        Input => "input",
        /// The observed invocation's output alone.
        Output => "output",
    }
}

wire_string_enum! {
    /// Observer lifecycle states (protocol.md §3.3), aligned with the language
    /// spec.
    pub enum ObserverState {
        Pending => "pending",
        Active => "active",
        Suspended => "suspended",
        Error => "error",
        Disposed => "disposed",
    }
}

wire_string_enum! {
    /// Outcome of a reported invocation. An emitted event is always `success`.
    pub enum InvocationStatus {
        Success => "success",
        Error => "error",
    }
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

/// `RegisterFunction.observe`: the wire half of a function's observe hooks.
/// Every field is optional so an absent one is not re-encoded.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ObserveDescriptor {
    /// Absent means [`Delivery::Auto`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<Delivery>,
    /// JSON Schema the engine validates observers' `args` against. Owner
    /// delivery only; with `auto` the registration is rejected
    /// (`INVALID_OBSERVE_DESCRIPTOR`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_format: Option<Value>,
    /// Phase 5 shim: old `RegisterTrigger { trigger_type }` binds to this
    /// function when no live provider of that type exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_trigger_type: Option<String>,
    /// Absent means `true`. `false` refuses observers (`NOT_OBSERVABLE`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow: Option<bool>,
}

impl ObserveDescriptor {
    pub fn delivery(&self) -> Delivery {
        self.delivery.clone().unwrap_or_default()
    }

    pub fn allow(&self) -> bool {
        self.allow.unwrap_or(true)
    }
}

/// The left side of `iii.observe`: the observed function, its args and filters,
/// and the facet (`on`, `capture`, `deliver`). Absent fields stay absent; the
/// accessors apply the defaults (CONTRACT D1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObserveSource {
    pub function_id: String,
    /// Same resolution as `InvokeFunction.namespace`: absent means `default`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// For the owner of an owner-delivered function, validated against its
    /// `args_format`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    /// filters.md. Absent or `{}` matches every event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<Vec<Capture>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<On>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deliver: Option<Deliver>,
}

impl ObserveSource {
    pub fn on(&self) -> On {
        self.on.clone().unwrap_or_default()
    }

    pub fn capture(&self) -> Vec<Capture> {
        self.capture.clone().unwrap_or_else(default_capture)
    }

    pub fn deliver(&self) -> Deliver {
        self.deliver.clone().unwrap_or_default()
    }
}

/// The right side of `iii.observe`: a registered function.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObserveTarget {
    pub function_id: String,
    /// Absent means `default`, as for `InvokeFunction`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

/// An observer as its function's owner sees it (`functionregistered`,
/// `observeradded`; CONTRACT D15): never its `deliver`, metadata, policy or
/// observing connection. The engine always sends `capture` and `on` resolved,
/// so the defaults here only matter when decoding a sparse frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObserverView {
    pub id: String,
    /// `{}` for an observer without args.
    #[serde(default = "empty_object")]
    pub args: Value,
    /// The observer's filters, normalized (filters.md §5).
    #[serde(default = "empty_object")]
    pub filters: Value,
    /// Target function id; its namespace is not disclosed (CONTRACT D6).
    pub target: String,
    #[serde(default = "default_capture")]
    pub capture: Vec<Capture>,
    #[serde(default)]
    pub on: On,
    /// Phase 5 shim: the old trigger's type and config. Absent otherwise; SDKs
    /// must also accept `null`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy: Option<LegacyTrigger>,
}

/// The `RegisterTrigger` an observer was created from by the legacy shim
/// (protocol.md §6). The owner reads `config` as the observer's args.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegacyTrigger {
    pub trigger_type: String,
    pub config: Value,
}

/// Who made an invocation (README §4.3). Every field is optional on the wire:
/// SDKs assert `function_id` / `invocation_id` / `parent`, the engine stamps
/// `worker_id` / `worker_name` from the connection.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Caller {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_name: Option<String>,
    /// The handler the call was made from; absent for top-level calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_id: Option<String>,
    /// The caller of that handler, one level only: a deeper `parent` on the
    /// wire is not decoded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ParentCaller>,
}

/// [`Caller::parent`]: a caller without a parent of its own.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParentCaller {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_id: Option<String>,
}

/// `InvokeFunction.activation`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Activation {
    pub observer_id: String,
    /// Engine→worker: unique per activation. Absent on an owner's `obs.call`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation_id: Option<String>,
}

/// One item of `invocationsobserved`: a matched local invocation of an auto
/// function, or an `obs.emit` event of an owner function.
///
/// `input` / `output` / `error` / `metadata` keep `null` apart from absent: a
/// function that returned `null` is not one whose output was not captured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservedItem {
    pub function_id: String,
    /// Absent means the reporting connection's namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub invocation_id: String,
    /// Auto: observer ids whose filters matched. Owner: the emitted-to
    /// observers whose filters matched (one event, coalesced).
    #[serde(default)]
    pub matched: Vec<String>,
    pub status: InvocationStatus,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub input: Option<Value>,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub output: Option<Value>,
    /// `{ code, message, stacktrace? }`, as in `InvocationResult.error`.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub error: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<Caller>,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub metadata: Option<Value>,
    pub started_at_ms: u64,
    pub duration_ms: f64,
    #[serde(default)]
    pub observe_depth: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traceparent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baggage: Option<String>,
}

/// The items of one `invocationsobserved`. Each is decoded on its own, in one
/// pass (payload fields are moved, never re-parsed): an item a newer or buggy
/// SDK got wrong lands in `rejected` instead of failing the batch. Only
/// `items` is on the wire.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObservedItems {
    pub items: Vec<ObservedItem>,
    pub rejected: Vec<RejectedItem>,
}

/// An `invocationsobserved` item that did not decode.
#[derive(Debug, Clone, PartialEq)]
pub struct RejectedItem {
    /// The function it named, when that much was readable.
    pub function_id: Option<String>,
    pub reason: String,
}

impl From<Vec<ObservedItem>> for ObservedItems {
    fn from(items: Vec<ObservedItem>) -> Self {
        Self {
            items,
            rejected: Vec::new(),
        }
    }
}

impl Serialize for ObservedItems {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(&self.items)
    }
}

impl<'de> Deserialize<'de> for ObservedItems {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut out = ObservedItems::default();
        match Value::deserialize(deserializer)? {
            Value::Null => {}
            Value::Array(raw) => {
                for item in raw {
                    match ObservedItem::from_value_lenient(item) {
                        Ok(item) => out.items.push(item),
                        Err(rejected) => out.rejected.push(rejected),
                    }
                }
            }
            _ => out.rejected.push(RejectedItem {
                function_id: None,
                reason: "`items` is not an array".to_string(),
            }),
        }
        Ok(out)
    }
}

impl ObservedItem {
    /// Builds an item from its JSON, moving the payload fields out instead of
    /// decoding them again. Unknown fields are ignored; an unknown `status`
    /// is kept (as [`InvocationStatus::Unknown`]) for the engine to drop.
    pub fn from_value_lenient(value: Value) -> Result<ObservedItem, RejectedItem> {
        let Value::Object(mut map) = value else {
            return Err(RejectedItem {
                function_id: None,
                reason: "not an object".to_string(),
            });
        };
        let Some(Value::String(function_id)) = map.remove("function_id") else {
            return Err(RejectedItem {
                function_id: None,
                reason: "`function_id` is missing or not a string".to_string(),
            });
        };
        let reject = |field: &str| RejectedItem {
            function_id: Some(function_id.clone()),
            reason: format!("`{field}` is missing or malformed"),
        };
        let optional_string = |value: Option<Value>| match value {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s)),
            Some(_) => Err(()),
        };

        let namespace =
            optional_string(map.remove("namespace")).map_err(|_| reject("namespace"))?;
        let Some(Value::String(invocation_id)) = map.remove("invocation_id") else {
            return Err(reject("invocation_id"));
        };
        let matched = match map.remove("matched") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(ids)) => ids
                .into_iter()
                .map(|id| match id {
                    Value::String(id) => Ok(id),
                    _ => Err(()),
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| reject("matched"))?,
            Some(_) => return Err(reject("matched")),
        };
        let status = match map.remove("status") {
            Some(Value::String(status)) => InvocationStatus::from(status.as_str()),
            _ => return Err(reject("status")),
        };
        let caller = match map.remove("caller") {
            None | Some(Value::Null) => None,
            Some(value) => Some(serde_json::from_value(value).map_err(|_| reject("caller"))?),
        };
        let started_at_ms = map
            .remove("started_at_ms")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| reject("started_at_ms"))?;
        let duration_ms = map
            .remove("duration_ms")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| reject("duration_ms"))?;
        let observe_depth = match map.remove("observe_depth") {
            None | Some(Value::Null) => 0,
            Some(value) => value
                .as_u64()
                .and_then(|depth| u32::try_from(depth).ok())
                .ok_or_else(|| reject("observe_depth"))?,
        };
        let traceparent =
            optional_string(map.remove("traceparent")).map_err(|_| reject("traceparent"))?;
        let baggage = optional_string(map.remove("baggage")).map_err(|_| reject("baggage"))?;

        Ok(ObservedItem {
            function_id,
            namespace,
            invocation_id,
            matched,
            status,
            input: map.remove("input"),
            output: map.remove("output"),
            error: map.remove("error"),
            caller,
            metadata: map.remove("metadata"),
            started_at_ms,
            duration_ms,
            observe_depth,
            traceparent,
            baggage,
        })
    }
}

/// Per-function counts in `localinvocationstats`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalFunctionStats {
    pub function_id: String,
    pub count: u64,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub errors: u64,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub duration_ms_sum: f64,
}

/// SDK-side report drops in `localinvocationstats`. `reason` is a drop reason
/// from [`observe_codes`] (`BUFFER_FULL`, `TOO_LARGE`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DroppedReportStats {
    pub function_id: String,
    pub reason: String,
    pub count: u64,
}

/// Worker resource metrics for health monitoring.
///
/// # JavaScript Precision Note
///
/// The `u64` fields (`memory_*`, `cpu_*_micros`, `uptime_seconds`, `timestamp_ms`)
/// can theoretically exceed JavaScript's `Number.MAX_SAFE_INTEGER` (2^53 - 1).
/// In practice:
/// - Memory values would need to exceed ~9 PB to lose precision
/// - CPU microseconds would need ~285 years of continuous uptime
/// - Timestamps are safe until the year 287396
///
/// For most use cases this is not a concern, but if you need guaranteed precision
/// for very large values, consider parsing these as BigInt on the JavaScript side.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkerMetrics {
    // Memory metrics (bytes)
    // Note: u64 values above 2^53-1 may lose precision in JavaScript
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_heap_used: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_heap_total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_rss: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_external: Option<u64>,

    // CPU metrics (microseconds since process start)
    // Note: u64 values above 2^53-1 may lose precision in JavaScript
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_user_micros: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_system_micros: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_percent: Option<f64>,

    // Runtime metrics
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_loop_lag_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime_seconds: Option<u64>,

    // Metadata
    pub timestamp_ms: u64,
    pub runtime: String, // "node", "rust", "python", etc.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stacktrace: Option<String>,
}

impl ErrorBody {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            stacktrace: None,
        }
    }
}

impl std::fmt::Display for ErrorBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChannelDirection {
    #[default]
    Read,
    Write,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct StreamChannelRef {
    pub channel_id: String,
    pub access_key: String,
    pub direction: ChannelDirection,
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_NAMESPACE, Message, TriggerAction, effective_namespace};
    use crate::{
        invocation::{auth::HttpAuthConfig, method::HttpMethod},
        protocol::HttpInvocationRef,
    };

    #[test]
    fn invocation_result_null_stays_null_and_absent_stays_absent() {
        let id = "8d3c6f9e-0f4b-4e6a-9c1d-2b3a4c5d6e7f";
        let with_null = format!(
            r#"{{"type":"invocationresult","invocation_id":"{id}","function_id":"state::get","result":null}}"#
        );
        let message: Message = serde_json::from_str(&with_null).expect("null result parses");
        let Message::InvocationResult { result, error, .. } = &message else {
            panic!("unexpected variant");
        };
        assert_eq!(result, &Some(serde_json::Value::Null));
        assert!(error.is_none());
        let wire = serde_json::to_string(&message).expect("serializes");
        assert!(
            wire.contains(r#""result":null"#),
            "null must stay on the wire: {wire}"
        );

        let absent = format!(
            r#"{{"type":"invocationresult","invocation_id":"{id}","function_id":"fire::forget"}}"#
        );
        let message: Message = serde_json::from_str(&absent).expect("absent result parses");
        let Message::InvocationResult { result, .. } = &message else {
            panic!("unexpected variant");
        };
        assert_eq!(result, &None);
        let wire = serde_json::to_string(&message).expect("serializes");
        assert!(
            !wire.contains(r#""result""#),
            "absent must stay absent: {wire}"
        );
    }

    #[test]
    fn deserialize_unregister_trigger_without_type() {
        let raw = r#"{"type":"unregistertrigger","id":"abc"}"#;
        let message: Message = serde_json::from_str(raw).expect("message should deserialize");

        match message {
            Message::UnregisterTrigger { id, trigger_type } => {
                assert_eq!(id, "abc");
                assert_eq!(trigger_type, None);
            }
            _ => panic!("unexpected message variant"),
        }
    }

    #[test]
    fn reattach_round_trips() {
        // Token present.
        let raw = r#"{"type":"reattach","previous_worker_id":"abc-123","reattach_token":"tok-1"}"#;
        let message: Message = serde_json::from_str(raw).expect("reattach should deserialize");
        match &message {
            Message::Reattach {
                previous_worker_id,
                reattach_token,
            } => {
                assert_eq!(previous_worker_id, "abc-123");
                assert_eq!(reattach_token.as_deref(), Some("tok-1"));
            }
            _ => panic!("unexpected message variant"),
        }
        let serialized = serde_json::to_string(&message).expect("serialize");
        assert_eq!(serialized, raw);

        // Token absent (older SDK): still deserializes, token None.
        let raw = r#"{"type":"reattach","previous_worker_id":"abc-123"}"#;
        let message: Message = serde_json::from_str(raw).expect("tokenless reattach deserializes");
        assert!(matches!(
            message,
            Message::Reattach {
                reattach_token: None,
                ..
            }
        ));
    }

    #[test]
    fn worker_registered_token_is_optional_on_the_wire() {
        // Older engines omit the token; SDKs must still parse the frame.
        let raw = r#"{"type":"workerregistered","worker_id":"w-1"}"#;
        let message: Message = serde_json::from_str(raw).expect("should deserialize");
        assert!(matches!(
            message,
            Message::WorkerRegistered {
                reattach_token: None,
                ..
            }
        ));
    }

    #[test]
    fn deserialize_unregister_trigger_with_type() {
        let raw = r#"{"type":"unregistertrigger","id":"abc","trigger_type":"http"}"#;
        let message: Message = serde_json::from_str(raw).expect("message should deserialize");

        match message {
            Message::UnregisterTrigger { id, trigger_type } => {
                assert_eq!(id, "abc");
                assert_eq!(trigger_type.as_deref(), Some("http"));
            }
            _ => panic!("unexpected message variant"),
        }
    }

    #[test]
    fn deserialize_register_function_with_http_invocation() {
        let raw = r#"{
            "type":"registerfunction",
            "id":"external.my_lambda",
            "description":"External Lambda function",
            "invocation":{
                "url":"https://example.com/lambda",
                "timeout_ms":30000,
                "headers":{"x-custom-header":"value"},
                "auth":{"type":"bearer","token_key":"LAMBDA_TOKEN"}
            }
        }"#;
        let message: Message = serde_json::from_str(raw).expect("message should deserialize");

        match message {
            Message::RegisterFunction {
                id,
                description,
                invocation,
                ..
            } => {
                assert_eq!(id, "external.my_lambda");
                assert_eq!(description.as_deref(), Some("External Lambda function"));

                let HttpInvocationRef {
                    url,
                    method,
                    timeout_ms,
                    headers,
                    auth,
                } = invocation.expect("invocation should be present");

                assert_eq!(url, "https://example.com/lambda");
                assert!(matches!(method, HttpMethod::Post));
                assert_eq!(timeout_ms, Some(30000));
                assert_eq!(
                    headers.get("x-custom-header").map(String::as_str),
                    Some("value")
                );
                match auth {
                    Some(HttpAuthConfig::Bearer { token_key }) => {
                        assert_eq!(token_key, "LAMBDA_TOKEN");
                    }
                    _ => panic!("unexpected auth variant"),
                }
            }
            _ => panic!("unexpected message variant"),
        }
    }

    #[test]
    fn deserialize_invoke_function_with_enqueue_action() {
        let raw = r#"{
            "type": "invokefunction",
            "invocation_id": null,
            "function_id": "payment.process",
            "data": {"amount": 100},
            "action": {"type": "enqueue", "queue": "payment"}
        }"#;
        let message: Message = serde_json::from_str(raw).expect("message should deserialize");

        match message {
            Message::InvokeFunction {
                function_id,
                action,
                ..
            } => {
                assert_eq!(function_id, "payment.process");
                match action {
                    Some(TriggerAction::Enqueue { queue }) => {
                        assert_eq!(queue, "payment");
                    }
                    other => panic!("expected Enqueue action, got {other:?}"),
                }
            }
            _ => panic!("unexpected message variant"),
        }
    }

    #[test]
    fn deserialize_invoke_function_with_void_action() {
        let raw = r#"{
            "type": "invokefunction",
            "invocation_id": null,
            "function_id": "audit.log",
            "data": {"event": "login"},
            "action": {"type": "void"}
        }"#;
        let message: Message = serde_json::from_str(raw).expect("message should deserialize");

        match message {
            Message::InvokeFunction {
                function_id,
                action,
                ..
            } => {
                assert_eq!(function_id, "audit.log");
                assert!(
                    matches!(action, Some(TriggerAction::Void)),
                    "expected Void action, got {action:?}"
                );
            }
            _ => panic!("unexpected message variant"),
        }
    }

    #[test]
    fn deserialize_invoke_function_without_action() {
        let raw = r#"{
            "type": "invokefunction",
            "invocation_id": null,
            "function_id": "sync.call",
            "data": {}
        }"#;
        let message: Message = serde_json::from_str(raw).expect("message should deserialize");

        match message {
            Message::InvokeFunction {
                function_id,
                action,
                ..
            } => {
                assert_eq!(function_id, "sync.call");
                assert!(action.is_none(), "expected no action, got {action:?}");
            }
            _ => panic!("unexpected message variant"),
        }
    }

    #[test]
    fn serialize_invoke_function_without_action_omits_field() {
        let msg = Message::InvokeFunction {
            invocation_id: None,
            function_id: "test.fn".to_string(),
            data: serde_json::json!({}),
            traceparent: None,
            baggage: None,
            action: None,
            metadata: None,
            namespace: None,
            caller: None,
            observe_depth: None,
            activation: None,
        };
        let json = serde_json::to_string(&msg).expect("message should serialize");

        assert!(
            !json.contains("\"action\""),
            "action field should be omitted when None, got: {json}"
        );
        assert!(
            !json.contains("\"traceparent\""),
            "traceparent field should be omitted when None, got: {json}"
        );
        assert!(
            !json.contains("\"baggage\""),
            "baggage field should be omitted when None, got: {json}"
        );
        assert!(
            !json.contains("\"metadata\""),
            "metadata field should be omitted when None, got: {json}"
        );
    }

    #[test]
    fn invoke_function_metadata_roundtrips_and_is_backward_compatible() {
        // Present: metadata survives a serialize → deserialize round-trip.
        let msg = Message::InvokeFunction {
            invocation_id: None,
            function_id: "test.fn".to_string(),
            data: serde_json::json!({ "k": 1 }),
            traceparent: None,
            baggage: None,
            action: None,
            metadata: Some(serde_json::json!({ "session_id": "s_1" })),
            namespace: None,
            caller: None,
            observe_depth: None,
            activation: None,
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        assert!(json.contains("\"metadata\""));
        match serde_json::from_str::<Message>(&json).expect("deserialize") {
            Message::InvokeFunction { metadata, data, .. } => {
                assert_eq!(metadata, Some(serde_json::json!({ "session_id": "s_1" })));
                // Payload is untouched — metadata is not folded into `data`.
                assert_eq!(data, serde_json::json!({ "k": 1 }));
            }
            _ => panic!("expected InvokeFunction"),
        }

        // Backward compatible: a payload from an older peer that omits the
        // field deserializes to `metadata: None`.
        let legacy =
            r#"{"type":"invokefunction","invocation_id":null,"function_id":"f","data":{}}"#;
        match serde_json::from_str::<Message>(legacy).expect("deserialize legacy") {
            Message::InvokeFunction { metadata, .. } => assert_eq!(metadata, None),
            _ => panic!("expected InvokeFunction"),
        }
    }

    #[test]
    fn effective_namespace_defaults_when_absent() {
        assert_eq!(effective_namespace(&None), DEFAULT_NAMESPACE);
        assert_eq!(effective_namespace(&Some("orders".to_string())), "orders");
    }

    #[test]
    fn invoke_function_serializes_namespace_when_set() {
        let msg = Message::InvokeFunction {
            invocation_id: None,
            function_id: "state::get".into(),
            data: serde_json::json!({}),
            traceparent: None,
            baggage: None,
            action: None,
            metadata: None,
            namespace: Some("orders".into()),
            caller: None,
            observe_depth: None,
            activation: None,
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains(r#""namespace":"orders""#));
    }

    #[test]
    fn invoke_function_omits_namespace_when_absent_and_parses_legacy() {
        // legacy wire (sem namespace) continua parseando
        let json = r#"{"type":"invokefunction","invocation_id":null,"function_id":"state::get","data":{}}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        match msg {
            Message::InvokeFunction { namespace, .. } => assert!(namespace.is_none()),
            _ => panic!("expected InvokeFunction"),
        }
    }

    #[test]
    fn registration_rejected_roundtrip() {
        let worker_conflict = Message::RegistrationRejected {
            code: "WORKER_NAMESPACE_CONFLICT".into(),
            namespace: "orders".into(),
            worker_name: Some("state".into()),
            function_id: None,
            owner_worker_id: "abc".into(),
        };
        let worker_json = serde_json::to_value(&worker_conflict).unwrap();
        assert_eq!(worker_json["worker_name"], "state");
        assert!(worker_json.get("function_id").is_none());
        let back: Message = serde_json::from_value(worker_json).unwrap();
        assert!(matches!(back, Message::RegistrationRejected { .. }));

        let function_conflict = Message::RegistrationRejected {
            code: "FUNCTION_NAMESPACE_CONFLICT".into(),
            namespace: "orders".into(),
            worker_name: None,
            function_id: Some("state::get".into()),
            owner_worker_id: "abc".into(),
        };
        let function_json = serde_json::to_value(&function_conflict).unwrap();
        assert_eq!(function_json["function_id"], "state::get");
        assert!(function_json.get("worker_name").is_none());
        let back: Message = serde_json::from_value(function_json).unwrap();
        assert!(matches!(back, Message::RegistrationRejected { .. }));
    }

    // ── observe (0.25) wire compatibility ─────────────────────────────────

    use super::{
        Activation, Caller, Capture, Deliver, Delivery, InvocationStatus, ObserveDescriptor,
        ObserverState, On, default_capture,
    };
    use serde_json::{Value, json};

    /// Keys a 0.25 engine may add to existing variants. None of them may appear
    /// on the wire unless set.
    const NEW_KEYS: [&str; 5] = [
        "observe",
        "caller",
        "observe_depth",
        "activation",
        "features",
    ];

    fn assert_no_new_keys(msg: &Message) {
        let wire = serde_json::to_value(msg).expect("serialize");
        for key in NEW_KEYS {
            assert!(
                wire.get(key).is_none(),
                "`{key}` must not be sent unset: {wire}"
            );
        }
    }

    #[test]
    fn pre_025_frames_decode_without_the_new_fields() {
        let invoke: Message = serde_json::from_str(
            r#"{"type":"invokefunction","invocation_id":"8d3c6f9e-0f4b-4e6a-9c1d-2b3a4c5d6e7f","function_id":"orders::create","data":{"sku":"A-1"},"traceparent":"00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"}"#,
        )
        .expect("old invokefunction decodes");
        let Message::InvokeFunction {
            caller,
            observe_depth,
            activation,
            ..
        } = invoke
        else {
            panic!("expected InvokeFunction");
        };
        assert!(caller.is_none() && observe_depth.is_none() && activation.is_none());

        // The oldest SDKs send neither format.
        let register: Message =
            serde_json::from_str(r#"{"type":"registerfunction","id":"orders::create"}"#)
                .expect("old registerfunction decodes");
        let Message::RegisterFunction {
            observe,
            request_format,
            response_format,
            ..
        } = register
        else {
            panic!("expected RegisterFunction");
        };
        assert!(observe.is_none() && request_format.is_none() && response_format.is_none());

        let registered: Message = serde_json::from_str(
            r#"{"type":"workerregistered","worker_id":"w-1","reattach_token":"t-1"}"#,
        )
        .expect("old workerregistered decodes");
        let Message::WorkerRegistered { features, .. } = registered else {
            panic!("expected WorkerRegistered");
        };
        assert!(features.is_empty());
    }

    #[test]
    fn default_new_fields_put_no_new_keys_on_the_wire() {
        assert_no_new_keys(&Message::InvokeFunction {
            invocation_id: Some(uuid::Uuid::new_v4()),
            function_id: "orders::create".into(),
            data: json!({ "sku": "A-1" }),
            traceparent: None,
            baggage: None,
            action: None,
            metadata: None,
            namespace: None,
            caller: None,
            observe_depth: None,
            activation: None,
        });
        assert_no_new_keys(&Message::RegisterFunction {
            id: "orders::create".into(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
            observe: None,
        });
        assert_no_new_keys(&Message::WorkerRegistered {
            worker_id: "w-1".into(),
            reattach_token: Some("t-1".into()),
            features: Vec::new(),
        });
    }

    #[test]
    fn an_absent_invocation_id_stays_absent_and_a_present_one_is_sent() {
        let void = Message::InvokeFunction {
            invocation_id: None,
            function_id: "audit::log".into(),
            data: json!({}),
            traceparent: None,
            baggage: None,
            action: Some(TriggerAction::Void),
            metadata: None,
            namespace: None,
            caller: None,
            observe_depth: None,
            activation: None,
        };
        let wire = serde_json::to_value(&void).unwrap();
        assert!(wire.get("invocation_id").is_none(), "{wire}");

        let id = uuid::Uuid::new_v4();
        let Message::InvokeFunction { data, .. } = void else {
            unreachable!()
        };
        let call = Message::InvokeFunction {
            invocation_id: Some(id),
            function_id: "audit::log".into(),
            data,
            traceparent: None,
            baggage: None,
            action: None,
            metadata: None,
            namespace: None,
            caller: None,
            observe_depth: None,
            activation: None,
        };
        let wire = serde_json::to_value(&call).unwrap();
        assert_eq!(wire["invocation_id"], json!(id.to_string()));
    }

    #[test]
    fn invoke_function_observe_fields_round_trip() {
        let msg = Message::InvokeFunction {
            invocation_id: Some(uuid::Uuid::new_v4()),
            function_id: "alerts::payment-failed".into(),
            data: json!({}),
            traceparent: None,
            baggage: None,
            action: None,
            metadata: None,
            namespace: None,
            caller: Some(Box::new(Caller {
                worker_name: Some("iii-engine".into()),
                function_id: Some("payments::charge".into()),
                ..Caller::default()
            })),
            observe_depth: Some(1),
            activation: Some(Activation {
                observer_id: "o-1".into(),
                activation_id: Some("a-1".into()),
            }),
        };
        let wire = serde_json::to_value(&msg).unwrap();
        assert_eq!(
            wire["caller"],
            json!({ "worker_name": "iii-engine", "function_id": "payments::charge" })
        );
        assert_eq!(wire["observe_depth"], json!(1));
        assert_eq!(
            wire["activation"],
            json!({ "observer_id": "o-1", "activation_id": "a-1" })
        );
        let back: Message = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&back).unwrap(), wire);
    }

    #[test]
    fn observe_source_defaults_follow_contract_d1() {
        let msg: Message = serde_json::from_value(json!({
            "type": "observe",
            "id": "o-1",
            "source": { "function_id": "orders::archive" },
            "target": { "function_id": "search::reindex-order" }
        }))
        .unwrap();
        let Message::Observe {
            source,
            target,
            metadata,
            max_fires,
            expires_in_ms,
            max_in_flight,
            ..
        } = msg
        else {
            panic!("expected Observe");
        };
        assert_eq!(source.on(), On::Success);
        assert_eq!(source.capture(), default_capture());
        assert_eq!(
            source.capture(),
            vec![Capture::Input, Capture::Output, Capture::Caller]
        );
        assert_eq!(source.deliver(), Deliver::Envelope);
        assert!(source.namespace.is_none() && source.args.is_none() && source.filters.is_none());
        assert!(target.namespace.is_none());
        assert!(
            metadata.is_none()
                && max_fires.is_none()
                && expires_in_ms.is_none()
                && max_in_flight.is_none()
        );
    }

    #[test]
    fn observe_descriptor_defaults() {
        let empty = ObserveDescriptor::default();
        assert_eq!(empty.delivery(), Delivery::Auto);
        assert!(empty.allow());
        assert_eq!(serde_json::to_value(&empty).unwrap(), json!({}));

        let refused: ObserveDescriptor = serde_json::from_value(json!({ "allow": false })).unwrap();
        assert!(!refused.allow());
        assert_eq!(refused.delivery(), Delivery::Auto);
    }

    /// A value added by a newer peer decodes as `Unknown`, keeps its spelling
    /// on re-encode, and does not take the rest of the frame down with it.
    #[test]
    fn unknown_enum_values_from_a_newer_peer_decode_and_round_trip() {
        let frame = json!({
            "type": "observe",
            "id": "o-1",
            "source": {
                "function_id": "payments::charge",
                "capture": ["input", "timing"],
                "on": "start",
                "deliver": "both"
            },
            "target": { "function_id": "audit::write" }
        });
        let msg: Message = serde_json::from_value(frame.clone()).expect("decodes");
        let Message::Observe { source, .. } = &msg else {
            panic!("expected Observe");
        };
        assert_eq!(
            source.capture.as_deref(),
            Some(&[Capture::Input, Capture::Unknown("timing".into())][..])
        );
        assert_eq!(source.on, Some(On::Unknown("start".into())));
        assert!(!source.on().is_known());
        assert_eq!(source.deliver, Some(Deliver::Unknown("both".into())));
        assert_eq!(serde_json::to_value(&msg).unwrap(), frame);

        let state: Message = serde_json::from_value(
            json!({ "type": "observerstatechanged", "id": "o-1", "state": "paused", "fires": 3 }),
        )
        .unwrap();
        let Message::ObserverStateChanged { state, fires, .. } = state else {
            panic!("expected ObserverStateChanged");
        };
        assert_eq!(state, ObserverState::Unknown("paused".into()));
        assert_eq!(fires, 3);

        let register: Message = serde_json::from_value(json!({
            "type": "registerfunction",
            "id": "events::on-broadcast",
            "observe": { "delivery": "broadcast" }
        }))
        .unwrap();
        let Message::RegisterFunction {
            observe: Some(descriptor),
            ..
        } = register
        else {
            panic!("expected a descriptor");
        };
        assert_eq!(descriptor.delivery(), Delivery::Unknown("broadcast".into()));
    }

    #[test]
    fn wire_enums_spell_their_values_in_lowercase() {
        for (value, wire) in [
            (serde_json::to_value(Delivery::Owner).unwrap(), "owner"),
            (serde_json::to_value(Capture::Metadata).unwrap(), "metadata"),
            (serde_json::to_value(On::Complete).unwrap(), "complete"),
            (serde_json::to_value(Deliver::Output).unwrap(), "output"),
            (
                serde_json::to_value(ObserverState::Suspended).unwrap(),
                "suspended",
            ),
            (
                serde_json::to_value(InvocationStatus::Error).unwrap(),
                "error",
            ),
        ] {
            assert_eq!(value, json!(wire));
        }
    }

    #[test]
    fn observed_item_keeps_null_apart_from_absent() {
        let msg: Message = serde_json::from_value(json!({
            "type": "invocationsobserved",
            "items": [{
                "function_id": "orders::archive",
                "invocation_id": "i-1",
                "matched": ["o-1"],
                "status": "success",
                "input": null,
                "output": null,
                "started_at_ms": 1,
                "duration_ms": 0.5
            }]
        }))
        .unwrap();
        let Message::InvocationsObserved { items } = &msg else {
            panic!("expected InvocationsObserved");
        };
        let item = &items.items[0];
        assert_eq!(
            item.input,
            Some(Value::Null),
            "the function was called with null"
        );
        assert_eq!(item.output, Some(Value::Null), "the function returned null");
        assert_eq!(item.error, None);
        assert_eq!(item.metadata, None);
        assert_eq!(item.observe_depth, 0);
        assert_eq!(item.status, InvocationStatus::Success);

        let wire = serde_json::to_value(&msg).unwrap();
        assert_eq!(wire["items"][0]["output"], Value::Null);
        assert!(wire["items"][0].get("error").is_none());
    }

    #[test]
    fn one_undecodable_item_does_not_drop_the_batch() {
        let msg: Message = serde_json::from_value(json!({
            "type": "invocationsobserved",
            "items": [
                { "function_id": "orders::archive", "matched": ["o-1"], "status": "success",
                  "started_at_ms": 1, "duration_ms": 0.5 },
                { "function_id": "orders::archive", "invocation_id": "i-2", "matched": ["o-1"],
                  "status": "success", "started_at_ms": 2, "duration_ms": 0.5 },
                "not an item",
                { "function_id": "orders::archive", "invocation_id": "i-4", "matched": ["o-1"],
                  "status": "paused", "started_at_ms": 4, "duration_ms": 0.5 }
            ]
        }))
        .expect("the frame decodes");
        let Message::InvocationsObserved { items } = msg else {
            panic!("expected InvocationsObserved");
        };
        assert_eq!(items.items.len(), 2, "the bad items are set aside alone");
        assert_eq!(items.items[0].invocation_id, "i-2");
        assert_eq!(
            items.items[1].status,
            InvocationStatus::Unknown("paused".into()),
            "an unknown status decodes; the engine drops the item"
        );
        assert_eq!(items.rejected.len(), 2);
        assert_eq!(
            items.rejected[0].function_id.as_deref(),
            Some("orders::archive")
        );
        assert!(items.rejected[0].reason.contains("invocation_id"));
        assert_eq!(items.rejected[1].function_id, None);

        let null_items: Message =
            serde_json::from_value(json!({ "type": "invocationsobserved", "items": null }))
                .unwrap();
        let Message::InvocationsObserved { items } = null_items else {
            panic!("expected InvocationsObserved");
        };
        assert!(items.items.is_empty() && items.rejected.is_empty());
    }

    #[test]
    fn caller_parent_is_one_level_only() {
        let caller: Caller = serde_json::from_value(json!({
            "worker_name": "database",
            "function_id": "database::execute",
            "parent": {
                "worker_name": "billing",
                "function_id": "billing::close-invoice",
                "parent": { "worker_name": "checkout" }
            }
        }))
        .unwrap();
        let parent = caller.parent.as_ref().expect("one level kept");
        assert_eq!(parent.worker_name.as_deref(), Some("billing"));
        let wire = serde_json::to_value(&caller).unwrap();
        assert!(wire["parent"].get("parent").is_none(), "{wire}");
    }

    #[test]
    fn engine_advertises_observe_and_local_dispatch() {
        assert_eq!(
            super::engine_features(),
            vec!["observe.v1".to_string(), "local-dispatch.v1".to_string()]
        );
    }

    #[test]
    fn features_are_read_leniently() {
        #[derive(serde::Deserialize)]
        struct Payload {
            #[serde(default, deserialize_with = "super::deserialize_features")]
            features: Vec<String>,
        }
        let read = |v: Value| serde_json::from_value::<Payload>(v).unwrap().features;
        assert_eq!(
            read(json!({ "features": ["observe.v1", "local-dispatch.v1"] })),
            vec!["observe.v1", "local-dispatch.v1"]
        );
        assert_eq!(
            read(json!({ "features": ["observe.v1", 7, null] })),
            vec!["observe.v1"]
        );
        assert!(read(json!({ "features": "observe.v1" })).is_empty());
        assert!(read(json!({ "features": null })).is_empty());
        assert!(read(json!({})).is_empty());
    }
}
