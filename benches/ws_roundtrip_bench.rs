mod common;

use std::{net::TcpListener as StdTcpListener, time::Duration};

use criterion::{Criterion, criterion_group, criterion_main};
use futures_util::{SinkExt, StreamExt, stream::SplitSink, stream::SplitStream};
use iii::{EngineBuilder, protocol::Message};
use serde_json::json;
use tempfile::NamedTempFile;
use tokio::{runtime::Runtime, task::JoinHandle, time::sleep};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message as WsMessage,
};
use uuid::Uuid;

type WsWriter = SplitSink<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>, WsMessage>;
type WsReader = SplitStream<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>;

struct WsBenchRuntime {
    ws_url: String,
    engine_task: JoinHandle<()>,
    service_worker_task: JoinHandle<()>,
}

impl WsBenchRuntime {
    async fn start() -> Self {
        let ws_port = reserve_local_port();
        let config = write_ws_only_config();
        let ws_addr = format!("127.0.0.1:{ws_port}");
        let ws_url = format!("ws://{ws_addr}");

        let builder = EngineBuilder::new()
            .address(&ws_addr)
            .config_file_or_default(config.path().to_str().expect("config path"))
            .expect("load config")
            .build()
            .await
            .expect("build engine");

        let engine_task = tokio::spawn(async move {
            let _ = builder.serve().await;
        });

        wait_for_ws_server(&ws_url).await;

        // Service worker: registers "bench.echo", echoes InvokeFunction → InvocationResult
        let service_worker_task = tokio::spawn(run_service_worker(ws_url.clone()));

        // Wait for service worker to register its function
        sleep(Duration::from_millis(100)).await;

        Self {
            ws_url,
            engine_task,
            service_worker_task,
        }
    }

    async fn connect_caller(&self) -> (WsWriter, WsReader) {
        let (socket, _) = connect_async(&self.ws_url)
            .await
            .expect("connect caller websocket");
        let (write, read) = socket.split();

        // Drain WorkerRegistered message
        let mut read = read;
        let write = write;
        loop {
            if let Some(Ok(WsMessage::Text(text))) = read.next().await {
                let msg: Message = serde_json::from_str(&text).expect("decode message");
                if let Message::WorkerRegistered { .. } = msg {
                    break;
                }
            }
        }

        (write, read)
    }

    async fn shutdown(self) {
        self.service_worker_task.abort();
        self.engine_task.abort();
        let _ = self.service_worker_task.await;
        let _ = self.engine_task.await;
    }
}

fn reserve_local_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("listener addr").port();
    drop(listener);
    port
}

fn write_ws_only_config() -> NamedTempFile {
    use std::io::Write;
    let mut file = NamedTempFile::new().expect("create temp config file");
    let yaml = "modules: []\n";
    file.write_all(yaml.as_bytes()).expect("write config");
    file.flush().expect("flush config");
    file
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

async fn run_service_worker(ws_url: String) {
    let (mut socket, _) = connect_async(&ws_url)
        .await
        .expect("connect service worker websocket");

    // Register the echo function
    socket
        .send(WsMessage::Text(
            serde_json::to_string(&Message::RegisterFunction {
                id: "bench.echo".to_string(),
                description: Some("ws roundtrip benchmark echo".to_string()),
                request_format: None,
                response_format: None,
                metadata: None,
            })
            .expect("serialize RegisterFunction"),
        ))
        .await
        .expect("send RegisterFunction");

    // Message loop: echo InvokeFunction → InvocationResult
    while let Some(frame) = socket.next().await {
        let frame = frame.expect("websocket frame");
        match frame {
            WsMessage::Text(text) => {
                let message: Message = serde_json::from_str(&text).expect("decode message");
                match message {
                    Message::WorkerRegistered { .. } => {}
                    Message::Ping => {
                        socket
                            .send(WsMessage::Text(
                                serde_json::to_string(&Message::Pong).expect("serialize Pong"),
                            ))
                            .await
                            .expect("send Pong");
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
                            result: Some(json!({ "status_code": 200, "body": data })),
                            error: None,
                            traceparent,
                            baggage,
                        };
                        socket
                            .send(WsMessage::Text(
                                serde_json::to_string(&response)
                                    .expect("serialize InvocationResult"),
                            ))
                            .await
                            .expect("send InvocationResult");
                    }
                    _ => {}
                }
            }
            WsMessage::Close(_) => break,
            _ => {}
        }
    }
}

/// Send InvokeFunction and wait for matching InvocationResult
async fn invoke_and_wait(
    writer: &mut WsWriter,
    reader: &mut WsReader,
    payload: &serde_json::Value,
) {
    let invocation_id = Uuid::new_v4();
    let msg = Message::InvokeFunction {
        invocation_id: Some(invocation_id),
        function_id: "bench.echo".to_string(),
        data: payload.clone(),
        traceparent: None,
        baggage: None,
    };
    writer
        .send(WsMessage::Text(
            serde_json::to_string(&msg).expect("serialize InvokeFunction"),
        ))
        .await
        .expect("send InvokeFunction");

    // Wait for InvocationResult with matching id
    loop {
        let frame = reader.next().await.expect("read frame").expect("ws frame");
        if let WsMessage::Text(text) = frame {
            let message: Message = serde_json::from_str(&text).expect("decode message");
            match message {
                Message::InvocationResult {
                    invocation_id: id, ..
                } if id == invocation_id => break,
                Message::Ping => {
                    writer
                        .send(WsMessage::Text(
                            serde_json::to_string(&Message::Pong).expect("serialize Pong"),
                        ))
                        .await
                        .expect("send Pong");
                }
                _ => {}
            }
        }
    }
}

fn ws_roundtrip_benchmark(c: &mut Criterion) {
    let rt = Runtime::new().expect("create tokio runtime");
    let runtime = rt.block_on(WsBenchRuntime::start());
    let (writer, reader) = rt.block_on(runtime.connect_caller());
    let payload = common::benchmark_payload();

    let writer = std::cell::RefCell::new(writer);
    let reader = std::cell::RefCell::new(reader);

    // Warmup: verify the round-trip works
    rt.block_on(invoke_and_wait(
        &mut writer.borrow_mut(),
        &mut reader.borrow_mut(),
        &payload,
    ));

    c.bench_function("ws_roundtrip/invoke_echo", |b| {
        b.iter(|| {
            rt.block_on(invoke_and_wait(
                &mut writer.borrow_mut(),
                &mut reader.borrow_mut(),
                &payload,
            ));
        });
    });

    rt.block_on(runtime.shutdown());
}

criterion_group!(benches, ws_roundtrip_benchmark);
criterion_main!(benches);
