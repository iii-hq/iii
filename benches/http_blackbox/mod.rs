use std::{net::TcpListener as StdTcpListener, time::Duration};

use futures_util::{SinkExt, StreamExt};
use iii::{EngineBuilder, protocol::Message};
use reqwest::Client;
use serde_json::json;
use tokio::{sync::mpsc, task::JoinHandle, time::sleep};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
use uuid::Uuid;

use crate::common;

pub struct BenchRuntime {
    pub base_http_url: String,
    pub client: Client,
    engine_task: JoinHandle<()>,
    worker_task: JoinHandle<()>,
}

impl BenchRuntime {
    pub async fn start(route_count: usize) -> Self {
        let ws_port = reserve_local_port();
        let http_port = reserve_local_port();

        let ws_addr = format!("127.0.0.1:{ws_port}");
        let ws_url = format!("ws://{ws_addr}");
        let base_http_url = format!("http://127.0.0.1:{http_port}");

        let builder = EngineBuilder::new()
            .address(&ws_addr)
            .add_module(
                "modules::api::RestApiModule",
                Some(json!({
                    "host": "127.0.0.1",
                    "port": http_port,
                    "default_timeout": 120000,
                })),
            )
            .build()
            .await
            .expect("build engine");

        let engine_task = tokio::spawn(async move {
            let _ = builder.serve().await;
        });

        wait_for_ws_server(&ws_url).await;
        let worker_task = tokio::spawn(run_worker(ws_url.clone(), route_count));

        let client = Client::builder().build().expect("build reqwest client");
        let ready_path = common::http_api_path(route_count.saturating_sub(1));
        wait_for_route(&client, &base_http_url, &ready_path).await;

        Self {
            base_http_url,
            client,
            engine_task,
            worker_task,
        }
    }

    pub async fn post_json(&self, path: &str, body: &serde_json::Value) -> reqwest::Response {
        self.client
            .post(format!("{}/{}", self.base_http_url, path))
            .json(body)
            .send()
            .await
            .expect("send http request")
    }

    pub async fn shutdown(self) {
        self.worker_task.abort();
        self.engine_task.abort();
        let _ = self.worker_task.await;
        let _ = self.engine_task.await;
    }
}

fn reserve_local_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("listener addr").port();
    drop(listener);
    port
}

async fn wait_for_ws_server(ws_url: &str) {
    for _ in 0..100 {
        if connect_async(ws_url).await.is_ok() {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("ws server did not become ready");
}

async fn wait_for_route(client: &Client, base_http_url: &str, path: &str) {
    for _ in 0..100 {
        let response = client
            .post(format!("{base_http_url}/{path}"))
            .json(&common::http_request_body())
            .send()
            .await;

        if let Ok(response) = response
            && response.status().is_success()
        {
            return;
        }

        sleep(Duration::from_millis(10)).await;
    }
    panic!("http route did not become ready");
}

async fn run_worker(ws_url: String, route_count: usize) {
    let (socket, _) = connect_async(&ws_url)
        .await
        .expect("connect worker websocket");
    let (mut ws_write, mut ws_read) = socket.split();

    // Register functions and triggers using a temporary combined socket via the writer
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<WsMessage>();

    for idx in 0..route_count {
        let function_id = common::http_function_id(idx);
        let route_id = format!("bench-route-{idx}");

        ws_write
            .send(WsMessage::Text(
                serde_json::to_string(&Message::RegisterFunction {
                    id: function_id.clone(),
                    description: Some("http bench worker function".to_string()),
                    request_format: None,
                    response_format: None,
                    metadata: None,
                })
                .expect("serialize RegisterFunction"),
            ))
            .await
            .expect("send RegisterFunction");

        ws_write
            .send(WsMessage::Text(
                serde_json::to_string(&Message::RegisterTrigger {
                    id: route_id,
                    trigger_type: "http".to_string(),
                    function_id: function_id.clone(),
                    config: json!({
                        "api_path": common::http_api_path(idx),
                        "http_method": "POST",
                    }),
                })
                .expect("serialize RegisterTrigger"),
            ))
            .await
            .expect("send RegisterTrigger");
    }

    // Spawn writer task that drains the outbound channel
    let writer_task = tokio::spawn(async move {
        while let Some(msg) = outbound_rx.recv().await {
            if ws_write.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Reader loop: process incoming messages and send responses via channel
    while let Some(frame) = ws_read.next().await {
        let frame = match frame {
            Ok(f) => f,
            Err(_) => break,
        };
        match frame {
            WsMessage::Text(text) => {
                let message: Message = serde_json::from_str(&text).expect("decode message");
                match message {
                    Message::WorkerRegistered { .. } => {}
                    Message::Ping => {
                        let _ = outbound_tx.send(WsMessage::Text(
                            serde_json::to_string(&Message::Pong).expect("serialize Pong"),
                        ));
                    }
                    Message::InvokeFunction {
                        invocation_id,
                        function_id,
                        data,
                        traceparent,
                        baggage,
                    } => {
                        let invocation_id = invocation_id.unwrap_or_else(Uuid::new_v4);
                        let response = Message::InvocationResult {
                            invocation_id,
                            function_id,
                            result: Some(json!({
                                "status_code": 200,
                                "body": data,
                            })),
                            error: None,
                            traceparent,
                            baggage,
                        };

                        let _ = outbound_tx.send(WsMessage::Text(
                            serde_json::to_string(&response).expect("serialize InvocationResult"),
                        ));
                    }
                    _ => {}
                }
            }
            WsMessage::Close(_) => break,
            _ => {}
        }
    }

    drop(outbound_tx);
    let _ = writer_task.await;
}
