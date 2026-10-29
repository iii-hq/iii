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

/// Routing action for [`CallRequest`]. Determines how the engine handles
/// the invocation.
///
/// - `Enqueue`: Routes through a named queue for async processing.
/// - `Void`: Fire-and-forget, no response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CallAction {
    /// Routes the invocation through a named queue.
    Enqueue { queue: String },
    /// Fire-and-forget routing.
    Void,
}

/// [`CallAction`] under its 0.25 name.
#[deprecated(since = "0.26.0", note = "use CallAction")]
pub type TriggerAction = CallAction;

/// Request object for [`IIIClient::call`](crate::IIIClient::call).
///
/// ```rust
/// # use iii_sdk::{CallAction, CallRequest};
/// # use serde_json::json;
/// // Simple call
/// CallRequest {
///     function_id: "my::function".to_string(),
///     payload: json!({ "key": "value" }),
///     action: None,
///     timeout_ms: None,
/// };
///
/// // With action
/// CallRequest {
///     function_id: "my::function".to_string(),
///     payload: json!({}),
///     action: Some(CallAction::Enqueue { queue: "payments".to_string() }),
///     timeout_ms: None,
/// };
///
/// // With metadata
/// CallRequest::new("my::function", json!({})).metadata(json!({ "tenant": "acme" }));
/// ```
#[derive(Debug, Clone)]
pub struct CallRequest {
    /// ID of the function to invoke.
    pub function_id: String,
    /// Input data passed to the function.
    pub payload: Value,
    /// Sets how the call is routed. `None` for a synchronous request/response.
    /// Set a routing scheme otherwise (e.g. `CallAction::Enqueue { .. }`, `CallAction::Void`).
    pub action: Option<CallAction>,
    /// Override the default invocation timeout, in milliseconds.
    pub timeout_ms: Option<u64>,
}

/// [`CallRequest`] under its 0.25 name.
#[deprecated(since = "0.26.0", note = "use CallRequest")]
pub type TriggerRequest = CallRequest;

impl CallRequest {
    /// A synchronous request/response call of `function_id` with `payload`,
    /// the default timeout and no routing action.
    ///
    /// ```rust
    /// # use iii_sdk::CallRequest;
    /// # use serde_json::json;
    /// let request = CallRequest::new("orders::create", json!({ "sku": "A-1" }));
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

    /// A call through `observer`: the engine runs the observer's target with
    /// `payload`, as given, and applies the observer's policy, metadata
    /// sidecar and loop guard. Only the worker that hosts the observer's
    /// function may make it (`NOT_FUNCTION_OWNER`); it never runs in-process.
    ///
    /// ```rust,no_run
    /// # use iii_sdk::{CallAction, CallRequest, IIIClient, Observer};
    /// # use serde_json::json;
    /// # async fn demo(iii: &IIIClient, obs: &Observer) -> Result<(), iii_sdk::Error> {
    /// let response = iii.call(CallRequest::observer(obs, json!({ "path": "/users/42" }))).await?;
    /// // fire-and-forget
    /// iii.call(CallRequest::observer(obs, json!({ "state": "initial" })).action(CallAction::Void))
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub fn observer(observer: &crate::Observer, payload: Value) -> CallRequestWithMetadata {
        let mut request =
            CallRequestWithMetadata::from(CallRequest::new(observer.target(), payload));
        request.observer = Some(observer.id().to_string());
        request
    }

    /// Attach per-invocation metadata without adding a required field to
    /// [`CallRequest`] struct literals.
    pub fn metadata(self, metadata: Value) -> CallRequestWithMetadata {
        CallRequestWithMetadata::from(self).metadata(metadata)
    }

    /// Target a specific namespace for this invocation without adding a
    /// required field to [`CallRequest`] struct literals. Serializes into
    /// [`Message::InvokeFunction`]'s `namespace`.
    ///
    /// Leaving it unset does not mean the engine's default: the call inherits
    /// this worker's namespace. Say `default` to reach the engine's from a
    /// namespaced worker.
    pub fn namespace(self, namespace: impl Into<String>) -> CallRequestWithMetadata {
        CallRequestWithMetadata::from(self).namespace(namespace)
    }

    /// Choose the path this call takes: [`Dispatch::Auto`] (the default) runs
    /// a function this worker hosts in-process when the engine allows it;
    /// [`Dispatch::Remote`] always goes through the engine.
    pub fn dispatch(self, dispatch: Dispatch) -> CallRequestWithMetadata {
        CallRequestWithMetadata::from(self).dispatch(dispatch)
    }
}

/// Which path [`IIIClient::call`](crate::IIIClient::call) takes.
///
/// A call to a function registered by this same worker runs in-process
/// ("local dispatch") when all of these hold:
///
/// 1. the function has a handler here (not [`RegisterFunction::http`](crate::RegisterFunction::http))
///    and was not registered with [`RegisterFunction::local_dispatch(false)`](crate::RegisterFunction::local_dispatch);
/// 2. the engine confirmed it with `functionregistered { local_dispatch: true }`;
/// 3. the call resolves to the namespace the function is registered in;
/// 4. the action is not [`CallAction::Enqueue`] (a `Void` call runs
///    locally, fire-and-forget);
/// 5. the dispatch is not [`Dispatch::Remote`], and the call is not made
///    through an observer ([`CallRequest::observer`]).
///
/// Otherwise the call goes through the engine. A local call returns the same
/// result, error shape and timeout as a remote one, and is reported to the
/// engine only when an observer's filters match it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Dispatch {
    /// Run in-process when the five conditions above hold, else remote.
    #[default]
    Auto,
    /// Always route through the engine (engine-side effects, tests).
    Remote,
}

/// A [`CallRequest`] plus optional per-invocation metadata, target namespace,
/// dispatch and the observer it is made through.
#[derive(Debug, Clone)]
pub struct CallRequestWithMetadata {
    pub(crate) request: CallRequest,
    pub(crate) metadata: Option<Value>,
    pub(crate) namespace: Option<String>,
    pub(crate) dispatch: Dispatch,
    /// Set by [`CallRequest::observer`]: the id of the observer whose target
    /// `request.function_id` is.
    pub(crate) observer: Option<String>,
}

impl CallRequestWithMetadata {
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

    /// How the call is routed; see [`CallRequest::action`].
    pub fn action(mut self, action: CallAction) -> Self {
        self.request.action = Some(action);
        self
    }

    /// Override the default invocation timeout.
    pub fn timeout(mut self, timeout: std::time::Duration) -> Self {
        self.request.timeout_ms = Some(timeout.as_millis() as u64);
        self
    }
}

impl<T> From<T> for CallRequestWithMetadata
where
    T: Into<CallRequest>,
{
    fn from(request: T) -> Self {
        Self {
            request: request.into(),
            metadata: None,
            namespace: None,
            dispatch: Dispatch::Auto,
            observer: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Message {
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
        /// JSON Schema of observers' `args`; the engine validates every
        /// `observe` against it (`INVALID_ARGS`). Absent: the function takes
        /// no args.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args_format: Option<Value>,
        /// Only the hosting connection may call it; others may observe it.
        #[serde(
            default,
            deserialize_with = "null_as_default",
            skip_serializing_if = "is_false"
        )]
        private: bool,
        /// `false`: nobody may observe it (`NOT_OBSERVABLE`).
        #[serde(
            default = "default_true",
            deserialize_with = "null_as_true",
            skip_serializing_if = "is_true"
        )]
        observable: bool,
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
        action: Option<CallAction>,
        /// Per-invocation metadata sidecar, surfaced to the handler as a
        /// distinct argument alongside `data`. Optional and additive
        /// for wire compatibility with engines that don't send it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
        /// Target namespace for routing. Optional and additive, so older peers
        /// that don't send it stay wire-compatible, and the engine reads an
        /// absent field as its default namespace.
        ///
        /// A call made through this SDK does not arrive absent: `call` fills
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
        /// Engine → worker: present on every observer activation and on the
        /// target's call of a call through an observer. Worker → engine: a
        /// call through an observer ([`CallRequest::observer`]), with
        /// `observer_id` only.
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
        /// Host → engine, from an `observe.v2` host: the observers iii's
        /// delivery of this routed call selected; the answer is the handler's
        /// outcome. The engine activates them and strips the field before
        /// forwarding the result.
        #[serde(
            default,
            deserialize_with = "null_as_default",
            skip_serializing_if = "Vec::is_empty"
        )]
        observed: Vec<String>,
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
        /// [`FEATURE_OBSERVE_V2`], [`FEATURE_LOCAL_DISPATCH_V1`]). Absent from
        /// engines older than 0.25; a malformed list reads as empty.
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
    /// Engine → host: the `registerfunction` success ack, with the full
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
    /// Engine → host: one observer attached to a function this connection
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
    /// Engine → host: one observer detached.
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
    /// Host → engine: accepts one added observer, or rejects it with `error`
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
    /// Host → engine: what the host's delivery matched in local calls,
    /// batched. A routed call's matches ride its `invocationresult`.
    InvocationsObserved {
        #[serde(default, deserialize_with = "null_as_default")]
        items: Vec<ObservedItem>,
    },
    /// Host → engine: local invocation counts and SDK-side report drops,
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
/// Protocol feature: the host delivers the calls of the functions it hosts,
/// routed ones included (`invocationresult.observed`), and registers
/// functions with `args_format`, `private` and `observable`. Includes
/// [`FEATURE_OBSERVE_V1`].
pub const FEATURE_OBSERVE_V2: &str = "observe.v2";
/// Protocol feature: the SDK runs its own functions in-process once the engine
/// confirms them with `functionregistered { local_dispatch: true }`.
pub const FEATURE_LOCAL_DISPATCH_V1: &str = "local-dispatch.v1";
/// Protocol feature, declared by the SDK only: it receives JSON messages up to
/// 64 MiB, so the engine sends it messages of that size whatever
/// [`WorkerMetadata::version`](crate::iii::WorkerMetadata::version) reports.
pub const FEATURE_LARGE_MESSAGES_V1: &str = "large-messages.v1";
/// What this SDK declares in the `engine::workers::register` payload.
pub const SDK_FEATURES: [&str; 4] = [
    FEATURE_OBSERVE_V1,
    FEATURE_OBSERVE_V2,
    FEATURE_LOCAL_DISPATCH_V1,
    FEATURE_LARGE_MESSAGES_V1,
];

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

fn default_true() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// A `bool` that defaults to `true`, sent as `null`, reads as `true`.
fn null_as_true<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<bool>::deserialize(deserializer)?.unwrap_or(true))
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
    /// The engine lacks the observe feature a call needs: `observe.v1` for
    /// `observe()`, `observe.v2` for a function that declares `args_format`,
    /// `private` or `observable: false`, or sets a deliver hook (raised by
    /// the SDK).
    pub const UNSUPPORTED: &str = "UNSUPPORTED";
    /// Args fail the function's `args_format`, or args on a function without one.
    pub const INVALID_ARGS: &str = "INVALID_ARGS";
    /// A registration's `args_format` is not a valid JSON Schema.
    pub const INVALID_ARGS_FORMAT: &str = "INVALID_ARGS_FORMAT";
    pub const INVALID_FILTER: &str = "INVALID_FILTER";
    pub const INVALID_FILTER_PATH: &str = "INVALID_FILTER_PATH";
    pub const NOT_OBSERVABLE: &str = "NOT_OBSERVABLE";
    pub const FORBIDDEN: &str = "FORBIDDEN";
    pub const SELF_TARGET: &str = "SELF_TARGET";
    pub const LIMIT_EXCEEDED: &str = "LIMIT_EXCEEDED";
    pub const OBSERVER_ID_CONFLICT: &str = "OBSERVER_ID_CONFLICT";
    /// An `on_observer_added` hook did not finish within 30 s.
    pub const OWNER_TIMEOUT: &str = "OWNER_TIMEOUT";
    /// An `on_observer_added` hook failed with a plain error.
    pub const OBSERVE_REJECTED: &str = "OBSERVE_REJECTED";
    pub const OBSERVER_NOT_FOUND: &str = "OBSERVER_NOT_FOUND";
    /// A call through an observer from a connection that does not host the
    /// observer's function.
    pub const NOT_FUNCTION_OWNER: &str = "NOT_FUNCTION_OWNER";
    /// A call through an observer that is not active.
    pub const OBSERVER_NOT_ACTIVE: &str = "OBSERVER_NOT_ACTIVE";
    pub const OVERFLOW: &str = "OVERFLOW";
    pub const LOOP_GUARD: &str = "LOOP_GUARD";
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
    /// Which outcomes of a call activate an observer.
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
        /// The observed call's input alone.
        Input => "input",
        /// The observed call's output alone.
        Output => "output",
    }
}

wire_string_enum! {
    /// Observer lifecycle states.
    pub enum ObserverState {
        /// Waiting for its function's host (and its ack) or its target.
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
    /// Outcome of an observed call.
    pub enum InvocationStatus {
        Success => "success",
        Error => "error",
    }
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
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

/// An observer as its function's host sees it (`functionregistered`,
/// `observeradded`): never the observer's metadata, policy or observing
/// worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObserverView {
    pub id: String,
    /// `{}` for an observer without args.
    #[serde(default = "empty_object", deserialize_with = "null_as_empty_object")]
    pub args: Value,
    /// The observer's filters in normal form; `{}` matches every call.
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
    /// Engine → worker: unique per activation. Absent on a call through an
    /// observer as its host sends it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation_id: Option<String>,
}

/// One item of `invocationsobserved`: one local call and the observers
/// iii's delivery selected for it.
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
    /// The handler returned its input: the item carries it once, as `input`,
    /// and the engine fills `output` from it.
    #[serde(
        default,
        deserialize_with = "null_as_default",
        skip_serializing_if = "is_false"
    )]
    pub output_is_input: bool,
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
    /// See [`RegisterFunction::args_format`](crate::RegisterFunction::args_format).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_format: Option<Value>,
    /// See [`RegisterFunction::private`](crate::RegisterFunction::private).
    #[serde(default, skip_serializing_if = "is_false")]
    pub private: bool,
    /// See [`RegisterFunction::observable`](crate::RegisterFunction::observable).
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub observable: bool,
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
            args_format: None,
            private: false,
            observable: true,
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
            args_format: self.args_format.clone(),
            private: self.private,
            observable: self.observable,
        }
    }

    /// Needs an engine that advertised [`FEATURE_OBSERVE_V2`]: it declares
    /// `args_format`, `private` or `observable: false`.
    pub(crate) fn declares_v2(&self) -> bool {
        self.args_format.is_some() || self.private || !self.observable
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
            args_format: None,
            private: false,
            observable: true,
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
        for default in ["args_format", "private", "observable", "observe"] {
            assert!(serialized.get(default).is_none(), "{default} is left out");
        }
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
            args_format: None,
            private: false,
            observable: true,
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
