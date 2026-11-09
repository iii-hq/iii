//! A channel relaying frames from a writer socket to a reader socket through
//! the engine, as an LLM provider streams its deltas to the router.
//!
//! `small_frame`: one 256-byte frame written and read back per iteration, the
//! size of a streamed delta. `large_frame`: one 1 MiB frame, read by the
//! engine in many socket reads. Both clients read with an 8 KiB buffer so the
//! numbers move with the engine's side only.

use std::{net::TcpListener as StdTcpListener, time::Duration};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use futures_util::{SinkExt, StreamExt};
use iii::EngineBuilder;
use serde_json::json;
use tokio::{net::TcpStream, runtime::Runtime, time::sleep};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, connect_async_with_config,
    tungstenite::{Message, protocol::WebSocketConfig},
};

type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn start_engine() -> (std::sync::Arc<iii::engine::Engine>, String) {
    let port = StdTcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("reserve a port")
        .port();
    let builder = EngineBuilder::new()
        .add_worker(
            "iii-worker-manager",
            Some(json!({ "host": "127.0.0.1", "port": port })),
        )
        .build()
        .await
        .expect("build engine");
    let engine = builder.engine_handle();
    tokio::spawn(async move {
        let _ = builder.serve().await;
    });
    let base = format!("ws://127.0.0.1:{port}");
    for _ in 0..1000 {
        if connect_async(&base).await.is_ok() {
            return (engine, base);
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("engine did not listen on {base}");
}

async fn connect(base: &str, id: &str, key: &str, dir: &str) -> Client {
    let config = WebSocketConfig::default().read_buffer_size(8 * 1024);
    let url = format!("{base}/ws/channels/{id}?key={key}&dir={dir}");
    connect_async_with_config(url, Some(config), false)
        .await
        .expect("connect channel socket")
        .0
}

async fn relay(writer: &mut Client, reader: &mut Client, frame: &[u8]) {
    writer
        .send(Message::Binary(frame.to_vec().into()))
        .await
        .expect("write frame");
    match reader.next().await {
        Some(Ok(Message::Binary(data))) => assert_eq!(data.len(), frame.len()),
        other => panic!("expected the relayed frame, got {other:?}"),
    }
}

fn channel_relay_benchmark(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");
    let (mut writer, mut reader) = rt.block_on(async {
        let (engine, base) = start_engine().await;
        let (write_end, read_end) = engine.channel_manager.create_channel(64, None);
        let writer = connect(&base, &write_end.channel_id, &write_end.access_key, "write").await;
        let reader = connect(&base, &read_end.channel_id, &read_end.access_key, "read").await;
        (writer, reader)
    });

    let mut group = c.benchmark_group("channel_relay");
    for (name, size) in [("small_frame", 256), ("large_frame", 1024 * 1024)] {
        let frame = vec![7u8; size];
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_function(name, |b| {
            b.iter(|| rt.block_on(relay(&mut writer, &mut reader, &frame)))
        });
    }
    group.finish();
}

criterion_group!(benches, channel_relay_benchmark);
criterion_main!(benches);
