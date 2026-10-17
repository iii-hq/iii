use iii_helpers::http::HttpInvocationConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// [`Message::RegistrationRejected`] code: another live worker already holds
/// this `(namespace, worker_name)`. The engine closes the connection; the SDK
/// must stop and not reconnect. Mirrors the engine constant of the same name.
pub const WORKER_NAMESPACE_CONFLICT: &str = "WORKER_NAMESPACE_CONFLICT";

/// [`Message::RegistrationRejected`] code: another live worker in this
/// namespace already exports this function id. Only that one registration is
/// refused; the connection stays open and the worker keeps serving its other
/// functions. Mirrors the engine constant of the same name.
pub const FUNCTION_NAMESPACE_CONFLICT: &str = "FUNCTION_NAMESPACE_CONFLICT";

/// Response header in which the engine lists optional transport capabilities
/// on every worker-listener response, including the WebSocket upgrade, as
/// comma-separated tokens. Workers ignore tokens they do not know.
pub const ENGINE_CAPABILITIES_HEADER: &str = "x-iii-capabilities";

/// Capability token: the engine's end of the connection has `TCP_NODELAY`, so
/// the worker may disable Nagle on its end too (MOT-5377). A worker must not
/// disable it without this token: Nagle on the engine's end alone is slower
/// than on both ends. The token describes the engine's own socket: behind a
/// proxy that forwards the header, the worker's peer is the proxy, which
/// should then set `TCP_NODELAY` on its side too (nginx and Envoy do).
pub const ENGINE_CAPABILITY_TCP_NODELAY: &str = "tcp-nodelay";

/// Routing action for [`TriggerRequest`]. Determines how the engine handles
/// the invocation.
///
/// - `Enqueue`: Routes through a named queue for async processing.
/// - `Void`: Fire-and-forget, no response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TriggerAction {
    /// Routes the invocation through a named queue.
    Enqueue { queue: String },
    /// Fire-and-forget routing.
    Void,
}

/// Request object for `trigger()`.
///
/// ```rust
/// # use iii_sdk::protocol::{TriggerRequest, TriggerAction};
/// # use serde_json::json;
/// // Simple call
/// TriggerRequest {
///     function_id: "my::function".to_string(),
///     payload: json!({ "key": "value" }),
///     action: None,
///     timeout_ms: None,
/// };
///
/// // With action
/// TriggerRequest {
///     function_id: "my::function".to_string(),
///     payload: json!({}),
///     action: Some(TriggerAction::Enqueue { queue: "payments".to_string() }),
///     timeout_ms: None,
/// };
///
/// // With metadata
/// TriggerRequest {
///     function_id: "my::function".to_string(),
///     payload: json!({}),
///     action: None,
///     timeout_ms: None,
/// }
/// .metadata(json!({ "tenant": "acme" }));
/// ```
#[derive(Debug, Clone)]
pub struct TriggerRequest {
    /// ID of the function to invoke.
    pub function_id: String,
    /// Input data passed to the function.
    pub payload: Value,
    /// Sets how the trigger is routed. `None` for a synchronous request/response.
    /// Set a routing scheme otherwise (e.g. `TriggerAction::Enqueue { .. }`, `TriggerAction::Void`).
    pub action: Option<TriggerAction>,
    /// Override the default invocation timeout, in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl TriggerRequest {
    /// A synchronous request/response call of `function_id` with `payload`,
    /// the default timeout and no routing action.
    ///
    /// ```rust
    /// # use iii_sdk::protocol::TriggerRequest;
    /// # use serde_json::json;
    /// let request = TriggerRequest::new("orders::create", json!({ "sku": "A-1" }));
    /// assert!(request.action.is_none());
    /// ```
    pub fn new(function_id: impl Into<String>, payload: Value) -> Self {
        Self {
            function_id: function_id.into(),
            payload,
            action: None,
            timeout_ms: None,
        }
    }

    /// Attach per-invocation metadata without adding a required field to
    /// [`TriggerRequest`] struct literals.
    pub fn metadata(self, metadata: Value) -> TriggerRequestWithMetadata {
        TriggerRequestWithMetadata::from(self).metadata(metadata)
    }

    /// Target a specific namespace for this invocation without adding a
    /// required field to [`TriggerRequest`] struct literals. Serializes into
    /// [`Message::InvokeFunction`]'s `namespace`.
    ///
    /// Leaving it unset does not mean the engine's default: the call inherits
    /// this worker's namespace. Say `default` to reach the engine's from a
    /// namespaced worker.
    pub fn namespace(self, namespace: impl Into<String>) -> TriggerRequestWithMetadata {
        TriggerRequestWithMetadata::from(self).namespace(namespace)
    }

    /// Choose the path this call takes: [`Dispatch::Auto`] (the default) runs
    /// a function this worker hosts in-process when the engine allows it;
    /// [`Dispatch::Remote`] always goes through the engine.
    pub fn dispatch(self, dispatch: Dispatch) -> TriggerRequestWithMetadata {
        TriggerRequestWithMetadata::from(self).dispatch(dispatch)
    }
}

/// Which path [`IIIClient::trigger`](crate::IIIClient::trigger) takes.
///
/// A call to a function registered by this same worker runs in-process
/// ("local dispatch") when all of these hold:
///
/// 1. the function has a handler here (not [`RegisterFunction::http`](crate::RegisterFunction::http))
///    and was not registered with [`RegisterFunction::local_dispatch(false)`](crate::RegisterFunction::local_dispatch);
/// 2. the engine confirmed it with `functionregistered { local_dispatch: true }`;
/// 3. the call resolves to the namespace the function is registered in;
/// 4. the action is not [`TriggerAction::Enqueue`] (a `Void` call runs
///    locally, fire-and-forget);
/// 5. the dispatch is not [`Dispatch::Remote`].
///
/// Otherwise the call goes through the engine as before. A local call returns
/// the same result, error shape and timeout as a remote one, and is reported
/// to the engine only when an observer's filters match it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Dispatch {
    /// Run in-process when the five conditions above hold, else remote.
    #[default]
    Auto,
    /// Always route through the engine (engine-side effects, tests).
    Remote,
}

/// Trigger request plus optional per-invocation metadata, target namespace
/// and dispatch.
#[derive(Debug, Clone)]
pub struct TriggerRequestWithMetadata {
    pub(crate) request: TriggerRequest,
    pub(crate) metadata: Option<Value>,
    pub(crate) namespace: Option<String>,
    pub(crate) dispatch: Dispatch,
}

impl TriggerRequestWithMetadata {
    /// Attach per-invocation metadata.
    pub fn metadata(mut self, metadata: Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Target a specific namespace for this invocation.
    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// Choose the path this call takes; see [`Dispatch`].
    pub fn dispatch(mut self, dispatch: Dispatch) -> Self {
        self.dispatch = dispatch;
        self
    }
}

impl<T> From<T> for TriggerRequestWithMetadata
where
    T: Into<TriggerRequest>,
{
    fn from(request: T) -> Self {
        Self {
            request: request.into(),
            metadata: None,
            namespace: None,
            dispatch: Dispatch::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Message {
    RegisterTriggerType {
        id: String,
        description: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        trigger_request_format: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        call_request_format: Option<Value>,
        /// Namespace this provider serves. Absent means the connection's own.
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
        /// Namespace the trigger's target function resolves in.
        ///
        /// The engine reads an absent field as its default namespace. The SDK
        /// does not leave it absent: `register_trigger` fills it from this
        /// worker's namespace, because the function a trigger names is one this
        /// worker registered, and that landed in the worker's namespace. Name
        /// another namespace, `default` included, to bind elsewhere.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        /// Namespace to find the provider in. Absent asks the engine to resolve
        /// it: this connection's namespace first, then the engine's own.
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
        trigger_type: String,
    },
    UnregisterTriggerType {
        id: String,
        /// Namespace the provider was registered in. Absent means the sending
        /// connection's namespace. The SDK never sets it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
    },
    RegisterFunction {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_format: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        response_format: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        invocation: Option<HttpInvocationConfig>,
        /// How observers of this function are delivered. Absent means
        /// `delivery: auto`, observable, no args. The SDK-local hooks never
        /// travel.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        observe: Option<Box<ObserveDescriptor>>,
    },
    UnregisterFunction {
        id: String,
    },
    InvokeFunction {
        /// Absent on a fire-and-forget (`void`) call.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        invocation_id: Option<Uuid>,
        function_id: String,
        data: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        traceparent: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        baggage: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        action: Option<TriggerAction>,
        /// Per-invocation metadata sidecar, surfaced to the handler as a
        /// distinct argument alongside `data`. Optional and additive
        /// for wire compatibility with engines that don't send it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
        /// Target namespace for routing. Optional and additive, so older peers
        /// that don't send it stay wire-compatible, and the engine reads an
        /// absent field as its default namespace.
        ///
        /// A call made through this SDK does not arrive absent: `trigger` fills
        /// it from this worker's namespace unless the request names one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        /// Who made the call. Worker → engine: the handler the call was made
        /// from, asserted by the SDK. Engine → worker: also `worker_id` /
        /// `worker_name`, stamped by the engine from the calling connection.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caller: Option<Box<Caller>>,
        /// Depth of the causal chain in observe hops. Absent means 0.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        observe_depth: Option<u32>,
        /// Engine → worker: present on every observer activation. Worker →
        /// engine: marks an owner delivery ([`Observer::call`](crate::Observer::call)).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        activation: Option<Activation>,
    },
    InvocationResult {
        invocation_id: Uuid,
        function_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<ErrorBody>,
        #[serde(skip_serializing_if = "Option::is_none")]
        traceparent: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        baggage: Option<String>,
    },
    Ping,
    Pong,
    /// Sent to the engine as the first message of a reconnect, before the
    /// registration replay: `previous_worker_id` and `reattach_token` are
    /// the values the engine assigned via `WorkerRegistered` on the previous
    /// connection. The engine retires that connection so the replay lands on
    /// a clean slate; the token is required because worker ids alone are
    /// publicly discoverable.
    Reattach {
        previous_worker_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reattach_token: Option<String>,
    },
    WorkerRegistered {
        worker_id: String,
        /// Secret to present in `Reattach` on reconnect; absent on older
        /// engines.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reattach_token: Option<String>,
        /// Protocol features the engine supports ([`FEATURE_OBSERVE_V1`],
        /// [`FEATURE_LOCAL_DISPATCH_V1`]). Absent from engines older than
        /// 0.25; a malformed list reads as empty.
        #[serde(
            default,
            deserialize_with = "deserialize_features",
            skip_serializing_if = "Vec::is_empty"
        )]
        features: Vec<String>,
    },
    /// Pushed by the engine when a registration collides with a live worker in
    /// the same namespace. The `code` distinguishes the two cases:
    /// [`WORKER_NAMESPACE_CONFLICT`] is fatal (the engine closes the connection;
    /// the SDK stops and does not reconnect), while [`FUNCTION_NAMESPACE_CONFLICT`]
    /// refuses a single function id and keeps the connection open.
    RegistrationRejected {
        code: String,
        namespace: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        worker_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        function_id: Option<String>,
        owner_worker_id: String,
    },

    // ── observe (0.25). The engine sends the engine → worker ones only to a
    // connection that declared `observe.v1`. ──
    /// Observing worker → engine: create, or idempotently replay, an observer
    /// owned by this connection.
    Observe {
        /// Client-generated UUID v4, stable across replays.
        id: String,
        /// The left side of `iii.observe`: what is observed.
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
    /// Observing worker → engine: dispose an observer this connection owns.
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
            deserialize_with = "null_as_default",
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
        #[serde(default, deserialize_with = "null_as_default")]
        fires: u64,
    },
    /// Engine → owner: the `registerfunction` success ack, with the full
    /// current observer set. Enables local dispatch for this function.
    FunctionRegistered {
        id: String,
        #[serde(
            default = "default_namespace",
            deserialize_with = "null_as_default_namespace"
        )]
        namespace: String,
        revision: u64,
        #[serde(default, deserialize_with = "null_as_default")]
        local_dispatch: bool,
        #[serde(default, deserialize_with = "null_as_default")]
        observers: Vec<ObserverView>,
    },
    /// Engine → owner: one observer attached to a function this connection
    /// hosts. Awaits `observerack`.
    ObserverAdded {
        function_id: String,
        #[serde(
            default = "default_namespace",
            deserialize_with = "null_as_default_namespace"
        )]
        namespace: String,
        revision: u64,
        observer: ObserverView,
    },
    /// Engine → owner: one observer detached.
    ObserverRemoved {
        function_id: String,
        #[serde(
            default = "default_namespace",
            deserialize_with = "null_as_default_namespace"
        )]
        namespace: String,
        revision: u64,
        id: String,
    },
    /// Owner → engine: accepts one added observer, or rejects it with `error`
    /// (also later, for an active observer: a late rejection).
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
        #[serde(default, deserialize_with = "null_as_default")]
        items: Vec<ObservedItem>,
    },
    /// Owner → engine: local invocation counts and SDK-side report drops,
    /// every 10 s and only when non-zero.
    LocalInvocationStats {
        window_ms: u64,
        #[serde(default, deserialize_with = "null_as_default")]
        functions: Vec<LocalFunctionStats>,
        #[serde(default, deserialize_with = "null_as_default")]
        dropped_reports: Vec<DroppedReportStats>,
    },

    /// Any message type this SDK does not know, sent by a newer engine. It is
    /// ignored (logged at debug level), never sent.
    #[serde(other, skip_serializing)]
    Unknown,
}

// ─────────────────────────────── observe ───────────────────────────────────

/// Protocol feature: observers, `functionregistered` and the observe
/// messages. The SDK declares it, and enables `observe()` only when the
/// engine advertises it too.
pub const FEATURE_OBSERVE_V1: &str = "observe.v1";
/// Protocol feature: the SDK runs its own functions in-process once the engine
/// confirms them with `functionregistered { local_dispatch: true }`.
pub const FEATURE_LOCAL_DISPATCH_V1: &str = "local-dispatch.v1";
/// What this SDK declares in the `engine::workers::register` payload.
pub const SDK_FEATURES: [&str; 2] = [FEATURE_OBSERVE_V1, FEATURE_LOCAL_DISPATCH_V1];

/// The engine's default namespace, which an absent `namespace` means.
pub const DEFAULT_NAMESPACE: &str = "default";

fn default_namespace() -> String {
    DEFAULT_NAMESPACE.to_string()
}

/// Reads a `features` list leniently: the string elements of an array, and
/// nothing for any other value.
fn deserialize_features<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
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

/// An optional field sent as `null` reads as its default, like an absent one.
fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

fn null_as_default_namespace<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_else(default_namespace))
}

fn null_as_default_capture<'de, D>(deserializer: D) -> Result<Vec<Capture>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<Vec<Capture>>::deserialize(deserializer)?.unwrap_or_else(default_capture))
}

fn null_as_empty_object<'de, D>(deserializer: D) -> Result<Value, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(match Value::deserialize(deserializer)? {
        Value::Null => empty_object(),
        value => value,
    })
}

/// Keeps a present `null` apart from an absent field: `Some(Value::Null)` is
/// a function that returned `null`, `None` a value that was not captured.
fn deserialize_present<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Value::deserialize(deserializer).map(Some)
}

/// Error codes, state reasons and drop reasons of observe, exactly as the
/// engine uses them (protocol.md §4).
pub mod observe_codes {
    /// The engine lacks `observe.v1` (raised by the SDK).
    pub const UNSUPPORTED: &str = "UNSUPPORTED";
    /// Args fail the function's `args_format`, or args on a function without one.
    pub const INVALID_ARGS: &str = "INVALID_ARGS";
    /// `args_format` with `delivery: auto`.
    pub const INVALID_OBSERVE_DESCRIPTOR: &str = "INVALID_OBSERVE_DESCRIPTOR";
    pub const INVALID_FILTER: &str = "INVALID_FILTER";
    pub const INVALID_FILTER_PATH: &str = "INVALID_FILTER_PATH";
    pub const NOT_OBSERVABLE: &str = "NOT_OBSERVABLE";
    pub const FORBIDDEN: &str = "FORBIDDEN";
    pub const SELF_TARGET: &str = "SELF_TARGET";
    pub const LIMIT_EXCEEDED: &str = "LIMIT_EXCEEDED";
    pub const OBSERVER_ID_CONFLICT: &str = "OBSERVER_ID_CONFLICT";
    /// `on_observer_added` did not finish within 30 s.
    pub const OWNER_TIMEOUT: &str = "OWNER_TIMEOUT";
    /// `on_observer_added` failed with a plain error.
    pub const OBSERVE_REJECTED: &str = "OBSERVE_REJECTED";
    pub const OBSERVER_NOT_FOUND: &str = "OBSERVER_NOT_FOUND";
    pub const NOT_FUNCTION_OWNER: &str = "NOT_FUNCTION_OWNER";
    /// `emit` / `call` on an auto function.
    pub const NOT_OWNER_DELIVERED: &str = "NOT_OWNER_DELIVERED";
    /// `emit` / `call` for an observer that is not active (any more).
    pub const OBSERVER_NOT_ACTIVE: &str = "OBSERVER_NOT_ACTIVE";
    pub const TARGET_MISMATCH: &str = "TARGET_MISMATCH";
    pub const OVERFLOW: &str = "OVERFLOW";
    pub const LOOP_GUARD: &str = "LOOP_GUARD";
    /// `call` on a payload the observer's filters reject; nothing was sent.
    pub const FILTERS_NOT_MATCHED: &str = "FILTERS_NOT_MATCHED";
    pub const OWNER_UNAVAILABLE: &str = "OWNER_UNAVAILABLE";
    pub const TARGET_UNAVAILABLE: &str = "TARGET_UNAVAILABLE";
    pub const OBSERVER_DISCONNECTED: &str = "OBSERVER_DISCONNECTED";
    pub const EXPIRED: &str = "EXPIRED";
    pub const MAX_FIRES: &str = "MAX_FIRES";
    pub const UNOBSERVED: &str = "UNOBSERVED";
    /// Drop reason: a report for an observer or function that is gone.
    pub const INACTIVE: &str = "INACTIVE";
    /// Drop reason: the report buffer was full (10 000 items or 8 MiB).
    pub const BUFFER_FULL: &str = "BUFFER_FULL";
    /// Drop reason: one report item larger than a frame.
    pub const TOO_LARGE: &str = "TOO_LARGE";
}

/// Declares a wire enum carried as a lowercase string, with an
/// `Unknown(String)` catch-all: a value added by a newer peer decodes instead
/// of failing the whole frame, and re-encodes unchanged.
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
            /// A value this SDK does not know, kept verbatim.
            Unknown(String),
        }

        impl $name {
            /// The wire spelling.
            pub fn as_str(&self) -> &str {
                match self {
                    $( Self::$variant => $wire, )+
                    Self::Unknown(other) => other,
                }
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

        impl JsonSchema for $name {
            fn schema_name() -> String {
                stringify!($name).to_string()
            }

            fn json_schema(_: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
                schemars::schema::SchemaObject {
                    instance_type: Some(schemars::schema::InstanceType::String.into()),
                    enum_values: Some(vec![$( Value::from($wire) ),+]),
                    ..Default::default()
                }
                .into()
            }
        }
    };
}

wire_string_enum! {
    /// How a function's observers are delivered.
    #[derive(Default)]
    pub enum Delivery {
        /// The function's invocations are the events; every observer is
        /// offered every invocation.
        #[default]
        Auto => "auto",
        /// The owner routes events to observers by their args
        /// ([`Observer::emit`](crate::Observer::emit) / [`Observer::call`](crate::Observer::call)).
        Owner => "owner",
    }
}

wire_string_enum! {
    /// What an activation ships to the target. A filter may read any root,
    /// whatever is captured.
    pub enum Capture {
        Input => "input",
        Output => "output",
        Error => "error",
        Caller => "caller",
        Metadata => "metadata",
    }
}

/// The default `capture`: `["input", "output", "caller"]`.
pub fn default_capture() -> Vec<Capture> {
    vec![Capture::Input, Capture::Output, Capture::Caller]
}

wire_string_enum! {
    /// Which outcomes of an auto function's invocation activate an observer.
    #[derive(Default)]
    pub enum On {
        #[default]
        Success => "success",
        Error => "error",
        Complete => "complete",
    }
}

wire_string_enum! {
    /// What the target's first argument is.
    #[derive(Default)]
    pub enum Deliver {
        /// The [`Observed`](crate::Observed) envelope.
        #[default]
        Envelope => "envelope",
        /// The observed invocation's input alone.
        Input => "input",
        /// The observed invocation's output alone.
        Output => "output",
    }
}

wire_string_enum! {
    /// Observer lifecycle states.
    pub enum ObserverState {
        /// Waiting for its function's owner (and its ack) or its target.
        Pending => "pending",
        /// Delivering.
        Active => "active",
        /// Its observing worker or its target is gone for now.
        Suspended => "suspended",
        /// Rejected; see the reason.
        Error => "error",
        /// Gone for good.
        Disposed => "disposed",
    }
}

wire_string_enum! {
    /// Outcome of an observed invocation. An emitted event is always `success`.
    pub enum InvocationStatus {
        Success => "success",
        Error => "error",
    }
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

/// `registerfunction.observe`: the wire half of a function's observe hooks.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ObserveDescriptor {
    /// Absent means [`Delivery::Auto`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<Delivery>,
    /// JSON Schema the engine validates observers' `args` against. Owner
    /// delivery only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_format: Option<Value>,
    /// Old `registerTrigger({ type })` calls bind to this function when no
    /// live provider of that trigger type exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_trigger_type: Option<String>,
    /// Absent means `true`; `false` refuses observers (`NOT_OBSERVABLE`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow: Option<bool>,
}

/// The left side of `iii.observe` on the wire. Absent fields stay absent; the
/// engine applies the defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObserveSource {
    pub function_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<Vec<Capture>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<On>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deliver: Option<Deliver>,
}

/// The right side of `iii.observe` on the wire: a registered function.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObserveTarget {
    pub function_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

/// An observer as its function's owner sees it (`functionregistered`,
/// `observeradded`): never the observer's metadata, policy or observing
/// worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObserverView {
    pub id: String,
    /// `{}` for an observer without args.
    #[serde(default = "empty_object", deserialize_with = "null_as_empty_object")]
    pub args: Value,
    /// The observer's filters in normal form; `{}` matches every event.
    #[serde(default = "empty_object", deserialize_with = "null_as_empty_object")]
    pub filters: Value,
    /// Target function id (its namespace is not disclosed).
    pub target: String,
    #[serde(
        default = "default_capture",
        deserialize_with = "null_as_default_capture"
    )]
    pub capture: Vec<Capture>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub on: On,
    /// Set when the engine's legacy shim created this observer from an old
    /// `registerTrigger`. `null` reads as absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy: Option<LegacyTrigger>,
}

/// The old `registerTrigger` an observer was created from by the engine's
/// legacy shim. The owner reads `config` as the observer's args.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegacyTrigger {
    pub trigger_type: String,
    pub config: Value,
}

/// Who made an invocation. `worker_id` / `worker_name` are stamped by the
/// engine from the calling connection; `function_id`, `invocation_id` and
/// `parent` are asserted by the calling SDK.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Caller {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_name: Option<String>,
    /// The handler the call was made from; absent for a top-level call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_id: Option<String>,
    /// The caller of that handler, one level only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ParentCaller>,
}

/// [`Caller::parent`]: a caller without a parent of its own.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
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

impl Caller {
    /// This caller without its own `parent`, as the `parent` of a call made
    /// from the handler it invoked.
    pub fn strip(&self) -> ParentCaller {
        ParentCaller {
            worker_id: self.worker_id.clone(),
            worker_name: self.worker_name.clone(),
            function_id: self.function_id.clone(),
            invocation_id: self.invocation_id.clone(),
        }
    }
}

/// `invokefunction.activation`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Activation {
    pub observer_id: String,
    /// Engine → worker: unique per activation. Absent on an owner's `call`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation_id: Option<String>,
}

/// One item of `invocationsobserved`: a matched local invocation of an auto
/// function, or an emitted event of an owner function.
///
/// `input` / `output` / `error` / `metadata` keep `null` apart from absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservedItem {
    pub function_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub invocation_id: String,
    #[serde(default, deserialize_with = "null_as_default")]
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
    #[serde(default, deserialize_with = "null_as_default")]
    pub observe_depth: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traceparent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baggage: Option<String>,
}

/// Per-function counts in `localinvocationstats`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalFunctionStats {
    pub function_id: String,
    pub count: u64,
    #[serde(default, deserialize_with = "null_as_default")]
    pub errors: u64,
    #[serde(default, deserialize_with = "null_as_default")]
    pub duration_ms_sum: f64,
}

/// SDK-side report drops in `localinvocationstats`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DroppedReportStats {
    pub function_id: String,
    /// [`observe_codes::BUFFER_FULL`], [`observe_codes::TOO_LARGE`] or
    /// [`observe_codes::INACTIVE`].
    pub reason: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterTriggerTypeMessage {
    /// Unique identifier for the trigger type (e.g. `state`, `durable:subscriber`).
    pub id: String,
    /// Human-readable description of what this trigger type does.
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_request_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_request_format: Option<Value>,
    /// Namespace this provider serves. `None` lets the engine use the
    /// connection's own, which is what a worker providing a trigger type for
    /// its own project wants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

impl RegisterTriggerTypeMessage {
    pub fn to_message(&self) -> Message {
        Message::RegisterTriggerType {
            id: self.id.clone(),
            description: self.description.clone(),
            trigger_request_format: self.trigger_request_format.clone(),
            call_request_format: self.call_request_format.clone(),
            namespace: self.namespace.clone(),
        }
    }
}

/// Input for [`IIIClient::register_trigger`](crate::IIIClient::register_trigger).
/// The `id` is auto-generated internally.
///
/// **Deprecated since 0.25.0** with `register_trigger`: use
/// [`IIIClient::observe`](crate::IIIClient::observe) with a
/// [`Source`](crate::Source) instead. Migration guide:
/// <https://iii.dev/docs/upgrading/from-0-24-x>.
///
/// Build it with [`RegisterTriggerInput::new`], or with
/// [`IIITrigger`](crate::builtin_triggers::IIITrigger) for a type the engine
/// provides. A struct literal has to name every field, so each field added here
/// breaks it: `namespace` did that once and `trigger_namespace` did it again,
/// which is why the constructors exist.
///
/// The two are different questions. `namespace` is where the target function
/// resolves; `trigger_namespace` is where the trigger type's provider is found,
/// and `None` there asks the engine to take this worker's namespace first and
/// the engine's own second.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterTriggerInput {
    /// Identifier of the registered trigger type this trigger uses (e.g. `storage::object-created`, `http`).
    pub trigger_type: String,
    /// ID of the function this trigger invokes when it fires.
    pub function_id: String,
    /// Trigger-type-specific configuration, matching the shape the trigger type expects.
    pub config: Value,
    /// Arbitrary user-specifiable metadata supplied to the triggered handler function on every invocation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    /// Namespace the trigger's target function resolves in. `None` inherits
    /// this worker's namespace; name another namespace, including `default`,
    /// to bind the trigger elsewhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Namespace to find the trigger type's provider in. `None` asks the
    /// engine to resolve it: this worker's namespace first, the engine's own
    /// second. Naming one is strict.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_namespace: Option<String>,
}

impl RegisterTriggerInput {
    /// A trigger of `trigger_type`, bound to `function_id`, configured by
    /// `config`.
    ///
    /// Use this for a trigger type nothing built in provides -- one this worker
    /// or a neighbour registered. For the engine's own types, prefer
    /// [`IIITrigger`](crate::builtin_triggers::IIITrigger), which knows the id
    /// and the config shape:
    ///
    /// ```no_run
    /// # use iii_sdk::protocol::RegisterTriggerInput;
    /// # use serde_json::json;
    /// RegisterTriggerInput::new("kick", "orders::sync", json!({ "every": "1m" }))
    ///     .in_namespace("billing")
    ///     .in_trigger_namespace("shop-a");
    /// ```
    ///
    /// Everything else defaults to "not named", which is what the great
    /// majority of registrations want. Reaching for a struct literal instead
    /// means every field added here becomes a breaking change for the caller,
    /// which is what this exists to stop.
    pub fn new(
        trigger_type: impl Into<String>,
        function_id: impl Into<String>,
        config: Value,
    ) -> Self {
        Self {
            trigger_type: trigger_type.into(),
            function_id: function_id.into(),
            config,
            metadata: None,
            namespace: None,
            trigger_namespace: None,
        }
    }

    /// Resolve this trigger's target function in `namespace` instead of this
    /// worker's.
    pub fn in_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// Find the trigger type's provider in `namespace`, and only there.
    ///
    /// A different question from [`in_namespace`](Self::in_namespace): that one
    /// locates the target function, this one locates the provider that fires
    /// it. Left unset, the engine resolves it -- this worker's namespace first,
    /// then its own -- which is what carries a worker that has not been
    /// migrated onto the engine's providers without saying anything.
    pub fn in_trigger_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.trigger_namespace = Some(namespace.into());
        self
    }

    /// Deliver `metadata` to the handler alongside every payload this trigger
    /// fires.
    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterTriggerMessage {
    pub id: String,
    pub trigger_type: String,
    pub function_id: String,
    pub config: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_namespace: Option<String>,
}

impl RegisterTriggerMessage {
    pub fn to_message(&self) -> Message {
        Message::RegisterTrigger {
            id: self.id.clone(),
            trigger_type: self.trigger_type.clone(),
            function_id: self.function_id.clone(),
            config: self.config.clone(),
            metadata: self.metadata.clone(),
            namespace: self.namespace.clone(),
            trigger_namespace: self.trigger_namespace.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnregisterTriggerMessage {
    pub id: String,
    pub trigger_type: String,
}

impl UnregisterTriggerMessage {
    pub fn to_message(&self) -> Message {
        Message::UnregisterTrigger {
            id: self.id.clone(),
            trigger_type: self.trigger_type.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnregisterTriggerTypeMessage {
    pub id: String,
}

impl UnregisterTriggerTypeMessage {
    pub fn to_message(&self) -> Message {
        Message::UnregisterTriggerType {
            id: self.id.clone(),
            namespace: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterFunctionMessage {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invocation: Option<HttpInvocationConfig>,
    /// The wire half of the function's observe hooks; see
    /// [`RegisterFunction::observe`](crate::RegisterFunction::observe).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observe: Option<ObserveDescriptor>,
}

impl RegisterFunctionMessage {
    pub fn with_id(name: String) -> Self {
        RegisterFunctionMessage {
            id: name,
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
            observe: None,
        }
    }
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }
    pub fn to_message(&self) -> Message {
        Message::RegisterFunction {
            id: self.id.clone(),
            description: self.description.clone(),
            request_format: self.request_format.clone(),
            response_format: self.response_format.clone(),
            metadata: self.metadata.clone(),
            invocation: self.invocation.clone(),
            observe: self.observe.clone().map(Box::new),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionMessage {
    pub function_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stacktrace: Option<String>,
}

impl ErrorBody {
    /// An error body without a stacktrace.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            stacktrace: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn register_function_to_message_and_serializes_type() {
        let msg = RegisterFunctionMessage {
            id: "functions.echo".to_string(),
            description: Some("Echo function".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
            observe: None,
        };

        let message = msg.to_message();
        match &message {
            Message::RegisterFunction {
                id, description, ..
            } => {
                assert_eq!(id, "functions.echo");
                assert_eq!(description.as_deref(), Some("Echo function"));
            }
            _ => panic!("unexpected message variant"),
        }

        let serialized = serde_json::to_value(&message).unwrap();
        assert_eq!(serialized["type"], "registerfunction");
        assert_eq!(serialized["id"], "functions.echo");
        assert_eq!(serialized["description"], "Echo function");
    }

    #[test]
    fn register_http_function_serializes_invocation() {
        use iii_helpers::http::{HttpInvocationConfig, HttpMethod};

        let msg = RegisterFunctionMessage {
            id: "external::my_lambda".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: Some(HttpInvocationConfig {
                url: "https://example.com/invoke".to_string(),
                method: HttpMethod::Post,
                timeout_ms: Some(30000),
                headers: HashMap::new(),
                auth: None,
            }),
            observe: None,
        };

        let serialized = serde_json::to_value(msg.to_message()).unwrap();
        assert_eq!(serialized["type"], "registerfunction");
        assert_eq!(serialized["id"], "external::my_lambda");
        assert!(serialized["invocation"].is_object());
        assert_eq!(
            serialized["invocation"]["url"],
            "https://example.com/invoke"
        );
        assert_eq!(serialized["invocation"]["method"], "POST");
    }
}
