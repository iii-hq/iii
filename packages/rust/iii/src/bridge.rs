use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::Value;
use tokio::{
    sync::{mpsc, oneshot},
    time::sleep,
};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
use uuid::Uuid;

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
}

#[derive(Clone)]
pub struct Bridge {
    inner: Arc<BridgeInner>,
}

impl Bridge {
    pub fn new(address: &str) -> Self {
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
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    pub async fn connect(&self) -> Result<(), BridgeError> {
        if self.inner.started.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let receiver = self.inner.receiver.lock().unwrap().take();
        let Some(rx) = receiver else {
            return Ok(());
        };

        self.inner.running.store(true, Ordering::SeqCst);
        let bridge = self.clone();
        tokio::spawn(async move {
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
        config: Value,
    ) -> Trigger {
        let id = Uuid::new_v4().to_string();
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

        let inner = self.inner.clone();
        let trigger_type = message.trigger_type.clone();
        let unregister_id = message.id.clone();
        let unregister_fn = Arc::new(move || {
            let _ = inner.triggers.lock().unwrap().remove(&unregister_id);
            let msg = UnregisterTriggerMessage {
                id: unregister_id.clone(),
                trigger_type: trigger_type.clone(),
            };
            let _ = inner.outbound.send(Outbound::Message(msg.to_message()));
        });

        Trigger::new(unregister_fn)
    }

    pub async fn invoke_function<TInput>(
        &self,
        function_path: &str,
        data: TInput,
    ) -> Result<Value, BridgeError>
    where
        TInput: Serialize,
    {
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

    pub async fn list_functions(&self) -> Result<Value, BridgeError> {
        self.invoke_function("engine.functions.list", serde_json::json!({}))
            .await
    }

    fn send_message(&self, message: Message) -> Result<(), BridgeError> {
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
                    if let Err(err) = self.flush_queue(&mut ws_tx, &mut queue).await {
                        tracing::warn!(error = %err, "failed to flush queue");
                        sleep(Duration::from_secs(2)).await;
                        continue;
                    }

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
                let _ = self.inner.outbound.send(Outbound::Message(Message::Pong));
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
        let handler = self
            .inner
            .functions
            .lock()
            .unwrap()
            .get(&function_path)
            .map(|data| data.handler.clone());

        let Some(handler) = handler else {
            if let Some(invocation_id) = invocation_id {
                let error = ErrorBody {
                    code: "function_not_found".to_string(),
                    message: "Function not found".to_string(),
                };
                let _ = self
                    .inner
                    .outbound
                    .send(Outbound::Message(Message::InvocationResult {
                        invocation_id,
                        function_path,
                        result: None,
                        error: Some(error),
                    }));
            }
            return;
        };

        let outbound = self.inner.outbound.clone();

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

                let _ = outbound.send(Outbound::Message(message));
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

        let outbound = self.inner.outbound.clone();

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

            let _ = outbound.send(Outbound::Message(message));
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
