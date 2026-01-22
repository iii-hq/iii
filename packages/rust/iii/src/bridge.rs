use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

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
    context::{Context, with_context},
    error::BridgeError,
    logger::{Logger, LoggerInvoker},
    protocol::{
        ErrorBody, Message, RegisterFunctionMessage, RegisterServiceMessage,
        RegisterTriggerMessage, RegisterTriggerTypeMessage, UnregisterTriggerMessage,
    },
    triggers::{Trigger, TriggerConfig, TriggerHandler},
    types::{RemoteFunctionData, RemoteFunctionHandler, RemoteTriggerTypeData},
};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Worker information returned by `engine.workers.list`
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

/// Function information returned by `engine.functions.list`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInfo {
    pub function_path: String,
    pub description: Option<String>,
    pub request_format: Option<Value>,
    pub response_format: Option<Value>,
    pub metadata: Option<Value>,
}

/// Trigger information returned by `engine.triggers.list`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerInfo {
    pub id: String,
    pub trigger_type: String,
    pub function_path: String,
    pub config: Value,
}

/// Worker metadata for auto-registration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerMetadata {
    pub runtime: String,
    pub version: String,
    pub name: String,
    pub os: String,
}

/// Process-level metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProcessMetrics {
    /// Current CPU usage percentage (0-100)
    pub cpu_percent: Option<f64>,
    /// Process memory usage in bytes
    pub memory_used_bytes: Option<u64>,
    /// Total available memory in bytes
    pub memory_total_bytes: Option<u64>,
    /// Time since worker started in seconds
    pub process_uptime_secs: Option<u64>,
}

/// Performance metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    /// Number of active threads
    pub thread_count: Option<u32>,
    /// Number of open network connections
    pub open_connections: Option<u32>,
    /// Invocations processed per second
    pub invocations_per_sec: Option<f64>,
    /// Average invocation latency in milliseconds
    pub avg_latency_ms: Option<f64>,
}

/// Extended metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtendedMetrics {
    /// Disk read bytes
    pub disk_read_bytes: Option<u64>,
    /// Disk write bytes
    pub disk_write_bytes: Option<u64>,
    /// Network bytes received
    pub network_rx_bytes: Option<u64>,
    /// Network bytes transmitted
    pub network_tx_bytes: Option<u64>,
    /// Number of open file descriptors
    pub open_file_descriptors: Option<u32>,
    /// Total failed invocations count
    pub error_count: Option<u64>,
}

/// Kubernetes/EKS-specific identifiers for correlation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KubernetesIdentifiers {
    /// Cluster name
    pub cluster: Option<String>,
    /// Kubernetes namespace
    pub namespace: Option<String>,
    /// Pod name
    pub pod_name: Option<String>,
    /// Container name
    pub container_name: Option<String>,
    /// Node name
    pub node_name: Option<String>,
    /// Pod UID for unique identification
    pub pod_uid: Option<String>,
}

/// Kubernetes core metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KubernetesCoreMetrics {
    /// CPU usage in cores (or millicores)
    pub cpu_usage_cores: Option<f64>,
    /// Memory working set bytes
    pub memory_working_set_bytes: Option<u64>,
    /// Pod phase (Pending, Running, Succeeded, Failed, Unknown)
    pub pod_phase: Option<String>,
    /// Whether pod is ready to accept traffic
    pub pod_ready: Option<bool>,
    /// Total container restarts
    pub container_restarts_total: Option<u32>,
    /// Last termination reason (e.g., OOMKilled, Error)
    pub last_termination_reason: Option<String>,
    /// Container uptime in seconds
    pub uptime_seconds: Option<u64>,
}

/// Kubernetes resource metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KubernetesResourceMetrics {
    /// CPU requests in cores
    pub cpu_requests_cores: Option<f64>,
    /// CPU limits in cores
    pub cpu_limits_cores: Option<f64>,
    /// Memory requests in bytes
    pub memory_requests_bytes: Option<u64>,
    /// Memory limits in bytes
    pub memory_limits_bytes: Option<u64>,
    /// CPU throttled time in seconds
    pub cpu_throttled_seconds_total: Option<f64>,
    /// Time pod spent in pending state
    pub pod_pending_seconds: Option<f64>,
}

/// Kubernetes extended metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KubernetesExtendedMetrics {
    /// Network received bytes (pod/container)
    pub network_rx_bytes_total: Option<u64>,
    /// Network transmitted bytes (pod/container)
    pub network_tx_bytes_total: Option<u64>,
    /// Filesystem usage in bytes
    pub fs_usage_bytes: Option<u64>,
    /// Node memory pressure
    pub node_memory_pressure: Option<bool>,
    /// Node disk pressure
    pub node_disk_pressure: Option<bool>,
    /// Node PID pressure
    pub node_pid_pressure: Option<bool>,
}

/// Complete worker metrics payload
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerMetrics {
    /// Timestamp when metrics were collected (Unix epoch ms)
    pub collected_at_ms: u64,

    #[serde(default)]
    pub process: ProcessMetrics,
    #[serde(default)]
    pub performance: PerformanceMetrics,
    #[serde(default)]
    pub extended: ExtendedMetrics,
    #[serde(default)]
    pub k8s_identifiers: Option<KubernetesIdentifiers>,
    #[serde(default)]
    pub k8s_core: Option<KubernetesCoreMetrics>,
    #[serde(default)]
    pub k8s_resources: Option<KubernetesResourceMetrics>,
    #[serde(default)]
    pub k8s_extended: Option<KubernetesExtendedMetrics>,
}

/// Worker metrics response with worker info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerMetricsInfo {
    pub worker_id: String,
    pub worker_name: Option<String>,
    pub metrics: WorkerMetrics,
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

        Self {
            runtime: "rust".to_string(),
            version: SDK_VERSION.to_string(),
            name: format!("{}:{}", hostname, pid),
            os: os_info,
        }
    }
}

enum Outbound {
    Message(Message),
    Shutdown,
}

type PendingInvocation = oneshot::Sender<Result<Value, BridgeError>>;

// WebSocket transmitter type alias
type WsTx = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    WsMessage,
>;

struct BridgeInner {
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
}

#[derive(Clone)]
pub struct Bridge {
    inner: Arc<BridgeInner>,
}

impl Bridge {
    /// Create a new Bridge with default worker metadata (auto-detected runtime, os, hostname)
    pub fn new(address: &str) -> Self {
        Self::with_metadata(address, WorkerMetadata::default())
    }

    /// Create a new Bridge with custom worker metadata
    pub fn with_metadata(address: &str, metadata: WorkerMetadata) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let inner = BridgeInner {
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
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Set custom worker metadata (call before connect)
    pub fn set_metadata(&self, metadata: WorkerMetadata) {
        *self.inner.worker_metadata.lock().unwrap() = Some(metadata);
    }

    pub async fn connect(&self) -> Result<(), BridgeError> {
        if self.inner.started.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let receiver = self.inner.receiver.lock().unwrap().take();
        let Some(rx) = receiver else {
            return Ok(());
        };

        let bridge = self.clone();

        tokio::spawn(async move {
            bridge.inner.running.store(true, Ordering::SeqCst);
            bridge.run_connection(rx).await;
        });

        Ok(())
    }

    pub fn disconnect(&self) {
        self.inner.running.store(false, Ordering::SeqCst);
        let _ = self.inner.outbound.send(Outbound::Shutdown);
    }

    pub fn register_function<F, Fut>(&self, function_path: impl Into<String>, handler: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Value, BridgeError>> + Send + 'static,
    {
        let message = RegisterFunctionMessage {
            function_path: function_path.into(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        };

        self.register_function_with(message, handler);
    }

    pub fn register_function_with_description<F, Fut>(
        &self,
        function_path: impl Into<String>,
        description: impl Into<String>,
        handler: F,
    ) where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Value, BridgeError>> + Send + 'static,
    {
        let message = RegisterFunctionMessage {
            function_path: function_path.into(),
            description: Some(description.into()),
            request_format: None,
            response_format: None,
            metadata: None,
        };

        self.register_function_with(message, handler);
    }

    pub fn register_function_with<F, Fut>(&self, message: RegisterFunctionMessage, handler: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Value, BridgeError>> + Send + 'static,
    {
        let function_path = message.function_path.clone();
        let bridge = self.clone();

        let user_handler = Arc::new(move |input: Value| Box::pin(handler(input)));

        let wrapped_handler: RemoteFunctionHandler = Arc::new(move |input: Value| {
            let function_path = function_path.clone();
            let bridge = bridge.clone();
            let user_handler = user_handler.clone();

            Box::pin(async move {
                let invoker: LoggerInvoker = Arc::new(move |path, params| {
                    let _ = bridge.invoke_function_async(path, params);
                });

                let logger = Logger::new(
                    Some(invoker),
                    Some(Uuid::new_v4().to_string()),
                    Some(function_path.clone()),
                );
                let context = Context { logger };

                with_context(context, || user_handler(input)).await
            })
        });

        let data = RemoteFunctionData {
            message: message.clone(),
            handler: wrapped_handler,
        };

        self.inner
            .functions
            .lock()
            .unwrap()
            .insert(message.function_path.clone(), data);
        let _ = self.send_message(message.to_message());
    }

    pub fn register_service(&self, id: impl Into<String>, description: Option<String>) {
        let id = id.into();
        let message = RegisterServiceMessage {
            id: id.clone(),
            name: id,
            description,
        };

        self.inner
            .services
            .lock()
            .unwrap()
            .insert(message.id.clone(), message.clone());
        let _ = self.send_message(message.to_message());
    }

    pub fn register_service_with_name(
        &self,
        id: impl Into<String>,
        name: impl Into<String>,
        description: Option<String>,
    ) {
        let message = RegisterServiceMessage {
            id: id.into(),
            name: name.into(),
            description,
        };

        self.inner
            .services
            .lock()
            .unwrap()
            .insert(message.id.clone(), message.clone());
        let _ = self.send_message(message.to_message());
    }

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

        self.inner.trigger_types.lock().unwrap().insert(
            message.id.clone(),
            RemoteTriggerTypeData {
                message: message.clone(),
                handler: Arc::new(handler),
            },
        );

        let _ = self.send_message(message.to_message());
    }

    pub fn unregister_trigger_type(&self, id: impl Into<String>) {
        let id = id.into();
        self.inner.trigger_types.lock().unwrap().remove(&id);
    }

    pub fn register_trigger(
        &self,
        trigger_type: impl Into<String>,
        function_path: impl Into<String>,
        config: impl serde::Serialize,
    ) -> Result<Trigger, BridgeError> {
        let id = Uuid::new_v4().to_string();
        let config = serde_json::to_value(config)?;
        let message = RegisterTriggerMessage {
            id: id.clone(),
            trigger_type: trigger_type.into(),
            function_path: function_path.into(),
            config,
        };

        self.inner
            .triggers
            .lock()
            .unwrap()
            .insert(message.id.clone(), message.clone());
        let _ = self.send_message(message.to_message());

        let bridge = self.clone();
        let trigger_type = message.trigger_type.clone();
        let unregister_id = message.id.clone();
        let unregister_fn = Arc::new(move || {
            let _ = bridge.inner.triggers.lock().unwrap().remove(&unregister_id);
            let msg = UnregisterTriggerMessage {
                id: unregister_id.clone(),
                trigger_type: trigger_type.clone(),
            };
            let _ = bridge.send_message(msg.to_message());
        });

        Ok(Trigger::new(unregister_fn))
    }

    pub async fn invoke_function(
        &self,
        function_path: &str,
        data: impl serde::Serialize,
    ) -> Result<Value, BridgeError> {
        let value = serde_json::to_value(data)?;
        self.invoke_function_with_timeout(function_path, value, DEFAULT_TIMEOUT)
            .await
    }

    pub async fn invoke_function_with_timeout(
        &self,
        function_path: &str,
        data: Value,
        timeout: Duration,
    ) -> Result<Value, BridgeError> {
        let invocation_id = Uuid::new_v4();
        let (tx, rx) = oneshot::channel();

        self.inner.pending.lock().unwrap().insert(invocation_id, tx);

        self.send_message(Message::InvokeFunction {
            invocation_id: Some(invocation_id),
            function_path: function_path.to_string(),
            data,
        })?;

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(BridgeError::NotConnected),
            Err(_) => {
                self.inner.pending.lock().unwrap().remove(&invocation_id);
                Err(BridgeError::Timeout)
            }
        }
    }

    pub fn invoke_function_async<TInput>(
        &self,
        function_path: &str,
        data: TInput,
    ) -> Result<(), BridgeError>
    where
        TInput: Serialize,
    {
        let value = serde_json::to_value(data)?;
        self.send_message(Message::InvokeFunction {
            invocation_id: None,
            function_path: function_path.to_string(),
            data: value,
        })
    }

    /// List all registered functions from the engine
    pub async fn list_functions(&self) -> Result<Vec<FunctionInfo>, BridgeError> {
        let result = self
            .invoke_function("engine.functions.list", serde_json::json!({}))
            .await?;

        let functions = result
            .get("functions")
            .and_then(|v| serde_json::from_value::<Vec<FunctionInfo>>(v.clone()).ok())
            .unwrap_or_default();

        Ok(functions)
    }

    /// List all connected workers from the engine
    pub async fn list_workers(&self) -> Result<Vec<WorkerInfo>, BridgeError> {
        let result = self
            .invoke_function("engine.workers.list", serde_json::json!({}))
            .await?;

        let workers = result
            .get("workers")
            .and_then(|v| serde_json::from_value::<Vec<WorkerInfo>>(v.clone()).ok())
            .unwrap_or_default();

        Ok(workers)
    }

    /// List all registered triggers from the engine
    pub async fn list_triggers(&self) -> Result<Vec<TriggerInfo>, BridgeError> {
        let result = self
            .invoke_function("engine.triggers.list", serde_json::json!({}))
            .await?;

        let triggers = result
            .get("triggers")
            .and_then(|v| serde_json::from_value::<Vec<TriggerInfo>>(v.clone()).ok())
            .unwrap_or_default();

        Ok(triggers)
    }

    /// Report worker metrics to the engine.
    /// Metrics are automatically associated with this worker via the injected worker_id.
    pub fn report_metrics(&self, mut metrics: WorkerMetrics) -> Result<(), BridgeError> {
        // Set timestamp if not already set
        if metrics.collected_at_ms == 0 {
            metrics.collected_at_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
        }
        self.invoke_function_async("engine.workers.report_metrics", metrics)
    }

    /// Get metrics for a specific worker by ID.
    pub async fn get_worker_metrics(
        &self,
        worker_id: &str,
    ) -> Result<Option<WorkerMetricsInfo>, BridgeError> {
        let result = self
            .invoke_function(
                "engine.workers.get_metrics",
                serde_json::json!({ "worker_id": worker_id }),
            )
            .await?;

        if result.get("error").is_some() {
            return Ok(None);
        }

        Ok(serde_json::from_value(result).ok())
    }

    /// Get metrics for all workers.
    pub async fn get_all_worker_metrics(&self) -> Result<Vec<WorkerMetricsInfo>, BridgeError> {
        let result = self
            .invoke_function("engine.workers.get_metrics", serde_json::json!({}))
            .await?;

        let metrics = result
            .get("workers")
            .and_then(|v| serde_json::from_value::<Vec<WorkerMetricsInfo>>(v.clone()).ok())
            .unwrap_or_default();

        Ok(metrics)
    }

    /// Register this worker's metadata with the engine (called automatically on connect)
    fn register_worker_metadata(&self) {
        if let Some(metadata) = self.inner.worker_metadata.lock().unwrap().clone() {
            let _ = self.invoke_function_async("engine.workers.register", metadata);
        }
    }

    fn send_message(&self, message: Message) -> Result<(), BridgeError> {
        if !self.inner.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        self.inner
            .outbound
            .send(Outbound::Message(message))
            .map_err(|_| BridgeError::NotConnected)
    }

    async fn run_connection(&self, mut rx: mpsc::UnboundedReceiver<Outbound>) {
        let mut queue: Vec<Message> = Vec::new();

        while self.inner.running.load(Ordering::SeqCst) {
            match connect_async(&self.inner.address).await {
                Ok((stream, _)) => {
                    tracing::info!(address = %self.inner.address, "bridge connected");
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

        for trigger_type in self.inner.trigger_types.lock().unwrap().values() {
            messages.push(trigger_type.message.to_message());
        }

        for service in self.inner.services.lock().unwrap().values() {
            messages.push(service.to_message());
        }

        for function in self.inner.functions.lock().unwrap().values() {
            messages.push(function.message.to_message());
        }

        for trigger in self.inner.triggers.lock().unwrap().values() {
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
                Message::RegisterFunction { function_path, .. } => {
                    format!("function:{function_path}")
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
    ) -> Result<(), BridgeError> {
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

    async fn send_ws(&self, ws_tx: &mut WsTx, message: &Message) -> Result<(), BridgeError> {
        let payload = serde_json::to_string(message)?;
        ws_tx.send(WsMessage::Text(payload)).await?;
        Ok(())
    }

    fn handle_frame(&self, frame: WsMessage) -> Result<(), BridgeError> {
        match frame {
            WsMessage::Text(text) => self.handle_message(&text),
            WsMessage::Binary(bytes) => {
                let text = String::from_utf8_lossy(&bytes).to_string();
                self.handle_message(&text)
            }
            _ => Ok(()),
        }
    }

    fn handle_message(&self, payload: &str) -> Result<(), BridgeError> {
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
                function_path,
                data,
            } => {
                self.handle_invoke_function(invocation_id, function_path, data);
            }
            Message::RegisterTrigger {
                id,
                trigger_type,
                function_path,
                config,
            } => {
                self.handle_register_trigger(id, trigger_type, function_path, config);
            }
            Message::Ping => {
                let _ = self.send_message(Message::Pong);
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
        let sender = self.inner.pending.lock().unwrap().remove(&invocation_id);
        if let Some(sender) = sender {
            let result = match error {
                Some(error) => Err(BridgeError::Remote {
                    code: error.code,
                    message: error.message,
                }),
                None => Ok(result.unwrap_or(Value::Null)),
            };
            let _ = sender.send(result);
        }
    }

    fn handle_invoke_function(
        &self,
        invocation_id: Option<Uuid>,
        function_path: String,
        data: Value,
    ) {
        tracing::debug!(function_path = %function_path, "Invoking function");

        let handler = self
            .inner
            .functions
            .lock()
            .unwrap()
            .get(&function_path)
            .map(|data| data.handler.clone());

        let Some(handler) = handler else {
            tracing::warn!(function_path = %function_path, "Invocation: Function not found");

            if let Some(invocation_id) = invocation_id {
                let error = ErrorBody {
                    code: "function_not_found".to_string(),
                    message: "Function not found".to_string(),
                };
                let result = self.send_message(Message::InvocationResult {
                    invocation_id,
                    function_path,
                    result: None,
                    error: Some(error),
                });

                if let Err(err) = result {
                    tracing::warn!(error = %err, "error sending invocation result");
                }
            }
            return;
        };

        let bridge = self.clone();

        tokio::spawn(async move {
            let result = handler(data).await;

            if let Some(invocation_id) = invocation_id {
                let message = match result {
                    Ok(value) => Message::InvocationResult {
                        invocation_id,
                        function_path,
                        result: Some(value),
                        error: None,
                    },
                    Err(err) => Message::InvocationResult {
                        invocation_id,
                        function_path,
                        result: None,
                        error: Some(ErrorBody {
                            code: "invocation_failed".to_string(),
                            message: err.to_string(),
                        }),
                    },
                };

                let _ = bridge.send_message(message);
            } else if let Err(err) = result {
                tracing::warn!(error = %err, "error handling async invocation");
            }
        });
    }

    fn handle_register_trigger(
        &self,
        id: String,
        trigger_type: String,
        function_path: String,
        config: Value,
    ) {
        let handler = self
            .inner
            .trigger_types
            .lock()
            .unwrap()
            .get(&trigger_type)
            .map(|data| data.handler.clone());

        let bridge = self.clone();

        tokio::spawn(async move {
            let message = if let Some(handler) = handler {
                let config = TriggerConfig {
                    id: id.clone(),
                    function_path: function_path.clone(),
                    config,
                };

                match handler.register_trigger(config).await {
                    Ok(()) => Message::TriggerRegistrationResult {
                        id,
                        trigger_type,
                        function_path,
                        error: None,
                    },
                    Err(err) => Message::TriggerRegistrationResult {
                        id,
                        trigger_type,
                        function_path,
                        error: Some(ErrorBody {
                            code: "trigger_registration_failed".to_string(),
                            message: err.to_string(),
                        }),
                    },
                }
            } else {
                Message::TriggerRegistrationResult {
                    id,
                    trigger_type,
                    function_path,
                    error: Some(ErrorBody {
                        code: "trigger_type_not_found".to_string(),
                        message: "Trigger type not found".to_string(),
                    }),
                }
            };

            let _ = bridge.send_message(message);
        });
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn register_trigger_unregister_removes_entry() {
        let bridge = Bridge::new("ws://localhost:1234");
        let trigger = bridge.register_trigger("demo", "functions.echo", json!({ "foo": "bar" }));

        assert_eq!(bridge.inner.triggers.lock().unwrap().len(), 1);

        trigger.unregister();

        assert_eq!(bridge.inner.triggers.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn invoke_function_times_out_and_clears_pending() {
        let bridge = Bridge::new("ws://localhost:1234");
        let result = bridge
            .invoke_function_with_timeout(
                "functions.echo",
                json!({ "a": 1 }),
                Duration::from_millis(10),
            )
            .await;

        assert!(matches!(result, Err(BridgeError::Timeout)));
        assert!(bridge.inner.pending.lock().unwrap().is_empty());
    }
}
