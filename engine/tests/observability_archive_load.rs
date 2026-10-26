// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Opt-in isolated engine/telemetry reproduction; fixed ports are reserved for
//! this diagnostic only. Run with --ignored --nocapture in a clean environment.
use futures_util::SinkExt;
use iii::{
    engine::Engine,
    workers::{
        observability::{
            ObservabilityWorker,
            config::{ObservabilityWorkerConfig, OtelExporterType, TraceStorageConfig},
            otel,
        },
        traits::Worker,
        worker::WorkerManager,
    },
};
use serde_json::json;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio_tungstenite::tungstenite::Message;

const MIB: u64 = 1024 * 1024;

fn directory_bytes(directory: &Path) -> u64 {
    std::fs::read_dir(directory)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|entry| entry.metadata().ok())
                .filter(|metadata| metadata.is_file())
                .map(|metadata| metadata.len())
                .sum()
        })
        .unwrap_or(0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "isolated load on reserved port 49834; takes over a minute"]
async fn telemetry_sampling_disk_cap_and_age_sweep() {
    let directory = tempfile::tempdir().unwrap();
    let config = ObservabilityWorkerConfig {
        enabled: Some(true),
        exporter: Some(OtelExporterType::Memory),
        sampling_ratio: Some(0.1),
        live_spans: Some(true),
        metrics_enabled: Some(true),
        logs_enabled: Some(true),
        logs_console_output: false,
        memory_max_spans: Some(10_000),
        trace_storage: Some(TraceStorageConfig {
            directory: directory.path().to_string_lossy().into_owned(),
            max_disk_bytes: 64 * 1024 * 1024,
            retention_seconds: 2,
            memory_max_bytes: 64 * 1024 * 1024,
            ..Default::default()
        }),
        ..Default::default()
    };
    otel::set_otel_config(config.clone());
    let _layer = otel::init_otel::<tracing_subscriber::Registry>(&otel::OtelConfig::default());
    let engine = Arc::new(Engine::new());
    let observability =
        ObservabilityWorker::create(engine.clone(), Some(serde_json::to_value(&config).unwrap()))
            .await
            .unwrap();
    observability.initialize().await.unwrap();
    let manager = WorkerManager::create(engine, Some(json!({"host":"127.0.0.1", "port":49834})))
        .await
        .unwrap();
    let (stop, shutdown) = tokio::sync::watch::channel(false);
    observability
        .start_background_tasks(shutdown.clone(), stop.clone())
        .await
        .unwrap();
    manager
        .start_background_tasks(shutdown, stop.clone())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (mut ws, _) = tokio_tungstenite::connect_async("ws://127.0.0.1:49834/otel")
        .await
        .unwrap();
    let archive = otel::get_trace_disk_storage().unwrap();
    let hot = otel::get_span_storage().unwrap();
    let mut last_bytes = 0;
    let mut maximum_bytes = 0;
    let mut decreases = 0;
    let load_started = tokio::time::Instant::now();
    // 4000 independent traces with deterministic IDs. Ten percent are kept.
    // At 64 KiB per accepted span this outgrows a 64 MiB archive repeatedly.
    for batch in 0..40_u128 {
        let spans: Vec<_> = (0..100_u128).map(|i| {
            let low = (2 * i + 1) * (1_u128 << 64) / 200;
            let trace = ((batch + 1) << 64) | low;
            json!({"traceId":format!("{trace:032x}"),"spanId":"0000000000000001",
                "name":"network-load","kind":1,"flags":1,
                "startTimeUnixNano":"1700000000000000000", "endTimeUnixNano":"1700000000100000000",
                "attributes":[{"key":"iii.payload.json","value":{"stringValue":"x".repeat(65536)}}]
            })
        }).collect();
        let request = json!({"resourceSpans":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"network-fixture"}}]},"scopeSpans":[{"spans":spans}]}]});
        let mut frame = b"OTLP".to_vec();
        frame.extend(request.to_string().as_bytes());
        ws.send(Message::Binary(frame.into())).await.unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if hot.len() == (batch as usize + 1) * 10 {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "telemetry ingest timed out at batch {batch}: {}",
                hot.len()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let writer = archive.clone();
        tokio::task::spawn_blocking(move || writer.flush())
            .await
            .unwrap()
            .unwrap();
        let bytes = archive.status().physical_bytes;
        if bytes < last_bytes {
            decreases += 1;
        }
        last_bytes = bytes;
        maximum_bytes = maximum_bytes.max(bytes);
        assert!(
            bytes <= 72 * 1024 * 1024,
            "disk cap plus SQLite allocation tolerance"
        );
    }
    let conn = rusqlite::Connection::open(directory.path().join("traces.sqlite3")).unwrap();
    let (count, payload_bytes, attribute_bytes): (i64,i64,i64) = conn.query_row(
        "SELECT (SELECT COUNT(*) FROM spans), (SELECT SUM(length(payload)) FROM spans), (SELECT SUM(length(value)) FROM span_attributes WHERE key='iii.payload.json')", [],
        |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
    let vacuum: i64 = conn
        .query_row("PRAGMA auto_vacuum", [], |r| r.get(0))
        .unwrap();
    drop(conn);
    assert_eq!(hot.len(), 400);
    assert!(
        count < 400 && decreases > 0,
        "sustained ingest must exercise eviction"
    );
    eprintln!(
        "NETWORK_EVIDENCE sent=4000 sampled_hot=400 archived={count} max_bytes={maximum_bytes} current_bytes={last_bytes} decreases={decreases} serialized_bytes={payload_bytes} payload_attribute_bytes={attribute_bytes} auto_vacuum={vacuum}"
    );
    // Do NOT call retain(): prove the worker's real 60-second sweep runs.
    // Watch the directory through the whole sweep: rows disappear before the
    // reclaim that returns their pages finishes, so COUNT=0 is not its end.
    let watching = Arc::new(AtomicBool::new(true));
    let watcher = {
        let watching = watching.clone();
        let sweep_directory = directory.path().to_path_buf();
        std::thread::spawn(move || {
            let mut peak = 0;
            while watching.load(Ordering::Relaxed) {
                peak = peak.max(directory_bytes(&sweep_directory));
                std::thread::sleep(Duration::from_millis(2));
            }
            peak
        })
    };
    // Check durable rows (not the hot cache, which intentionally retains its
    // bounded recent view) before observing the physical database footprint.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(125);
    let archive_rows = loop {
        let conn = rusqlite::Connection::open(directory.path().join("traces.sqlite3")).unwrap();
        let rows = conn
            .query_row("SELECT COUNT(*) FROM spans", [], |row| row.get::<_, i64>(0))
            .unwrap();
        if rows == 0 {
            break rows;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "age sweep failed: {rows} durable rows remain"
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    };
    assert_eq!(archive_rows, 0, "the age sweep removed all durable rows");
    let at_count_zero = archive.status().physical_bytes;
    // The sweep is over when the expired pages are back on the filesystem.
    let reclaim_deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let after = loop {
        let bytes = archive.status().physical_bytes;
        if bytes <= MIB {
            break bytes;
        }
        assert!(
            tokio::time::Instant::now() < reclaim_deadline,
            "expired pages returned to filesystem: {bytes} bytes remain"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    watching.store(false, Ordering::Relaxed);
    let sweep_peak = watcher.join().unwrap();
    eprintln!(
        "AGE_EVIDENCE archived_after={archive_rows} physical_before={last_bytes} physical_at_count_zero={at_count_zero} physical_after_reclaim={after} sweep_peak={sweep_peak} elapsed={:?}",
        load_started.elapsed()
    );
    assert!(
        after < last_bytes / 4,
        "expired pages returned to filesystem ({last_bytes} -> {after})"
    );
    assert!(
        sweep_peak <= 64 * MIB + 8 * MIB,
        "retention transient {sweep_peak} bytes exceeded limit plus tolerance"
    );
    ws.close(None).await.unwrap();
    stop.send(true).unwrap();
    archive.shutdown();
    tokio::time::sleep(Duration::from_millis(200)).await;
}
