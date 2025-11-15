use std::{collections::HashSet, net::SocketAddr, sync::Arc};

mod function;
mod invocation;
mod logging;
mod services;
mod trigger;
mod workers;

use crate::{
    function::FunctionHandler,
    invocation::{Invocation, InvocationHandler},
};
use axum::{
    Router,
    extract::{
        ConnectInfo, State,
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use tokio::{
    net::TcpListener,
    sync::{RwLock, mpsc},
};
use uuid::Uuid;

mod protocol;
use protocol::*;

use crate::{
    services::ServicesRegistry,
    trigger::{Trigger, TriggerRegistry, TriggerType},
    workers::{Worker, WorkerRegistry},
};

#[derive(Debug)]
pub enum Outbound {
    Protocol(Message),
    Raw(WsMessage),
}

struct Function {
    handler: Box<
        dyn Fn(
                Option<Uuid>,
                serde_json::Value,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<Option<serde_json::Value>, ErrorBody>>
                        + Send,
                >,
            > + Send
            + Sync,
    >,
    _function_path: String,
    _description: Option<String>,
}

#[derive(Default)]
struct Engine {
    worker_registry: WorkerRegistry,
    functions: Arc<DashMap<String, Function>>,
    trigger_registry: TriggerRegistry,
    service_registry: ServicesRegistry,
    pending_invocations: Arc<RwLock<DashMap<Uuid, Uuid>>>,
}

impl Engine {
    fn new() -> Self {
        Self {
            worker_registry: WorkerRegistry::new(),
            functions: Arc::new(DashMap::new()),
            trigger_registry: TriggerRegistry::new(),
            pending_invocations: Arc::new(RwLock::new(DashMap::new())),
            service_registry: ServicesRegistry::new(),
        }
    }

    async fn send_msg(&self, worker: &Worker, msg: Message) -> bool {
        worker.channel.send(Outbound::Protocol(msg)).await.is_ok()
    }

    fn remove_function(&self, function_path: &str) {
        tracing::info!(function_path = %function_path, "Removing function");
        self.functions.remove(function_path);
    }

    async fn remember_invocation(&self, worker: &Worker, invocation_id: Uuid, function_path: &str) {
        tracing::info!(
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
            .write()
            .await
            .insert(invocation_id, worker.id);
    }

    async fn remove_invocation(&self, invocation_id: &Uuid) {
        tracing::info!(invocation_id = %invocation_id, "Removing invocation");
        self.pending_invocations.write().await.remove(invocation_id);
    }

    async fn halt_invocation(&self, invocation_id: &Uuid) {
        tracing::info!(invocation_id = %invocation_id, "Halting invocation");
        if let Some((_, worker_id)) = self.pending_invocations.write().await.remove(invocation_id)
            && let Some(worker_ref) = self.worker_registry.get_worker(&worker_id).await
        {
            worker_ref.halt_invocation(invocation_id).await;
        }
    }

    async fn notify_new_functions(&self, duration_secs: u64) {
        let mut current_funcion_hash = self.generate_functions_hash().await;
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(duration_secs)).await;
            tracing::info!("Checking for new functions");

            let new_functions: HashSet<String> = self
                .functions
                .iter()
                .map(|entry| entry.key().clone())
                .collect();

            let new_function_hash = self.generate_functions_hash().await;

            if new_function_hash != current_funcion_hash {
                tracing::info!("New functions detected, notifying workers");
                let message = Message::FunctionsAvailable {
                    functions: new_functions.into_iter().collect(),
                };
                self.broadcast_msg(message).await;
                current_funcion_hash = new_function_hash;
            }
        }
    }

    async fn generate_functions_hash(&self) -> String {
        let functions: HashSet<String> = self
            .functions
            .iter()
            .map(|entry| entry.key().clone())
            .collect();

        let mut function_hash = functions.iter().cloned().collect::<Vec<String>>();
        function_hash.sort();
        format!("{:?}", function_hash)
    }

    async fn broadcast_msg(&self, msg: Message) {
        for worker in self.worker_registry.workers.read().await.iter() {
            worker
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
        let (_, worker_id) = self
            .pending_invocations
            .write()
            .await
            .remove(invocation_id)?;
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
                tracing::info!(
                    worker_id = %worker.id,
                    trigger_type_id = %id,
                    description = %description,
                    "RegisterTriggerType"
                );
                self.trigger_registry
                    .register_trigger_type(TriggerType {
                        id: id.clone(),
                        _description: description.clone(),
                        registrator: Box::new(worker.clone()),
                        worker_id: Some(worker.id),
                    })
                    .await?;
                Ok(())
            }
            Message::RegisterTrigger {
                id,
                trigger_type,
                function_path,
                config,
            } => {
                tracing::info!(
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
                tracing::info!(
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
                tracing::info!(
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
                    tracing::info!(function_path = %function_path, "Found function handler");
                    match (function.handler)(*invocation_id, data.clone()).await {
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
                                invocation_id: invocation_id,
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
                tracing::info!(
                    function_path = %function_path,
                    invocation_id = %invocation_id,
                    result = ?result,
                    error = ?error,
                    "InvocationResult"
                );
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
            } => {
                tracing::info!(
                    worker_id = %worker.id,
                    function_path = %function_path,
                    description = ?description,
                    "RegisterFunction"
                );

                let handler_worker = worker.clone();
                self.service_registry
                    .register_service_from_func_path(function_path)
                    .await;

                let handler_function_path = function_path.clone();
                let new_function = Function {
                    handler: Box::new(
                        move |invocation_id: Option<Uuid>, input: serde_json::Value| {
                            let function_path = handler_function_path.clone();
                            let worker = handler_worker.clone();

                            Box::pin(async move {
                                let _ = worker
                                    .handle_function(invocation_id, function_path, input)
                                    .await;
                                Ok(None)
                            })
                        },
                    ),
                    _function_path: function_path.clone(),
                    _description: description.clone(),
                };
                self.functions.insert(function_path.clone(), new_function);
                worker.include_function_path(function_path).await;
                Ok(())
            }
            Message::RegisterService {
                id,
                name,
                description,
            } => {
                tracing::info!(
                    service_id = %id,
                    service_name = %name,
                    description = ?description,
                    "RegisterService"
                );
                {
                    let services = self.service_registry.services.read().await;
                    tracing::info!(services = ?services, "Current services");
                }

                self.service_registry
                    .insert_service(services::Service::new(name.clone(), id.clone()))
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

    async fn handle_worker(
        self: Arc<Self>,
        socket: WebSocket,
        peer: SocketAddr,
    ) -> anyhow::Result<()> {
        tracing::info!(peer = %peer, "Worker connected via WebSocket");
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

        tracing::info!(worker_id = %worker.id, peer = %peer, "Assigned worker ID");
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
                    tracing::info!(peer = %peer, "Worker disconnected");
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
        tracing::info!(peer = %peer, "Worker disconnected (writer aborted)");
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
            self.halt_invocation(&entry.key()).await;
        }

        tracing::info!(worker_id = %worker.id, "Worker triggers unregistered");

        let lock = self.pending_invocations.read().await;
        let invocations = lock
            .iter()
            .map(|entry| entry.key().clone())
            .filter(|invocation_id| {
                if let Some(worker_id) = lock.get(invocation_id) {
                    *worker_id == worker.id
                } else {
                    false
                }
            })
            .collect::<HashSet<Uuid>>();

        // remove pending invocations from this worker
        for invocation_id in invocations {
            self.remove_invocation(&invocation_id).await;
        }
    }
}

async fn ws_handler(
    State(engine): State<Arc<Engine>>,
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        let engine = engine.clone();
        async move {
            if let Err(err) = engine.handle_worker(socket, addr).await {
                tracing::error!(addr = %addr, error = ?err, "worker error");
            }
        }
    })
}

fn register_core_functions(engine: &Arc<Engine>) {
    engine.functions.insert(
        "logger.info".to_string(),
        Function {
            handler: Box::new(
                move |_invocation_id: Option<Uuid>, input: serde_json::Value| {
                    Box::pin(async move {
                        tracing::info!(input = ?input, "logger.info invoked");
                        Ok(None)
                    })
                },
            ),
            _function_path: "logger.info".to_string(),
            _description: Some("Log an info message".to_string()),
        },
    );
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logging::init_tracing();

    let engine = Arc::new(Engine::new());
    register_core_functions(&engine);
    let engine_clone = engine.clone();
    tokio::spawn(async move {
        engine_clone.notify_new_functions(5).await;
    });

    let app = Router::new()
        .route("/", get(ws_handler))
        .with_state(engine.clone());

    let addr = "127.0.0.1:49134";
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(address = addr, "Engine listening");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
