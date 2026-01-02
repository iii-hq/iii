use std::{net::SocketAddr, sync::Arc};

use axum::extract::ws::{Message as WsMessage, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot::error::RecvError};
use uuid::Uuid;

use crate::{
    function::{Function, FunctionHandler, FunctionResult, FunctionsRegistry},
    invocation::InvocationHandler,
    protocol::{ErrorBody, FunctionMessage, Message},
    services::{Service, ServicesRegistry},
    trigger::{Trigger, TriggerRegistry, TriggerType},
    workers::{Worker, WorkerRegistry},
};

#[derive(Debug)]
pub enum Outbound {
    Protocol(Message),
    Raw(WsMessage),
}

pub struct RegisterFunctionRequest {
    pub function_path: String,
    pub description: Option<String>,
    pub request_format: Option<Value>,
    pub response_format: Option<Value>,
}

pub struct Handler<H> {
    f: H,
}

impl<H, F> Handler<H>
where
    H: Fn(Value) -> F + Send + Sync + 'static,
    F: Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send + 'static,
{
    pub fn new(f: H) -> Self {
        Self { f }
    }

    pub fn call(&self, input: Value) -> F {
        (self.f)(input)
    }
}

#[allow(async_fn_in_trait)]
pub trait EngineTrait: Send + Sync {
    async fn invoke_function(
        &self,
        function_path: &str,
        input: Value,
    ) -> Result<Option<Value>, ErrorBody>;
    async fn register_trigger_type(&self, trigger_type: TriggerType);
    fn register_function(
        &self,
        request: RegisterFunctionRequest,
        handler: Box<dyn FunctionHandler + Send + Sync>,
    );
    fn register_function_handler<H, F>(
        &self,
        request: RegisterFunctionRequest,
        handler: Handler<H>,
    ) where
        H: Fn(Value) -> F + Send + Sync + 'static,
        F: Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send + 'static;
}

#[derive(Default, Clone)]
pub struct Engine {
    pub worker_registry: Arc<WorkerRegistry>,
    pub functions: Arc<FunctionsRegistry>,
    pub trigger_registry: Arc<TriggerRegistry>,
    pub service_registry: Arc<ServicesRegistry>,
    pub invocations: Arc<InvocationHandler>,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            worker_registry: Arc::new(WorkerRegistry::new()),
            functions: Arc::new(FunctionsRegistry::new()),
            trigger_registry: Arc::new(TriggerRegistry::new()),
            service_registry: Arc::new(ServicesRegistry::new()),
            invocations: Arc::new(InvocationHandler::new()),
        }
    }

    async fn send_msg(&self, worker: &Worker, msg: Message) -> bool {
        worker.channel.send(Outbound::Protocol(msg)).await.is_ok()
    }

    fn remove_function(&self, function_path: &str) {
        self.functions.remove(function_path);
    }

    async fn remember_invocation(
        &self,
        worker: &Worker,
        invocation_id: Option<Uuid>,
        function_path: &str,
        body: Value,
    ) -> Result<Result<Option<Value>, ErrorBody>, RecvError> {
        tracing::debug!(
            worker_id = %worker.id,
            ?invocation_id,
            function_path = function_path,
            "Remembering invocation for worker"
        );

        if let Some(function) = self.functions.get(function_path) {
            if let Some(invocation_id) = invocation_id {
                worker.add_invocation(invocation_id).await;
            }

            self.invocations
                .handle_invocation(
                    invocation_id,
                    Some(worker.id),
                    function_path.to_string(),
                    body,
                    function,
                )
                .await
        } else {
            tracing::error!(function_path = %function_path, "Function not found");

            Ok(Err(ErrorBody {
                code: "function_not_found".into(),
                message: "Function not found".into(),
            }))
        }
    }

    pub async fn notify_new_functions(&self, duration_secs: u64) {
        let mut current_functions_hash = self.functions.functions_hash();

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(duration_secs)).await;
            let new_functions_hash = self.functions.functions_hash();
            if new_functions_hash != current_functions_hash {
                tracing::info!("New functions detected, notifying workers");
                let message: Vec<FunctionMessage> = self
                    .functions
                    .iter()
                    .map(|entry| FunctionMessage::from(entry.value()))
                    .collect();
                let message = Message::FunctionsAvailable { functions: message };
                self.broadcast_msg(message).await;
                current_functions_hash = new_functions_hash;
            }
        }
    }

    async fn broadcast_msg(&self, msg: Message) {
        for worker in self.worker_registry.workers.read().await.iter() {
            let _ = worker
                .value()
                .channel
                .send(Outbound::Protocol(msg.clone()))
                .await;
        }
    }

    async fn router_msg(&self, worker: &Worker, msg: &Message) -> anyhow::Result<()> {
        match msg {
            Message::FunctionsAvailable { functions } => {
                tracing::debug!(?functions, "FunctionsAvailable");
                Ok(())
            }
            Message::TriggerRegistrationResult {
                id,
                trigger_type,
                function_path,
                error,
            } => {
                tracing::debug!(id = %id, trigger_type = %trigger_type, function_path = %function_path, error = ?error, "TriggerRegistrationResult");
                Ok(())
            }
            Message::RegisterTriggerType { id, description } => {
                tracing::debug!(
                    worker_id = %worker.id,
                    trigger_type_id = %id,
                    description = %description,
                    "RegisterTriggerType"
                );
                let trigger_type = TriggerType {
                    id: id.clone(),
                    _description: description.clone(),
                    registrator: Box::new(worker.clone()),
                    worker_id: Some(worker.id),
                };

                let _ = self
                    .trigger_registry
                    .register_trigger_type(trigger_type)
                    .await;

                Ok(())
            }
            Message::RegisterTrigger {
                id,
                trigger_type,
                function_path,
                config,
            } => {
                tracing::debug!(
                    trigger_id = %id,
                    trigger_type = %trigger_type,
                    function_path = %function_path,
                    config = ?config,
                    "RegisterTrigger"
                );

                let _ = self
                    .trigger_registry
                    .register_trigger(Trigger {
                        id: id.clone(),
                        trigger_type: trigger_type.clone(),
                        function_path: function_path.clone(),
                        config: config.clone(),
                        worker_id: Some(worker.id),
                    })
                    .await;

                Ok(())
            }
            Message::UnregisterTrigger { id, trigger_type } => {
                tracing::debug!(
                    trigger_id = %id,
                    trigger_type = %trigger_type,
                    "UnregisterTrigger"
                );

                let _ = self
                    .trigger_registry
                    .unregister_trigger(id.clone(), trigger_type.clone())
                    .await;

                Ok(())
            }

            Message::InvokeFunction {
                invocation_id,
                function_path,
                data,
            } => {
                tracing::debug!(
                    worker_id = %worker.id,
                    invocation_id = ?invocation_id,
                    function_path = %function_path,
                    payload = ?data,
                    "InvokeFunction"
                );

                let result = self
                    .remember_invocation(worker, *invocation_id, function_path, data.clone())
                    .await;

                if let Some(invocation_id) = invocation_id {
                    match result {
                        Ok(result) => match result {
                            Ok(result) => {
                                self.send_msg(
                                    worker,
                                    Message::InvocationResult {
                                        invocation_id: *invocation_id,
                                        function_path: function_path.to_string(),
                                        result: result.clone(),
                                        error: None,
                                    },
                                )
                                .await;
                            }
                            Err(err) => {
                                self.send_msg(
                                    worker,
                                    Message::InvocationResult {
                                        invocation_id: *invocation_id,
                                        function_path: function_path.to_string(),
                                        result: None,
                                        error: Some(err.clone()),
                                    },
                                )
                                .await;
                            }
                        },
                        Err(err) => {
                            tracing::error!(error = ?err, "Error remembering invocation");
                            self.send_msg(
                                worker,
                                Message::InvocationResult {
                                    invocation_id: *invocation_id,
                                    function_path: function_path.to_string(),
                                    result: None,
                                    error: Some(ErrorBody {
                                        code: "invocation_error".into(),
                                        message: err.to_string(),
                                    }),
                                },
                            )
                            .await;
                        }
                    }
                }
                Ok(())
            }
            Message::InvocationResult {
                invocation_id,
                function_path,
                result,
                error,
            } => {
                tracing::debug!(
                    function_path = %function_path,
                    invocation_id = %invocation_id,
                    result = ?result,
                    error = ?error,
                    "InvocationResult"
                );

                worker.remove_invocation(invocation_id).await;

                if let Some(invocation) = self.invocations.remove(invocation_id) {
                    if let Some(err) = error {
                        let _ = invocation.sender.send(Err(err.clone()));
                    } else {
                        let _ = invocation.sender.send(Ok(result.clone()));
                    };
                    return Ok(());
                } else {
                    tracing::warn!(
                        invocation_id = %invocation_id,
                        "Did not find caller for invocation"
                    );
                }
                Ok(())
            }
            Message::RegisterFunction {
                function_path,
                description,
                request_format: req,
                response_format: res,
            } => {
                tracing::debug!(
                    worker_id = %worker.id,
                    function_path = %function_path,
                    description = ?description,
                    "RegisterFunction"
                );

                self.service_registry
                    .register_service_from_func_path(function_path)
                    .await;

                self.register_function(
                    RegisterFunctionRequest {
                        function_path: function_path.clone(),
                        description: description.clone(),
                        request_format: req.clone(),
                        response_format: res.clone(),
                    },
                    Box::new(worker.clone()),
                );

                worker.include_function_path(function_path).await;
                Ok(())
            }
            Message::RegisterService {
                id,
                name,
                description,
            } => {
                tracing::debug!(
                    service_id = %id,
                    service_name = %name,
                    description = ?description,
                    "RegisterService"
                );
                {
                    let services = self.service_registry.services.read().await;
                    tracing::debug!(services = ?services, "Current services");
                }

                self.service_registry
                    .insert_service(Service::new(name.clone(), id.clone()))
                    .await;

                Ok(())
            }
            Message::Ping => {
                self.send_msg(worker, Message::Pong).await;
                Ok(())
            }
            Message::Pong => Ok(()),
        }
    }

    pub async fn handle_worker(&self, socket: WebSocket, peer: SocketAddr) -> anyhow::Result<()> {
        tracing::debug!(peer = %peer, "Worker connected via WebSocket");
        let (mut ws_tx, mut ws_rx) = socket.split();
        let (tx, mut rx) = mpsc::channel::<Outbound>(64);

        let writer = tokio::spawn(async move {
            while let Some(outbound) = rx.recv().await {
                let send_result = match outbound {
                    Outbound::Protocol(msg) => match serde_json::to_string(&msg) {
                        Ok(payload) => ws_tx.send(WsMessage::Text(payload.into())).await,
                        Err(err) => {
                            tracing::error!(peer = %peer, error = ?err, "serialize error");
                            continue;
                        }
                    },
                    Outbound::Raw(frame) => ws_tx.send(frame).await,
                };

                if send_result.is_err() {
                    break;
                }
            }
        });

        let worker = Worker::new(tx.clone());

        tracing::debug!(worker_id = %worker.id, peer = %peer, "Assigned worker ID");
        self.worker_registry.register_worker(worker.clone()).await;

        while let Some(frame) = ws_rx.next().await {
            match frame {
                Ok(WsMessage::Text(text)) => {
                    if text.trim().is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Message>(&text) {
                        Ok(msg) => self.router_msg(&worker, &msg).await?,
                        Err(err) => tracing::warn!(peer = %peer, error = ?err, "json decode error"),
                    }
                }
                Ok(WsMessage::Binary(bytes)) => match serde_json::from_slice::<Message>(&bytes) {
                    Ok(msg) => self.router_msg(&worker, &msg).await?,
                    Err(err) => tracing::warn!(peer = %peer, error = ?err, "binary decode error"),
                },
                Ok(WsMessage::Close(_)) => {
                    tracing::debug!(peer = %peer, "Worker disconnected");
                    break;
                }
                Ok(WsMessage::Ping(payload)) => {
                    let _ = tx.send(Outbound::Raw(WsMessage::Pong(payload))).await;
                }
                Ok(WsMessage::Pong(_)) => {}
                Err(_err) => {
                    break;
                }
            }
        }

        writer.abort();
        self.cleanup_worker(&worker).await;
        tracing::debug!(peer = %peer, "Worker disconnected (writer aborted)");
        Ok(())
    }

    async fn cleanup_worker(&self, worker: &Worker) {
        let worker_functions = worker
            .function_paths
            .read()
            .await
            .iter()
            .cloned()
            .collect::<Vec<String>>();

        tracing::debug!(worker_id = %worker.id, functions = ?worker_functions, "Worker registered functions");
        for function_path in worker_functions.iter() {
            self.remove_function(function_path);
            self.service_registry
                .remove_function_from_services(function_path)
                .await;
        }

        let worker_invocations = worker.invocations.read().await;
        tracing::debug!(worker_id = %worker.id, invocations = ?worker_invocations, "Worker invocations");
        for invocation_id in worker_invocations.iter() {
            tracing::debug!(invocation_id = %invocation_id, "Halting invocation");
            self.invocations.halt_invocation(invocation_id);
        }

        self.trigger_registry.unregister_worker(&worker.id).await;
        self.worker_registry.unregister_worker(&worker.id).await;

        tracing::debug!(worker_id = %worker.id, "Worker triggers unregistered");
    }
}

impl EngineTrait for Engine {
    async fn invoke_function(
        &self,
        function_path: &str,
        input: Value,
    ) -> Result<Option<Value>, ErrorBody> {
        let function_opt = self.functions.get(function_path);

        if let Some(function) = function_opt {
            let result = self
                .invocations
                .handle_invocation(None, None, function_path.to_string(), input, function)
                .await;

            match result {
                Ok(result) => result,
                Err(err) => Err(ErrorBody {
                    code: "invocation_error".into(),
                    message: err.to_string(),
                }),
            }
        } else {
            Err(ErrorBody {
                code: "function_not_found".into(),
                message: "Function not found".into(),
            })
        }
    }

    async fn register_trigger_type(&self, trigger_type: TriggerType) {
        let trigger_type_id = &trigger_type.id;
        let existing = self.trigger_registry.trigger_types.read().await;
        if existing.contains_key(trigger_type_id) {
            tracing::warn!(trigger_type_id = %trigger_type_id, "Trigger type already registered");
            return;
        }
        drop(existing);

        let _ = self
            .trigger_registry
            .register_trigger_type(trigger_type)
            .await;
    }

    fn register_function(
        &self,
        request: RegisterFunctionRequest,
        handler: Box<dyn FunctionHandler + Send + Sync>,
    ) {
        let RegisterFunctionRequest {
            function_path,
            description,
            request_format,
            response_format,
        } = request;

        let handler_arc: Arc<dyn FunctionHandler + Send + Sync> = handler.into();
        let handler_function_path = function_path.clone();

        let function = Function {
            handler: Arc::new(move |invocation_id, input| {
                let handler = handler_arc.clone();
                let path = handler_function_path.clone();
                Box::pin(async move { handler.handle_function(invocation_id, path, input).await })
            }),
            _function_path: function_path.clone(),
            _description: description,
            request_format,
            response_format,
        };

        self.functions.register_function(function_path, function);
    }

    fn register_function_handler<H, F>(&self, request: RegisterFunctionRequest, handler: Handler<H>)
    where
        H: Fn(Value) -> F + Send + Sync + 'static,
        F: Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send + 'static,
    {
        let handler_arc: Arc<H> = Arc::new(handler.f);

        let function = Function {
            handler: Arc::new(move |_id, input| {
                let handler = handler_arc.clone();
                Box::pin(async move { handler(input).await })
            }),
            _function_path: request.function_path.clone(),
            _description: request.description,
            request_format: request.request_format,
            response_format: request.response_format,
        };

        self.functions
            .register_function(request.function_path, function);
    }
}
