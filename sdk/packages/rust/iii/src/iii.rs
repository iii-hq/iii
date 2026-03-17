use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicUsize, Ordering},
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

#[cfg(feature = "otel")]
use crate::telemetry;
#[cfg(feature = "otel")]
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

        Self {
            runtime: "rust".to_string(),
            version: SDK_VERSION.to_string(),
            name: format!("{}:{}", hostname, pid),
            os: os_info,
            pid: Some(pid),
            telemetry: Some(WorkerTelemetryMeta {
                language,
                ..Default::default()
            }),
        }
    }
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
/// Returns (traceparent, baggage) - both None when otel feature is disabled.
#[cfg(feature = "otel")]
fn inject_trace_headers() -> (Option<String>, Option<String>) {
    use crate::telemetry::context;
    (context::inject_traceparent(), context::inject_baggage())
}

#[cfg(not(feature = "otel"))]
fn inject_trace_headers() -> (Option<String>, Option<String>) {
    (None, None)
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

/// Callback function type for functions available events
pub type FunctionsAvailableCallback = Arc<dyn Fn(Vec<FunctionInfo>) + Send + Sync>;

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
    functions_available_callbacks: Mutex<HashMap<usize, FunctionsAvailableCallback>>,
    functions_available_callback_counter: AtomicUsize,
    functions_available_function_id: Mutex<Option<String>>,
    functions_available_trigger: Mutex<Option<Trigger>>,
    #[cfg(feature = "otel")]
    otel_config: Mutex<Option<OtelConfig>>,
}

/// WebSocket client for communication with the III Engine.
///
/// Create with [`register_worker`](crate::register_worker).
#[derive(Clone)]
pub struct III {
    inner: Arc<IIIInner>,
}

/// Guard that unsubscribes from functions available events when dropped
pub struct FunctionsAvailableGuard {
    iii: III,
    callback_id: usize,
}

impl Drop for FunctionsAvailableGuard {
    fn drop(&mut self) {
        let mut callbacks = self
            .iii
            .inner
            .functions_available_callbacks
            .lock_or_recover();
        callbacks.remove(&self.callback_id);

        if callbacks.is_empty() {
            let mut trigger = self.iii.inner.functions_available_trigger.lock_or_recover();
            if let Some(trigger) = trigger.take() {
                trigger.unregister();
            }
        }
    }
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
            functions_available_callbacks: Mutex::new(HashMap::new()),
            functions_available_callback_counter: AtomicUsize::new(0),
            functions_available_function_id: Mutex::new(None),
            functions_available_trigger: Mutex::new(None),
            #[cfg(feature = "otel")]
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

    /// Set OpenTelemetry configuration (call before connect)
    #[cfg(feature = "otel")]
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
        tokio::spawn(async move {
            iii.run_connection(rx).await;
        });

        #[cfg(feature = "otel")]
        {
            let config = self.inner.otel_config.lock_or_recover().take();
            if let Some(mut config) = config {
                if config.engine_ws_url.is_none() {
                    config.engine_ws_url = Some(self.inner.address.clone());
                }
                tokio::spawn(async move {
                    telemetry::init_otel(config).await;
                });
            }
        }
    }

    /// Shutdown the III client.
    ///
    /// This stops the connection loop and sends a shutdown signal.
    /// If the `otel` feature is enabled, this will spawn a background task
    /// to flush telemetry data, but does NOT wait for it to complete.
    /// For guaranteed telemetry flush, use `shutdown_async()` instead.
    #[deprecated(note = "Use shutdown_async() for guaranteed telemetry flush")]
    pub fn shutdown(&self) {
        self.inner.running.store(false, Ordering::SeqCst);
        let _ = self.inner.outbound.send(Outbound::Shutdown);
        self.set_connection_state(IIIConnectionState::Disconnected);

        #[cfg(feature = "otel")]
        {
            tracing::warn!(
                "shutdown() does not await telemetry flush; use shutdown_async() instead"
            );
            tokio::spawn(async {
                telemetry::shutdown_otel().await;
            });
        }
    }

    /// Shutdown the III client and flush all pending telemetry data.
    ///
    /// This method stops the connection loop and sends a shutdown signal.
    /// When the `otel` feature is enabled, it additionally awaits the
    /// OpenTelemetry flush, ensuring all spans, metrics, and logs are
    /// exported before returning.
    pub async fn shutdown_async(&self) {
        self.inner.running.store(false, Ordering::SeqCst);
        let _ = self.inner.outbound.send(Outbound::Shutdown);
        self.set_connection_state(IIIConnectionState::Disconnected);

        #[cfg(feature = "otel")]
        telemetry::shutdown_otel().await;
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
    /// use iii_sdk::{register_worker, InitOptions, RegisterFunctionMessage};
    /// use serde_json::{json, Value};
    /// let iii = register_worker("ws://localhost:49134", InitOptions::default());
    /// iii.register_function(
    ///     RegisterFunctionMessage {
    ///         id: "greet".to_string(),
    ///         description: None,
    ///         request_format: None,
    ///         response_format: None,
    ///         metadata: None,
    ///         invocation: None,
    ///     },
    ///     |input: Value| async move {
    ///         Ok(json!({"message": format!("Hello, {}!", input["name"])}))
    ///     },
    /// );
    /// ```
    pub fn register_function<H: IntoFunctionHandler>(
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
    /// # Arguments
    /// * `id` - Unique trigger type identifier.
    /// * `description` - Human-readable description.
    /// * `handler` - Handler implementing [`TriggerHandler`].
    pub fn register_trigger_type<H>(
        &self,
        id: impl Into<String>,
        description: impl Into<String>,
        handler: H,
    ) where
        H: TriggerHandler + 'static,
    {
        let message = RegisterTriggerTypeMessage {
            id: id.into(),
            description: description.into(),
        };

        self.inner.trigger_types.lock_or_recover().insert(
            message.id.clone(),
            RemoteTriggerTypeData {
                message: message.clone(),
                handler: Arc::new(handler),
            },
        );

        let _ = self.send_message(message.to_message());
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
    ///     function_id: "enqueue".to_string(),
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

    /// List all registered functions from the engine
    pub async fn list_functions(&self) -> Result<Vec<FunctionInfo>, IIIError> {
        let result = self
            .trigger(TriggerRequest {
                function_id: "engine::functions::list".to_string(),
                payload: serde_json::json!({}),
                action: None,
                timeout_ms: None,
            })
            .await?;

        let functions = result
            .get("functions")
            .and_then(|v| serde_json::from_value::<Vec<FunctionInfo>>(v.clone()).ok())
            .unwrap_or_default();

        Ok(functions)
    }

    /// Subscribe to function availability events
    /// Returns a guard that will unsubscribe when dropped
    pub fn on_functions_available<F>(&self, callback: F) -> FunctionsAvailableGuard
    where
        F: Fn(Vec<FunctionInfo>) + Send + Sync + 'static,
    {
        let callback = Arc::new(callback);
        let callback_id = self
            .inner
            .functions_available_callback_counter
            .fetch_add(1, Ordering::Relaxed);

        self.inner
            .functions_available_callbacks
            .lock_or_recover()
            .insert(callback_id, callback);

        // Set up trigger if not already done
        let mut trigger_guard = self.inner.functions_available_trigger.lock_or_recover();
        if trigger_guard.is_none() {
            // Get or create function path (reuse existing if trigger registration previously failed)
            let function_id = {
                let mut path_guard = self.inner.functions_available_function_id.lock_or_recover();
                if path_guard.is_none() {
                    let path = format!("iii.on_functions_available.{}", Uuid::new_v4());
                    *path_guard = Some(path.clone());
                    path
                } else {
                    path_guard.clone().unwrap()
                }
            };

            // Register handler function only if it doesn't already exist
            let function_exists = self
                .inner
                .functions
                .lock_or_recover()
                .contains_key(&function_id);
            if !function_exists {
                let iii = self.clone();
                self.register_function(
                    RegisterFunctionMessage {
                        id: function_id.clone(),
                        description: None,
                        request_format: None,
                        response_format: None,
                        metadata: None,
                        invocation: None,
                    },
                    move |input: Value| {
                        let iii = iii.clone();
                        async move {
                            let functions = input
                                .get("functions")
                                .and_then(|v| {
                                    serde_json::from_value::<Vec<FunctionInfo>>(v.clone()).ok()
                                })
                                .unwrap_or_default();

                            let callbacks =
                                iii.inner.functions_available_callbacks.lock_or_recover();
                            for cb in callbacks.values() {
                                cb(functions.clone());
                            }
                            Ok(Value::Null)
                        }
                    },
                );
            }

            match self.register_trigger(RegisterTriggerInput {
                trigger_type: "engine::functions-available".to_string(),
                function_id,
                config: serde_json::json!({}),
            }) {
                Ok(trigger) => {
                    *trigger_guard = Some(trigger);
                }
                Err(err) => {
                    tracing::warn!(error = %err, "Failed to register functions_available trigger");
                }
            }
        }

        FunctionsAvailableGuard {
            iii: self.clone(),
            callback_id,
        }
    }

    /// List all connected workers from the engine
    pub async fn list_workers(&self) -> Result<Vec<WorkerInfo>, IIIError> {
        let result = self
            .trigger(TriggerRequest {
                function_id: "engine::workers::list".to_string(),
                payload: serde_json::json!({}),
                action: None,
                timeout_ms: None,
            })
            .await?;

        let workers = result
            .get("workers")
            .and_then(|v| serde_json::from_value::<Vec<WorkerInfo>>(v.clone()).ok())
            .unwrap_or_default();

        Ok(workers)
    }

    /// List all registered triggers from the engine
    pub async fn list_triggers(
        &self,
        include_internal: bool,
    ) -> Result<Vec<TriggerInfo>, IIIError> {
        let result = self
            .trigger(TriggerRequest {
                function_id: "engine::triggers::list".to_string(),
                payload: serde_json::json!({ "include_internal": include_internal }),
                action: None,
                timeout_ms: None,
            })
            .await?;

        let triggers = result
            .get("triggers")
            .and_then(|v| serde_json::from_value::<Vec<TriggerInfo>>(v.clone()).ok())
            .unwrap_or_default();

        Ok(triggers)
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
        if let Some(metadata) = self.inner.worker_metadata.lock_or_recover().clone() {
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

            match connect_async(&self.inner.address).await {
                Ok((stream, _)) => {
                    tracing::info!(address = %self.inner.address, "iii connected");
                    has_connected_before = true;
                    self.set_connection_state(IIIConnectionState::Connected);
                    let (mut ws_tx, mut ws_rx) = stream.split();

                    queue.extend(self.collect_registrations());
                    Self::dedupe_registrations(&mut queue);
                    if let Err(err) = self.flush_queue(&mut ws_tx, &mut queue).await {
                        tracing::warn!(error = %err, "failed to flush queue");
                        sleep(Duration::from_secs(2)).await;
                        continue;
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

    fn dedupe_registrations(queue: &mut Vec<Message>) {
        let mut seen = HashSet::new();
        let mut deduped_rev = Vec::with_capacity(queue.len());

        for message in queue.iter().rev() {
            let key = match message {
                Message::RegisterTriggerType { id, .. } => format!("trigger_type:{id}"),
                Message::RegisterTrigger { id, .. } => format!("trigger:{id}"),
                Message::RegisterFunction { id, .. } => {
                    format!("function:{id}")
                }
                Message::RegisterService { id, .. } => format!("service:{id}"),
                _ => {
                    deduped_rev.push(message.clone());
                    continue;
                }
            };

            if seen.insert(key) {
                deduped_rev.push(message.clone());
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
            } => {
                self.handle_register_trigger(id, trigger_type, function_id, config);
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
            #[cfg(feature = "otel")]
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

            #[cfg(feature = "otel")]
            let result = {
                use opentelemetry::trace::FutureExt as OtelFutureExt;
                handler(data).with_context(otel_cx.clone()).await
            };

            #[cfg(not(feature = "otel"))]
            let result = handler(data).await;

            // Record span status based on result
            #[allow(unused_mut)]
            let mut error_stacktrace: Option<String> = None;
            #[cfg(feature = "otel")]
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
                #[cfg(feature = "otel")]
                let (resp_tp, resp_bg) = {
                    let _guard = otel_cx.attach();
                    inject_trace_headers()
                };
                #[cfg(not(feature = "otel"))]
                let (resp_tp, resp_bg) = inject_trace_headers();

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

        let func_ref = iii.register_function(
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

        iii.register_function(
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
}
