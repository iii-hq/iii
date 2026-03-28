mod common;

use std::{net::TcpListener as StdTcpListener, time::Duration};

use criterion::{Criterion, criterion_group, criterion_main};
use futures_util::{SinkExt, StreamExt, stream::SplitSink, stream::SplitStream};
use iii::{EngineBuilder, modules::config::EngineConfig, protocol::Message};
use serde_json::json;
use tempfile::NamedTempFile;
use tokio::{runtime::Runtime, task::JoinHandle, time, time::sleep};
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
        // Retry startup to handle ephemeral port TOCTOU races
        for attempt in 0..3 {
            match Self::try_start().await {
                Ok(runtime) => return runtime,
                Err(e) if attempt < 2 => {
                    eprintln!("bench startup attempt {attempt} failed: {e}, retrying...");
                    sleep(Duration::from_millis(50)).await;
                }
                Err(e) => panic!("bench startup failed after 3 attempts: {e}"),
            }
        }
        unreachable!()
    }

    async fn try_start() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let ws_port = reserve_local_port();
        let config = write_ws_only_config();
        let ws_addr = format!("127.0.0.1:{ws_port}");
        let ws_url = format!("ws://{ws_addr}");
        let config =
            EngineConfig::config_file(&config.path().to_str().expect("config path")).unwrap();

        let builder = EngineBuilder::new()
            // .address(&ws_addr) // TODO before-merge: we need to add the port here
            .with_config(config)
            .build()
            .await?;

        let engine_task = tokio::spawn(async move {
            let _ = builder.serve().await;
        });

        wait_for_ws_server(&ws_url).await;

        // Service worker: registers "bench.echo", echoes InvokeFunction → InvocationResult
        let service_worker_task = tokio::spawn(run_service_worker(ws_url.clone()));

        // Wait for service worker to register its function via a probe round-trip
        wait_for_worker_ready(&ws_url).await;

        Ok(Self {
            ws_url,
            engine_task,
            service_worker_task,
        })
    }

    async fn connect_caller(&self) -> (WsWriter, WsReader) {
        let (socket, _) = connect_async(&self.ws_url)
            .await
            .expect("connect caller websocket");
        let (write, read) = socket.split();

        // Drain WorkerRegistered message
        let mut read = read;
        let write = write;
        time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(Ok(WsMessage::Text(text))) = read.next().await {
                    let msg: Message = serde_json::from_str(&text).expect("decode message");
                    if let Message::WorkerRegistered { .. } = msg {
                        break;
                    }
                }
            }
        })
        .await
        .expect("timed out waiting for WorkerRegistered in connect_caller");

        (write, read)
    }

    async fn shutdown(self) {
        self.service_worker_task.abort();
        self.engine_task.abort();
        let _ = self.service_worker_task.await;
        let _ = self.engine_task.await;
    }
}

/// Reserves an ephemeral port by binding and releasing a listener.
/// NOTE: This has a small TOCTOU race window where another process could claim
/// the port between release and the actual bind. Acceptable for local benchmarks.
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
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if connect_async(ws_url).await.is_ok() {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("ws server did not become ready within 10s");
}

async fn wait_for_worker_ready(ws_url: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if let Ok((mut socket, _)) = connect_async(ws_url).await {
            let probe = Message::InvokeFunction {
                invocation_id: Some(Uuid::new_v4()),
                function_id: "bench.echo".to_string(),
                data: serde_json::json!({"probe": true}),
                traceparent: None,
                baggage: None,
                action: None,
            };
            let _ = socket
                .send(WsMessage::Text(
                    serde_json::to_string(&probe)
                        .expect("serialize probe")
                        .into(),
                ))
                .await;

            if let Some(Ok(WsMessage::Text(text))) = socket.next().await {
                if let Ok(Message::WorkerRegistered { .. }) = serde_json::from_str::<Message>(&text)
                {
                    // Drain WorkerRegistered, then wait for InvocationResult
                    if let Some(Ok(WsMessage::Text(text))) = socket.next().await {
                        if matches!(
                            serde_json::from_str::<Message>(&text),
                            Ok(Message::InvocationResult { .. })
                        ) {
                            return;
                        }
                    }
                } else if matches!(
                    serde_json::from_str::<Message>(&text),
                    Ok(Message::InvocationResult { .. })
                ) {
                    return;
                }
            }
        }
        sleep(Duration::from_millis(25)).await;
    }
    panic!("worker did not become ready within 10s");
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
                invocation: None,
            })
            .expect("serialize RegisterFunction")
            .into(),
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
                                serde_json::to_string(&Message::Pong)
                                    .expect("serialize Pong")
                                    .into(),
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
                        ..
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
                                    .expect("serialize InvocationResult")
                                    .into(),
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
        action: None,
    };
    writer
        .send(WsMessage::Text(
            serde_json::to_string(&msg)
                .expect("serialize InvokeFunction")
                .into(),
        ))
        .await
        .expect("send InvokeFunction");

    // Wait for InvocationResult with matching id
    time::timeout(Duration::from_secs(5), async {
        loop {
            let frame = reader.next().await.expect("read frame").expect("ws frame");
            if let WsMessage::Text(text) = frame {
                let message: Message = serde_json::from_str(&text).expect("decode message");
                match message {
                    Message::InvocationResult {
                        invocation_id: id,
                        error,
                        ..
                    } if id == invocation_id => {
                        assert!(error.is_none(), "InvocationResult returned an error");
                        break;
                    }
                    Message::Ping => {
                        writer
                            .send(WsMessage::Text(
                                serde_json::to_string(&Message::Pong)
                                    .expect("serialize Pong")
                                    .into(),
                            ))
                            .await
                            .expect("send Pong");
                    }
                    _ => {}
                }
            }
        }
    })
    .await
    .expect("timed out waiting for InvocationResult in invoke_and_wait");
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
