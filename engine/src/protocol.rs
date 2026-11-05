// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, fmt, sync::Arc};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, value::RawValue};
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
/// every call and observer it makes then follows it, and the one place an
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

/// How a caller wants an `InvokeFunction` handled: queued (`enqueue`) or
/// fire-and-forget (`void`). Absent means request/response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CallAction {
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

/// [`deserialize_present`] for a payload kept as its JSON text.
fn deserialize_present_raw<'de, D>(deserializer: D) -> Result<Option<RawJson>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    RawJson::deserialize(deserializer).map(Some)
}

/// Reads `null` as `true`, like an absent field defaulting to `true`.
fn deserialize_null_true<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<bool>::deserialize(deserializer).map(|flag| flag.unwrap_or(true))
}

/// Reads `invocationresult.matched` leniently, so a malformed list never
/// costs the caller its result: `null` is absent (the host did not narrow
/// the call's observers), an array is its string ids (any other element is
/// skipped), and any other value is an empty list (the host narrowed them to
/// none).
fn deserialize_matched<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(match Value::deserialize(deserializer)? {
        Value::Null => None,
        Value::Array(ids) => Some(
            ids.into_iter()
                .filter_map(|id| match id {
                    Value::String(id) => Some(id),
                    _ => None,
                })
                .collect(),
        ),
        _ => Some(Vec::new()),
    })
}

/// Reads a flag that only `true` sets: `null` and any other value read as
/// `false`, like an absent field.
fn deserialize_only_true<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(matches!(
        Value::deserialize(deserializer)?,
        Value::Bool(true)
    ))
}

fn default_true() -> bool {
    true
}

fn is_false(flag: &bool) -> bool {
    !*flag
}

fn is_true(flag: &bool) -> bool {
    *flag
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Message {
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
        /// JSON Schema the engine validates observers' `args` against. A
        /// function without one takes no args.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args_format: Option<Value>,
        /// Only the connection that hosts the function may invoke it; anyone
        /// may observe it, and it is never an observer's target.
        #[serde(
            default,
            deserialize_with = "deserialize_null_default",
            skip_serializing_if = "is_false"
        )]
        private: bool,
        /// `false`: every `observe` of the function fails with
        /// `NOT_OBSERVABLE`.
        #[serde(
            default = "default_true",
            deserialize_with = "deserialize_null_true",
            skip_serializing_if = "is_true"
        )]
        observable: bool,
        /// The 0.25 `observe` object, which 0.25 SDKs send and v2 SDKs never
        /// do. Boxed like `Observe.source`.
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
        action: Option<CallAction>,
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
        /// Engine→worker: present on every observer activation and on every
        /// call through an observer. Worker→engine: a call through the
        /// observer `observer_id` (`iii.call({ observer, payload })`).
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
        /// Host→engine: the observers the function's match hook matched for
        /// this routed call (v2 §8.2, decision 55). The engine delivers it to
        /// those of them that are active observers of the function and whose
        /// `on` and filters match it. Absent when the host did not narrow the
        /// call's observers (the function has no match hook): the engine then
        /// evaluates every active observer of the function. Read from a
        /// connection that declared `observe.v2` only, and leniently
        /// (`null` is absent; a malformed list is empty). The engine never
        /// forwards it.
        #[serde(
            default,
            deserialize_with = "deserialize_matched",
            skip_serializing_if = "Option::is_none"
        )]
        matched: Option<Vec<String>>,
        /// Host→engine: the function's deliver hook delivered this routed
        /// call, and the engine delivers it to no observer (decision 55).
        /// Sent only as `true`, which outweighs `matched`. Read from a
        /// connection that declared `observe.v2` only; anything but `true`
        /// reads as `false`. The engine never forwards it.
        #[serde(
            default,
            deserialize_with = "deserialize_only_true",
            skip_serializing_if = "is_false"
        )]
        delivered: bool,
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
        /// The activation's metadata sidecar. Never shown to the host.
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
    /// Engine → host: the `RegisterFunction` success ack, with the full current
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
    /// Engine → host: one observer attached to a function the connection
    /// hosts. Awaits `observerack`.
    ObserverAdded {
        function_id: String,
        #[serde(default = "default_namespace")]
        namespace: String,
        revision: u64,
        observer: ObserverView,
    },
    /// Engine → host: one observer detached.
    ObserverRemoved {
        function_id: String,
        #[serde(default = "default_namespace")]
        namespace: String,
        revision: u64,
        id: String,
    },
    /// Host → engine: acknowledges one added observer, or rejects it with
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
    /// Host → engine: the matched calls of the functions it hosts, batched.
    InvocationsObserved {
        /// Decoded one by one: an item a newer or buggy SDK got wrong is set
        /// aside on its own instead of failing the whole batch.
        #[serde(default)]
        items: ObservedItems,
    },
    /// Host → engine: periodic local invocation counts and SDK-side drops,
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
/// Feature: `observe.v1`, plus a host that delivers its own functions' calls
/// (`invocationresult.matched` and `delivered`, `output_is_input`) and the
/// function options `args_format`, `private` and `observable`.
pub const FEATURE_OBSERVE_V2: &str = "observe.v2";
/// Engine feature: SDKs may run their own functions in-process once the engine
/// confirms them with `functionregistered { local_dispatch: true }`.
pub const FEATURE_LOCAL_DISPATCH_V1: &str = "local-dispatch.v1";
/// What this engine advertises in [`Message::WorkerRegistered`], in this
/// order.
pub const ENGINE_FEATURES: [&str; 3] = [
    FEATURE_OBSERVE_V1,
    FEATURE_OBSERVE_V2,
    FEATURE_LOCAL_DISPATCH_V1,
];
/// Worker feature, declared on `engine::workers::register` and never
/// advertised by the engine: the connection receives JSON messages up to
/// 64 MiB, so the engine sends it messages of that size whatever `version` it
/// reports (never to `runtime: "browser"`).
pub const FEATURE_LARGE_MESSAGES_V1: &str = "large-messages.v1";

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
    /// A registration whose `args_format` is not a valid JSON Schema.
    pub const INVALID_ARGS_FORMAT: &str = "INVALID_ARGS_FORMAT";
    pub const INVALID_FILTER: &str = "INVALID_FILTER";
    pub const INVALID_FILTER_PATH: &str = "INVALID_FILTER_PATH";
    pub const NOT_OBSERVABLE: &str = "NOT_OBSERVABLE";
    pub const FORBIDDEN: &str = "FORBIDDEN";
    pub const SELF_TARGET: &str = "SELF_TARGET";
    pub const LIMIT_EXCEEDED: &str = "LIMIT_EXCEEDED";
    pub const OBSERVER_ID_CONFLICT: &str = "OBSERVER_ID_CONFLICT";
    pub const OWNER_TIMEOUT: &str = "OWNER_TIMEOUT";
    /// A host's `onObserverAdded` threw a plain error.
    pub const OBSERVE_REJECTED: &str = "OBSERVE_REJECTED";

    // calls through an observer (v2 §4.6)
    pub const OBSERVER_NOT_FOUND: &str = "OBSERVER_NOT_FOUND";
    pub const NOT_FUNCTION_OWNER: &str = "NOT_FUNCTION_OWNER";
    pub const OBSERVER_NOT_ACTIVE: &str = "OBSERVER_NOT_ACTIVE";
    pub const OVERFLOW: &str = "OVERFLOW";
    pub const LOOP_GUARD: &str = "LOOP_GUARD";

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
    /// SDK-side: report items lost to a closed connection, reported through
    /// `localinvocationstats`.
    pub const DISCONNECTED: &str = "DISCONNECTED";
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
    /// Which outcomes of a function's invocation activate an observer.
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
    /// Outcome of a reported invocation.
    pub enum InvocationStatus {
        Success => "success",
        Error => "error",
    }
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

/// `RegisterFunction.observe`: the object a 0.25 SDK sends to describe a
/// function's observers. The engine reads `delivery` and `allow`; every other
/// key is decoded, kept for re-encoding and ignored.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ObserveDescriptor {
    /// `auto` (or absent) is the only delivery this engine accepts: any other
    /// value refuses the registration with `UNSUPPORTED`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<String>,
    /// Absent means `true`. `false` registers the function as
    /// `observable: false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow: Option<bool>,
    /// The keys this engine ignores, as they arrived.
    #[serde(flatten)]
    pub ignored: serde_json::Map<String, Value>,
}

impl ObserveDescriptor {
    /// The delivery this engine accepts: `delivery` absent or `auto`.
    pub fn is_supported(&self) -> bool {
        self.delivery
            .as_deref()
            .is_none_or(|delivery| delivery == "auto")
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
    /// For the worker that hosts the function, validated against its
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

/// An observer as its function's host sees it (`functionregistered`,
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
    /// Engine→worker: unique per activation. Absent on a host's call through
    /// an observer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation_id: Option<String>,
}

/// One item of `invocationsobserved`: a local call of a function the
/// reporting connection hosts, with the observers its delivery matched.
///
/// `input` / `output` / `error` / `metadata` keep `null` apart from absent: a
/// function that returned `null` is not one whose output was not captured.
/// They stay the JSON text the host sent ([`RawJson`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservedItem {
    pub function_id: String,
    /// Absent means the reporting connection's namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub invocation_id: String,
    /// The observers the host's delivery matched.
    #[serde(default)]
    pub matched: Vec<String>,
    pub status: InvocationStatus,
    #[serde(
        default,
        deserialize_with = "deserialize_present_raw",
        skip_serializing_if = "Option::is_none"
    )]
    pub input: Option<RawJson>,
    #[serde(
        default,
        deserialize_with = "deserialize_present_raw",
        skip_serializing_if = "Option::is_none"
    )]
    pub output: Option<RawJson>,
    /// `{ code, message, stacktrace? }`, as in `InvocationResult.error`.
    #[serde(
        default,
        deserialize_with = "deserialize_present_raw",
        skip_serializing_if = "Option::is_none"
    )]
    pub error: Option<RawJson>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<Caller>,
    #[serde(
        default,
        deserialize_with = "deserialize_present_raw",
        skip_serializing_if = "Option::is_none"
    )]
    pub metadata: Option<RawJson>,
    pub started_at_ms: u64,
    pub duration_ms: f64,
    #[serde(default)]
    pub observe_depth: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traceparent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baggage: Option<String>,
    /// The handler returned its input: the item carries it once, as `input`,
    /// and `output` is the same value.
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        skip_serializing_if = "is_false"
    )]
    pub output_is_input: bool,
}

/// The items of one `invocationsobserved`. Each is decoded on its own, its
/// payloads kept as JSON text ([`RawJson`]): an item a newer or buggy SDK got
/// wrong lands in `rejected` instead of failing the batch. Only `items` is on
/// the wire.
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
        // Through `Message`, whose tag buffers the frame first, there is no
        // raw text left to lend: each item is decoded, then encoded again
        // for `ObservedItem::from_value_lenient` (see `Message::from_frame`).
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
    /// Builds an item from its JSON value: encoded once and read as
    /// [`ObservedItem::from_raw_lenient`] reads an item off a frame.
    pub fn from_value_lenient(value: Value) -> Result<ObservedItem, RejectedItem> {
        Self::from_raw_lenient(&value.to_string())
    }

    /// Builds an item from its JSON text, each payload field left the raw
    /// text it is. Unknown fields are ignored and a field given twice reads
    /// as its last value, as in a decoded object; an unknown `status` is kept
    /// (as [`InvocationStatus::Unknown`]) for the engine to drop.
    pub fn from_raw_lenient(text: &str) -> Result<ObservedItem, RejectedItem> {
        let Ok(fields) = serde_json::from_str::<RawItemFields>(text) else {
            return Err(RejectedItem {
                function_id: None,
                reason: "not an object".to_string(),
            });
        };
        let Some(function_id) = fields.function_id.and_then(decode_raw::<String>) else {
            return Err(RejectedItem {
                function_id: None,
                reason: "`function_id` is missing or not a string".to_string(),
            });
        };
        let reject = |field: &str| RejectedItem {
            function_id: Some(function_id.clone()),
            reason: format!("`{field}` is missing or malformed"),
        };

        let namespace = optional_string(fields.namespace).map_err(|_| reject("namespace"))?;
        let Some(invocation_id) = fields.invocation_id.and_then(decode_raw::<String>) else {
            return Err(reject("invocation_id"));
        };
        let matched = match present(fields.matched) {
            None => Vec::new(),
            Some(ids) => decode_raw::<Vec<String>>(ids).ok_or_else(|| reject("matched"))?,
        };
        let Some(status) = fields.status.and_then(decode_raw::<String>) else {
            return Err(reject("status"));
        };
        let caller = match present(fields.caller) {
            None => None,
            // Through a value, as a decoded item reads it: a key given twice
            // keeps its last value.
            Some(caller) => Some(
                decode_raw::<Value>(caller)
                    .and_then(|caller| serde_json::from_value(caller).ok())
                    .ok_or_else(|| reject("caller"))?,
            ),
        };
        let started_at_ms = fields
            .started_at_ms
            .and_then(decode_raw::<u64>)
            .ok_or_else(|| reject("started_at_ms"))?;
        let duration_ms = fields
            .duration_ms
            .and_then(decode_raw::<f64>)
            .ok_or_else(|| reject("duration_ms"))?;
        let observe_depth = match present(fields.observe_depth) {
            None => 0,
            Some(depth) => decode_raw::<u64>(depth)
                .and_then(|depth| u32::try_from(depth).ok())
                .ok_or_else(|| reject("observe_depth"))?,
        };
        let traceparent = optional_string(fields.traceparent).map_err(|_| reject("traceparent"))?;
        let baggage = optional_string(fields.baggage).map_err(|_| reject("baggage"))?;
        let output_is_input = match present(fields.output_is_input) {
            None => false,
            Some(flag) => decode_raw::<bool>(flag).ok_or_else(|| reject("output_is_input"))?,
        };

        Ok(ObservedItem {
            function_id,
            namespace,
            invocation_id,
            matched,
            status: InvocationStatus::from(status.as_str()),
            input: fields.input.map(RawJson::of),
            output: fields.output.map(RawJson::of),
            error: fields.error.map(RawJson::of),
            caller,
            metadata: fields.metadata.map(RawJson::of),
            started_at_ms,
            duration_ms,
            observe_depth,
            traceparent,
            baggage,
            output_is_input,
        })
    }
}

/// A JSON value kept as its text: an `invocationsobserved` payload, read off
/// the frame without being decoded, sent on as is, and decoded only for a
/// reader that needs a [`Value`]. Always valid JSON; two are equal when their
/// texts are.
#[derive(Clone, PartialEq, Eq)]
pub struct RawJson(Arc<str>);

impl RawJson {
    /// `value`'s JSON text.
    pub fn from_value(value: &Value) -> Self {
        Self(Arc::from(value.to_string()))
    }

    fn of(raw: &RawValue) -> Self {
        Self(Arc::from(raw.get()))
    }

    /// Its JSON text.
    pub fn get(&self) -> &str {
        &self.0
    }

    /// Its JSON text, shared.
    pub fn into_shared(self) -> Arc<str> {
        self.0
    }

    /// The value it encodes; `None` for valid JSON a [`Value`] cannot hold
    /// (a number out of range, nesting past the decoder's limit).
    pub fn to_value(&self) -> Option<Value> {
        serde_json::from_str(&self.0).ok()
    }
}

impl fmt::Debug for RawJson {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for RawJson {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Valid JSON: written as it is.
        let raw: &RawValue = serde_json::from_str(&self.0).map_err(serde::ser::Error::custom)?;
        raw.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RawJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Box::<RawValue>::deserialize(deserializer).map(|raw| Self::of(&raw))
    }
}

/// A raw field unless absent or `null`, which an optional field reads alike.
fn present(raw: Option<&RawValue>) -> Option<&RawValue> {
    raw.filter(|raw| raw.get() != "null")
}

/// A raw field decoded as a `T`; `None` when it is not one.
fn decode_raw<'a, T: Deserialize<'a>>(raw: &'a RawValue) -> Option<T> {
    serde_json::from_str(raw.get()).ok()
}

/// An optional string field: `Err` when present and not a string.
fn optional_string(raw: Option<&RawValue>) -> Result<Option<String>, ()> {
    match present(raw) {
        None => Ok(None),
        Some(raw) => decode_raw(raw).map(Some).ok_or(()),
    }
}

/// The fields of one `invocationsobserved` item, each the raw JSON text it
/// was given, for [`ObservedItem::from_raw_lenient`] to read.
#[derive(Default)]
struct RawItemFields<'a> {
    function_id: Option<&'a RawValue>,
    namespace: Option<&'a RawValue>,
    invocation_id: Option<&'a RawValue>,
    matched: Option<&'a RawValue>,
    status: Option<&'a RawValue>,
    input: Option<&'a RawValue>,
    output: Option<&'a RawValue>,
    error: Option<&'a RawValue>,
    caller: Option<&'a RawValue>,
    metadata: Option<&'a RawValue>,
    started_at_ms: Option<&'a RawValue>,
    duration_ms: Option<&'a RawValue>,
    observe_depth: Option<&'a RawValue>,
    traceparent: Option<&'a RawValue>,
    baggage: Option<&'a RawValue>,
    output_is_input: Option<&'a RawValue>,
}

/// The fields of an `invocationsobserved` item the engine reads.
#[derive(Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum ItemField {
    FunctionId,
    Namespace,
    InvocationId,
    Matched,
    Status,
    Input,
    Output,
    Error,
    Caller,
    Metadata,
    StartedAtMs,
    DurationMs,
    ObserveDepth,
    Traceparent,
    Baggage,
    OutputIsInput,
    #[serde(other)]
    Unknown,
}

impl<'de> Deserialize<'de> for RawItemFields<'de> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Fields;

        impl<'de> serde::de::Visitor<'de> for Fields {
            type Value = RawItemFields<'de>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an invocationsobserved item")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut fields = RawItemFields::default();
                while let Some(field) = map.next_key()? {
                    let slot = match field {
                        ItemField::FunctionId => &mut fields.function_id,
                        ItemField::Namespace => &mut fields.namespace,
                        ItemField::InvocationId => &mut fields.invocation_id,
                        ItemField::Matched => &mut fields.matched,
                        ItemField::Status => &mut fields.status,
                        ItemField::Input => &mut fields.input,
                        ItemField::Output => &mut fields.output,
                        ItemField::Error => &mut fields.error,
                        ItemField::Caller => &mut fields.caller,
                        ItemField::Metadata => &mut fields.metadata,
                        ItemField::StartedAtMs => &mut fields.started_at_ms,
                        ItemField::DurationMs => &mut fields.duration_ms,
                        ItemField::ObserveDepth => &mut fields.observe_depth,
                        ItemField::Traceparent => &mut fields.traceparent,
                        ItemField::Baggage => &mut fields.baggage,
                        ItemField::OutputIsInput => &mut fields.output_is_input,
                        ItemField::Unknown => {
                            map.next_value::<serde::de::IgnoredAny>()?;
                            continue;
                        }
                    };
                    // A field given twice: its last value, as in a decoded
                    // object.
                    *slot = Some(map.next_value()?);
                }
                Ok(fields)
            }
        }

        deserializer.deserialize_map(Fields)
    }
}

/// The text an `invocationsobserved` frame starts with as the SDKs write it.
const OBSERVED_FRAME_HEAD: &[u8] = br#"{"type":"invocationsobserved""#;

/// The fields of an `invocationsobserved` frame [`Message::from_frame`]
/// reads.
#[derive(Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum FrameField {
    Type,
    Items,
    #[serde(other)]
    Unknown,
}

/// An `invocationsobserved` frame's items, read in the one pass that scans
/// the frame (see [`Message::from_frame`]).
struct ObservedFrame(ObservedItems);

impl<'de> Deserialize<'de> for ObservedFrame {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Frame;

        impl<'de> serde::de::Visitor<'de> for Frame {
            type Value = ObservedFrame;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an invocationsobserved frame")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                use serde::de::Error;
                let (mut tagged, mut items) = (false, None);
                while let Some(field) = map.next_key()? {
                    match field {
                        FrameField::Type if !tagged => {
                            tagged = map.next_value::<&str>()? == "invocationsobserved";
                            if !tagged {
                                return Err(A::Error::custom("another type"));
                            }
                        }
                        FrameField::Items if items.is_none() => {
                            items = Some(map.next_value::<RawItems>()?.0);
                        }
                        // Given twice: what `Message` makes of it.
                        FrameField::Type | FrameField::Items => {
                            return Err(A::Error::custom("a field given twice"));
                        }
                        FrameField::Unknown => {
                            map.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                if !tagged {
                    return Err(A::Error::custom("no type"));
                }
                Ok(ObservedFrame(items.unwrap_or_default()))
            }
        }

        deserializer.deserialize_map(Frame)
    }
}

/// An `invocationsobserved` frame's `items`: `null` as none, an array as its
/// items, each decoded on its own off its raw text. Anything else is refused,
/// for `Message` to read it as one rejected item.
struct RawItems(ObservedItems);

impl<'de> Deserialize<'de> for RawItems {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Items;

        impl<'de> serde::de::Visitor<'de> for Items {
            type Value = RawItems;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an array of items, or null")
            }

            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(RawItems(ObservedItems::default()))
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut out = ObservedItems::default();
                while let Some(item) = seq.next_element::<&'de RawValue>()? {
                    match ObservedItem::from_raw_lenient(item.get()) {
                        Ok(item) => out.items.push(item),
                        Err(rejected) => out.rejected.push(rejected),
                    }
                }
                Ok(RawItems(out))
            }
        }

        deserializer.deserialize_any(Items)
    }
}

impl Message {
    /// Decodes a frame a worker sent, as `serde_json::from_str::<Message>`
    /// does. An `invocationsobserved` frame written as the SDKs write it
    /// (`type` first) is read in one pass instead, its items' payloads kept as
    /// the raw text they came in ([`RawJson`]): through `Message`, its tag
    /// buffers the whole frame as a tree, which is then decoded again into
    /// values. What that pass does not take (a field given twice, malformed
    /// JSON) decodes as any other frame does, to the same result.
    pub fn from_frame(text: &str) -> serde_json::Result<Message> {
        if text.as_bytes().starts_with(OBSERVED_FRAME_HEAD)
            && let Ok(ObservedFrame(items)) = serde_json::from_str(text)
        {
            return Ok(Message::InvocationsObserved { items });
        }
        serde_json::from_str(text)
    }

    /// [`Message::from_frame`] for a binary frame.
    pub fn from_frame_bytes(bytes: &[u8]) -> serde_json::Result<Message> {
        if bytes.starts_with(OBSERVED_FRAME_HEAD)
            && let Ok(text) = std::str::from_utf8(bytes)
        {
            return Self::from_frame(text);
        }
        serde_json::from_slice(bytes)
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
/// from [`observe_codes`] (`BUFFER_FULL`, `TOO_LARGE`, `DISCONNECTED`).
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

/// The text of an engine-to-worker `invokefunction` whose `data` is already
/// encoded: the message [`Message::InvokeFunction`] with these fields is,
/// with `data` written last (and no `action` or `namespace`, which no
/// invocation the engine sends a worker carries). The activations of one
/// observed event share its encoded payloads this way instead of each
/// copying and encoding them, also while they wait for the connection's
/// writer: the text stays in `data`'s pieces, and the writer sends the
/// pieces without joining them. `Err` gives `data` back when a field does
/// not encode (not expected: every one does).
#[allow(clippy::too_many_arguments)]
pub fn invoke_frame(
    invocation_id: Option<Uuid>,
    function_id: &str,
    data: crate::invocation::encoded::EncodedJson,
    traceparent: Option<&str>,
    baggage: Option<&str>,
    metadata: Option<&Value>,
    caller: Option<&Caller>,
    observe_depth: Option<u32>,
    activation: Option<&Activation>,
) -> Result<crate::invocation::encoded::EncodedJson, crate::invocation::encoded::EncodedJson> {
    /// Every field of `Message::InvokeFunction` but `data`, written as it
    /// writes them.
    #[derive(Serialize)]
    struct Head<'a> {
        #[serde(rename = "type")]
        kind: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        invocation_id: Option<Uuid>,
        function_id: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        traceparent: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        baggage: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<&'a Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        caller: Option<&'a Caller>,
        #[serde(skip_serializing_if = "Option::is_none")]
        observe_depth: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        activation: Option<&'a Activation>,
    }
    let Ok(mut head) = serde_json::to_string(&Head {
        kind: "invokefunction",
        invocation_id,
        function_id,
        traceparent,
        baggage,
        metadata,
        caller,
        observe_depth,
        activation,
    }) else {
        return Err(data);
    };
    // `head` is an object with at least `type` and `function_id`: `data`
    // goes before its closing brace.
    head.pop();
    head.push_str(",\"data\":");
    Ok(data.enclosed(head, "}"))
}

#[cfg(test)]
mod tests {
    use super::{CallAction, DEFAULT_NAMESPACE, Message, effective_namespace};
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
                    Some(CallAction::Enqueue { queue }) => {
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
                    matches!(action, Some(CallAction::Void)),
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
        Activation, Caller, Capture, Deliver, InvocationStatus, ObserveDescriptor, ObservedItem,
        ObserverState, On, RawJson, default_capture,
    };
    use serde_json::{Value, json};

    /// Keys observe adds to existing variants. None of them may appear on the
    /// wire unless set.
    const NEW_KEYS: [&str; 10] = [
        "observe",
        "args_format",
        "private",
        "observable",
        "caller",
        "observe_depth",
        "activation",
        "features",
        "matched",
        "delivered",
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
            args_format,
            private,
            observable,
            ..
        } = register
        else {
            panic!("expected RegisterFunction");
        };
        assert!(observe.is_none() && request_format.is_none() && response_format.is_none());
        assert!(args_format.is_none() && !private && observable);

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
            args_format: None,
            private: false,
            observable: true,
            observe: None,
        });
        assert_no_new_keys(&Message::InvocationResult {
            invocation_id: uuid::Uuid::new_v4(),
            function_id: "orders::create".into(),
            result: Some(json!({ "id": 1 })),
            error: None,
            traceparent: None,
            baggage: None,
            matched: None,
            delivered: false,
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
            action: Some(CallAction::Void),
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

    /// The 0.25 `observe` object: `auto` or no delivery is supported, `allow`
    /// defaults to `true`, and the keys the engine ignores survive a round
    /// trip.
    #[test]
    fn the_025_observe_object_is_decoded() {
        let empty = ObserveDescriptor::default();
        assert!(empty.is_supported() && empty.allow());
        assert_eq!(serde_json::to_value(&empty).unwrap(), json!({}));

        let refused: ObserveDescriptor =
            serde_json::from_value(json!({ "delivery": "auto", "allow": false })).unwrap();
        assert!(refused.is_supported() && !refused.allow());

        for delivery in ["owner", "broadcast"] {
            let descriptor: ObserveDescriptor =
                serde_json::from_value(json!({ "delivery": delivery })).unwrap();
            assert!(!descriptor.is_supported(), "{delivery}");
        }

        let wire = json!({ "delivery": "owner", "args_format": { "type": "object" } });
        let descriptor: ObserveDescriptor = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(descriptor.ignored.len(), 1);
        assert_eq!(serde_json::to_value(&descriptor).unwrap(), wire);
    }

    /// `private: false`, `observable: true`, an absent `matched` and
    /// `delivered: false` are the defaults: left out on the wire, and `null`
    /// reads as the default.
    #[test]
    fn function_options_and_routed_answers_default_when_absent_or_null() {
        let register: Message = serde_json::from_value(json!({
            "type": "registerfunction", "id": "vault::read",
            "args_format": null, "private": null, "observable": null
        }))
        .unwrap();
        let Message::RegisterFunction {
            args_format,
            private,
            observable,
            ..
        } = &register
        else {
            panic!("expected RegisterFunction");
        };
        assert!(args_format.is_none() && !private && *observable);
        assert_no_new_keys(&register);

        let id = "8d3c6f9e-0f4b-4e6a-9c1d-2b3a4c5d6e7f";
        let answer = |fields: Value| -> (Option<Vec<String>>, bool) {
            let mut frame = json!({
                "type": "invocationresult", "invocation_id": id,
                "function_id": "orders::create", "result": 1
            });
            frame
                .as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            let message: Message =
                serde_json::from_value(frame).expect("a malformed field does not cost the result");
            let Message::InvocationResult {
                matched, delivered, ..
            } = message
            else {
                panic!("expected InvocationResult");
            };
            (matched, delivered)
        };
        let ids = |ids: &[&str]| Some(ids.iter().map(|id| id.to_string()).collect::<Vec<_>>());
        // Not narrowed: absent, or `null`.
        assert_eq!(answer(json!({})), (None, false));
        assert_eq!(
            answer(json!({ "matched": null, "delivered": null })),
            (None, false)
        );
        // Narrowed: the string ids, possibly none; a malformed list is none.
        assert_eq!(
            answer(json!({ "matched": ["o-1", 7, null, "o-2"] })),
            (ids(&["o-1", "o-2"]), false),
            "a malformed id is skipped"
        );
        assert_eq!(answer(json!({ "matched": [] })), (ids(&[]), false));
        assert_eq!(answer(json!({ "matched": "o-1" })), (ids(&[]), false));
        // Delivered by the host: only `true`.
        assert_eq!(answer(json!({ "delivered": true })), (None, true));
        assert_eq!(answer(json!({ "delivered": "yes" })), (None, false));
        assert_eq!(answer(json!({ "delivered": 1 })), (None, false));
        // An empty list is sent; an absent one and `false` are not.
        let wire = serde_json::to_value(Message::InvocationResult {
            invocation_id: uuid::Uuid::parse_str(id).unwrap(),
            function_id: "orders::create".into(),
            result: Some(json!(1)),
            error: None,
            traceparent: None,
            baggage: None,
            matched: Some(Vec::new()),
            delivered: false,
        })
        .unwrap();
        assert_eq!(wire["matched"], json!([]));
        assert!(wire.get("delivered").is_none());
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
    }

    #[test]
    fn wire_enums_spell_their_values_in_lowercase() {
        for (value, wire) in [
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
            item.input.as_ref().map(RawJson::get),
            Some("null"),
            "the function was called with null"
        );
        assert_eq!(
            item.output.as_ref().map(RawJson::get),
            Some("null"),
            "the function returned null"
        );
        assert_eq!(item.error, None);
        assert_eq!(item.metadata, None);
        assert_eq!(item.observe_depth, 0);
        assert_eq!(item.status, InvocationStatus::Success);
        assert!(!item.output_is_input);

        let wire = serde_json::to_value(&msg).unwrap();
        assert_eq!(wire["items"][0]["output"], Value::Null);
        assert!(wire["items"][0].get("error").is_none());
        assert!(wire["items"][0].get("output_is_input").is_none());
    }

    /// An event source's item carries its event once, as `input`, with
    /// `output_is_input`; a value that is not a boolean sets the item aside.
    #[test]
    fn output_is_input_is_read_from_an_item() {
        let item = |flag: Value| {
            json!({
                "function_id": "database::on-row-changed", "invocation_id": "i-1",
                "matched": ["o-1"], "status": "success", "input": { "op": "insert" },
                "output_is_input": flag, "started_at_ms": 1, "duration_ms": 0.5
            })
        };
        let msg: Message = serde_json::from_value(json!({
            "type": "invocationsobserved",
            "items": [item(json!(true)), item(Value::Null), item(json!("yes"))]
        }))
        .unwrap();
        let Message::InvocationsObserved { items } = &msg else {
            panic!("expected InvocationsObserved");
        };
        assert!(items.items[0].output_is_input);
        assert!(!items.items[1].output_is_input, "null reads as false");
        assert_eq!(items.rejected.len(), 1);
        let wire = serde_json::to_value(&msg).unwrap();
        assert_eq!(wire["items"][0]["output_is_input"], json!(true));
        assert!(wire["items"][1].get("output_is_input").is_none());
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

    /// A report written as the SDKs write it keeps its payloads as the text
    /// they came in, `null` apart from absent; read through `Message`, the
    /// same frame gives the same values.
    #[test]
    fn a_reports_payloads_stay_the_text_they_came_in() {
        let frame = r#"{"type":"invocationsobserved","items":[{"function_id":"orders::archive","invocation_id":"i-1","matched":["o-1"],"status":"error","input":{"z": 1, "a": [1.50, "\u00e9"]},"output":null,"error":{"code":"E","message":"m"},"started_at_ms":1,"duration_ms":0.5}]}"#;
        let Message::InvocationsObserved { items } = Message::from_frame(frame).unwrap() else {
            panic!("expected InvocationsObserved");
        };
        let item = &items.items[0];
        let text = |part: &Option<RawJson>| part.as_ref().map(|raw| raw.get().to_string());
        assert_eq!(
            text(&item.input).as_deref(),
            Some(r#"{"z": 1, "a": [1.50, "\u00e9"]}"#)
        );
        assert_eq!(text(&item.output).as_deref(), Some("null"));
        assert_eq!(
            text(&item.error).as_deref(),
            Some(r#"{"code":"E","message":"m"}"#)
        );
        assert_eq!(item.metadata, None);

        let Message::InvocationsObserved { items: through } =
            serde_json::from_str::<Message>(frame).unwrap()
        else {
            panic!("expected InvocationsObserved");
        };
        let value = |item: &ObservedItem| item.input.as_ref().and_then(RawJson::to_value);
        assert_eq!(value(&through.items[0]), value(item));
        assert_eq!(value(item), Some(json!({ "z": 1, "a": [1.5, "\u{e9}"] })));
    }

    /// `Message::from_frame` reads every frame as `Message` does: the same
    /// items, field by field and payload by payload value, the same rejected
    /// items, the same errors.
    #[test]
    fn a_frame_reads_alike_through_from_frame() {
        // Over a valid item: a field given here is given twice, and its last
        // value counts.
        let with = |field: &str| {
            format!(
                r#"{{"function_id":"f","invocation_id":"i-9","status":"success","started_at_ms":5,"duration_ms":0.5,{field}}}"#
            )
        };
        let mut items = vec![
            r#"{"function_id":"f","namespace":"ns","invocation_id":"i-1","matched":["o-1","o-2"],"status":"success","input":{"n":1,"_caller_worker_id":"w"},"output":[1,2.5,null],"error":null,"caller":{"worker_name":"w","worker_name":"x"},"metadata":{"t":1},"started_at_ms":5,"duration_ms":1,"observe_depth":2,"traceparent":"00-ab","baggage":"k=v","output_is_input":false,"unknown":{"x":[1]}}"#.to_string(),
            r#"{"function_id":"f","invocation_id":"i-2","\u006fbserve_depth":3,"status":"error","started_at_ms":5,"duration_ms":0.5,"matched":null,"output_is_input":null,"traceparent":null}"#.to_string(),
            r#"{"function_id":"f","invocation_id":"i-3","status":"paused","started_at_ms":5,"duration_ms":0.5}"#.to_string(),
            r#""not an item""#.to_string(),
            "[1]".to_string(),
            r#"{"invocation_id":"i-4"}"#.to_string(),
            r#"{"function_id":7}"#.to_string(),
            r#"{"function_id":"f","status":"success","started_at_ms":5,"duration_ms":0.5}"#.to_string(),
        ];
        items.extend(
            [
                r#""namespace":7"#,
                r#""invocation_id":"i-10""#,
                r#""matched":[1]"#,
                r#""matched":"o-1""#,
                r#""status":7"#,
                r#""caller":"w""#,
                r#""started_at_ms":1.5"#,
                r#""started_at_ms":-1"#,
                r#""duration_ms":"1""#,
                r#""observe_depth":4294967296"#,
                r#""observe_depth":1.0"#,
                r#""traceparent":1"#,
                r#""baggage":false"#,
                r#""output_is_input":"yes""#,
                r#""output_is_input":true,"input":{"op":"insert"}"#,
            ]
            .map(with),
        );
        let items = format!("[{}]", items.join(","));
        let frames = [
            format!(r#"{{"type":"invocationsobserved","items":{items}}}"#),
            format!(r#"{{"items":{items},"type":"invocationsobserved"}}"#),
            r#"{"type":"invocationsobserved","items":null}"#.to_string(),
            r#"{"type":"invocationsobserved","items":"nope"}"#.to_string(),
            r#"{"type":"invocationsobserved","items":{"a":1}}"#.to_string(),
            r#"{"type":"invocationsobserved"}"#.to_string(),
            r#"{"type":"invocationsobserved","extra":{"x":[1]},"items":[]}"#.to_string(),
            r#"{"type":"invocationsobserved","items":[],"items":[]}"#.to_string(),
            r#"{"type":"invocationsobserved","type":"invocationsobserved","items":[]}"#.to_string(),
            r#"{"type":"invocationsobserved","items":[{"function_id":"f"}"#.to_string(),
            r#"{"type":"invocationsobserved","items":[]} trailing"#.to_string(),
            r#"{"type":"invocationsobservedx","items":[]}"#.to_string(),
            r#"{"type":"unobserve","id":"o-1"}"#.to_string(),
        ];
        for frame in &frames {
            match (
                Message::from_frame(frame),
                serde_json::from_str::<Message>(frame),
            ) {
                (Ok(fast), Ok(slow)) => {
                    assert_eq!(
                        serde_json::to_value(&fast).unwrap(),
                        serde_json::to_value(&slow).unwrap(),
                        "{frame}"
                    );
                    if let (
                        Message::InvocationsObserved { items: fast },
                        Message::InvocationsObserved { items: slow },
                    ) = (&fast, &slow)
                    {
                        assert_eq!(fast.rejected, slow.rejected, "{frame}");
                    }
                }
                (Err(fast), Err(slow)) => assert_eq!(fast.to_string(), slow.to_string(), "{frame}"),
                (fast, slow) => panic!("{frame}: {fast:?} / {slow:?}"),
            }
        }
        let Message::InvocationsObserved { items } = Message::from_frame(&frames[0]).unwrap()
        else {
            panic!("expected InvocationsObserved");
        };
        assert_eq!((items.items.len(), items.rejected.len()), (5, 18));
        assert_eq!(
            items.items[1].observe_depth, 3,
            "an escaped key reads as its name"
        );
        assert_eq!(
            items.items[3].invocation_id, "i-10",
            "the last value counts"
        );
        assert!(items.items[4].output_is_input);
        // The binary path reads it alike.
        let Message::InvocationsObserved { items: binary } =
            Message::from_frame_bytes(frames[0].as_bytes()).unwrap()
        else {
            panic!("expected InvocationsObserved");
        };
        assert_eq!(binary, items);
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
    fn engine_advertises_observe_v1_v2_and_local_dispatch() {
        assert_eq!(
            super::engine_features(),
            vec![
                "observe.v1".to_string(),
                "observe.v2".to_string(),
                "local-dispatch.v1".to_string()
            ]
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

    /// An encoded `invokefunction` frame is the message the engine would
    /// write for the same fields.
    #[test]
    fn an_invoke_frame_is_the_invokefunction_message() {
        use super::{Activation, Caller, invoke_frame};
        use crate::invocation::encoded::EncodedJson;
        use serde_json::{Value, json};

        let data = json!({ "observer_id": "o-1", "input": { "k": [1, "\"two\""] } });
        let caller = Caller {
            worker_name: Some("iii-engine".to_string()),
            function_id: Some("orders::create".to_string()),
            ..Caller::default()
        };
        let activation = Activation {
            observer_id: "o-1".to_string(),
            activation_id: Some("a-1".to_string()),
        };
        let invocation_id = uuid::Uuid::new_v4();
        let metadata = json!({ "tenant": "t" });
        let message = Message::InvokeFunction {
            invocation_id: Some(invocation_id),
            function_id: "audit::record".to_string(),
            data: data.clone(),
            traceparent: Some(
                "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01".to_string(),
            ),
            baggage: Some("k=v".to_string()),
            action: None,
            metadata: Some(metadata.clone()),
            namespace: None,
            caller: Some(Box::new(caller.clone())),
            observe_depth: Some(2),
            activation: Some(activation.clone()),
        };
        let frame = invoke_frame(
            Some(invocation_id),
            "audit::record",
            EncodedJson::of(&data),
            Some("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"),
            Some("k=v"),
            Some(&metadata),
            Some(&caller),
            Some(2),
            Some(&activation),
        )
        .unwrap()
        .into_text();
        assert_eq!(
            serde_json::from_str::<Value>(&frame).unwrap(),
            serde_json::to_value(&message).unwrap()
        );
        assert!(matches!(
            serde_json::from_str::<Message>(&frame).unwrap(),
            Message::InvokeFunction { data: decoded, .. } if decoded == data
        ));

        // The optional fields absent, as on a worker without `observe.v1`.
        let bare = invoke_frame(
            None,
            "f",
            EncodedJson::of(&Value::Null),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap()
        .into_text();
        assert_eq!(
            serde_json::from_str::<Value>(&bare).unwrap(),
            json!({ "type": "invokefunction", "function_id": "f", "data": null })
        );
    }

    /// The exact text of an encoded `invokefunction` frame, the message a
    /// worker reassembles: the same whichever pieces the engine keeps it in.
    #[test]
    fn an_invoke_frame_is_this_exact_text() {
        use super::{Activation, Caller, invoke_frame};
        use crate::invocation::encoded::ObjectWriter;
        use serde_json::json;
        use std::sync::Arc;

        let payload: Arc<str> = Arc::from(json!({ "k": [1, "\u{e9}\"two\""] }).to_string());
        let mut writer = ObjectWriter::new();
        writer.field("observer_id", &"o-1");
        writer.shared_field("input", &payload);
        writer.field("duration_ms", &1.5);
        let data = writer.finish();
        let caller = Caller {
            worker_name: Some("iii-engine".to_string()),
            function_id: Some("orders::create".to_string()),
            ..Caller::default()
        };
        let activation = Activation {
            observer_id: "o-1".to_string(),
            activation_id: Some("a-1".to_string()),
        };
        let invocation_id: uuid::Uuid = "8d3c6f9e-0f4b-4e6a-9c1d-2b3a4c5d6e7f".parse().unwrap();
        let frame = invoke_frame(
            Some(invocation_id),
            "audit::record",
            data,
            Some("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"),
            Some("k=v"),
            Some(&json!({ "tenant": "t" })),
            Some(&caller),
            Some(2),
            Some(&activation),
        )
        .unwrap();
        // The text the engine wrote before it kept frames in pieces.
        let before = concat!(
            r#"{"type":"invokefunction","invocation_id":"8d3c6f9e-0f4b-4e6a-9c1d-2b3a4c5d6e7f","#,
            r#""function_id":"audit::record","#,
            r#""traceparent":"00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01","#,
            r#""baggage":"k=v","metadata":{"tenant":"t"},"#,
            r#""caller":{"worker_name":"iii-engine","function_id":"orders::create"},"#,
            r#""observe_depth":2,"activation":{"observer_id":"o-1","activation_id":"a-1"},"#,
            "\"data\":{\"observer_id\":\"o-1\",\"input\":{\"k\":[1,\"\u{e9}\\\"two\\\"\"]},\"duration_ms\":1.5}}",
        );
        assert_eq!(frame.len(), before.len());
        // The frame shares the event's payload instead of copying it.
        assert_eq!(Arc::strong_count(&payload), 2);
        assert_eq!(frame.into_text(), before);
    }
}
