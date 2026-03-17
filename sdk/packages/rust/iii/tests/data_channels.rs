//! Integration tests for data channel streaming between workers.
//!
//! Requires a running III engine. Set III_URL or use ws://localhost:49134 default.

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::Mutex;

use iii_sdk::{IIIError, RegisterFunctionMessage, TriggerRequest};

#[tokio::test]
async fn stream_data_from_sender_to_processor() {
    let iii = common::shared_iii();

    let iii_for_processor = iii.clone();
    iii.register_function(
        RegisterFunctionMessage {
            id: "test.data.processor.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |input: Value| {
            let iii = iii_for_processor.clone();
            async move {
                let label = input["label"].as_str().unwrap_or_default().to_string();

                let refs = iii_sdk::extract_channel_refs(&input);
                let reader_ref = refs
                    .iter()
                    .find(|(k, r)| k == "reader" && matches!(r.direction, iii_sdk::ChannelDirection::Read))
                    .map(|(_, r)| r.clone())
                    .expect("missing reader channel ref");

                let reader = iii_sdk::ChannelReader::new(iii.address(), &reader_ref);
                let raw = reader.read_all().await.map_err(|e| IIIError::Handler(e.to_string()))?;
                let records: Vec<Value> = serde_json::from_slice(&raw)
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                let values: Vec<f64> = records
                    .iter()
                    .filter_map(|r| r["value"].as_f64())
                    .collect();

                let sum: f64 = values.iter().sum();
                let count = values.len();
                let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
                let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

                Ok(json!({
                    "label": label,
                    "messages": [
                        {"type": "stat", "key": "count", "value": count},
                        {"type": "stat", "key": "sum", "value": sum},
                        {"type": "stat", "key": "average", "value": if count > 0 { sum / count as f64 } else { 0.0 }},
                        {"type": "stat", "key": "min", "value": min},
                        {"type": "stat", "key": "max", "value": max},
                    ],
                }))
            }
        },
    );

    let iii_for_sender = iii.clone();
    iii.register_function(
        RegisterFunctionMessage {
            id: "test.data.sender.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |input: Value| {
            let iii = iii_for_sender.clone();
            async move {
                let records = input["records"].clone();
                let channel = iii
                    .create_channel(None)
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                let payload =
                    serde_json::to_vec(&records).map_err(|e| IIIError::Handler(e.to_string()))?;
                channel
                    .writer
                    .write(&payload)
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;
                channel
                    .writer
                    .close()
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                let result = iii
                    .trigger(TriggerRequest {
                        function_id: "test.data.processor.rs".to_string(),
                        payload: json!({
                            "label": "metrics-batch",
                            "reader": channel.reader_ref,
                        }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                Ok(result)
            }
        },
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
        .trigger(TriggerRequest {
            function_id: "test.data.sender.rs".to_string(),
            payload: json!({"records": records}),
            action: None,
            timeout_ms: None,
        })
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

#[tokio::test]
async fn bidirectional_streaming() {
    let iii = common::shared_iii();

    let iii_for_worker = iii.clone();
    iii.register_function(
        RegisterFunctionMessage {
            id: "test.stream.worker.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |input: Value| {
            let iii = iii_for_worker.clone();
            async move {
                let refs = iii_sdk::extract_channel_refs(&input);

                let reader_ref = refs
                    .iter()
                    .find(|(k, r)| {
                        k == "reader" && matches!(r.direction, iii_sdk::ChannelDirection::Read)
                    })
                    .map(|(_, r)| r.clone())
                    .expect("missing reader");

                let writer_ref = refs
                    .iter()
                    .find(|(k, r)| {
                        k == "writer" && matches!(r.direction, iii_sdk::ChannelDirection::Write)
                    })
                    .map(|(_, r)| r.clone())
                    .expect("missing writer");

                let reader = iii_sdk::ChannelReader::new(iii.address(), &reader_ref);
                let writer = iii_sdk::ChannelWriter::new(iii.address(), &writer_ref);

                let mut chunks: Vec<Vec<u8>> = Vec::new();
                let mut chunk_count = 0;

                while let Some(chunk) = reader
                    .next_binary()
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?
                {
                    chunks.push(chunk);
                    chunk_count += 1;
                    writer
                        .send_message(
                            &serde_json::to_string(&json!({
                                "type": "progress",
                                "chunks_received": chunk_count,
                            }))
                            .unwrap(),
                        )
                        .await
                        .map_err(|e| IIIError::Handler(e.to_string()))?;
                }

                let full_data: Vec<u8> = chunks.iter().flatten().copied().collect();
                let text = String::from_utf8_lossy(&full_data);
                let words: Vec<&str> = text.split_whitespace().collect();

                writer
                    .send_message(
                        &serde_json::to_string(&json!({
                            "type": "complete",
                            "word_count": words.len(),
                            "byte_count": full_data.len(),
                        }))
                        .unwrap(),
                    )
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                let result_json = serde_json::to_vec(&json!({
                    "words": &words[..5.min(words.len())],
                    "total": words.len(),
                }))
                .map_err(|e| IIIError::Handler(e.to_string()))?;
                writer
                    .write(&result_json)
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;
                writer
                    .close()
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                Ok(json!({"status": "done"}))
            }
        },
    );

    let iii_for_coord = iii.clone();
    iii.register_function(
        RegisterFunctionMessage {
            id: "test.stream.coordinator.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |input: Value| {
            let iii = iii_for_coord.clone();
            async move {
                let text = input["text"].as_str().unwrap_or_default().to_string();
                let chunk_size = input["chunkSize"].as_u64().unwrap_or(10) as usize;

                let input_channel = iii
                    .create_channel(None)
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;
                let output_channel = iii
                    .create_channel(None)
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                let messages: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
                let msgs_clone = messages.clone();
                output_channel
                    .reader
                    .on_message(move |msg| {
                        if let Ok(parsed) = serde_json::from_str::<Value>(&msg) {
                            let msgs = msgs_clone.clone();
                            tokio::spawn(async move {
                                msgs.lock().await.push(parsed);
                            });
                        }
                    })
                    .await;

                let text_bytes = text.as_bytes().to_vec();
                let writer = input_channel.writer;
                let write_handle = tokio::spawn(async move {
                    let mut offset = 0;
                    while offset < text_bytes.len() {
                        let end = (offset + chunk_size).min(text_bytes.len());
                        writer
                            .write(&text_bytes[offset..end])
                            .await
                            .expect("channel write failed");
                        offset = end;
                    }
                    writer.close().await.expect("channel close failed");
                });

                let call_handle = {
                    let iii = iii.clone();
                    let reader_ref = input_channel.reader_ref.clone();
                    let writer_ref = output_channel.writer_ref.clone();
                    tokio::spawn(async move {
                        iii.trigger(TriggerRequest {
                            function_id: "test.stream.worker.rs".to_string(),
                            payload: json!({
                                "reader": reader_ref,
                                "writer": writer_ref,
                            }),
                            action: None,
                            timeout_ms: None,
                        })
                        .await
                    })
                };

                let result_data = output_channel
                    .reader
                    .read_all()
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                write_handle
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;
                let worker_result = call_handle
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                let binary_result: Value = serde_json::from_slice(&result_data)
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                // Give a moment for async message callbacks to settle
                tokio::time::sleep(Duration::from_millis(100)).await;
                let collected_messages = messages.lock().await.clone();

                Ok(json!({
                    "messages": collected_messages,
                    "binaryResult": binary_result,
                    "workerResult": worker_result,
                }))
            }
        },
    );

    common::settle().await;

    let text = "The quick brown fox jumps over the lazy dog and then runs around the park";

    let result = iii
        .trigger(TriggerRequest {
            function_id: "test.stream.coordinator.rs".to_string(),
            payload: json!({
                "text": text,
                "chunkSize": 10,
            }),
            action: None,
            timeout_ms: None,
        })
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
