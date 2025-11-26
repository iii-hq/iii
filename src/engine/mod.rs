use std::{collections::HashSet, net::SocketAddr, sync::Arc};

use axum::extract::ws::{Message as WsMessage, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    function::{Function, FunctionHandler, FunctionsRegistry},
    invocation::{Invocation, InvocationHandler, NonWorkerInvocations},
    modules::logger::{LogLevel, log},
    pending_invocations::PendingInvocations,
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

pub trait EngineTrait: Send + Sync {
    fn invoke_function(&self, function_path: &str, input: Value);
    async fn register_trigger_type(&self, trigger_type: TriggerType);
    fn register_function(
        &self,
        request: RegisterFunctionRequest,
        handler: Box<dyn FunctionHandler + Send + Sync>,
    );
}

#[derive(Default, Clone)]
pub struct Engine {
    pub worker_registry: Arc<WorkerRegistry>,
    pub functions: Arc<FunctionsRegistry>,
    pub trigger_registry: Arc<TriggerRegistry>,
    pub service_registry: Arc<ServicesRegistry>,
    pub pending_invocations: Arc<PendingInvocations>,
    pub non_worker_invocations: Arc<NonWorkerInvocations>,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            worker_registry: Arc::new(WorkerRegistry::new()),
            functions: Arc::new(FunctionsRegistry::new()),
            trigger_registry: Arc::new(TriggerRegistry::new()),
            pending_invocations: Arc::new(PendingInvocations::new()),
            service_registry: Arc::new(ServicesRegistry::new()),
            non_worker_invocations: Arc::new(NonWorkerInvocations::new()),
        }
    }

    async fn send_msg(&self, worker: &Worker, msg: Message) -> bool {
        worker.channel.send(Outbound::Protocol(msg)).await.is_ok()
    }

    fn remove_function(&self, function_path: &str) {
        tracing::debug!(function_path = %function_path, "Removing function");
        self.functions.remove(function_path);
    }

    async fn remember_invocation(&self, worker: &Worker, invocation_id: Uuid, function_path: &str) {
        tracing::debug!(
            worker_id = %worker.id,
            %invocation_id,
            function_path = function_path,
            "Remembering invocation for worker"
        );
        worker
            .add_invocation(Invocation {
                invocation_id,
                function_path: function_path.to_string(),
            })
            .await;
        self.pending_invocations
            .insert(invocation_id, worker.id)
            .await;
    }

    async fn remove_invocation(&self, invocation_id: &Uuid) {
        tracing::debug!(invocation_id = %invocation_id, "Removing invocation");
        let _ = self.pending_invocations.remove(invocation_id).await;
    }

    async fn halt_invocation(&self, invocation_id: &Uuid) {
        tracing::debug!(invocation_id = %invocation_id, "Halting invocation");
        if let Some(worker_id) = self.pending_invocations.remove(invocation_id).await
            && let Some(worker_ref) = self.worker_registry.get_worker(&worker_id).await
        {
            worker_ref.halt_invocation(invocation_id).await;
        }
    }

    pub async fn notify_new_functions(&self, duration_secs: u64) {
        let mut current_funcion_hash = self.functions.functions_hash();

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(duration_secs)).await;
            let new_function_hash = self.functions.functions_hash();
            if new_function_hash != current_funcion_hash {
                log(
                    LogLevel::Info,
                    "core::Engine",
                    "New functions detected, notifying workers",
                    None,
                    None,
                );
                let message: Vec<FunctionMessage> = self
                    .functions
                    .iter()
                    .map(|entry| FunctionMessage::from(entry.value()))
                    .collect();
                let message = Message::FunctionsAvailable { functions: message };
                self.broadcast_msg(message).await;
                current_funcion_hash = new_function_hash;
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

    async fn take_invocation_sender(
        &self,
        invocation_id: &Uuid,
    ) -> Option<(mpsc::Sender<Outbound>, Invocation)> {
        let worker_id = self.pending_invocations.remove(invocation_id).await?;
        let worker_ref = self.worker_registry.get_worker(&worker_id).await?;
        let sender = worker_ref.channel.clone();
        let invocation = worker_ref
            .invocations
            .write()
            .await
            .remove(invocation_id)?
            .1;
        Some((sender, invocation))
    }

    async fn router_msg(&self, worker: &Worker, msg: &Message) -> anyhow::Result<()> {
        match msg {
            Message::FunctionsAvailable { functions } => {
                println!("FunctionsAvailable {:?}", functions);
                Ok(())
            }
            Message::TriggerRegistrationResult {
                id,
                trigger_type,
                function_path,
                error,
            } => {
                println!("TriggerRegistrationResult {id} {trigger_type} {function_path} {error:?}");
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

                if let Some(id) = *invocation_id {
                    self.remember_invocation(worker, id, function_path).await;
                }

                if let Some(function) = self.functions.get(function_path) {
                    tracing::debug!(function_path = %function_path, "Found function handler");

                    match function.call_handler(*invocation_id, data.clone()).await {
                        Ok(Some(result)) => {
                            if let Some(invocation_id) = *invocation_id {
                                let _ = worker
                                    .handle_invocation_result(
                                        Invocation {
                                            invocation_id,
                                            function_path: function_path.clone(),
                                        },
                                        Some(result),
                                        None,
                                    )
                                    .await;
                                self.remove_invocation(&invocation_id).await;
                            } else {
                                tracing::warn!(
                                    function_path = %function_path,
                                    "Invocation returned result without id"
                                );
                            }
                        }
                        Ok(None) => {}
                        Err(error_body) => {
                            if let Some(invocation_id) = *invocation_id {
                                let _ = worker
                                    .handle_invocation_result(
                                        Invocation {
                                            invocation_id,
                                            function_path: function_path.clone(),
                                        },
                                        None,
                                        Some(error_body.clone()),
                                    )
                                    .await;
                                self.remove_invocation(&invocation_id).await;
                            } else {
                                tracing::error!(
                                    function_path = %function_path,
                                    error = ?error_body,
                                    "Invocation failed without id"
                                );
                            }
                        }
                    }
                } else if let Some(invocation_id) = *invocation_id {
                    let _ = worker
                        .handle_invocation_result(
                            Invocation {
                                invocation_id,
                                function_path: function_path.clone(),
                            },
                            None,
                            Some(ErrorBody {
                                code: "function_not_found".into(),
                                message: "Function not found".to_string(),
                            }),
                        )
                        .await;
                    self.remove_invocation(&invocation_id).await;
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

                if let Some(sender) = self.non_worker_invocations.remove(invocation_id) {
                    let payload = if let Some(err) = error {
                        Err(err.clone())
                    } else {
                        Ok(result.clone())
                    };
                    let _ = sender.send(payload);
                    return Ok(());
                }

                if let Some((caller, _invocation)) =
                    self.take_invocation_sender(invocation_id).await
                {
                    caller
                        .send(Outbound::Protocol(Message::InvocationResult {
                            invocation_id: *invocation_id,
                            function_path: function_path.clone(),
                            result: result.clone(),
                            error: error.clone(),
                        }))
                        .await?;
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
                        Ok(payload) => ws_tx.send(WsMessage::Text(payload)).await,
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
        for function_path in worker
            .function_paths
            .read()
            .await
            .iter()
            .cloned()
            .collect::<HashSet<String>>()
        {
            self.remove_function(&function_path);
        }

        self.trigger_registry.unregister_worker(&worker.id).await;
        self.worker_registry.unregister_worker(&worker.id).await;

        let worker_invocations = worker.invocations.read().await;

        for entry in worker_invocations.iter() {
            self.halt_invocation(entry.key()).await;
        }

        tracing::debug!(worker_id = %worker.id, "Worker triggers unregistered");

        let invocations = self
            .pending_invocations
            .invocations_for_worker(&worker.id)
            .await;

        // remove pending invocations from this worker
        for invocation_id in invocations {
            self.remove_invocation(&invocation_id).await;
        }
    }
}

impl EngineTrait for Engine {
    fn invoke_function(&self, function_path: &str, input: Value) {
        let function_opt = self.functions.get(function_path);
        if let Some(function) = function_opt {
            let future = (function.handler)(None, input);
            let function_path = function_path.to_string();

            tokio::spawn(async move {
                tracing::debug!(function_path = %function_path, "Invoking function");

                let result = future.await;
                tracing::debug!(result = ?result, "Function result");
            });
        } else {
            tracing::warn!(function_path = %function_path, "Function not found");
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
}
