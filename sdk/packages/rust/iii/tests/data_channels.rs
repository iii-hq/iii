//! Integration tests for data streaming between functions with call-native
//! streams: an `InputStream` or `OutputStream` in a payload crosses an engine
//! pipe (`Dispatch::Remote`).
//!
//! Only the shared client lives in this binary, so the streams the test
//! bodies receive bind to it lazily, with no `bind` call.
//!
//! Requires a running III engine. Set III_URL, or use the default.

mod common;

use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt};
use serde::Deserialize;
use serde_json::{Value, json};

use iii_sdk::protocol::{ChannelItem, STREAM_ABORTED};
use iii_sdk::{CallRequest, Dispatch, Error, InputStream, OutputStream, Pipe, RegisterFunction};

#[derive(Deserialize, schemars::JsonSchema)]
struct Batch {
    label: String,
    reader: InputStream,
}

#[tokio::test]
async fn stream_data_from_sender_to_processor() {
    let iii = common::shared_iii();

    iii.register_function(
        "test::data::processor::rs",
        RegisterFunction::new_async(|batch: Batch| async move {
            let raw = batch.reader.read_all().await?;
            let records: Vec<Value> = serde_json::from_slice(&raw)?;

            let values: Vec<f64> = records
                .iter()
                .filter_map(|r| r["value"].as_f64())
                .collect();

            let sum: f64 = values.iter().sum();
            let count = values.len();
            let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

            Ok::<_, Error>(json!({
                "label": batch.label,
                "messages": [
                    {"type": "stat", "key": "count", "value": count},
                    {"type": "stat", "key": "sum", "value": sum},
                    {"type": "stat", "key": "average", "value": if count > 0 { sum / count as f64 } else { 0.0 }},
                    {"type": "stat", "key": "min", "value": min},
                    {"type": "stat", "key": "max", "value": max},
                ],
            }))
        }),
    );

    let iii_for_sender = iii.clone();
    iii.register_function(
        "test::data::sender::rs",
        RegisterFunction::new_async(move |input: Value| {
            let iii = iii_for_sender.clone();
            async move {
                let payload = serde_json::to_vec(&input["records"])?;
                iii.call(
                    CallRequest::new(
                        "test::data::processor::rs",
                        json!({
                            "label": "metrics-batch",
                            "reader": InputStream::from_bytes(payload),
                        }),
                    )
                    .dispatch(Dispatch::Remote),
                )
                .await
            }
        }),
    );

    common::settle().await;

    let records = json!([
        {"name": "cpu_usage", "value": 72},
        {"name": "memory_mb", "value": 2048},
        {"name": "disk_iops", "value": 340},
        {"name": "network_mbps", "value": 95},
        {"name": "latency_ms", "value": 12},
    ]);

    let result = iii
        .call(CallRequest::new(
            "test::data::sender::rs",
            json!({"records": records}),
        ))
        .await
        .expect("call failed");

    assert_eq!(result["label"], "metrics-batch");

    let messages = result["messages"]
        .as_array()
        .expect("messages should be array");
    assert_eq!(messages.len(), 5);

    let stats: std::collections::HashMap<String, f64> = messages
        .iter()
        .map(|m| {
            (
                m["key"].as_str().unwrap().to_string(),
                m["value"].as_f64().unwrap(),
            )
        })
        .collect();

    assert_eq!(stats["count"], 5.0);
    assert_eq!(stats["sum"], 2567.0);
    assert!((stats["average"] - 513.4).abs() < 0.01);
    assert_eq!(stats["min"], 12.0);
    assert_eq!(stats["max"], 2048.0);
}

#[derive(Deserialize, schemars::JsonSchema)]
struct Duplex {
    reader: InputStream,
    writer: OutputStream,
}

#[tokio::test]
async fn bidirectional_streaming() {
    let iii = common::shared_iii();

    iii.register_function(
        "test::stream::worker::rs",
        RegisterFunction::new_async(|duplex: Duplex| async move {
            let Duplex { mut reader, writer } = duplex;
            let mut full_data = Vec::new();
            let mut chunk_count = 0;
            while let Some(chunk) = reader.next().await {
                full_data.extend_from_slice(&chunk?);
                chunk_count += 1;
                writer
                    .send(&json!({ "type": "progress", "chunks_received": chunk_count }))
                    .await?;
            }

            let text = String::from_utf8_lossy(&full_data);
            let words: Vec<&str> = text.split_whitespace().collect();
            writer
                .send(&json!({
                    "type": "complete",
                    "word_count": words.len(),
                    "byte_count": full_data.len(),
                }))
                .await?;
            let result_json = serde_json::to_vec(&json!({
                "words": &words[..5.min(words.len())],
                "total": words.len(),
            }))?;
            writer.write(&result_json).await?;
            writer.close().await?;

            Ok::<_, Error>(json!({"status": "done"}))
        }),
    );

    let iii_for_coord = iii.clone();
    iii.register_function(
        "test::stream::coordinator::rs",
        RegisterFunction::new_async(move |input: Value| {
            let iii = iii_for_coord.clone();
            async move {
                let text = input["text"].as_str().unwrap_or_default().to_string();
                let chunk_size = input["chunkSize"].as_u64().unwrap_or(10) as usize;
                let chunks: Vec<Result<Bytes, std::io::Error>> = text
                    .as_bytes()
                    .chunks(chunk_size)
                    .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
                    .collect();

                let Pipe {
                    input: output_reader,
                    output,
                } = iii.pipe().await?;
                let call = iii.call(
                    CallRequest::new(
                        "test::stream::worker::rs",
                        json!({
                            "reader": InputStream::from_stream(futures_util::stream::iter(chunks)),
                            "writer": output,
                        }),
                    )
                    .dispatch(Dispatch::Remote),
                );
                let frames = output_reader.frames().try_collect::<Vec<ChannelItem>>();
                let (worker_result, frames) = tokio::try_join!(call, frames)?;

                let mut messages = Vec::new();
                let mut binary = Vec::new();
                for frame in frames {
                    match frame {
                        ChannelItem::Text(text) => {
                            messages.push(serde_json::from_str::<Value>(&text)?)
                        }
                        ChannelItem::Binary(data) => binary.extend(data),
                    }
                }
                let binary_result: Value = serde_json::from_slice(&binary)?;

                Ok::<_, Error>(json!({
                    "messages": messages,
                    "binaryResult": binary_result,
                    "workerResult": worker_result,
                }))
            }
        }),
    );

    common::settle().await;

    let text = "The quick brown fox jumps over the lazy dog and then runs around the park";

    let result = iii
        .call(CallRequest::new(
            "test::stream::coordinator::rs",
            json!({
                "text": text,
                "chunkSize": 10,
            }),
        ))
        .await
        .expect("call failed");

    let messages = result["messages"].as_array().expect("messages array");
    let progress: Vec<&Value> = messages
        .iter()
        .filter(|m| m["type"] == "progress")
        .collect();
    let complete = messages.iter().find(|m| m["type"] == "complete");

    assert!(!progress.is_empty(), "should have progress messages");
    assert!(complete.is_some(), "should have complete message");

    let word_count: usize = text.split_whitespace().count();
    assert_eq!(complete.unwrap()["word_count"], word_count);

    assert_eq!(result["binaryResult"]["total"], word_count);
    let words = result["binaryResult"]["words"].as_array().unwrap();
    assert_eq!(words.len(), 5);
    assert_eq!(words, &["The", "quick", "brown", "fox", "jumps"]);

    assert_eq!(result["workerResult"]["status"], "done");
}

/// A writer dropped without `close()` (a crash, a cancelled task) cuts the
/// stream: the reader gets the data sent before it, then `STREAM_ABORTED`,
/// never a clean end.
#[tokio::test]
async fn reader_fails_when_the_writer_drops_without_close() {
    let iii = common::shared_iii();
    let Pipe { input, output } = iii.pipe().await.expect("create pipe");
    output.write(b"partial").await.expect("write");
    drop(output);

    let mut input = input;
    let mut read = Vec::new();
    let error = loop {
        match input.next().await {
            Some(Ok(chunk)) => read.extend_from_slice(&chunk),
            None => panic!("a cut stream must not end cleanly"),
            Some(Err(error)) => break error,
        }
    };
    assert_eq!(read, b"partial");
    assert_eq!(error.code(), Some(STREAM_ABORTED), "{error}");
}

/// A stream a function returns reaches a caller outside any handler, which
/// reads it with no `bind`: it binds to the only client alive.
#[tokio::test]
async fn a_returned_stream_binds_lazily_to_the_only_client() {
    let iii = common::shared_iii();
    iii.register_function(
        "test::stream::export::rs",
        RegisterFunction::new_async(|_: Value| async move {
            Ok::<_, Error>(json!({ "csv": InputStream::from_bytes(&b"a,b\n1,2\n"[..]) }))
        }),
    );
    common::settle().await;

    let result = iii
        .call(CallRequest::new("test::stream::export::rs", json!({})).dispatch(Dispatch::Remote))
        .await
        .expect("call failed");
    let csv: InputStream = serde_json::from_value(result["csv"].clone()).unwrap();
    let lines: Vec<String> = csv.lines().try_collect().await.unwrap();
    assert_eq!(lines, ["a,b", "1,2"]);
}
