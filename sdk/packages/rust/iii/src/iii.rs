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
    time::sleep,
};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
use uuid::Uuid;

const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

use crate::{
    channels::{ChannelReader, ChannelWriter, StreamChannelRef},
    error::IIIError,
    protocol::{
        ErrorBody, HttpInvocationConfig, Message, RegisterFunctionMessage, RegisterServiceMessage,
        RegisterTriggerInput, RegisterTriggerMessage, RegisterTriggerTypeMessage, TriggerAction,
        TriggerRequest, UnregisterTriggerMessage, UnregisterTriggerTypeMessage,
    },
    triggers::{Trigger, TriggerConfig, TriggerHandler},
    types::{Channel, RemoteFunctionData, RemoteFunctionHandler, RemoteTriggerTypeData},
};

use crate::telemetry;
use crate::telemetry::types::OtelConfig;

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
}

/// Function information returned by `engine::functions::list`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInfo {
    pub function_id: String,
    pub description: Option<String>,
    pub request_format: Option<Value>,
    pub response_format: Option<Value>,
    pub metadata: Option<Value>,
}

/// Trigger information returned by `engine::triggers::list`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerInfo {
    pub id: String,
    pub trigger_type: String,
    pub function_id: String,
    pub config: Value,
    pub metadata: Option<Value>,
}

/// Trigger type information returned by `engine::trigger-types::list`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerTypeInfo {
    pub id: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_request_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_request_format: Option<Value>,
}

/// Builder for registering a custom trigger type with optional format schemas.
///
/// Type parameters:
/// - `C` tracks the trigger registration type (set via `.trigger_request_format::<T>()`)
/// - `R` tracks the call request type (set via `.call_request_format::<T>()`)
///
/// Both default to `Value` (untyped) and change when the respective builder
/// method is called. This allows [`III::register_trigger_type`] to return a
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

/// Typed handle returned by [`III::register_trigger_type`].
///
/// Type parameters:
/// - `C` — trigger registration type for [`register_trigger`](Self::register_trigger)
/// - `R` — call request type for [`register_function`](Self::register_function)
#[derive(Clone)]
pub struct TriggerTypeRef<C = Value, R = Value> {
    iii: III,
    trigger_type_id: String,
    _phantom: std::marker::PhantomData<(C, R)>,
}

impl<C: Serialize, R> TriggerTypeRef<C, R> {
    /// Register a trigger with compile-time validated trigger config.
    pub fn register_trigger(
        &self,
        function_id: impl Into<String>,
        config: C,
    ) -> Result<Trigger, IIIError> {
        self.register_trigger_with_metadata(function_id, config, None)
    }

    /// Register a trigger with compile-time validated trigger config and optional metadata.
    pub fn register_trigger_with_metadata(
        &self,
        function_id: impl Into<String>,
        config: C,
        metadata: Option<Value>,
    ) -> Result<Trigger, IIIError> {
        self.iii.register_trigger(RegisterTriggerInput {
            trigger_type: self.trigger_type_id.clone(),
            function_id: function_id.into(),
            config: serde_json::to_value(config).map_err(|e| IIIError::Handler(e.to_string()))?,
            metadata,
        })
    }
}

impl<C, R> TriggerTypeRef<C, R>
where
    R: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static,
{
    /// Register a sync function whose input type must match
    /// the call request format `R`.
    pub fn register_function<O, E, F>(&self, id: impl Into<String>, f: F) -> FunctionRef
    where
        O: Serialize + schemars::JsonSchema + Send + 'static,
        E: std::fmt::Display + Send + 'static,
        F: Fn(R) -> Result<O, E> + Send + Sync + 'static,
    {
        self.iii.register_function(RegisterFunction::new(id, f))
    }

    /// Register an async function whose input type must match
    /// the call request format `R`.
    pub fn register_function_async<O, E, F, Fut>(&self, id: impl Into<String>, f: F) -> FunctionRef
    where
        O: Serialize + schemars::JsonSchema + Send + 'static,
        E: std::fmt::Display + Send + 'static,
        F: Fn(R) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<O, E>> + Send + 'static,
    {
        self.iii
            .register_function(RegisterFunction::new_async(id, f))
    }
}

/// Telemetry metadata provided by the SDK to the engine.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkerTelemetryMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub framework: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<WorkerTelemetryMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolation: Option<String>,
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
            name: format!("{}:{}", hostname, pid),
            os: os_info,
            pid: Some(pid),
            telemetry: Some(WorkerTelemetryMeta {
                language,
                project_name,
                ..Default::default()
            }),
            isolation: std::env::var("III_ISOLATION")
                .ok()
                .filter(|s| !s.is_empty()),
        }
    }
}

/// Returns a project identifier for telemetry, derived from the current
/// working directory. Reads `[package] name` from `Cargo.toml` if present at
/// `cwd`; otherwise falls back to the basename of `cwd`. Returns `None`
/// only when both signals are unavailable.
///
/// No directory walking — only inspects `cwd` itself, so the SDK never
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

type PendingInvocation = oneshot::Sender<Result<Value, IIIError>>;

// WebSocket transmitter type alias
type WsTx = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    WsMessage,
>;

/// Inject trace context headers for outbound messages.
fn inject_trace_headers() -> (Option<String>, Option<String>) {
    use crate::telemetry::context;
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

pub trait IntoFunctionHandler {
    fn into_parts(self, message: &mut RegisterFunctionMessage) -> Option<RemoteFunctionHandler>;
}

/// Trait for types that can be passed to [`III::register_function`].
///
/// Implemented for:
/// - [`RegisterFunction`] — the builder API (recommended)
/// - `(RegisterFunctionMessage, H)` — the legacy tuple API
pub trait IntoFunctionRegistration {
    fn into_registration(self) -> (RegisterFunctionMessage, Option<RemoteFunctionHandler>);
}

impl IntoFunctionRegistration for RegisterFunction {
    fn into_registration(self) -> (RegisterFunctionMessage, Option<RemoteFunctionHandler>) {
        (self.message, Some(self.handler))
    }
}

impl<H: IntoFunctionHandler> IntoFunctionRegistration for (RegisterFunctionMessage, H) {
    fn into_registration(self) -> (RegisterFunctionMessage, Option<RemoteFunctionHandler>) {
        let (mut message, handler) = self;
        let handler = handler.into_parts(&mut message);
        (message, handler)
    }
}

impl IntoFunctionHandler for HttpInvocationConfig {
    fn into_parts(self, message: &mut RegisterFunctionMessage) -> Option<RemoteFunctionHandler> {
        message.invocation = Some(self);
        None
    }
}

impl<F, Fut> IntoFunctionHandler for F
where
    F: Fn(Value) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Value, IIIError>> + Send + 'static,
{
    fn into_parts(self, _message: &mut RegisterFunctionMessage) -> Option<RemoteFunctionHandler> {
        Some(Arc::new(move |input: Value| Box::pin(self(input))))
    }
}

// =============================================================================
// iii_fn — sync function wrapper
// =============================================================================

/// Wrapper for registering sync functions as III handlers via [`iii_fn`].
///
/// Created by [`iii_fn`]. Stores a pre-erased handler so that a single
/// [`IntoFunctionHandler`] impl covers all supported arities.
pub struct IIIFn<F = ()> {
    handler: RemoteFunctionHandler,
    request_format: Option<Value>,
    response_format: Option<Value>,
    _marker: std::marker::PhantomData<F>,
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
/// [`RemoteFunctionHandler`].
#[doc(hidden)]
pub trait IntoSyncHandler<Marker>: Send + Sync + 'static {
    fn into_handler(self) -> RemoteFunctionHandler;
    fn request_format() -> Option<Value> {
        None
    }
    fn response_format() -> Option<Value> {
        None
    }
}

// 1-arg sync — deserializes the entire JSON input as T
impl<F, T, R, E> IntoSyncHandler<(T, R, E)> for F
where
    F: Fn(T) -> Result<R, E> + Send + Sync + 'static,
    T: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static,
    R: serde::Serialize + schemars::JsonSchema + Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    fn into_handler(self) -> RemoteFunctionHandler {
        Arc::new(move |input: Value| {
            let output = serde_json::from_value::<T>(input)
                .map_err(|e| IIIError::Handler(e.to_string()))
                .and_then(|arg| (self)(arg).map_err(|e| IIIError::Handler(e.to_string())))
                .and_then(|val| {
                    serde_json::to_value(&val).map_err(|e| IIIError::Handler(e.to_string()))
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

/// Wraps a **sync** function into an III-compatible handler.
///
/// The function must take a single argument implementing
/// [`serde::de::DeserializeOwned`] and return `Result<R, E>`
/// where `R: Serialize` and `E: Display`.
///
/// The entire JSON input is deserialized as the argument type.
/// Use a `#[derive(Deserialize)]` struct for named JSON keys.
///
/// For async functions, use [`iii_async_fn`] instead.
pub fn iii_fn<F, M>(f: F) -> IIIFn<F>
where
    F: IntoSyncHandler<M>,
{
    IIIFn {
        request_format: F::request_format(),
        response_format: F::response_format(),
        handler: f.into_handler(),
        _marker: std::marker::PhantomData,
    }
}

impl<F> IntoFunctionHandler for IIIFn<F> {
    fn into_parts(self, message: &mut RegisterFunctionMessage) -> Option<RemoteFunctionHandler> {
        if message.request_format.is_none() {
            message.request_format = self.request_format;
        }
        if message.response_format.is_none() {
            message.response_format = self.response_format;
        }
        Some(self.handler)
    }
}

// =============================================================================
// iii_async_fn — async function wrapper
// =============================================================================

/// Wrapper for registering async functions as III handlers via [`iii_async_fn`].
///
/// Created by [`iii_async_fn`]. Stores a pre-erased handler so that a single
/// [`IntoFunctionHandler`] impl covers all supported arities.
pub struct IIIAsyncFn<F = ()> {
    handler: RemoteFunctionHandler,
    request_format: Option<Value>,
    response_format: Option<Value>,
    _marker: std::marker::PhantomData<F>,
}

/// Helper trait used internally to convert an async function into a
/// [`RemoteFunctionHandler`].
#[doc(hidden)]
pub trait IntoAsyncHandler<Marker>: Send + Sync + 'static {
    fn into_handler(self) -> RemoteFunctionHandler;
    fn request_format() -> Option<Value> {
        None
    }
    fn response_format() -> Option<Value> {
        None
    }
}

// 1-arg async — deserializes the entire JSON input as T
impl<F, T, Fut, R, E> IntoAsyncHandler<(T, Fut, R, E)> for F
where
    F: Fn(T) -> Fut + Send + Sync + 'static,
    T: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static,
    Fut: std::future::Future<Output = Result<R, E>> + Send + 'static,
    R: serde::Serialize + schemars::JsonSchema + Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    fn into_handler(self) -> RemoteFunctionHandler {
        Arc::new(
            move |input: Value| -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<Value, IIIError>> + Send>,
            > {
                match serde_json::from_value::<T>(input) {
                    Ok(arg) => {
                        let fut = (self)(arg);
                        Box::pin(async move {
                            fut.await
                                .map_err(|e| IIIError::Handler(e.to_string()))
                                .and_then(|val| {
                                    serde_json::to_value(&val)
                                        .map_err(|e| IIIError::Handler(e.to_string()))
                                })
                        })
                    }
                    Err(e) => Box::pin(async move { Err(IIIError::Handler(e.to_string())) }),
                }
            },
        )
    }

    fn request_format() -> Option<Value> {
        json_schema_for::<T>()
    }

    fn response_format() -> Option<Value> {
        json_schema_for::<R>()
    }
}

/// Wraps an **async** function into an III-compatible handler.
///
/// The function must take a single argument implementing
/// [`serde::de::DeserializeOwned`] and return
/// `impl Future<Output = Result<R, E>>` where `R: Serialize` and `E: Display`.
pub fn iii_async_fn<F, M>(f: F) -> IIIAsyncFn<F>
where
    F: IntoAsyncHandler<M>,
{
    IIIAsyncFn {
        request_format: F::request_format(),
        response_format: F::response_format(),
        handler: f.into_handler(),
        _marker: std::marker::PhantomData,
    }
}

impl<F> IntoFunctionHandler for IIIAsyncFn<F> {
    fn into_parts(self, message: &mut RegisterFunctionMessage) -> Option<RemoteFunctionHandler> {
        if message.request_format.is_none() {
            message.request_format = self.request_format;
        }
        if message.response_format.is_none() {
            message.response_format = self.response_format;
        }
        Some(self.handler)
    }
}

// =============================================================================
// RegisterFunction — one-step registration builder
// =============================================================================

/// One-step function registration combining ID, handler, and auto-generated schemas.
///
/// Use [`RegisterFunction::new`] for sync functions or [`RegisterFunction::new_async`]
/// for async functions, then register with [`III::register`].
pub struct RegisterFunction {
    message: RegisterFunctionMessage,
    handler: RemoteFunctionHandler,
}

impl RegisterFunction {
    /// Create a registration for a **sync** function.
    pub fn new<F, M>(id: impl Into<String>, f: F) -> Self
    where
        F: IntoSyncHandler<M>,
    {
        Self {
            message: RegisterFunctionMessage {
                id: id.into(),
                description: None,
                request_format: F::request_format(),
                response_format: F::response_format(),
                metadata: None,
                invocation: None,
            },
            handler: f.into_handler(),
        }
    }

    /// Create a registration for an **async** function.
    pub fn new_async<F, M>(id: impl Into<String>, f: F) -> Self
    where
        F: IntoAsyncHandler<M>,
    {
        Self {
            message: RegisterFunctionMessage {
                id: id.into(),
                description: None,
                request_format: F::request_format(),
                response_format: F::response_format(),
                metadata: None,
                invocation: None,
            },
            handler: f.into_handler(),
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

    /// Get the auto-generated request format.
    pub fn request_format(&self) -> Option<&Value> {
        self.message.request_format.as_ref()
    }

    /// Get the auto-generated response format.
    pub fn response_format(&self) -> Option<&Value> {
        self.message.response_format.as_ref()
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
    services: Mutex<HashMap<String, RegisterServiceMessage>>,
    worker_metadata: Mutex<Option<WorkerMetadata>>,
    connection_state: Mutex<IIIConnectionState>,
    connection_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    headers: Mutex<Option<HashMap<String, String>>>,
    otel_config: Mutex<Option<OtelConfig>>,
}

/// WebSocket client for communication with the III Engine.
///
/// Create with [`register_worker`](crate::register_worker).
#[derive(Clone)]
pub struct III {
    inner: Arc<IIIInner>,
}

impl III {
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
            services: Mutex::new(HashMap::new()),
            worker_metadata: Mutex::new(Some(metadata)),
            connection_state: Mutex::new(IIIConnectionState::Disconnected),
            connection_thread: Mutex::new(None),
            headers: Mutex::new(None),
            otel_config: Mutex::new(None),
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
    /// the background connection thread. Telemetry is flushed inside the
    /// connection thread before it exits.
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
    /// Unlike [`shutdown`](Self::shutdown), this method does **not** block
    /// to wait for `run_connection()` to finish, making it safe to call from
    /// an async context without stalling the executor.
    /// `telemetry::shutdown_otel()` still runs inside the connection thread
    /// after `run_connection()` returns, so it may not complete unless
    /// [`shutdown`](Self::shutdown) is used to join the thread.
    pub async fn shutdown_async(&self) {
        self.inner.running.store(false, Ordering::SeqCst);
        let _ = self.inner.outbound.send(Outbound::Shutdown);
        self.set_connection_state(IIIConnectionState::Disconnected);
    }

    fn register_function_inner(
        &self,
        message: RegisterFunctionMessage,
        handler: Option<RemoteFunctionHandler>,
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
    /// Pass a closure/async fn for local execution, or an [`HttpInvocationConfig`]
    /// for HTTP-invoked functions (Lambda, Cloudflare Workers, etc.).
    ///
    /// # Arguments
    /// * `message` - Function registration message with id and optional metadata.
    /// * `handler` - Async handler or HTTP invocation config.
    ///
    /// # Panics
    /// Panics if `id` is empty or already registered.
    ///
    /// # Examples
    /// ```rust,no_run
    /// use iii_sdk::{register_worker, InitOptions, RegisterFunction};
    /// use serde::Deserialize;
    /// use schemars::JsonSchema;
    ///
    /// #[derive(Deserialize, JsonSchema)]
    /// struct Input { name: String }
    /// fn greet(input: Input) -> Result<String, String> {
    ///     Ok(format!("Hello, {}!", input.name))
    /// }
    ///
    /// let iii = register_worker("ws://localhost:49134", InitOptions::default());
    /// iii.register_function(RegisterFunction::new("greet", greet));
    /// ```
    ///
    /// Also accepts a two-argument form via [`register_function_with`](III::register_function_with):
    /// ```rust,no_run
    /// # use iii_sdk::{register_worker, InitOptions, RegisterFunctionMessage};
    /// # use serde_json::{json, Value};
    /// # let iii = register_worker("ws://localhost:49134", InitOptions::default());
    /// iii.register_function_with(
    ///     RegisterFunctionMessage::with_id("echo".to_string()),
    ///     |input: Value| async move { Ok(json!({"echo": input})) },
    /// );
    /// ```
    pub fn register_function<R: IntoFunctionRegistration>(&self, registration: R) -> FunctionRef {
        let (message, handler) = registration.into_registration();
        self.register_function_inner(message, handler)
    }

    /// Register a function with a message and handler directly.
    pub fn register_function_with<H: IntoFunctionHandler>(
        &self,
        mut message: RegisterFunctionMessage,
        handler: H,
    ) -> FunctionRef {
        let handler = handler.into_parts(&mut message);
        self.register_function_inner(message, handler)
    }

    /// Register a service with the engine.
    ///
    /// # Arguments
    /// * `message` - Service registration message with id, name, and optional metadata.
    pub fn register_service(&self, message: RegisterServiceMessage) {
        self.inner
            .services
            .lock_or_recover()
            .insert(message.id.clone(), message.clone());
        let _ = self.send_message(message.to_message());
    }

    /// Register a custom trigger type with the engine.
    ///
    /// Returns a [`TriggerTypeRef`] handle that can register triggers and
    /// functions with compile-time validated types.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use iii_sdk::{III, RegisterTriggerType};
    /// # struct MyHandler;
    /// # #[async_trait::async_trait]
    /// # impl iii_sdk::TriggerHandler for MyHandler {
    /// #     async fn register_trigger(&self, _: iii_sdk::TriggerConfig) -> Result<(), iii_sdk::IIIError> { Ok(()) }
    /// #     async fn unregister_trigger(&self, _: iii_sdk::TriggerConfig) -> Result<(), iii_sdk::IIIError> { Ok(()) }
    /// # }
    /// # #[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)] struct MyConfig { url: String }
    /// # #[derive(serde::Deserialize, schemars::JsonSchema)] struct MyRequest { data: String }
    /// # let iii = III::new("ws://localhost:49134");
    /// let my_trigger = iii.register_trigger_type(
    ///     RegisterTriggerType::new("my-trigger", "My custom trigger", MyHandler)
    ///         .trigger_request_format::<MyConfig>()
    ///         .call_request_format::<MyRequest>(),
    /// );
    ///
    /// // Compile-time safe: config must be MyConfig, function input must be MyRequest
    /// my_trigger.register_function("my::handler", |req: MyRequest| -> Result<serde_json::Value, String> {
    ///     Ok(serde_json::json!({ "data": req.data }))
    /// });
    /// my_trigger.register_trigger("my::handler", MyConfig { url: "/hook".into() });
    /// ```
    pub fn register_trigger_type<H, C, R>(
        &self,
        registration: RegisterTriggerType<H, C, R>,
    ) -> TriggerTypeRef<C, R>
    where
        H: TriggerHandler + 'static,
    {
        let message = RegisterTriggerTypeMessage {
            id: registration.id,
            description: registration.description,
            trigger_request_format: registration.trigger_request_format,
            call_request_format: registration.call_request_format,
        };

        let trigger_type_id = message.id.clone();

        self.inner.trigger_types.lock_or_recover().insert(
            message.id.clone(),
            RemoteTriggerTypeData {
                message: message.clone(),
                handler: Arc::new(registration.handler),
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
    pub fn unregister_trigger_type(&self, id: impl Into<String>) {
        let id = id.into();
        self.inner.trigger_types.lock_or_recover().remove(&id);
        let msg = UnregisterTriggerTypeMessage { id };
        let _ = self.send_message(msg.to_message());
    }

    /// Bind a trigger configuration to a registered function.
    ///
    /// # Arguments
    /// * `input` - Trigger registration input with trigger_type, function_id, and config.
    ///
    /// # Examples
    /// ```rust
    /// # use iii_sdk::{III, RegisterTriggerInput};
    /// # use serde_json::json;
    /// # let iii = III::new("ws://localhost:49134");
    /// let trigger = iii.register_trigger(RegisterTriggerInput {
    ///     trigger_type: "http".to_string(),
    ///     function_id: "greet".to_string(),
    ///     config: json!({ "api_path": "/greet", "http_method": "GET" }),
    ///     metadata: None,
    /// })?;
    /// // Later...
    /// trigger.unregister();
    /// # Ok::<(), iii_sdk::IIIError>(())
    /// ```
    pub fn register_trigger(&self, input: RegisterTriggerInput) -> Result<Trigger, IIIError> {
        let id = Uuid::new_v4().to_string();
        let message = RegisterTriggerMessage {
            id: id.clone(),
            trigger_type: input.trigger_type,
            function_id: input.function_id,
            config: input.config,
            metadata: input.metadata,
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
    ///
    /// The routing behavior depends on the `action` field of the request:
    /// - No action: synchronous -- waits for the function to return.
    /// - [`TriggerAction::Enqueue`] - async via named queue.
    /// - [`TriggerAction::Void`] — fire-and-forget.
    ///
    /// # Examples
    /// ```rust
    /// # use iii_sdk::{III, TriggerRequest, TriggerAction};
    /// # use serde_json::json;
    /// # async fn example(iii: &III) -> Result<(), iii_sdk::IIIError> {
    /// // Synchronous
    /// let result = iii.trigger(TriggerRequest {
    ///     function_id: "greet".to_string(),
    ///     payload: json!({"name": "World"}),
    ///     action: None,
    ///     timeout_ms: None,
    /// }).await?;
    ///
    /// // Fire-and-forget
    /// iii.trigger(TriggerRequest {
    ///     function_id: "notify".to_string(),
    ///     payload: json!({}),
    ///     action: Some(TriggerAction::Void),
    ///     timeout_ms: None,
    /// }).await?;
    ///
    /// // Enqueue
    /// let receipt = iii.trigger(TriggerRequest {
    ///     function_id: "iii::durable::publish".to_string(),
    ///     payload: json!({"topic": "test"}),
    ///     action: Some(TriggerAction::Enqueue { queue: "test".to_string() }),
    ///     timeout_ms: None,
    /// }).await?;
    ///
    /// # Ok(())
    /// # }
    /// ```
    pub async fn trigger(
        &self,
        request: impl Into<crate::protocol::TriggerRequest>,
    ) -> Result<Value, IIIError> {
        let req = request.into();
        let (tp, bg) = inject_trace_headers();

        // Void is fire-and-forget — no invocation_id, no response
        if matches!(req.action, Some(TriggerAction::Void)) {
            self.send_message(Message::InvokeFunction {
                invocation_id: None,
                function_id: req.function_id,
                data: req.payload,
                traceparent: tp,
                baggage: bg,
                action: req.action,
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
        })?;

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(IIIError::NotConnected),
            Err(_) => {
                self.inner.pending.lock_or_recover().remove(&invocation_id);
                Err(IIIError::Timeout)
            }
        }
    }

    /// Get the current connection state.
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

    /// Create a streaming channel pair for worker-to-worker data transfer.
    ///
    /// Returns a `Channel` with writer, reader, and their serializable refs
    /// that can be passed as fields in invocation data to other functions.
    pub async fn create_channel(&self, buffer_size: Option<usize>) -> Result<Channel, IIIError> {
        let result = self
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
                .ok_or_else(|| IIIError::Serde("missing 'writer' in channel response".into()))?,
        )
        .map_err(|e| IIIError::Serde(e.to_string()))?;

        let reader_ref: StreamChannelRef = serde_json::from_value(
            result
                .get("reader")
                .cloned()
                .ok_or_else(|| IIIError::Serde("missing 'reader' in channel response".into()))?,
        )
        .map_err(|e| IIIError::Serde(e.to_string()))?;

        Ok(Channel {
            writer: ChannelWriter::new(&self.inner.address, &writer_ref),
            reader: ChannelReader::new(&self.inner.address, &reader_ref),
            writer_ref,
            reader_ref,
        })
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
                });
            }
        }
    }

    fn send_message(&self, message: Message) -> Result<(), IIIError> {
        if !self.inner.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        self.inner
            .outbound
            .send(Outbound::Message(message))
            .map_err(|_| IIIError::NotConnected)
    }

    async fn run_connection(&self, mut rx: mpsc::UnboundedReceiver<Outbound>) {
        let mut queue: Vec<Message> = Vec::new();
        let mut has_connected_before = false;

        while self.inner.running.load(Ordering::SeqCst) {
            self.set_connection_state(if has_connected_before {
                IIIConnectionState::Reconnecting
            } else {
                IIIConnectionState::Connecting
            });

            let custom_headers = self.inner.headers.lock_or_recover().clone();

            let connect_result = if let Some(ref h) = custom_headers {
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
            };

            match connect_result {
                Ok((stream, _)) => {
                    tracing::info!(address = %self.inner.address, "iii connected");
                    has_connected_before = true;
                    self.set_connection_state(IIIConnectionState::Connected);
                    let (mut ws_tx, mut ws_rx) = stream.split();

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
                        sleep(Duration::from_secs(2)).await;
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
                            sleep(Duration::from_secs(2)).await;
                            continue;
                        }
                    }

                    // Auto-register worker metadata on connect (like Node SDK)
                    self.register_worker_metadata();

                    let mut should_reconnect = false;

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
                        }
                    }
                }
                Err(err) => {
                    tracing::warn!(error = %err, "failed to connect; retrying");
                }
            }

            if self.inner.running.load(Ordering::SeqCst) {
                sleep(Duration::from_secs(2)).await;
            }
        }
    }

    fn collect_registrations(&self) -> Vec<Message> {
        let mut messages = Vec::new();

        for trigger_type in self.inner.trigger_types.lock_or_recover().values() {
            messages.push(trigger_type.message.to_message());
        }

        for service in self.inner.services.lock_or_recover().values() {
            messages.push(service.to_message());
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
            Message::RegisterService { id, .. } => Some(format!("service:{id}")),
            _ => None,
        }
    }

    /// Drain everything currently pending in the outbound `rx` channel,
    /// dropping register messages whose keys are already covered by
    /// `snapshot_ids` (already sent via `collect_registrations()`),
    /// and pushing every other message onto `queue` for re-flushing.
    ///
    /// Returns `true` if a `Shutdown` signal was observed during the
    /// drain — the caller should then stop the connection loop.
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

    async fn flush_queue(
        &self,
        ws_tx: &mut WsTx,
        queue: &mut Vec<Message>,
    ) -> Result<(), IIIError> {
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

    async fn send_ws(&self, ws_tx: &mut WsTx, message: &Message) -> Result<(), IIIError> {
        let payload = serde_json::to_string(message)?;
        ws_tx.send(WsMessage::Text(payload.into())).await?;
        Ok(())
    }

    fn handle_frame(&self, frame: WsMessage) -> Result<(), IIIError> {
        match frame {
            WsMessage::Text(text) => self.handle_message(&text),
            WsMessage::Binary(bytes) => {
                let text = String::from_utf8_lossy(&bytes).to_string();
                self.handle_message(&text)
            }
            _ => Ok(()),
        }
    }

    fn handle_message(&self, payload: &str) -> Result<(), IIIError> {
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
            } => {
                self.handle_invoke_function(invocation_id, function_id, data, traceparent, baggage);
            }
            Message::RegisterTrigger {
                id,
                trigger_type,
                function_id,
                config,
                metadata,
            } => {
                self.handle_register_trigger(id, trigger_type, function_id, config, metadata);
            }
            Message::Ping => {
                let _ = self.send_message(Message::Pong);
            }
            Message::WorkerRegistered { worker_id } => {
                tracing::debug!(worker_id = %worker_id, "Worker registered");
            }
            _ => {}
        }

        Ok(())
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
                Some(error) => Err(IIIError::Remote {
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
                use crate::telemetry::context::extract_context;
                use opentelemetry::trace::{SpanKind, TraceContextExt, Tracer};

                let parent_cx = extract_context(traceparent.as_deref(), baggage.as_deref());
                let tracer = opentelemetry::global::tracer("iii-rust-sdk");
                let span = tracer
                    .span_builder(format!("call {}", function_id))
                    .with_kind(SpanKind::Server)
                    .start_with_context(&tracer, &parent_cx);
                parent_cx.with_span(span)
            };

            let result = {
                use opentelemetry::trace::FutureExt as OtelFutureExt;
                handler(data).with_context(otel_cx.clone()).await
            };

            // Record span status based on result
            let mut error_stacktrace: Option<String> = None;
            {
                use opentelemetry::KeyValue;
                use opentelemetry::trace::{Status, TraceContextExt};
                let span = otel_cx.span();
                match &result {
                    Ok(_) => span.set_status(Status::Ok),
                    Err(err) => {
                        let (exc_type, exc_message, stacktrace) = match err {
                            IIIError::Remote {
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
                            IIIError::Remote {
                                code,
                                message,
                                stacktrace,
                            } => ErrorBody {
                                code,
                                message,
                                stacktrace: stacktrace.or(error_stacktrace).or_else(|| {
                                    Some(std::backtrace::Backtrace::force_capture().to_string())
                                }),
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
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::*;
    use crate::{
        InitOptions,
        protocol::{HttpInvocationConfig, HttpMethod, RegisterTriggerInput},
        register_worker,
    };

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
    async fn register_function_with_http_config_stores_and_unregister_removes() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let config = HttpInvocationConfig {
            url: "https://example.com/invoke".to_string(),
            method: HttpMethod::Post,
            timeout_ms: Some(30000),
            headers: HashMap::new(),
            auth: None,
        };

        let func_ref = iii.register_function_with(
            RegisterFunctionMessage {
                id: "external::my_lambda".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
                invocation: None,
            },
            config,
        );

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

        iii.register_function_with(
            RegisterFunctionMessage {
                id: "".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
                invocation: None,
            },
            config,
        );
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

        assert!(matches!(result, Err(IIIError::Timeout)));
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

    fn make_register_service(id: &str) -> Message {
        Message::RegisterService {
            id: id.to_string(),
            name: "svc".to_string(),
            description: None,
            parent_service_id: None,
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
        }
    }

    #[test]
    fn registration_key_returns_typed_keys_for_register_messages() {
        assert_eq!(
            III::registration_key(&make_register_function("greet")),
            Some("function:greet".to_string())
        );
        assert_eq!(
            III::registration_key(&make_register_trigger("t1")),
            Some("trigger:t1".to_string())
        );
        assert_eq!(
            III::registration_key(&make_register_trigger_type("tt1")),
            Some("trigger_type:tt1".to_string())
        );
        assert_eq!(
            III::registration_key(&make_register_service("svc1")),
            Some("service:svc1".to_string())
        );
    }

    #[test]
    fn registration_key_returns_none_for_non_register_messages() {
        assert_eq!(III::registration_key(&make_invoke("f")), None);
        assert_eq!(III::registration_key(&Message::Ping), None);
        assert_eq!(III::registration_key(&Message::Pong), None);
        assert_eq!(
            III::registration_key(&Message::WorkerRegistered {
                worker_id: "w".to_string()
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
        tx.send(Outbound::Message(make_register_service("dup-svc")))
            .unwrap();

        let snapshot_ids: HashSet<String> = [
            "function:dup-fn".to_string(),
            "trigger:dup-trig".to_string(),
            "service:dup-svc".to_string(),
        ]
        .into_iter()
        .collect();

        let mut queue: Vec<Message> = Vec::new();
        let shutdown = III::drain_pre_connect_duplicates(&mut rx, &mut queue, &snapshot_ids);

        assert!(!shutdown);
        let kept_keys: Vec<Option<String>> = queue.iter().map(III::registration_key).collect();
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
        let shutdown = III::drain_pre_connect_duplicates(&mut rx, &mut queue, &snapshot_ids);

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

        let shutdown = III::drain_pre_connect_duplicates(&mut rx, &mut queue, &snapshot_ids);

        assert!(!shutdown);
        assert!(queue.is_empty());
    }
}
