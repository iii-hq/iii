use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// Extension trait for Mutex that recovers from poisoning instead of panicking.
/// This is safe when the protected data is still valid after a panic in another thread.
trait MutexExt<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|e| e.into_inner())
    }
}

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
    sync::{mpsc, oneshot},
    time::{Instant, interval, sleep, sleep_until},
};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
use uuid::Uuid;

const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

use iii_helpers::http::HttpInvocationConfig;

use crate::{
    channels::{ChannelReader, ChannelWriter, StreamChannelRef},
    error::Error,
    protocol::{
        ErrorBody, FUNCTION_NAMESPACE_CONFLICT, Message, RegisterFunctionMessage,
        RegisterTriggerInput, RegisterTriggerMessage, RegisterTriggerTypeMessage, TriggerAction,
        TriggerRequest, TriggerRequestWithMetadata, UnregisterTriggerMessage,
        UnregisterTriggerTypeMessage, WORKER_NAMESPACE_CONFLICT,
    },
    triggers::{Trigger, TriggerConfig, TriggerHandler},
    types::{
        Channel, RemoteFunctionData, RemoteFunctionHandlerWithMetadata, RemoteTriggerTypeData,
    },
};

use iii_helpers::observability as telemetry;
use iii_helpers::observability::OtelConfig;

const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// Worker information returned by `engine::workers::list`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerInfo {
    pub id: String,
    pub name: Option<String>,
    pub runtime: Option<String>,
    pub version: Option<String>,
    pub os: Option<String>,
    pub ip_address: Option<String>,
    pub status: String,
    pub connected_at_ms: u64,
    pub function_count: usize,
    pub functions: Vec<String>,
    pub active_invocations: usize,
    #[serde(default)]
    pub isolation: Option<String>,
    #[serde(default)]
    pub namespace: Option<String>,
}

/// Function information returned by `engine::functions::list`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInfo {
    pub function_id: String,
    pub description: Option<String>,
    pub request_format: Option<Value>,
    pub response_format: Option<Value>,
    pub metadata: Option<Value>,
    #[serde(default)]
    pub namespace: Option<String>,
}

/// Trigger information returned by `engine::triggers::list`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerInfo {
    pub id: String,
    pub trigger_type: String,
    pub function_id: String,
    pub config: Value,
    pub metadata: Option<Value>,
    #[serde(default)]
    pub namespace: Option<String>,
}

/// Builder for registering a custom trigger type with optional format schemas.
///
/// Type parameters:
/// - `C` tracks the trigger registration type (set via `.trigger_request_format::<T>()`)
/// - `R` tracks the call request type (set via `.call_request_format::<T>()`)
///
/// Both default to `Value` (untyped) and change when the respective builder
/// method is called. This allows [`IIIClient::register_trigger_type`] to return a
/// [`TriggerTypeRef<C, R>`] with compile-time safety for both config and
/// function input types.
pub struct RegisterTriggerType<H, C = Value, R = Value> {
    id: String,
    description: String,
    handler: H,
    trigger_request_format: Option<Value>,
    call_request_format: Option<Value>,
    _phantom: std::marker::PhantomData<(C, R)>,
}

impl<H: TriggerHandler> RegisterTriggerType<H> {
    pub fn new(id: impl Into<String>, description: impl Into<String>, handler: H) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            handler,
            trigger_request_format: None,
            call_request_format: None,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<H: TriggerHandler, C, R> RegisterTriggerType<H, C, R> {
    /// Set the trigger request format schema from a type.
    /// Changes `C`, enabling compile-time validation on
    /// [`TriggerTypeRef::register_trigger`].
    pub fn trigger_request_format<T: schemars::JsonSchema + Serialize>(
        self,
    ) -> RegisterTriggerType<H, T, R> {
        RegisterTriggerType {
            id: self.id,
            description: self.description,
            handler: self.handler,
            trigger_request_format: json_schema_for::<T>(),
            call_request_format: self.call_request_format,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Set the call request format schema from a type.
    /// Changes `R`, enabling compile-time validation on
    /// [`TriggerTypeRef::register_function`].
    pub fn call_request_format<T: schemars::JsonSchema>(self) -> RegisterTriggerType<H, C, T> {
        RegisterTriggerType {
            id: self.id,
            description: self.description,
            handler: self.handler,
            trigger_request_format: self.trigger_request_format,
            call_request_format: json_schema_for::<T>(),
            _phantom: std::marker::PhantomData,
        }
    }
}

/// Typed handle returned by [`IIIClient::register_trigger_type`].
///
/// Type parameters:
/// - `C`: trigger registration type for [`register_trigger`](Self::register_trigger)
/// - `R`: call request type for [`register_function`](Self::register_function)
#[derive(Clone)]
pub struct TriggerTypeRef<C = Value, R = Value> {
    iii: IIIClient,
    trigger_type_id: String,
    _phantom: std::marker::PhantomData<(C, R)>,
}

impl<C: Serialize, R> TriggerTypeRef<C, R> {
    /// Register a trigger with compile-time validated trigger config.
    pub fn register_trigger(
        &self,
        function_id: impl Into<String>,
        config: C,
    ) -> Result<Trigger, Error> {
        self.register_trigger_with_metadata(function_id, config, None)
    }

    /// Register a trigger with compile-time validated trigger config and optional metadata.
    pub fn register_trigger_with_metadata(
        &self,
        function_id: impl Into<String>,
        config: C,
        metadata: Option<Value>,
    ) -> Result<Trigger, Error> {
        self.iii.register_trigger(RegisterTriggerInput {
            trigger_type: self.trigger_type_id.clone(),
            function_id: function_id.into(),
            config: serde_json::to_value(config).map_err(|e| Error::Handler(e.to_string()))?,
            metadata,
            namespace: None,
        })
    }
}

impl<C, R> TriggerTypeRef<C, R>
where
    R: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static,
{
    /// Register a sync function whose input type must match
    /// the call request format `R`.
    pub fn register_function<O, F>(&self, id: impl Into<String>, f: F) -> FunctionRef
    where
        O: Serialize + schemars::JsonSchema + Send + 'static,
        F: Fn(R) -> Result<O, Error> + Send + Sync + 'static,
    {
        self.iii.register_function(id, RegisterFunction::new(f))
    }

    /// Register an async function whose input type must match
    /// the call request format `R`.
    pub fn register_function_async<O, F, Fut>(&self, id: impl Into<String>, f: F) -> FunctionRef
    where
        O: Serialize + schemars::JsonSchema + Send + 'static,
        F: Fn(R) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<O, Error>> + Send + 'static,
    {
        self.iii
            .register_function(id, RegisterFunction::new_async(f))
    }
}

/// Worker metadata reported to the engine (language, framework, project).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelemetryOptions {
    /// Programming language of the worker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Name of the project this worker belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    /// Framework name, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub framework: Option<String>,
    /// Amplitude API key for product analytics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amplitude_api_key: Option<String>,
}

/// Worker metadata for auto-registration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerMetadata {
    pub runtime: String,
    pub version: String,
    pub name: String,
    pub os: String,
    /// One-line, human/LLM-readable summary of what this worker does.
    /// Surfaces in `engine::workers::list` / `engine::workers::info`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<TelemetryOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolation: Option<String>,
    /// Namespace this worker registers under. Absent means the engine applies
    /// its default namespace. Resolved from `InitOptions.namespace` /
    /// `III_NAMESPACE` (see [`resolve_namespace`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

impl Default for WorkerMetadata {
    fn default() -> Self {
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        let pid = std::process::id();
        let os_info = format!(
            "{} {} ({})",
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::env::consts::FAMILY
        );

        let language = std::env::var("LANG")
            .or_else(|_| std::env::var("LC_ALL"))
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| s.split('.').next().unwrap_or(&s).to_string());

        let project_name = detect_project_name(None);

        Self {
            runtime: "rust".to_string(),
            version: SDK_VERSION.to_string(),
            // III_WORKER_NAME carries the config.yaml entry name for managed
            // workers (set by iii-worker at spawn). Engine truth (`iii worker
            // status`/`list`) matches connections by name, so the managed
            // identity must win over the hostname:pid fallback.
            name: std::env::var("III_WORKER_NAME")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("{}:{}", hostname, pid)),
            os: os_info,
            description: None,
            pid: Some(pid),
            telemetry: Some(TelemetryOptions {
                language,
                project_name,
                ..Default::default()
            }),
            isolation: std::env::var("III_ISOLATION")
                .ok()
                .filter(|s| !s.is_empty()),
            // III_NAMESPACE carries the namespace for managed workers, the same
            // way III_WORKER_NAME carries the name. Absent leaves routing to the
            // engine's default namespace. `InitOptions.namespace` overrides this
            // (see `resolve_namespace`).
            namespace: std::env::var("III_NAMESPACE")
                .ok()
                .filter(|s| !s.is_empty()),
        }
    }
}

/// Resolve the effective worker namespace: an explicit `InitOptions.namespace`
/// wins, then the `III_NAMESPACE` env var, then `None` (the engine applies its
/// default namespace). Mirrors the `III_WORKER_NAME` precedence.
pub(crate) fn resolve_namespace(explicit: Option<String>) -> Option<String> {
    explicit.filter(|s| !s.is_empty()).or_else(|| {
        std::env::var("III_NAMESPACE")
            .ok()
            .filter(|s| !s.is_empty())
    })
}

/// Returns a project identifier for telemetry, derived from the current
/// working directory. Reads `[package] name` from `Cargo.toml` if present at
/// `cwd`; otherwise falls back to the basename of `cwd`. Returns `None`
/// only when both signals are unavailable.
///
/// No directory walking, only inspects `cwd` itself, so the SDK never
/// reads files outside the user's explicit working directory.
pub(crate) fn detect_project_name(cwd: Option<std::path::PathBuf>) -> Option<String> {
    let cwd = cwd.or_else(|| std::env::current_dir().ok())?;

    let manifest = cwd.join("Cargo.toml");
    if let Ok(content) = std::fs::read_to_string(&manifest) {
        if let Some(name) = parse_cargo_package_name(&content) {
            return Some(name);
        }
    }

    cwd.file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Minimal parser for the `name` key inside the `[package]` table of a
/// `Cargo.toml` file. Avoids adding a TOML dependency for a single field.
fn parse_cargo_package_name(content: &str) -> Option<String> {
    let mut in_package = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(stripped) = trimmed.strip_prefix('[') {
            in_package = stripped.trim_end_matches(']').trim() == "package";
            continue;
        }
        if !in_package {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("name") else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim().strip_prefix('"')?;
        let end = rest.find('"')?;
        let name = rest[..end].trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

#[allow(clippy::large_enum_variant)]
enum Outbound {
    Message(Message),
    Shutdown,
}

type PendingInvocation = oneshot::Sender<Result<Value, Error>>;

// WebSocket transmitter type alias
type WsTx = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    WsMessage,
>;

/// Inject trace context headers for outbound messages.
fn inject_trace_headers() -> (Option<String>, Option<String>) {
    use iii_helpers::observability as context;
    (context::inject_traceparent(), context::inject_baggage())
}

/// Connection state for the III WebSocket client
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IIIConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Failed,
}

#[derive(Clone)]
pub struct FunctionRef {
    pub id: String,
    unregister_fn: Arc<dyn Fn() + Send + Sync>,
}

impl FunctionRef {
    pub fn unregister(&self) {
        (self.unregister_fn)();
    }
}

fn json_schema_for<T: schemars::JsonSchema>() -> Option<Value> {
    serde_json::to_value(
        schemars::r#gen::SchemaSettings::draft07()
            .into_generator()
            .into_root_schema_for::<T>(),
    )
    .ok()
}

/// Helper trait used internally to convert a sync function into a
/// [`RemoteFunctionHandlerWithMetadata`].
#[doc(hidden)]
pub trait IntoSyncHandler<Marker>: Send + Sync + 'static {
    fn into_handler(self) -> RemoteFunctionHandlerWithMetadata;
    fn request_format() -> Option<Value> {
        None
    }
    fn response_format() -> Option<Value> {
        None
    }
}

// 1-arg sync, deserializes the entire JSON input as T.
//
// Error type is fixed to [`Error`] (instead of generic `E: Display`) so
// closures using bare `Ok(...)` infer cleanly without explicit error
// annotations, required for ergonomic registration of `Fn(Value) -> ...`
// handlers. Other error types convert via `From<E> for Error` (impls
// for `String` / `&str` / `serde_json::Error` ship with the SDK).
impl<F, T, R> IntoSyncHandler<(T, R)> for F
where
    F: Fn(T) -> Result<R, Error> + Send + Sync + 'static,
    T: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static,
    R: serde::Serialize + schemars::JsonSchema + Send + 'static,
{
    fn into_handler(self) -> RemoteFunctionHandlerWithMetadata {
        Arc::new(move |input: Value, _metadata: Option<Value>| {
            let output = serde_json::from_value::<T>(input)
                .map_err(|e| Error::Serde(e.to_string()))
                .and_then(&self)
                .and_then(|val| {
                    serde_json::to_value(&val).map_err(|e| Error::Serde(e.to_string()))
                });
            Box::pin(async move { output })
        })
    }

    fn request_format() -> Option<Value> {
        json_schema_for::<T>()
    }

    fn response_format() -> Option<Value> {
        json_schema_for::<R>()
    }
}

// 2-arg sync, deserializes the entire JSON input as T and passes the
// per-invocation metadata sidecar as the second handler argument.
impl<F, T, R> IntoSyncHandler<(T, Option<Value>, R)> for F
where
    F: Fn(T, Option<Value>) -> Result<R, Error> + Send + Sync + 'static,
    T: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static,
    R: serde::Serialize + schemars::JsonSchema + Send + 'static,
{
    fn into_handler(self) -> RemoteFunctionHandlerWithMetadata {
        Arc::new(move |input: Value, metadata: Option<Value>| {
            let output = serde_json::from_value::<T>(input)
                .map_err(|e| Error::Serde(e.to_string()))
                .and_then(|arg| self(arg, metadata))
                .and_then(|val| {
                    serde_json::to_value(&val).map_err(|e| Error::Serde(e.to_string()))
                });
            Box::pin(async move { output })
        })
    }

    fn request_format() -> Option<Value> {
        json_schema_for::<T>()
    }

    fn response_format() -> Option<Value> {
        json_schema_for::<R>()
    }
}

// =============================================================================
// IntoAsyncHandler, async function schema-extraction trait
// =============================================================================

/// Helper trait used internally to convert an async function into a
/// [`RemoteFunctionHandlerWithMetadata`].
#[doc(hidden)]
pub trait IntoAsyncHandler<Marker>: Send + Sync + 'static {
    fn into_handler(self) -> RemoteFunctionHandlerWithMetadata;
    fn request_format() -> Option<Value> {
        None
    }
    fn response_format() -> Option<Value> {
        None
    }
}

/// Build the dispatchable handler for a typed async function: deserialize
/// the JSON input as `T`, run `f`, serialize the result. Deserialization
/// failures surface as [`Error::Serde`].
fn async_handler<F, T, Fut, R>(f: F) -> RemoteFunctionHandlerWithMetadata
where
    F: Fn(T) -> Fut + Send + Sync + 'static,
    T: serde::de::DeserializeOwned + Send + 'static,
    Fut: std::future::Future<Output = Result<R, Error>> + Send + 'static,
    R: serde::Serialize + Send + 'static,
{
    Arc::new(
        move |input: Value,
              _metadata: Option<Value>|
              -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Value, Error>> + Send>,
        > {
            match serde_json::from_value::<T>(input) {
                Ok(arg) => {
                    let fut = f(arg);
                    Box::pin(async move {
                        fut.await.and_then(|val| {
                            serde_json::to_value(&val).map_err(|e| Error::Serde(e.to_string()))
                        })
                    })
                }
                Err(e) => {
                    let err = Error::Serde(e.to_string());
                    Box::pin(async move { Err(err) })
                }
            }
        },
    )
}

/// Build the dispatchable handler for a typed async function that also accepts
/// per-invocation metadata as its second argument.
fn async_handler_with_metadata<F, T, Fut, R>(f: F) -> RemoteFunctionHandlerWithMetadata
where
    F: Fn(T, Option<Value>) -> Fut + Send + Sync + 'static,
    T: serde::de::DeserializeOwned + Send + 'static,
    Fut: std::future::Future<Output = Result<R, Error>> + Send + 'static,
    R: serde::Serialize + Send + 'static,
{
    Arc::new(
        move |input: Value,
              metadata: Option<Value>|
              -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Value, Error>> + Send>,
        > {
            match serde_json::from_value::<T>(input) {
                Ok(arg) => {
                    let fut = f(arg, metadata);
                    Box::pin(async move {
                        fut.await.and_then(|val| {
                            serde_json::to_value(&val).map_err(|e| Error::Serde(e.to_string()))
                        })
                    })
                }
                Err(e) => {
                    let err = Error::Serde(e.to_string());
                    Box::pin(async move { Err(err) })
                }
            }
        },
    )
}

// 1-arg async, deserializes the entire JSON input as T.
//
// Error type is fixed to [`Error`] (see [`IntoSyncHandler`] for the
// rationale). Use `From<E> for Error` to lift custom error types,
// or `?` propagation in the closure body.
impl<F, T, Fut, R> IntoAsyncHandler<(T, Fut, R)> for F
where
    F: Fn(T) -> Fut + Send + Sync + 'static,
    T: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static,
    Fut: std::future::Future<Output = Result<R, Error>> + Send + 'static,
    R: serde::Serialize + schemars::JsonSchema + Send + 'static,
{
    fn into_handler(self) -> RemoteFunctionHandlerWithMetadata {
        async_handler(self)
    }

    fn request_format() -> Option<Value> {
        json_schema_for::<T>()
    }

    fn response_format() -> Option<Value> {
        json_schema_for::<R>()
    }
}

// 2-arg async, deserializes the entire JSON input as T and passes the
// per-invocation metadata sidecar as the second handler argument.
impl<F, T, Fut, R> IntoAsyncHandler<(T, Option<Value>, Fut, R)> for F
where
    F: Fn(T, Option<Value>) -> Fut + Send + Sync + 'static,
    T: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static,
    Fut: std::future::Future<Output = Result<R, Error>> + Send + 'static,
    R: serde::Serialize + schemars::JsonSchema + Send + 'static,
{
    fn into_handler(self) -> RemoteFunctionHandlerWithMetadata {
        async_handler_with_metadata(self)
    }

    fn request_format() -> Option<Value> {
        json_schema_for::<T>()
    }

    fn response_format() -> Option<Value> {
        json_schema_for::<R>()
    }
}

// =============================================================================
// RegisterFunction, single registration builder
// =============================================================================

fn empty_message() -> RegisterFunctionMessage {
    RegisterFunctionMessage {
        id: String::new(),
        description: None,
        request_format: None,
        response_format: None,
        metadata: None,
        invocation: None,
    }
}

/// Function registration builder.
///
/// The function ID is supplied separately at registration time via
/// [`IIIClient::register_function`], `RegisterFunction` only carries the handler
/// and optional metadata.
///
/// Constructors:
/// - [`RegisterFunction::new`][]: sync function. Accepts both typed handlers
///   (schemas auto-extracted via `schemars`) and `Fn(Value, Option<Value>) -> Result<Value, Error>`
///   closures. The second argument is the per-invocation metadata sidecar and
///   is `None` when absent.
/// - [`RegisterFunction::new_async`][]: async equivalent of `new`.
/// - [`RegisterFunction::http`][]: function invoked over HTTP (Lambda,
///   Cloudflare Workers, etc.).
///
/// Builder methods (all consume `self`):
/// - [`description`](Self::description)
/// - [`metadata`](Self::metadata)
/// - [`request_format`](Self::request_format): overrides any auto-extracted schema.
/// - [`response_format`](Self::response_format): overrides any auto-extracted schema.
pub struct RegisterFunction {
    message: RegisterFunctionMessage,
    handler: Option<RemoteFunctionHandlerWithMetadata>,
}

impl RegisterFunction {
    /// Create a registration for a **sync** typed function.
    ///
    /// Auto-extracts `request_format` / `response_format` from the function's
    /// argument and return types via `schemars`.
    pub fn new<F, M>(f: F) -> Self
    where
        F: IntoSyncHandler<M>,
    {
        let mut message = empty_message();
        message.request_format = F::request_format();
        message.response_format = F::response_format();
        Self {
            message,
            handler: Some(f.into_handler()),
        }
    }

    /// Create a registration for an **async** typed function.
    ///
    /// Auto-extracts `request_format` / `response_format` from the function's
    /// argument and return types via `schemars`.
    pub fn new_async<F, M>(f: F) -> Self
    where
        F: IntoAsyncHandler<M>,
    {
        let mut message = empty_message();
        message.request_format = F::request_format();
        message.response_format = F::response_format();
        Self {
            message,
            handler: Some(f.into_handler()),
        }
    }

    /// Create a registration for an **HTTP-invoked** function (Lambda,
    /// Cloudflare Workers, etc.). No local handler runs.
    pub fn http(config: HttpInvocationConfig) -> Self {
        let mut message = empty_message();
        message.invocation = Some(config);
        Self {
            message,
            handler: None,
        }
    }

    /// Set the function description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.message.description = Some(desc.into());
        self
    }

    /// Set function metadata.
    pub fn metadata(mut self, meta: Value) -> Self {
        self.message.metadata = Some(meta);
        self
    }

    /// Set the request format schema. Overrides any auto-extracted schema.
    pub fn request_format(mut self, schema: Value) -> Self {
        self.message.request_format = Some(schema);
        self
    }

    /// Set the response format schema. Overrides any auto-extracted schema.
    pub fn response_format(mut self, schema: Value) -> Self {
        self.message.response_format = Some(schema);
        self
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        RegisterFunctionMessage,
        Option<RemoteFunctionHandlerWithMetadata>,
    ) {
        (self.message, self.handler)
    }
}

/// Connection-loop timings.
///
/// Private knobs — unit tests shorten them to exercise reconnect paths
/// quickly.
// ponytail: knobs stay private; promote to InitOptions when an operator asks.
#[derive(Clone, Copy, Debug)]
struct ConnTimings {
    /// Cap on a single WS connect (TCP + TLS + HTTP upgrade). Without it a
    /// stalled socket wedges the reconnect loop forever (MOT-3857).
    connect_timeout: Duration,
    /// How often to send a WS ping so an idle link produces traffic.
    ping_interval: Duration,
    /// Reconnect if no frame (pongs included) arrives for this long —
    /// detects half-open sockets where the engine already dropped us and
    /// unregistered our functions.
    idle_timeout: Duration,
    /// Delay between reconnect attempts.
    retry_delay: Duration,
}

impl Default for ConnTimings {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            ping_interval: Duration::from_secs(20),
            idle_timeout: Duration::from_secs(60),
            retry_delay: Duration::from_secs(2),
        }
    }
}

struct IIIInner {
    address: String,
    outbound: mpsc::UnboundedSender<Outbound>,
    receiver: Mutex<Option<mpsc::UnboundedReceiver<Outbound>>>,
    running: AtomicBool,
    started: AtomicBool,
    pending: Mutex<HashMap<Uuid, PendingInvocation>>,
    functions: Mutex<HashMap<String, RemoteFunctionData>>,
    trigger_types: Mutex<HashMap<String, RemoteTriggerTypeData>>,
    triggers: Mutex<HashMap<String, RegisterTriggerMessage>>,
    worker_metadata: Mutex<Option<WorkerMetadata>>,
    connection_state: Mutex<IIIConnectionState>,
    /// Set when the engine rejects registration (fatal, no reconnect).
    fatal_error: Mutex<Option<Error>>,
    connection_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    headers: Mutex<Option<HashMap<String, String>>>,
    otel_config: Mutex<Option<OtelConfig>>,
    timings: Mutex<ConnTimings>,
    /// Engine-assigned worker id from `WorkerRegistered`; presented back via
    /// `Message::Reattach` on reconnect so the engine retires the previous
    /// connection before the registration replay.
    worker_id: Mutex<Option<String>>,
    /// Secret paired with `worker_id` (from the same `WorkerRegistered`
    /// frame); required by the engine to authorize the reattach — ids alone
    /// are publicly discoverable.
    reattach_token: Mutex<Option<String>>,
}

/// WebSocket client for communication with the III Engine.
///
/// Create with [`register_worker`](crate::register_worker).
#[derive(Clone)]
pub struct IIIClient {
    inner: Arc<IIIInner>,
}

impl IIIClient {
    /// Create a new III with default worker metadata (auto-detected runtime, os, hostname)
    pub fn new(address: &str) -> Self {
        Self::with_metadata(address, WorkerMetadata::default())
    }

    /// Create a new III with custom worker metadata
    pub fn with_metadata(address: &str, metadata: WorkerMetadata) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let inner = IIIInner {
            address: address.into(),
            outbound: tx,
            receiver: Mutex::new(Some(rx)),
            running: AtomicBool::new(false),
            started: AtomicBool::new(false),
            pending: Mutex::new(HashMap::new()),
            functions: Mutex::new(HashMap::new()),
            trigger_types: Mutex::new(HashMap::new()),
            triggers: Mutex::new(HashMap::new()),
            worker_metadata: Mutex::new(Some(metadata)),
            connection_state: Mutex::new(IIIConnectionState::Disconnected),
            fatal_error: Mutex::new(None),
            connection_thread: Mutex::new(None),
            headers: Mutex::new(None),
            otel_config: Mutex::new(None),
            timings: Mutex::new(ConnTimings::default()),
            worker_id: Mutex::new(None),
            reattach_token: Mutex::new(None),
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Get the engine WebSocket address this client connects to.
    pub fn address(&self) -> &str {
        &self.inner.address
    }

    /// Set custom worker metadata (call before connect)
    pub fn set_metadata(&self, metadata: WorkerMetadata) {
        *self.inner.worker_metadata.lock_or_recover() = Some(metadata);
    }

    /// Override the worker's target namespace (call before connect). Applied by
    /// [`register_worker`](crate::register_worker) after resolving
    /// `InitOptions.namespace` and `III_NAMESPACE`.
    pub fn set_namespace(&self, namespace: impl Into<String>) {
        if let Some(md) = self.inner.worker_metadata.lock_or_recover().as_mut() {
            md.namespace = Some(namespace.into());
        }
    }

    /// Fatal error that stopped the worker, if any. Set when the engine rejects
    /// registration (see [`Error::RegistrationRejected`]); the worker does not
    /// reconnect once this is populated.
    pub fn fatal_error(&self) -> Option<Error> {
        self.inner.fatal_error.lock_or_recover().clone()
    }

    /// Set custom HTTP headers for the WebSocket handshake (call before connect).
    pub fn set_headers(&self, headers: HashMap<String, String>) {
        *self.inner.headers.lock_or_recover() = Some(headers);
    }

    /// Set OpenTelemetry configuration (call before connect)
    pub fn set_otel_config(&self, config: OtelConfig) {
        *self.inner.otel_config.lock_or_recover() = Some(config);
    }

    pub(crate) fn connect(&self) {
        if self.inner.started.swap(true, Ordering::SeqCst) {
            return;
        }

        let receiver = self.inner.receiver.lock_or_recover().take();
        let Some(rx) = receiver else { return };

        self.inner.running.store(true, Ordering::SeqCst);

        let iii = self.clone();

        let otel_config = {
            let mut config = self
                .inner
                .otel_config
                .lock_or_recover()
                .take()
                .unwrap_or_default();
            if config.engine_ws_url.is_none() {
                config.engine_ws_url = Some(self.inner.address.clone());
            }
            config
        };

        // Spawn a dedicated OS thread with its own tokio runtime so
        // the connection loop is independent of the caller's runtime.
        // In Rust, a spawned thread does not keep the process alive on its own;
        // call shutdown() to signal the thread and join connection_thread so
        // run_connection() can exit cleanly before main() returns.
        let handle = std::thread::Builder::new()
            .name("iii-connection".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to create iii connection runtime");

                rt.block_on(async move {
                    let otel_active = telemetry::init_otel(otel_config).await;

                    iii.run_connection(rx).await;

                    if otel_active {
                        telemetry::shutdown_otel().await;
                    }
                });
            })
            .expect("failed to spawn iii connection thread");

        *self.inner.connection_thread.lock_or_recover() = Some(handle);
    }

    /// Shutdown the III client and wait for the connection thread to finish.
    ///
    /// This stops the connection loop, sends a shutdown signal, and joins
    /// the background connection thread. OpenTelemetry is flushed inside the
    /// connection thread before it exits.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use iii_sdk::{register_worker, InitOptions};
    /// # let worker = register_worker("ws://localhost:49134", InitOptions::default());
    /// worker.shutdown();
    /// ```
    pub fn shutdown(&self) {
        self.inner.running.store(false, Ordering::SeqCst);
        let _ = self.inner.outbound.send(Outbound::Shutdown);
        self.set_connection_state(IIIConnectionState::Disconnected);

        if let Some(handle) = self.inner.connection_thread.lock_or_recover().take() {
            let _ = handle.join();
        }
    }

    /// Shutdown the III client.
    ///
    /// This stops the connection loop and sends a shutdown signal, but it
    /// does not join `connection_thread`.
    ///
    /// This method returns without waiting for `run_connection()` to finish,
    /// making it safe to call from an async context without stalling the
    /// executor; [`shutdown`](Self::shutdown) blocks and joins the thread.
    /// The OpenTelemetry flush (`telemetry::shutdown_otel()`) still runs inside the connection thread
    /// after `run_connection()` returns, so it may not complete unless
    /// [`shutdown`](Self::shutdown) is used to join the thread.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use iii_sdk::{register_worker, InitOptions};
    /// # async fn docs() {
    /// # let worker = register_worker("ws://localhost:49134", InitOptions::default());
    /// worker.shutdown_async().await;
    /// # }
    /// ```
    pub async fn shutdown_async(&self) {
        self.inner.running.store(false, Ordering::SeqCst);
        let _ = self.inner.outbound.send(Outbound::Shutdown);
        self.set_connection_state(IIIConnectionState::Disconnected);
    }

    fn register_function_inner(
        &self,
        message: RegisterFunctionMessage,
        handler: Option<RemoteFunctionHandlerWithMetadata>,
    ) -> FunctionRef {
        let id = message.id.clone();
        if id.trim().is_empty() {
            panic!("id is required");
        }
        let data = RemoteFunctionData {
            message: message.clone(),
            handler,
        };
        let mut funcs = self.inner.functions.lock_or_recover();
        match funcs.entry(id.clone()) {
            std::collections::hash_map::Entry::Occupied(_) => {
                panic!("function id '{}' already registered", id);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(data);
            }
        }
        drop(funcs);
        let _ = self.send_message(message.to_message());

        let iii = self.clone();
        let unregister_id = id.clone();
        let unregister_fn = Arc::new(move || {
            let _ = iii.inner.functions.lock_or_recover().remove(&unregister_id);
            let _ = iii.send_message(Message::UnregisterFunction {
                id: unregister_id.clone(),
            });
        });

        FunctionRef { id, unregister_fn }
    }

    /// Register a function with the engine.
    ///
    /// Argument order matches the Node and Python SDKs:
    /// `(id, registration)`.
    ///
    /// # Arguments
    /// * `id` - Unique identifier for the function.
    /// * `registration` - Built via [`RegisterFunction::new`],
    ///   [`RegisterFunction::new_async`], or [`RegisterFunction::http`].
    ///   Chain `.description(...)`, `.metadata(...)`, `.request_format(...)`,
    ///   `.response_format(...)` as needed.
    ///
    /// # Panics
    /// Panics if `id` is empty or already registered.
    ///
    /// # Examples
    /// ```rust,no_run
    /// use iii_sdk::{register_worker, InitOptions, Error, RegisterFunction};
    /// use serde::{Deserialize, Serialize};
    /// use schemars::JsonSchema;
    ///
    /// #[derive(Deserialize, JsonSchema)]
    /// struct Input { name: String }
    /// #[derive(Serialize, JsonSchema)]
    /// struct Output { message: String }
    ///
    /// async fn greet(input: Input) -> Result<Output, Error> {
    ///     Ok(Output { message: format!("Hello, {}!", input.name) })
    /// }
    ///
    /// let worker = register_worker("ws://localhost:49134", InitOptions::default());
    /// worker.register_function(
    ///     "greetings::greet",
    ///     RegisterFunction::new_async(greet).description("Greets a user"),
    /// );
    /// ```
    ///
    /// Registration metadata stays on the builder, so the no-metadata path remains
    /// clean:
    /// ```rust,no_run
    /// # use iii_sdk::{register_worker, InitOptions, RegisterFunction};
    /// # use serde_json::{json, Value};
    /// # let worker = register_worker("ws://localhost:49134", InitOptions::default());
    /// worker.register_function(
    ///     "orders::create",
    ///     RegisterFunction::new_async(|input: Value| async move { Ok(input) })
    ///         .metadata(json!({"owner": "billing-team", "priority": "high"})),
    /// );
    /// ```
    ///
    /// Untyped handler taking `serde_json::Value`:
    /// ```rust,no_run
    /// # use iii_sdk::{register_worker, InitOptions, RegisterFunction};
    /// # use serde_json::{json, Value};
    /// # let worker = register_worker("ws://localhost:49134", InitOptions::default());
    /// worker.register_function(
    ///     "debug::echo",
    ///     RegisterFunction::new_async(|input: Value| async move { Ok(json!({"echo": input})) }),
    /// );
    /// ```
    ///
    /// HTTP-invoked function:
    /// ```rust,no_run
    /// # use iii_sdk::{register_worker, InitOptions, RegisterFunction};
    /// # use iii_helpers::http::{HttpInvocationConfig, HttpMethod};
    /// # use std::collections::HashMap;
    /// # let worker = register_worker("ws://localhost:49134", InitOptions::default());
    /// let config = HttpInvocationConfig {
    ///     url: "https://example.com/invoke".into(),
    ///     method: HttpMethod::Post,
    ///     timeout_ms: Some(30_000),
    ///     headers: HashMap::new(),
    ///     auth: None,
    /// };
    /// worker.register_function("ext::lambda", RegisterFunction::http(config));
    /// ```
    pub fn register_function(
        &self,
        id: impl Into<String>,
        registration: RegisterFunction,
    ) -> FunctionRef {
        let (mut message, handler) = registration.into_parts();
        message.id = id.into();
        self.register_function_inner(message, handler)
    }

    /// Register a custom trigger type with the engine.
    ///
    /// Returns a [`TriggerTypeRef`] handle that can register triggers and
    /// functions with compile-time validated types.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use iii_sdk::{IIIClient, RegisterTriggerType};
    /// # use iii_sdk::trigger::{TriggerConfig, TriggerHandler};
    /// # struct MyHandler;
    /// # #[async_trait::async_trait]
    /// # impl TriggerHandler for MyHandler {
    /// #     async fn register_trigger(&self, _: TriggerConfig) -> Result<(), iii_sdk::Error> { Ok(()) }
    /// #     async fn unregister_trigger(&self, _: TriggerConfig) -> Result<(), iii_sdk::Error> { Ok(()) }
    /// # }
    /// # #[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)] struct MyConfig { url: String }
    /// # #[derive(serde::Deserialize, schemars::JsonSchema)] struct MyRequest { data: String }
    /// # let worker = IIIClient::new("ws://localhost:49134");
    /// let my_trigger = worker.register_trigger_type(
    ///     RegisterTriggerType::new("my-trigger", "My custom trigger", MyHandler)
    ///         .trigger_request_format::<MyConfig>()
    ///         .call_request_format::<MyRequest>(),
    /// );
    ///
    /// // Compile-time safe: config must be MyConfig, function input must be MyRequest
    /// my_trigger.register_function("my::handler", |req: MyRequest| -> Result<serde_json::Value, iii_sdk::Error> {
    ///     Ok(serde_json::json!({ "data": req.data }))
    /// });
    /// my_trigger.register_trigger("my::handler", MyConfig { url: "/hook".into() });
    /// ```
    pub fn register_trigger_type<H, C, R>(
        &self,
        trigger_type: RegisterTriggerType<H, C, R>,
    ) -> TriggerTypeRef<C, R>
    where
        H: TriggerHandler + 'static,
    {
        let message = RegisterTriggerTypeMessage {
            id: trigger_type.id,
            description: trigger_type.description,
            trigger_request_format: trigger_type.trigger_request_format,
            call_request_format: trigger_type.call_request_format,
        };

        let trigger_type_id = message.id.clone();

        self.inner.trigger_types.lock_or_recover().insert(
            message.id.clone(),
            RemoteTriggerTypeData {
                message: message.clone(),
                handler: Arc::new(trigger_type.handler),
            },
        );

        let _ = self.send_message(message.to_message());

        TriggerTypeRef {
            iii: self.clone(),
            trigger_type_id,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Unregister a previously registered trigger type.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use iii_sdk::{register_worker, InitOptions};
    /// # let worker = register_worker("ws://localhost:49134", InitOptions::default());
    /// worker.unregister_trigger_type("cron");
    /// ```
    pub fn unregister_trigger_type(&self, id: impl Into<String>) {
        let id = id.into();
        self.inner.trigger_types.lock_or_recover().remove(&id);
        let msg = UnregisterTriggerTypeMessage { id };
        let _ = self.send_message(msg.to_message());
    }

    /// Bind a trigger configuration to a registered function.
    /// <!-- docs:expand-params -->
    ///
    /// # Arguments
    /// * `input` - Trigger registration input with trigger_type, function_id, and config.
    ///
    /// # Examples
    /// ```rust
    /// # use iii_sdk::IIIClient;
    /// # use iii_sdk::protocol::RegisterTriggerInput;
    /// # use serde_json::json;
    /// # let worker = IIIClient::new("ws://localhost:49134");
    /// let trigger = worker.register_trigger(RegisterTriggerInput {
    ///     trigger_type: "http".to_string(),
    ///     function_id: "greet".to_string(),
    ///     config: json!({ "api_path": "/greet", "http_method": "GET" }),
    ///     metadata: None,
    /// })?;
    /// // Later...
    /// trigger.unregister();
    /// # Ok::<(), iii_sdk::Error>(())
    /// ```
    pub fn register_trigger(&self, input: RegisterTriggerInput) -> Result<Trigger, Error> {
        let id = Uuid::new_v4().to_string();
        let message = RegisterTriggerMessage {
            id: id.clone(),
            trigger_type: input.trigger_type,
            function_id: input.function_id,
            config: input.config,
            metadata: input.metadata,
            namespace: input.namespace,
        };

        self.inner
            .triggers
            .lock_or_recover()
            .insert(message.id.clone(), message.clone());
        let _ = self.send_message(message.to_message());

        let iii = self.clone();
        let trigger_type = message.trigger_type.clone();
        let unregister_id = message.id.clone();
        let unregister_fn = Arc::new(move || {
            let _ = iii.inner.triggers.lock_or_recover().remove(&unregister_id);
            let msg = UnregisterTriggerMessage {
                id: unregister_id.clone(),
                trigger_type: trigger_type.clone(),
            };
            let _ = iii.send_message(msg.to_message());
        });

        Ok(Trigger::new(unregister_fn))
    }

    /// Invoke a remote function.
    /// <!-- docs:expand-params: TriggerRequest -->
    ///
    /// The routing behavior depends on the `action` field of the request:
    /// - No action: synchronous, waits for the function to return.
    /// - [`TriggerAction::Enqueue`]: async via named queue.
    /// - [`TriggerAction::Void`][]: fire-and-forget.
    ///
    /// # Examples
    /// ```rust
    /// # use iii_sdk::{IIIClient, TriggerAction};
    /// # use iii_sdk::protocol::TriggerRequest;
    /// # use serde_json::json;
    /// # async fn example(worker: &IIIClient) -> Result<(), iii_sdk::Error> {
    /// // Synchronous
    /// let result = worker.trigger(TriggerRequest {
    ///     function_id: "greet".to_string(),
    ///     payload: json!({"name": "World"}),
    ///     action: None,
    ///     timeout_ms: None,
    /// }).await?;
    ///
    /// // Fire-and-forget
    /// worker.trigger(TriggerRequest {
    ///     function_id: "notify".to_string(),
    ///     payload: json!({}),
    ///     action: Some(TriggerAction::Void),
    ///     timeout_ms: None,
    /// }).await?;
    ///
    /// // Enqueue (the queue must be declared in the queue worker's
    /// // queue_configs)
    /// let receipt = worker.trigger(TriggerRequest {
    ///     function_id: "iii::durable::publish".to_string(),
    ///     payload: json!({"topic": "test"}),
    ///     action: Some(TriggerAction::Enqueue { queue: "test".to_string() }),
    ///     timeout_ms: None,
    /// }).await?;
    ///
    /// // Metadata
    /// worker.trigger(
    ///     TriggerRequest {
    ///         function_id: "audit::write".to_string(),
    ///         payload: json!({"event": "checkout"}),
    ///         action: Some(TriggerAction::Void),
    ///         timeout_ms: None,
    ///     }
    ///     .metadata(json!({"tenant": "acme"})),
    /// ).await?;
    ///
    /// # Ok(())
    /// # }
    /// ```
    pub async fn trigger(
        &self,
        request: impl Into<TriggerRequestWithMetadata>,
    ) -> Result<Value, Error> {
        let request = request.into();
        let req = request.request;
        let metadata = request.metadata;
        let namespace = request.namespace;
        let (tp, bg) = inject_trace_headers();

        // Void is fire-and-forget, no invocation_id, no response
        if matches!(req.action, Some(TriggerAction::Void)) {
            self.send_message(Message::InvokeFunction {
                invocation_id: None,
                function_id: req.function_id,
                data: req.payload,
                traceparent: tp,
                baggage: bg,
                action: req.action,
                metadata,
                namespace,
            })?;
            return Ok(Value::Null);
        }

        // Enqueue and default: use invocation_id to receive acknowledgement/result
        let timeout = Duration::from_millis(req.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS));
        let invocation_id = Uuid::new_v4();
        let (tx, rx) = oneshot::channel();

        self.inner
            .pending
            .lock_or_recover()
            .insert(invocation_id, tx);

        self.send_message(Message::InvokeFunction {
            invocation_id: Some(invocation_id),
            function_id: req.function_id,
            data: req.payload,
            traceparent: tp,
            baggage: bg,
            action: req.action,
            metadata,
            namespace,
        })?;

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(Error::NotConnected),
            Err(_) => {
                self.inner.pending.lock_or_recover().remove(&invocation_id);
                Err(Error::Timeout)
            }
        }
    }

    /// Get the current connection state.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use iii_sdk::{register_worker, InitOptions};
    /// # use iii_sdk::runtime::IIIConnectionState;
    /// # let worker = register_worker("ws://localhost:49134", InitOptions::default());
    /// if worker.get_connection_state() != IIIConnectionState::Connected {
    ///     eprintln!("engine not reachable yet");
    /// }
    /// ```
    pub fn get_connection_state(&self) -> IIIConnectionState {
        *self.inner.connection_state.lock_or_recover()
    }

    fn set_connection_state(&self, state: IIIConnectionState) {
        let mut current = self.inner.connection_state.lock_or_recover();
        if *current == state {
            return;
        }
        *current = state;
    }

    /// Register this worker's metadata with the engine (called automatically on connect)
    fn register_worker_metadata(&self) {
        if let Some(mut metadata) = self.inner.worker_metadata.lock_or_recover().clone() {
            let fw = metadata
                .telemetry
                .as_ref()
                .and_then(|t| t.framework.as_deref())
                .unwrap_or("");
            if fw.is_empty() {
                let telem = metadata.telemetry.get_or_insert_with(Default::default);
                telem.framework = Some("iii-rust".to_string());
            }
            if let Ok(value) = serde_json::to_value(metadata) {
                let _ = self.send_message(Message::InvokeFunction {
                    invocation_id: None,
                    function_id: "engine::workers::register".to_string(),
                    data: value,
                    traceparent: None,
                    baggage: None,
                    action: Some(TriggerAction::Void),
                    metadata: None,
                    namespace: None,
                });
            }
        }
    }

    fn send_message(&self, message: Message) -> Result<(), Error> {
        if !self.inner.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        self.inner
            .outbound
            .send(Outbound::Message(message))
            .map_err(|_| Error::NotConnected)
    }

    async fn run_connection(&self, mut rx: mpsc::UnboundedReceiver<Outbound>) {
        let mut queue: Vec<Message> = Vec::new();
        let mut has_connected_before = false;

        while self.inner.running.load(Ordering::SeqCst) {
            let t = *self.inner.timings.lock_or_recover();
            self.set_connection_state(if has_connected_before {
                IIIConnectionState::Reconnecting
            } else {
                IIIConnectionState::Connecting
            });

            let custom_headers = self.inner.headers.lock_or_recover().clone();

            // Cap the whole connect (TCP + TLS + WS upgrade): a stalled
            // socket otherwise wedges this loop forever and the worker sits
            // in Reconnecting with zero functions registered engine-side
            // (MOT-3857).
            let connect_result = tokio::time::timeout(t.connect_timeout, async {
                if let Some(ref h) = custom_headers {
                    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
                    use tokio_tungstenite::tungstenite::http;
                    let mut request = self
                        .inner
                        .address
                        .as_str()
                        .into_client_request()
                        .expect("valid ws request");
                    for (k, v) in h {
                        if let (Ok(name), Ok(val)) = (
                            http::header::HeaderName::from_bytes(k.as_bytes()),
                            http::header::HeaderValue::from_str(v),
                        ) {
                            request.headers_mut().insert(name, val);
                        }
                    }
                    connect_async(request).await
                } else {
                    connect_async(&self.inner.address).await
                }
            })
            .await;

            match connect_result {
                Ok(Ok((stream, _))) => {
                    tracing::info!(address = %self.inner.address, "iii connected");
                    has_connected_before = true;
                    self.set_connection_state(IIIConnectionState::Connected);
                    let (mut ws_tx, mut ws_rx) = stream.split();

                    // Reconnect: present the previous engine-assigned identity
                    // BEFORE the registration replay so the engine retires the
                    // old connection and the replay lands on a clean slate
                    // instead of racing its cleanup. The token proves we ARE
                    // that worker (ids alone are publicly listable). Sent
                    // directly (not via `queue`) so it never accumulates
                    // across retries.
                    let previous_worker_id = self.inner.worker_id.lock_or_recover().clone();
                    if let Some(previous_worker_id) = previous_worker_id {
                        let reattach_token = self.inner.reattach_token.lock_or_recover().clone();
                        if let Err(err) = self
                            .send_ws(
                                &mut ws_tx,
                                &Message::Reattach {
                                    previous_worker_id,
                                    reattach_token,
                                },
                            )
                            .await
                        {
                            tracing::warn!(error = %err, "failed to send reattach; reconnecting");
                            sleep(t.retry_delay).await;
                            continue;
                        }
                    }

                    queue.extend(self.collect_registrations());
                    Self::dedupe_registrations(&mut queue);

                    // Snapshot the registration keys we're about to send so
                    // we can drop duplicate copies still pending in `rx`.
                    // These are leftover from `register_*` calls made by user
                    // threads before the WS handshake completed: each call
                    // both inserts into the in-memory map (replayed via
                    // `collect_registrations`) AND queues into `outbound`.
                    let snapshot_ids: HashSet<String> =
                        queue.iter().filter_map(Self::registration_key).collect();

                    if let Err(err) = self.flush_queue(&mut ws_tx, &mut queue).await {
                        tracing::warn!(error = %err, "failed to flush queue");
                        sleep(t.retry_delay).await;
                        continue;
                    }

                    // Drain pre-connect leftovers from `rx`, dropping
                    // register duplicates and preserving everything else
                    // (invocations, results, channel ops, and any
                    // registrations added after the snapshot was taken).
                    let shutdown =
                        Self::drain_pre_connect_duplicates(&mut rx, &mut queue, &snapshot_ids);
                    if shutdown {
                        self.inner.running.store(false, Ordering::SeqCst);
                        return;
                    }

                    if !queue.is_empty() {
                        if let Err(err) = self.flush_queue(&mut ws_tx, &mut queue).await {
                            tracing::warn!(
                                error = %err,
                                "failed to flush post-drain queue"
                            );
                            sleep(t.retry_delay).await;
                            continue;
                        }
                    }

                    // Auto-register worker metadata on connect (like Node SDK)
                    self.register_worker_metadata();

                    let mut should_reconnect = false;

                    // Keepalive: pings make an idle link produce traffic, and
                    // a frameless idle window means the link is dead even if
                    // our sends still "succeed" (half-open socket: the engine
                    // has already dropped us and unregistered our functions
                    // while we still look Connected — MOT-3857).
                    let mut ping = interval(t.ping_interval);
                    let mut last_rx = Instant::now();

                    while self.inner.running.load(Ordering::SeqCst) && !should_reconnect {
                        tokio::select! {
                            outgoing = rx.recv() => {
                                match outgoing {
                                    Some(Outbound::Message(message)) => {
                                        if let Err(err) = self.send_ws(&mut ws_tx, &message).await {
                                            tracing::warn!(error = %err, "send failed; reconnecting");
                                            queue.push(message);
                                            should_reconnect = true;
                                        }
                                    }
                                    Some(Outbound::Shutdown) => {
                                        self.inner.running.store(false, Ordering::SeqCst);
                                        return;
                                    }
                                    None => {
                                        self.inner.running.store(false, Ordering::SeqCst);
                                        return;
                                    }
                                }
                            }
                            incoming = ws_rx.next() => {
                                match incoming {
                                    Some(Ok(frame)) => {
                                        last_rx = Instant::now();
                                        if let Err(err) = self.handle_frame(frame) {
                                            tracing::warn!(error = %err, "failed to handle frame");
                                        }
                                    }
                                    Some(Err(err)) => {
                                        tracing::warn!(error = %err, "websocket receive error");
                                        should_reconnect = true;
                                    }
                                    None => {
                                        should_reconnect = true;
                                    }
                                }
                            }
                            _ = sleep_until(last_rx + t.idle_timeout) => {
                                tracing::warn!(
                                    idle_timeout = ?t.idle_timeout,
                                    "no frames from engine within idle window; forcing reconnect"
                                );
                                should_reconnect = true;
                            }
                            _ = ping.tick() => {
                                // Bounded like every send: an unbounded await
                                // here wedges the whole select loop on a full
                                // TCP window (see send_ws).
                                let ping_send = ws_tx.send(WsMessage::Ping(Default::default()));
                                match tokio::time::timeout(t.idle_timeout, ping_send).await {
                                    Ok(Ok(())) => {}
                                    Ok(Err(err)) => {
                                        tracing::warn!(error = %err, "keepalive ping failed; reconnecting");
                                        should_reconnect = true;
                                    }
                                    Err(_) => {
                                        tracing::warn!("keepalive ping timed out; reconnecting");
                                        should_reconnect = true;
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(Err(err)) => {
                    tracing::warn!(error = %err, "failed to connect; retrying");
                }
                Err(_) => {
                    tracing::warn!(
                        connect_timeout = ?t.connect_timeout,
                        "connect attempt timed out; retrying"
                    );
                }
            }

            if self.inner.running.load(Ordering::SeqCst) {
                sleep(t.retry_delay).await;
            }
        }
    }

    fn collect_registrations(&self) -> Vec<Message> {
        let mut messages = Vec::new();

        for trigger_type in self.inner.trigger_types.lock_or_recover().values() {
            messages.push(trigger_type.message.to_message());
        }

        for function in self.inner.functions.lock_or_recover().values() {
            messages.push(function.message.to_message());
        }

        for trigger in self.inner.triggers.lock_or_recover().values() {
            messages.push(trigger.to_message());
        }

        messages
    }

    /// Returns a stable identity key for a registration message, or `None`
    /// for non-registration messages (invocations, ping/pong, etc.).
    ///
    /// Used both to deduplicate within `queue` and to detect leftover
    /// pre-connect register messages in `rx` whose state has already been
    /// re-sent via `collect_registrations()`.
    fn registration_key(message: &Message) -> Option<String> {
        match message {
            Message::RegisterTriggerType { id, .. } => Some(format!("trigger_type:{id}")),
            Message::RegisterTrigger { id, .. } => Some(format!("trigger:{id}")),
            Message::RegisterFunction { id, .. } => Some(format!("function:{id}")),
            _ => None,
        }
    }

    /// Drain everything currently pending in the outbound `rx` channel,
    /// dropping register messages whose keys are already covered by
    /// `snapshot_ids` (already sent via `collect_registrations()`),
    /// and pushing every other message onto `queue` for re-flushing.
    ///
    /// Returns `true` if a `Shutdown` signal was observed during the
    /// drain, the caller should then stop the connection loop.
    fn drain_pre_connect_duplicates(
        rx: &mut mpsc::UnboundedReceiver<Outbound>,
        queue: &mut Vec<Message>,
        snapshot_ids: &HashSet<String>,
    ) -> bool {
        loop {
            match rx.try_recv() {
                Ok(Outbound::Message(msg)) => {
                    let is_dup = Self::registration_key(&msg)
                        .map(|k| snapshot_ids.contains(&k))
                        .unwrap_or(false);
                    if is_dup {
                        continue;
                    }
                    queue.push(msg);
                }
                Ok(Outbound::Shutdown) => return true,
                Err(_) => return false,
            }
        }
    }

    fn dedupe_registrations(queue: &mut Vec<Message>) {
        let mut seen = HashSet::new();
        let mut deduped_rev = Vec::with_capacity(queue.len());

        for message in queue.iter().rev() {
            match Self::registration_key(message) {
                Some(key) => {
                    if seen.insert(key) {
                        deduped_rev.push(message.clone());
                    }
                }
                None => {
                    deduped_rev.push(message.clone());
                }
            }
        }

        deduped_rev.reverse();
        *queue = deduped_rev;
    }

    async fn flush_queue(&self, ws_tx: &mut WsTx, queue: &mut Vec<Message>) -> Result<(), Error> {
        let mut drained = Vec::new();
        std::mem::swap(queue, &mut drained);

        let mut iter = drained.into_iter();
        while let Some(message) = iter.next() {
            if let Err(err) = self.send_ws(ws_tx, &message).await {
                queue.push(message);
                queue.extend(iter);
                return Err(err);
            }
        }

        Ok(())
    }

    async fn send_ws(&self, ws_tx: &mut WsTx, message: &Message) -> Result<(), Error> {
        let payload = serde_json::to_string(message)?;
        // Bound the send: on a blackholed peer with a full TCP send window,
        // send().await can block indefinitely — and since callers await this
        // inside select! handlers, an unbounded send wedges the entire
        // connection loop (idle detection included). A send that can't
        // complete within the idle window is a dead link; reconnect.
        let t = *self.inner.timings.lock_or_recover();
        match tokio::time::timeout(t.idle_timeout, ws_tx.send(WsMessage::Text(payload.into())))
            .await
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(err.into()),
            Err(_) => {
                tracing::warn!("websocket send timed out; treating link as dead");
                Err(Error::Timeout)
            }
        }
    }

    fn handle_frame(&self, frame: WsMessage) -> Result<(), Error> {
        match frame {
            WsMessage::Text(text) => self.handle_message(&text),
            WsMessage::Binary(bytes) => {
                let text = String::from_utf8_lossy(&bytes).to_string();
                self.handle_message(&text)
            }
            _ => Ok(()),
        }
    }

    fn handle_message(&self, payload: &str) -> Result<(), Error> {
        let message: Message = serde_json::from_str(payload)?;

        match message {
            Message::InvocationResult {
                invocation_id,
                result,
                error,
                ..
            } => {
                self.handle_invocation_result(invocation_id, result, error);
            }
            Message::InvokeFunction {
                invocation_id,
                function_id,
                data,
                traceparent,
                baggage,
                action: _,
                metadata,
                namespace: _,
            } => {
                self.handle_invoke_function(
                    invocation_id,
                    function_id,
                    data,
                    traceparent,
                    baggage,
                    metadata,
                );
            }
            Message::RegisterTrigger {
                id,
                trigger_type,
                function_id,
                config,
                metadata,
                // Routing namespace is resolved engine-side; a custom trigger-type
                // handler does not need it.
                namespace: _,
            } => {
                self.handle_register_trigger(id, trigger_type, function_id, config, metadata);
            }
            Message::UnregisterTrigger { id, trigger_type } => {
                self.handle_unregister_trigger(id, trigger_type);
            }
            Message::Ping => {
                let _ = self.send_message(Message::Pong);
            }
            Message::WorkerRegistered {
                worker_id,
                reattach_token,
            } => {
                tracing::debug!(worker_id = %worker_id, "Worker registered");
                *self.inner.worker_id.lock_or_recover() = Some(worker_id);
                *self.inner.reattach_token.lock_or_recover() = reattach_token;
            }
            Message::RegistrationRejected {
                code,
                namespace,
                worker_name,
                owner_worker_id,
            } => {
                self.handle_registration_rejected(code, namespace, worker_name, owner_worker_id);
            }
            Message::TriggerRegistrationResult {
                id,
                trigger_type,
                function_id: _,
                error: Some(err),
            } => {
                tracing::error!(
                    trigger_id = %id,
                    trigger_type = %trigger_type,
                    code = %err.code,
                    "[iii] Trigger registration failed for {:?}: {}",
                    id,
                    err.message
                );
            }
            _ => {}
        }

        Ok(())
    }

    /// Dispatch a `RegistrationRejected` message by its `code`.
    ///
    /// The two rejection codes carry different semantics. A worker-name
    /// conflict is fatal — the engine has closed the connection and the SDK
    /// must not reconnect into the same collision. A function-id conflict costs
    /// the worker only that one function: the connection stays open and every
    /// other export keeps serving, so it must not be treated as fatal. Any
    /// unrecognised code is treated as fatal, the safe default.
    fn handle_registration_rejected(
        &self,
        code: String,
        namespace: String,
        worker_name: String,
        owner_worker_id: String,
    ) {
        match code.as_str() {
            FUNCTION_NAMESPACE_CONFLICT => {
                // Non-fatal. `worker_name` carries the rejected function id here
                // (the engine reuses the struct field).
                tracing::warn!(
                    code = %code,
                    namespace = %namespace,
                    function_id = %worker_name,
                    owner_worker_id = %owner_worker_id,
                    "function registration rejected: another worker in this namespace already \
                     owns this function id; the worker keeps serving its other functions"
                );
            }
            WORKER_NAMESPACE_CONFLICT => {
                self.fail_registration_fatal(code, namespace, worker_name, owner_worker_id);
            }
            _ => {
                tracing::error!(
                    code = %code,
                    "registration rejected with an unknown code; treating as fatal"
                );
                self.fail_registration_fatal(code, namespace, worker_name, owner_worker_id);
            }
        }
    }

    /// Record a fatal registration rejection: surface the error, mark the
    /// connection failed, and clear the running flag so the connection loop
    /// exits instead of reconnecting into the same collision.
    fn fail_registration_fatal(
        &self,
        code: String,
        namespace: String,
        worker_name: String,
        owner_worker_id: String,
    ) {
        let err = Error::RegistrationRejected {
            code,
            namespace,
            worker_name,
            owner_worker_id,
        };
        tracing::error!(error = %err, "worker registration rejected; not reconnecting");
        *self.inner.fatal_error.lock_or_recover() = Some(err);
        self.set_connection_state(IIIConnectionState::Failed);
        self.inner.running.store(false, Ordering::SeqCst);
    }

    fn handle_invocation_result(
        &self,
        invocation_id: Uuid,
        result: Option<Value>,
        error: Option<ErrorBody>,
    ) {
        let sender = self.inner.pending.lock_or_recover().remove(&invocation_id);
        if let Some(sender) = sender {
            let result = match error {
                Some(error) => Err(Error::Remote {
                    code: error.code,
                    message: error.message,
                    stacktrace: error.stacktrace,
                }),
                None => Ok(result.unwrap_or(Value::Null)),
            };
            let _ = sender.send(result);
        }
    }

    fn handle_invoke_function(
        &self,
        invocation_id: Option<Uuid>,
        function_id: String,
        data: Value,
        traceparent: Option<String>,
        baggage: Option<String>,
        metadata: Option<Value>,
    ) {
        tracing::debug!(function_id = %function_id, traceparent = ?traceparent, baggage = ?baggage, "Invoking function");

        let func_data = self
            .inner
            .functions
            .lock_or_recover()
            .get(&function_id)
            .cloned();
        let handler = func_data.as_ref().and_then(|d| d.handler.clone());

        let Some(handler) = handler else {
            let (code, message) = match &func_data {
                Some(_) => (
                    "function_not_invokable".to_string(),
                    "Function is HTTP-invoked and cannot be invoked locally".to_string(),
                ),
                None => (
                    "function_not_found".to_string(),
                    "Function not found".to_string(),
                ),
            };
            tracing::warn!(function_id = %function_id, "Invocation: {}", message);

            if let Some(invocation_id) = invocation_id {
                let (resp_tp, resp_bg) = inject_trace_headers();

                let error = ErrorBody {
                    code,
                    message,
                    stacktrace: None,
                };
                let result = self.send_message(Message::InvocationResult {
                    invocation_id,
                    function_id,
                    result: None,
                    error: Some(error),
                    traceparent: resp_tp,
                    baggage: resp_bg,
                });

                if let Err(err) = result {
                    tracing::warn!(error = %err, "error sending invocation result");
                }
            }
            return;
        };

        let iii = self.clone();

        tokio::spawn(async move {
            // Extract incoming trace context and create a span for this invocation.
            // This ensures the handler and any outbound calls it makes (e.g.
            // invoke_function_with_timeout) are linked as children of the caller's trace.
            // We use FutureExt::with_context() instead of cx.attach() because
            // ContextGuard is !Send and can't be held across .await in tokio::spawn.
            let otel_cx = {
                use iii_helpers::observability::extract_context;
                use iii_helpers::observability::opentelemetry::trace::{
                    SpanKind, TraceContextExt, Tracer,
                };

                let parent_cx = extract_context(traceparent.as_deref(), baggage.as_deref());
                let tracer =
                    iii_helpers::observability::opentelemetry::global::tracer("iii-rust-sdk");
                // INTERNAL and named `execute` (not `call`/`trigger`): the engine
                // already emits the SERVER `call <fn>` span for this hop AND a
                // `trigger <fn>` span from fire_triggers. Reusing either name would
                // duplicate an engine span under the worker's service. `execute` is
                // unique, so the worker handler span reads as a clean internal child
                // of the engine's call span (and is collapsible by a single rule).
                let span = tracer
                    .span_builder(format!("execute {}", function_id))
                    .with_kind(SpanKind::Internal)
                    .start_with_context(&tracer, &parent_cx);
                parent_cx.with_span(span)
            };

            let trace_payloads = !std::env::var("III_DISABLE_TRACE_PAYLOADS")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);

            let payload_max_bytes = iii_helpers::observability::resolve_max_bytes_from_env();

            if trace_payloads {
                use iii_helpers::observability::opentelemetry::KeyValue;
                use iii_helpers::observability::opentelemetry::trace::TraceContextExt;
                use iii_helpers::observability::redact_and_truncate;
                let span = otel_cx.span();
                if span.span_context().is_valid() {
                    let (input_json, truncated) = redact_and_truncate(&data, payload_max_bytes);
                    span.add_event(
                        "iii.invocation.input",
                        vec![
                            KeyValue::new("iii.payload.json", input_json),
                            KeyValue::new("iii.payload.truncated", truncated),
                        ],
                    );
                }
            }

            let result = {
                use iii_helpers::observability::opentelemetry::trace::FutureExt as OtelFutureExt;
                handler(data, metadata).with_context(otel_cx.clone()).await
            };

            if trace_payloads {
                use iii_helpers::observability::opentelemetry::KeyValue;
                use iii_helpers::observability::opentelemetry::trace::TraceContextExt;
                use iii_helpers::observability::redact_and_truncate;
                let span = otel_cx.span();
                if span.span_context().is_valid() {
                    let (output_json, truncated, ok) = match &result {
                        Ok(value) => {
                            let (j, t) = redact_and_truncate(value, payload_max_bytes);
                            (j, t, true)
                        }
                        Err(err) => {
                            let payload = serde_json::json!({ "error": err.to_string() });
                            let (j, t) = redact_and_truncate(&payload, payload_max_bytes);
                            (j, t, false)
                        }
                    };
                    span.add_event(
                        "iii.invocation.output",
                        vec![
                            KeyValue::new("iii.payload.json", output_json),
                            KeyValue::new("iii.payload.truncated", truncated),
                            KeyValue::new("iii.payload.ok", ok),
                        ],
                    );
                }
            }

            // Record span status based on result
            let mut error_stacktrace: Option<String> = None;
            {
                use iii_helpers::observability::opentelemetry::KeyValue;
                use iii_helpers::observability::opentelemetry::trace::{Status, TraceContextExt};
                let span = otel_cx.span();
                match &result {
                    Ok(_) => span.set_status(Status::Ok),
                    Err(err) => {
                        let (exc_type, exc_message, stacktrace) = match err {
                            Error::Remote {
                                code,
                                message,
                                stacktrace,
                            } => (
                                code.clone(),
                                message.clone(),
                                stacktrace.clone().unwrap_or_else(|| {
                                    std::backtrace::Backtrace::force_capture().to_string()
                                }),
                            ),
                            other => (
                                "InvocationError".to_string(),
                                other.to_string(),
                                std::backtrace::Backtrace::force_capture().to_string(),
                            ),
                        };
                        span.set_status(Status::error(exc_message.clone()));
                        span.add_event(
                            "exception",
                            vec![
                                KeyValue::new("exception.type", exc_type),
                                KeyValue::new("exception.message", exc_message),
                                KeyValue::new("exception.stacktrace", stacktrace.clone()),
                            ],
                        );
                        // Consumed only by the non-`Remote` fallback arm when
                        // building the wire ErrorBody below; `Remote` passes
                        // its own (possibly absent) stacktrace through.
                        error_stacktrace = Some(stacktrace);
                    }
                }
            }

            if let Some(invocation_id) = invocation_id {
                // Inject trace context from our span into the response.
                // We briefly attach the otel context (no .await crossing)
                // so inject_traceparent/inject_baggage can read it.
                let (resp_tp, resp_bg) = {
                    let _guard = otel_cx.attach();
                    inject_trace_headers()
                };

                let message = match result {
                    Ok(value) => Message::InvocationResult {
                        invocation_id,
                        function_id,
                        result: Some(value),
                        error: None,
                        traceparent: resp_tp,
                        baggage: resp_bg,
                    },
                    Err(err) => {
                        let error_body = match err {
                            Error::Remote {
                                code,
                                message,
                                stacktrace,
                            } => ErrorBody {
                                code,
                                message,
                                // `Remote` is a structured, expected error owned by
                                // the handler, respect `stacktrace: None` instead of
                                // backfilling the dispatch-loop backtrace, which
                                // points at the SDK event loop, not the error site.
                                stacktrace,
                            },
                            other => ErrorBody {
                                code: "invocation_failed".to_string(),
                                message: other.to_string(),
                                stacktrace: error_stacktrace.or_else(|| {
                                    Some(std::backtrace::Backtrace::force_capture().to_string())
                                }),
                            },
                        };
                        Message::InvocationResult {
                            invocation_id,
                            function_id,
                            result: None,
                            error: Some(error_body),
                            traceparent: resp_tp,
                            baggage: resp_bg,
                        }
                    }
                };

                let _ = iii.send_message(message);
            } else if let Err(err) = result {
                tracing::warn!(error = %err, "error handling async invocation");
            }
        });
    }

    fn handle_register_trigger(
        &self,
        id: String,
        trigger_type: String,
        function_id: String,
        config: Value,
        metadata: Option<Value>,
    ) {
        let handler = self
            .inner
            .trigger_types
            .lock_or_recover()
            .get(&trigger_type)
            .map(|data| data.handler.clone());

        let iii = self.clone();

        tokio::spawn(async move {
            let message = if let Some(handler) = handler {
                let config = TriggerConfig {
                    id: id.clone(),
                    function_id: function_id.clone(),
                    config,
                    metadata,
                };

                match handler.register_trigger(config).await {
                    Ok(()) => Message::TriggerRegistrationResult {
                        id,
                        trigger_type,
                        function_id,
                        error: None,
                    },
                    Err(err) => Message::TriggerRegistrationResult {
                        id,
                        trigger_type,
                        function_id,
                        error: Some(ErrorBody {
                            code: "trigger_registration_failed".to_string(),
                            message: err.to_string(),
                            stacktrace: None,
                        }),
                    },
                }
            } else {
                Message::TriggerRegistrationResult {
                    id,
                    trigger_type,
                    function_id,
                    error: Some(ErrorBody {
                        code: "trigger_type_not_found".to_string(),
                        message: "Trigger type not found".to_string(),
                        stacktrace: None,
                    }),
                }
            };

            let _ = iii.send_message(message);
        });
    }

    fn handle_unregister_trigger(&self, id: String, trigger_type: String) {
        let handler = self
            .inner
            .trigger_types
            .lock_or_recover()
            .get(&trigger_type)
            .map(|data| data.handler.clone());

        let Some(handler) = handler else {
            return;
        };

        tokio::spawn(async move {
            let config = TriggerConfig {
                id: id.clone(),
                function_id: String::new(),
                config: Value::Null,
                metadata: None,
            };

            if let Err(err) = handler.unregister_trigger(config).await {
                tracing::warn!(trigger_id = %id, error = %err, "Error unregistering trigger");
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Internal implementations for items relocated to the `helpers` submodule.
// Exposed at `pub(crate)` so the thin wrappers in `crate::helpers` can call
// them; user code must continue to go through `crate::helpers::*`.
// ---------------------------------------------------------------------------

pub(crate) async fn internal_create_channel(
    iii: &IIIClient,
    buffer_size: Option<usize>,
) -> Result<Channel, Error> {
    let result = iii
        .trigger(TriggerRequest {
            function_id: "engine::channels::create".to_string(),
            payload: serde_json::json!({ "buffer_size": buffer_size }),
            action: None,
            timeout_ms: None,
        })
        .await?;

    let writer_ref: StreamChannelRef = serde_json::from_value(
        result
            .get("writer")
            .cloned()
            .ok_or_else(|| Error::Serde("missing 'writer' in channel response".into()))?,
    )
    .map_err(|e| Error::Serde(e.to_string()))?;

    let reader_ref: StreamChannelRef = serde_json::from_value(
        result
            .get("reader")
            .cloned()
            .ok_or_else(|| Error::Serde("missing 'reader' in channel response".into()))?,
    )
    .map_err(|e| Error::Serde(e.to_string()))?;

    Ok(Channel {
        writer: ChannelWriter::new(&iii.inner.address, &writer_ref),
        reader: ChannelReader::new(&iii.inner.address, &reader_ref),
        writer_ref,
        reader_ref,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use iii_helpers::http::{HttpInvocationConfig, HttpMethod};

    use super::*;
    use crate::trigger::{TriggerConfig, TriggerHandler};
    use crate::{InitOptions, protocol::RegisterTriggerInput, register_worker};

    use std::sync::atomic::AtomicUsize;

    /// Raw TCP listener that accepts and then holds sockets open without
    /// ever answering (or even reading) the WS handshake — the stalled /
    /// half-open link from MOT-3857.
    async fn spawn_stalled_listener() -> (std::net::SocketAddr, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accepted = Arc::new(AtomicUsize::new(0));
        let counter = accepted.clone();
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((sock, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                held.push(sock);
            }
        });
        (addr, accepted)
    }

    /// WS server that completes handshakes and counts them. A `silent`
    /// server then holds the socket without polling it (no auto-pong, no
    /// frames — half-open link); a polling server reads frames, which lets
    /// tungstenite's auto-pong answer client pings.
    async fn spawn_ws_server(silent: bool) -> (std::net::SocketAddr, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handshakes = Arc::new(AtomicUsize::new(0));
        let counter = handshakes.clone();
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let counter = counter.clone();
                tokio::spawn(async move {
                    let Ok(mut ws) = tokio_tungstenite::accept_async(sock).await else {
                        return;
                    };
                    counter.fetch_add(1, Ordering::SeqCst);
                    if silent {
                        std::future::pending::<()>().await;
                    }
                    while let Some(frame) = ws.next().await {
                        if frame.is_err() {
                            break;
                        }
                    }
                });
            }
        });
        (addr, handshakes)
    }

    fn client_with_timings(addr: std::net::SocketAddr, timings: ConnTimings) -> IIIClient {
        let iii = IIIClient::new(&format!("ws://{addr}"));
        // Keep OTel out of these tests: its exporter would dial the same
        // server and pollute the connection counters.
        *iii.inner.otel_config.lock_or_recover() = Some(OtelConfig {
            enabled: Some(false),
            ..Default::default()
        });
        *iii.inner.timings.lock_or_recover() = timings;
        iii.connect();
        iii
    }

    async fn wait_for_count(counter: &AtomicUsize, want: usize, within: Duration) -> usize {
        let deadline = Instant::now() + within;
        loop {
            let n = counter.load(Ordering::SeqCst);
            if n >= want || Instant::now() >= deadline {
                return n;
            }
            sleep(Duration::from_millis(25)).await;
        }
    }

    #[tokio::test]
    async fn connect_timeout_abandons_stalled_connect_and_retries() {
        let (addr, accepted) = spawn_stalled_listener().await;
        let iii = client_with_timings(
            addr,
            ConnTimings {
                connect_timeout: Duration::from_millis(150),
                retry_delay: Duration::from_millis(100),
                ..ConnTimings::default()
            },
        );

        // Without the connect timeout the first attempt wedges forever and
        // no second TCP connect ever happens.
        let n = wait_for_count(&accepted, 2, Duration::from_secs(5)).await;
        iii.shutdown_async().await;
        assert!(
            n >= 2,
            "stalled connect must be abandoned and retried, saw {n} attempts"
        );
    }

    #[tokio::test]
    async fn idle_timeout_reconnects_when_engine_goes_silent() {
        let (addr, handshakes) = spawn_ws_server(true).await;
        let iii = client_with_timings(
            addr,
            ConnTimings {
                connect_timeout: Duration::from_secs(5),
                // Ping never fires: idle detection must not depend on the
                // ping cadence (and a polled test server would auto-pong).
                ping_interval: Duration::from_secs(30),
                idle_timeout: Duration::from_millis(250),
                retry_delay: Duration::from_millis(100),
            },
        );

        // A half-open link produces no frames; the idle deadline must tear
        // the connection down and reconnect (replaying registrations).
        let n = wait_for_count(&handshakes, 2, Duration::from_secs(5)).await;
        iii.shutdown_async().await;
        assert!(
            n >= 2,
            "silent link must force a reconnect, saw {n} handshakes"
        );
    }

    #[tokio::test]
    async fn keepalive_pings_hold_healthy_connection_open() {
        let (addr, handshakes) = spawn_ws_server(false).await;
        let iii = client_with_timings(
            addr,
            ConnTimings {
                connect_timeout: Duration::from_secs(5),
                ping_interval: Duration::from_millis(100),
                idle_timeout: Duration::from_secs(1),
                retry_delay: Duration::from_millis(100),
            },
        );

        // Pongs elicited by our pings count as liveness: well past the idle
        // window, the original connection must still be the only one.
        sleep(Duration::from_millis(1500)).await;
        let n = handshakes.load(Ordering::SeqCst);
        iii.shutdown_async().await;
        assert_eq!(n, 1, "healthy pinged connection must not be torn down");
    }

    struct NoopTriggerHandler;

    #[async_trait::async_trait]
    impl TriggerHandler for NoopTriggerHandler {
        async fn register_trigger(&self, _: TriggerConfig) -> Result<(), Error> {
            Ok(())
        }

        async fn unregister_trigger(&self, _: TriggerConfig) -> Result<(), Error> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn register_trigger_unregister_removes_entry() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let trigger = iii
            .register_trigger(RegisterTriggerInput {
                trigger_type: "demo".to_string(),
                function_id: "functions.echo".to_string(),
                config: json!({ "foo": "bar" }),
                metadata: None,
            })
            .unwrap();

        assert_eq!(iii.inner.triggers.lock().unwrap().len(), 1);

        trigger.unregister();

        assert_eq!(iii.inner.triggers.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn typed_trigger_ref_register_trigger_keeps_legacy_two_arg_shape() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let trigger_type = iii.register_trigger_type(
            RegisterTriggerType::new("typed-trigger", "typed trigger", NoopTriggerHandler)
                .trigger_request_format::<Value>(),
        );

        let trigger = trigger_type
            .register_trigger("functions.echo", json!({ "foo": "bar" }))
            .expect("register trigger");

        assert_eq!(iii.inner.triggers.lock().unwrap().len(), 1);
        trigger.unregister();
    }

    #[tokio::test]
    async fn typed_trigger_ref_register_trigger_with_metadata_stores_metadata() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let trigger_type = iii.register_trigger_type(
            RegisterTriggerType::new("typed-trigger-meta", "typed trigger", NoopTriggerHandler)
                .trigger_request_format::<Value>(),
        );

        let metadata = json!({ "owner": "billing-team" });
        let trigger = trigger_type
            .register_trigger_with_metadata(
                "functions.echo",
                json!({ "foo": "bar" }),
                Some(metadata.clone()),
            )
            .expect("register trigger");

        {
            let triggers = iii.inner.triggers.lock().unwrap();
            let stored = triggers.values().next().expect("stored trigger");
            assert_eq!(stored.metadata, Some(metadata));
        }
        trigger.unregister();
    }

    #[test]
    fn remote_function_handler_alias_remains_single_arg() {
        // Compile-time back-compat guard: the public RemoteFunctionHandler alias keeps
        // its pre-metadata single-argument shape; the sidecar-aware form is the separate
        // RemoteFunctionHandlerWithMetadata alias.
        let legacy: crate::types::RemoteFunctionHandler =
            std::sync::Arc::new(|input| Box::pin(async move { Ok(input) }));
        let with_meta: crate::types::RemoteFunctionHandlerWithMetadata =
            std::sync::Arc::new(|input, _metadata| Box::pin(async move { Ok(input) }));
        drop((legacy, with_meta));
    }

    #[tokio::test]
    async fn register_function_with_http_config_stores_and_unregister_removes() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let config = HttpInvocationConfig {
            url: "https://example.com/invoke".to_string(),
            method: HttpMethod::Post,
            timeout_ms: Some(30000),
            headers: HashMap::new(),
            auth: None,
        };

        let func_ref = iii.register_function("external::my_lambda", RegisterFunction::http(config));

        assert_eq!(func_ref.id, "external::my_lambda");
        assert_eq!(iii.inner.functions.lock().unwrap().len(), 1);

        func_ref.unregister();

        assert_eq!(iii.inner.functions.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    #[should_panic(expected = "id is required")]
    async fn register_function_rejects_empty_id() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let config = HttpInvocationConfig {
            url: "https://example.com/invoke".to_string(),
            method: HttpMethod::Post,
            timeout_ms: None,
            headers: HashMap::new(),
            auth: None,
        };

        iii.register_function("", RegisterFunction::http(config));
    }

    #[tokio::test]
    async fn register_function_takes_id_then_builder() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let func_ref = iii.register_function(
            "test::reshaped::ordering",
            RegisterFunction::new_async(|input: Value| async move { Ok(input) })
                .description("reshaped")
                .metadata(json!({"owner": "sdk"})),
        );
        assert_eq!(func_ref.id, "test::reshaped::ordering");

        let funcs = iii.inner.functions.lock().unwrap();
        let stored = funcs.get("test::reshaped::ordering").expect("stored");
        assert_eq!(stored.message.id, "test::reshaped::ordering");
        assert_eq!(stored.message.description.as_deref(), Some("reshaped"));
        assert_eq!(stored.message.metadata, Some(json!({"owner": "sdk"})));
        assert!(stored.handler.is_some());
    }

    #[tokio::test]
    async fn register_function_metadata_builder_is_optional() {
        let clean = RegisterFunction::new_async(|input: Value| async move { Ok(input) });
        assert!(
            clean.message.metadata.is_none(),
            "metadata should be absent unless the builder method is used"
        );

        let metadata = json!({"owner": "billing-team", "priority": "high"});
        let with_metadata = RegisterFunction::new_async(|input: Value| async move { Ok(input) })
            .metadata(metadata.clone());
        assert_eq!(with_metadata.message.metadata, Some(metadata));
    }

    #[tokio::test]
    async fn register_function_http_variant_has_no_handler() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let config = HttpInvocationConfig {
            url: "https://example.com/invoke".to_string(),
            method: HttpMethod::Post,
            timeout_ms: Some(30_000),
            headers: HashMap::new(),
            auth: None,
        };

        let func_ref = iii.register_function("external::reshaped", RegisterFunction::http(config));

        assert_eq!(func_ref.id, "external::reshaped");
        let funcs = iii.inner.functions.lock().unwrap();
        let stored = funcs.get("external::reshaped").expect("stored");
        assert!(
            stored.handler.is_none(),
            "handler should be None for HTTP invocation"
        );
        assert!(
            stored.message.invocation.is_some(),
            "invocation should be set"
        );
    }

    #[tokio::test]
    async fn register_function_new_async_extracts_schemas() {
        #[derive(serde::Deserialize, schemars::JsonSchema)]
        struct In {
            name: String,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct Out {
            message: String,
        }
        async fn greet(input: In) -> Result<Out, Error> {
            Ok(Out {
                message: format!("Hello, {}!", input.name),
            })
        }

        let reg = RegisterFunction::new_async(greet);
        assert!(reg.message.request_format.is_some());
        assert!(reg.message.response_format.is_some());
        assert_eq!(reg.message.request_format.as_ref().unwrap()["title"], "In");
        assert_eq!(
            reg.message.response_format.as_ref().unwrap()["title"],
            "Out"
        );
    }

    #[tokio::test]
    async fn register_function_request_format_setter_overrides_auto_extraction() {
        #[derive(serde::Deserialize, schemars::JsonSchema)]
        struct In {
            name: String,
        }
        async fn handler(input: In) -> Result<String, Error> {
            Ok(input.name)
        }

        let custom = json!({"custom": true});
        let reg = RegisterFunction::new_async(handler).request_format(custom.clone());
        assert_eq!(reg.message.request_format.as_ref().unwrap(), &custom);
    }

    #[tokio::test]
    async fn register_function_untyped_runs_handler() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let _func_ref = iii.register_function(
            "test::untyped",
            RegisterFunction::new_async(|input: Value| async move { Ok(json!({ "echo": input })) }),
        );
        let handler = {
            let funcs = iii.inner.functions.lock().unwrap();
            let stored = funcs.get("test::untyped").expect("stored");
            stored.handler.as_ref().expect("has handler").clone()
        };
        let out = handler(json!({"name": "world"}), None).await.unwrap();
        assert_eq!(out, json!({"echo": {"name": "world"}}));
    }

    #[tokio::test]
    async fn new_async_delivers_metadata_as_second_arg() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        iii.register_function(
            "test::with_meta",
            RegisterFunction::new_async(|input: Value, metadata: Option<Value>| async move {
                // Echo back both the payload and the metadata sidecar so the
                // test can prove they arrive as distinct arguments.
                Ok(json!({ "input": input, "metadata": metadata }))
            }),
        );
        let handler = {
            let funcs = iii.inner.functions.lock().unwrap();
            funcs
                .get("test::with_meta")
                .expect("stored")
                .handler
                .as_ref()
                .expect("has handler")
                .clone()
        };

        // Metadata present: delivered as the second argument, payload untouched.
        let out = handler(json!({ "x": 1 }), Some(json!({ "session_id": "s" })))
            .await
            .unwrap();
        assert_eq!(out["input"], json!({ "x": 1 }));
        assert_eq!(out["metadata"], json!({ "session_id": "s" }));

        // Metadata absent: handler still runs, sees null.
        let out_none = handler(json!({ "x": 2 }), None).await.unwrap();
        assert_eq!(out_none["metadata"], Value::Null);
    }

    #[tokio::test]
    async fn trigger_request_metadata_is_sent_by_trigger() {
        let iii = IIIClient::new("ws://localhost:1234");
        iii.inner
            .running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let _ = iii
            .trigger(
                TriggerRequest {
                    function_id: "svc::work".to_string(),
                    payload: json!({ "x": 1 }),
                    action: Some(TriggerAction::Void),
                    timeout_ms: None,
                }
                .metadata(json!({ "tenant": "acme" })),
            )
            .await
            .expect("void trigger should enqueue");

        let mut rx = iii.inner.receiver.lock().unwrap().take().expect("receiver");
        let sent = rx.try_recv().expect("sent invoke");
        match sent {
            Outbound::Message(Message::InvokeFunction {
                function_id,
                data,
                metadata,
                action,
                ..
            }) => {
                assert_eq!(function_id, "svc::work");
                assert_eq!(data, json!({ "x": 1 }));
                assert_eq!(metadata, Some(json!({ "tenant": "acme" })));
                assert!(matches!(action, Some(TriggerAction::Void)));
            }
            _ => panic!("expected InvokeFunction"),
        }
    }

    #[tokio::test]
    async fn trigger_request_without_metadata_keeps_legacy_struct_literal_shape() {
        let iii = IIIClient::new("ws://localhost:1234");
        iii.inner
            .running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let _ = iii
            .trigger(TriggerRequest {
                function_id: "svc::work".to_string(),
                payload: json!({ "x": 1 }),
                action: Some(TriggerAction::Void),
                timeout_ms: None,
            })
            .await
            .expect("void trigger should enqueue");

        let mut rx = iii.inner.receiver.lock().unwrap().take().expect("receiver");
        let sent = rx.try_recv().expect("sent invoke");
        match sent {
            Outbound::Message(Message::InvokeFunction {
                function_id,
                metadata,
                ..
            }) => {
                assert_eq!(function_id, "svc::work");
                assert!(metadata.is_none());
            }
            _ => panic!("expected invoke"),
        }
    }

    #[tokio::test]
    async fn invoke_function_times_out_and_clears_pending() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let result = iii
            .trigger(TriggerRequest {
                function_id: "functions.echo".to_string(),
                payload: json!({ "a": 1 }),
                action: None,
                timeout_ms: Some(10),
            })
            .await;

        assert!(matches!(result, Err(Error::Timeout)));
        assert!(iii.inner.pending.lock().unwrap().is_empty());
    }

    // Single test covers both branches so the env var mutation is serialized
    // within one function (env vars are process-global and cargo runs tests in parallel).
    #[test]
    fn worker_metadata_default_reads_iii_isolation_env_var() {
        let previous = std::env::var("III_ISOLATION").ok();

        // SAFETY: env mutations are serialized within this test and restored at the end.
        unsafe {
            std::env::remove_var("III_ISOLATION");
        }
        assert!(WorkerMetadata::default().isolation.is_none());

        unsafe {
            std::env::set_var("III_ISOLATION", "docker");
        }
        assert_eq!(
            WorkerMetadata::default().isolation.as_deref(),
            Some("docker")
        );

        unsafe {
            match previous {
                Some(val) => std::env::set_var("III_ISOLATION", val),
                None => std::env::remove_var("III_ISOLATION"),
            }
        }
    }

    // Single test covers both branches so the env var mutation is serialized
    // within one function (env vars are process-global and cargo runs tests in parallel).
    #[test]
    fn worker_metadata_default_reads_iii_worker_name_env_var() {
        let previous = std::env::var("III_WORKER_NAME").ok();

        // SAFETY: env mutations are serialized within this test and restored at the end.
        unsafe {
            std::env::remove_var("III_WORKER_NAME");
        }
        let fallback = WorkerMetadata::default().name;
        assert!(
            fallback.ends_with(&format!(":{}", std::process::id())),
            "expected hostname:pid fallback, got {fallback}"
        );

        unsafe {
            std::env::set_var("III_WORKER_NAME", "managed-worker");
        }
        assert_eq!(WorkerMetadata::default().name, "managed-worker");

        unsafe {
            match previous {
                Some(val) => std::env::set_var("III_WORKER_NAME", val),
                None => std::env::remove_var("III_WORKER_NAME"),
            }
        }
    }

    #[test]
    fn parse_cargo_package_name_extracts_name_field() {
        let toml = "[package]\nname = \"my-crate\"\nversion = \"1.0.0\"\n";
        assert_eq!(parse_cargo_package_name(toml), Some("my-crate".to_string()));
    }

    #[test]
    fn parse_cargo_package_name_ignores_other_tables() {
        let toml = "[dependencies]\nname = \"not-the-package\"\n[package]\nname = \"the-pkg\"\n";
        assert_eq!(parse_cargo_package_name(toml), Some("the-pkg".to_string()));
    }

    #[test]
    fn parse_cargo_package_name_returns_none_when_missing() {
        let toml = "[package]\nversion = \"1.0.0\"\n";
        assert_eq!(parse_cargo_package_name(toml), None);
    }

    #[test]
    fn parse_cargo_package_name_returns_none_when_blank() {
        let toml = "[package]\nname = \"\"\n";
        assert_eq!(parse_cargo_package_name(toml), None);
    }

    #[test]
    fn detect_project_name_reads_cargo_toml_in_cwd() {
        let tmp = std::env::temp_dir().join(format!("iii-rust-detect-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[package]\nname = \"detected-crate\"\n",
        )
        .unwrap();

        assert_eq!(
            detect_project_name(Some(tmp.clone())),
            Some("detected-crate".to_string())
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn detect_project_name_falls_back_to_dir_basename_without_cargo_toml() {
        let tmp = std::env::temp_dir().join(format!("iii-rust-fallback-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();

        let basename = tmp.file_name().unwrap().to_str().unwrap().to_string();
        assert_eq!(detect_project_name(Some(tmp.clone())), Some(basename));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn detect_project_name_falls_back_to_dir_basename_when_cargo_toml_lacks_name() {
        let tmp =
            std::env::temp_dir().join(format!("iii-rust-fallback-noname-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("Cargo.toml"), "[package]\nversion = \"1.0.0\"\n").unwrap();

        let basename = tmp.file_name().unwrap().to_str().unwrap().to_string();
        assert_eq!(detect_project_name(Some(tmp.clone())), Some(basename));

        std::fs::remove_dir_all(&tmp).ok();
    }

    fn make_register_function(id: &str) -> Message {
        Message::RegisterFunction {
            id: id.to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        }
    }

    fn make_register_trigger(id: &str) -> Message {
        Message::RegisterTrigger {
            id: id.to_string(),
            trigger_type: "demo".to_string(),
            function_id: "fn".to_string(),
            config: json!({}),
            metadata: None,
        }
    }

    fn make_register_trigger_type(id: &str) -> Message {
        Message::RegisterTriggerType {
            id: id.to_string(),
            description: "tt".to_string(),
            trigger_request_format: None,
            call_request_format: None,
        }
    }

    fn make_invoke(function_id: &str) -> Message {
        Message::InvokeFunction {
            invocation_id: None,
            function_id: function_id.to_string(),
            data: json!({}),
            traceparent: None,
            baggage: None,
            action: None,
            metadata: None,
            namespace: None,
        }
    }

    #[test]
    fn registration_key_returns_typed_keys_for_register_messages() {
        assert_eq!(
            IIIClient::registration_key(&make_register_function("greet")),
            Some("function:greet".to_string())
        );
        assert_eq!(
            IIIClient::registration_key(&make_register_trigger("t1")),
            Some("trigger:t1".to_string())
        );
        assert_eq!(
            IIIClient::registration_key(&make_register_trigger_type("tt1")),
            Some("trigger_type:tt1".to_string())
        );
    }

    #[test]
    fn registration_key_returns_none_for_non_register_messages() {
        assert_eq!(IIIClient::registration_key(&make_invoke("f")), None);
        assert_eq!(IIIClient::registration_key(&Message::Ping), None);
        assert_eq!(IIIClient::registration_key(&Message::Pong), None);
        assert_eq!(
            IIIClient::registration_key(&Message::WorkerRegistered {
                worker_id: "w".to_string(),
                reattach_token: None
            }),
            None
        );
    }

    #[tokio::test]
    async fn drain_pre_connect_duplicates_drops_only_known_register_ids() {
        let (tx, mut rx) = mpsc::unbounded_channel::<Outbound>();

        tx.send(Outbound::Message(make_register_function("dup-fn")))
            .unwrap();
        tx.send(Outbound::Message(make_invoke("some::fn"))).unwrap();
        tx.send(Outbound::Message(make_register_function("new-fn")))
            .unwrap();
        tx.send(Outbound::Message(Message::Pong)).unwrap();
        tx.send(Outbound::Message(make_register_trigger("dup-trig")))
            .unwrap();
        tx.send(Outbound::Message(make_register_trigger("new-trig")))
            .unwrap();

        let snapshot_ids: HashSet<String> = [
            "function:dup-fn".to_string(),
            "trigger:dup-trig".to_string(),
        ]
        .into_iter()
        .collect();

        let mut queue: Vec<Message> = Vec::new();
        let shutdown = IIIClient::drain_pre_connect_duplicates(&mut rx, &mut queue, &snapshot_ids);

        assert!(!shutdown);
        let kept_keys: Vec<Option<String>> =
            queue.iter().map(IIIClient::registration_key).collect();
        assert_eq!(
            kept_keys,
            vec![
                None,
                Some("function:new-fn".to_string()),
                None,
                Some("trigger:new-trig".to_string()),
            ],
            "kept queue mismatch: {queue:#?}"
        );
    }

    #[tokio::test]
    async fn drain_pre_connect_duplicates_signals_shutdown() {
        let (tx, mut rx) = mpsc::unbounded_channel::<Outbound>();

        tx.send(Outbound::Message(make_register_function("a")))
            .unwrap();
        tx.send(Outbound::Shutdown).unwrap();
        tx.send(Outbound::Message(make_register_function("b")))
            .unwrap();

        let snapshot_ids: HashSet<String> = ["function:a".to_string()].into_iter().collect();
        let mut queue: Vec<Message> = Vec::new();
        let shutdown = IIIClient::drain_pre_connect_duplicates(&mut rx, &mut queue, &snapshot_ids);

        assert!(shutdown, "expected shutdown signal to be reported");
        assert!(
            queue.is_empty(),
            "queue must be empty when shutdown short-circuits the drain: {queue:#?}"
        );
    }

    #[tokio::test]
    async fn drain_pre_connect_duplicates_returns_false_on_empty_channel() {
        let (_tx, mut rx) = mpsc::unbounded_channel::<Outbound>();
        let snapshot_ids: HashSet<String> = HashSet::new();
        let mut queue: Vec<Message> = Vec::new();

        let shutdown = IIIClient::drain_pre_connect_duplicates(&mut rx, &mut queue, &snapshot_ids);

        assert!(!shutdown);
        assert!(queue.is_empty());
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn trigger_registration_result_error_is_logged() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let payload = serde_json::json!({
            "type": "triggerregistrationresult",
            "id": "trig-1",
            "trigger_type": "http",
            "function_id": "fn-1",
            "error": {
                "code": "trigger_type_not_found",
                "message": "Trigger type \"http\" not found — worker iii-http is missing. Run: iii worker add iii-http",
            },
        })
        .to_string();

        iii.handle_message(&payload).unwrap();

        assert!(logs_contain("iii worker add iii-http"));
        assert!(logs_contain("trig-1"));
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn trigger_registration_result_success_does_not_log_error() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let payload = serde_json::json!({
            "type": "triggerregistrationresult",
            "id": "trig-2",
            "trigger_type": "http",
            "function_id": "fn-2",
        })
        .to_string();

        iii.handle_message(&payload).unwrap();

        assert!(!logs_contain("Trigger registration failed"));
    }

    // Single test covers every namespace-resolution branch so the env var
    // mutation is serialized within one function (env vars are process-global
    // and cargo runs tests in parallel).
    #[test]
    fn namespace_resolution_reads_env_and_prefers_explicit_option() {
        let previous = std::env::var("III_NAMESPACE").ok();

        // SAFETY: env mutations are serialized within this test and restored at the end.
        unsafe {
            std::env::remove_var("III_NAMESPACE");
        }
        // Absent everywhere -> None (engine applies its default namespace).
        assert!(WorkerMetadata::default().namespace.is_none());
        assert!(resolve_namespace(None).is_none());
        // Explicit option still wins with no env set.
        assert_eq!(
            resolve_namespace(Some("payments".into())).as_deref(),
            Some("payments")
        );

        unsafe {
            std::env::set_var("III_NAMESPACE", "orders");
        }
        // III_NAMESPACE flows into worker metadata, mirroring III_WORKER_NAME.
        assert_eq!(
            WorkerMetadata::default().namespace.as_deref(),
            Some("orders")
        );
        // Env is the fallback when no explicit option is given.
        assert_eq!(resolve_namespace(None).as_deref(), Some("orders"));
        // options.namespace beats the env var.
        assert_eq!(
            resolve_namespace(Some("payments".into())).as_deref(),
            Some("payments")
        );

        unsafe {
            match previous {
                Some(val) => std::env::set_var("III_NAMESPACE", val),
                None => std::env::remove_var("III_NAMESPACE"),
            }
        }
    }

    #[tokio::test]
    async fn trigger_request_namespace_is_sent_by_trigger() {
        let iii = IIIClient::new("ws://localhost:1234");
        iii.inner
            .running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let _ = iii
            .trigger(
                TriggerRequest {
                    function_id: "svc::work".to_string(),
                    payload: json!({ "x": 1 }),
                    action: Some(TriggerAction::Void),
                    timeout_ms: None,
                }
                .namespace("orders"),
            )
            .await
            .expect("void trigger should enqueue");

        let mut rx = iii.inner.receiver.lock().unwrap().take().expect("receiver");
        let sent = rx.try_recv().expect("sent invoke");
        match sent {
            Outbound::Message(msg @ Message::InvokeFunction { .. }) => {
                let Message::InvokeFunction { namespace, .. } = &msg else {
                    unreachable!()
                };
                assert_eq!(namespace.as_deref(), Some("orders"));
                // And it actually reaches the wire.
                let wire = serde_json::to_string(&msg).unwrap();
                assert!(wire.contains(r#""namespace":"orders""#), "wire: {wire}");
            }
            _ => panic!("expected InvokeFunction"),
        }
    }

    #[tokio::test]
    async fn trigger_request_without_namespace_omits_it_on_the_wire() {
        let iii = IIIClient::new("ws://localhost:1234");
        iii.inner
            .running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let _ = iii
            .trigger(TriggerRequest {
                function_id: "svc::work".to_string(),
                payload: json!({ "x": 1 }),
                action: Some(TriggerAction::Void),
                timeout_ms: None,
            })
            .await
            .expect("void trigger should enqueue");

        let mut rx = iii.inner.receiver.lock().unwrap().take().expect("receiver");
        let sent = rx.try_recv().expect("sent invoke");
        match sent {
            Outbound::Message(msg @ Message::InvokeFunction { .. }) => {
                let Message::InvokeFunction { namespace, .. } = &msg else {
                    unreachable!()
                };
                assert!(namespace.is_none());
                let wire = serde_json::to_string(&msg).unwrap();
                assert!(!wire.contains("namespace"), "wire: {wire}");
            }
            _ => panic!("expected InvokeFunction"),
        }
    }

    #[test]
    fn worker_name_conflict_is_fatal_and_stops_worker() {
        let iii = IIIClient::new("ws://localhost:1234");
        iii.inner
            .running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let payload = json!({
            "type": "registrationrejected",
            "code": "WORKER_NAMESPACE_CONFLICT",
            "namespace": "orders",
            "worker_name": "checkout",
            "owner_worker_id": "worker-abc",
        })
        .to_string();

        iii.handle_message(&payload).unwrap();

        // Fatal: the connection loop must not reconnect.
        assert!(
            !iii.inner.running.load(std::sync::atomic::Ordering::SeqCst),
            "worker must stop; running flag should be cleared"
        );
        assert_eq!(iii.get_connection_state(), IIIConnectionState::Failed);

        let err = iii.fatal_error().expect("fatal error must surface");
        match err {
            Error::RegistrationRejected {
                code,
                namespace,
                worker_name,
                owner_worker_id,
            } => {
                assert_eq!(code, "WORKER_NAMESPACE_CONFLICT");
                assert_eq!(namespace, "orders");
                assert_eq!(worker_name, "checkout");
                assert_eq!(owner_worker_id, "worker-abc");
            }
            other => panic!("expected RegistrationRejected, got {other:?}"),
        }
    }

    #[test]
    fn function_conflict_is_not_fatal_and_keeps_serving() {
        let iii = IIIClient::new("ws://localhost:1234");
        iii.inner
            .running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        // Simulate a live, connected worker.
        iii.set_connection_state(IIIConnectionState::Connected);

        // The engine refused a single duplicate function id but deliberately
        // kept the connection open. `worker_name` carries the function id.
        let payload = json!({
            "type": "registrationrejected",
            "code": "FUNCTION_NAMESPACE_CONFLICT",
            "namespace": "orders",
            "worker_name": "orders::charge",
            "owner_worker_id": "worker-abc",
        })
        .to_string();

        iii.handle_message(&payload).unwrap();

        // Non-fatal: the worker keeps running, stays connected, and no fatal
        // error is recorded — its other functions are still served.
        assert!(
            iii.inner.running.load(std::sync::atomic::Ordering::SeqCst),
            "worker must keep running after a function-id conflict"
        );
        assert_eq!(iii.get_connection_state(), IIIConnectionState::Connected);
        assert!(
            iii.fatal_error().is_none(),
            "a function-id conflict must not be fatal"
        );
    }

    #[test]
    fn function_info_captures_namespace_and_tolerates_absence() {
        // Engine listings now carry `namespace`; the typed struct exposes it.
        let with_ns: FunctionInfo = serde_json::from_value(json!({
            "function_id": "svc::work",
            "description": null,
            "request_format": null,
            "response_format": null,
            "metadata": null,
            "namespace": "orders",
        }))
        .unwrap();
        assert_eq!(with_ns.namespace.as_deref(), Some("orders"));

        // Legacy payloads without the field still deserialize.
        let without_ns: FunctionInfo = serde_json::from_value(json!({
            "function_id": "svc::work",
            "description": null,
            "request_format": null,
            "response_format": null,
            "metadata": null,
        }))
        .unwrap();
        assert!(without_ns.namespace.is_none());
    }
}
