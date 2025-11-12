use std::{net::SocketAddr, sync::Arc};

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
use tokio::{net::TcpListener, sync::mpsc};
use uuid::Uuid;

mod protocol;
use protocol::*;

#[derive(Clone)]
struct Worker {
    channel: mpsc::Sender<Outbound>,
}

#[derive(Debug)]
enum Outbound {
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

struct Invocation {
    _invocation_id: Uuid,
    _function_path: String,
    worker: Worker,
}

#[derive(Default)]
struct Engine {
    functions: Arc<DashMap<String, Function>>,
    invocations: Arc<DashMap<Uuid, Invocation>>,
}

impl Engine {
    fn new() -> Self {
        Self {
            functions: Arc::new(DashMap::new()),
            invocations: Arc::new(DashMap::new()),
        }
    }

    async fn send_msg(&self, worker: &Worker, msg: Message) -> bool {
        worker.channel.send(Outbound::Protocol(msg)).await.is_ok()
    }

    async fn router_msg(&self, worker: &Worker, msg: &Message) -> anyhow::Result<()> {
        match msg {
            Message::InvokeFunction {
                invocation_id,
                function_path,
                data,
            } => {
                println!("InvokeFunction {function_path} {invocation_id:?} {data:?}");

                if let Some(function) = self.functions.get(function_path) {
                    if let Ok(result) =
                        (function.handler)(invocation_id.clone(), data.clone()).await
                    {
                        if let Some(invocation_id) = invocation_id
                            && result.is_some()
                        {
                            self.send_msg(
                                worker,
                                Message::InvocationResult {
                                    invocation_id: invocation_id.clone(),
                                    function_path: function_path.clone(),
                                    result: result.clone(),
                                    error: None,
                                },
                            )
                            .await;
                        }
                    }
                } else if let Some(invocation_id) = invocation_id {
                    self.send_msg(
                        worker,
                        Message::InvocationResult {
                            invocation_id: invocation_id.clone(),
                            function_path: function_path.clone(),
                            result: None,
                            error: Some(ErrorBody {
                                code: "function_not_found".into(),
                                message: "Function not found".to_string(),
                            }),
                        },
                    )
                    .await;
                }
                Ok(())
            }
            Message::InvocationResult {
                invocation_id,
                function_path,
                result,
                error,
            } => {
                println!("InvocationResult {function_path} {invocation_id} {result:?} {error:?}");

                if let Some(invocation) = self.invocations.get(invocation_id) {
                    invocation
                        .worker
                        .channel
                        .send(Outbound::Protocol(Message::InvocationResult {
                            invocation_id: *invocation_id,
                            function_path: function_path.clone(),
                            result: result.clone(),
                            error: error.clone(),
                        }))
                        .await?;
                }
                Ok(())
            }
            Message::RegisterFunction {
                function_path,
                description,
            } => {
                println!(
                    "RegisterFunction {function_path} {}",
                    description.as_ref().unwrap_or(&"".to_string())
                );

                let handler_worker = worker.clone();
                let handler_function_path = function_path.clone();

                let new_function = Function {
                    handler: Box::new(
                        move |invocation_id: Option<Uuid>, input: serde_json::Value| {
                            let worker = handler_worker.clone();
                            let function_path = handler_function_path.clone();

                            Box::pin(async move {
                                worker
                                    .channel
                                    .send(Outbound::Protocol(Message::InvokeFunction {
                                        invocation_id,
                                        function_path,
                                        data: input,
                                    }))
                                    .await
                                    .map_err(|err| ErrorBody {
                                        code: "channel_send_failed".into(),
                                        message: err.to_string(),
                                    })?;

                                Ok(None)
                            })
                        },
                    ),
                    _function_path: function_path.clone(),
                    _description: description.clone(),
                };
                self.functions.insert(function_path.clone(), new_function);
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
        println!(">> Worker {peer} connected via WebSocket");
        let (mut ws_tx, mut ws_rx) = socket.split();
        let (tx, mut rx) = mpsc::channel::<Outbound>(64);

        let writer = tokio::spawn(async move {
            while let Some(outbound) = rx.recv().await {
                let send_result = match outbound {
                    Outbound::Protocol(msg) => match serde_json::to_string(&msg) {
                        Ok(payload) => ws_tx.send(WsMessage::Text(payload)).await,
                        Err(err) => {
                            eprintln!("serialize error: {err:?}");
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

        let worker = Worker {
            channel: tx.clone(),
        };

        while let Some(frame) = ws_rx.next().await {
            match frame {
                Ok(WsMessage::Text(text)) => {
                    if text.trim().is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Message>(&text) {
                        Ok(msg) => self.router_msg(&worker, &msg).await?,
                        Err(err) => eprintln!("json decode error from {peer}: {err}"),
                    }
                }
                Ok(WsMessage::Binary(bytes)) => match serde_json::from_slice::<Message>(&bytes) {
                    Ok(msg) => self.router_msg(&worker, &msg).await?,
                    Err(err) => eprintln!("binary decode error from {peer}: {err}"),
                },
                Ok(WsMessage::Close(_)) => {
                    println!("Worker {peer} disconnected");
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
        println!(">> Worker {peer} disconnected");
        Ok(())
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
                eprintln!("worker error: {err:?}");
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
                        println!("logger.info {input:?}");
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
    tracing_subscriber::fmt::init();

    let engine = Arc::new(Engine::new());
    let app = Router::new()
        .route("/", get(ws_handler))
        .with_state(engine.clone());

    register_core_functions(&engine);

    let addr = "127.0.0.1:49134";
    let listener = TcpListener::bind(addr).await?;
    println!("Engine listening on ws://{addr}/");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
