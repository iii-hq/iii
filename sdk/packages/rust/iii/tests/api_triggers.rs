//! Integration tests for HTTP API trigger endpoints.
//!
//! Requires a running III engine. Set III_URL and III_HTTP_URL, or use localhost:49134 defaults.

mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::Mutex;

use iii_sdk::{IIIError, RegisterFunctionMessage, RegisterTriggerInput};
use tokio::time::sleep;

fn test_pdf_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("test-assets")
        .join("handbook.pdf")
}

#[tokio::test]
async fn get_endpoint() {
    let iii = common::shared_iii();

    iii.register_function(
        RegisterFunctionMessage {
            id: "test.api.get.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        |_input: Value| async move {
            Ok(json!({
                "status_code": 200,
                "body": {"message": "Hello from GET"},
            }))
        },
    );

    iii.register_trigger(RegisterTriggerInput {
        trigger_type: "http".to_string(),
        function_id: "test.api.get.rs".to_string(),
        config: json!({
            "api_path": "test/rs/hello",
            "http_method": "GET",
        }),
    })
    .expect("register trigger");

    common::settle().await;

    sleep(Duration::from_millis(500)).await;

    let resp = common::http_client()
        .get(format!("{}/test/rs/hello", common::engine_http_url()))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status().as_u16(), 200);
    let data: Value = resp.json().await.expect("json parse");
    assert_eq!(data["message"], "Hello from GET");
}

#[tokio::test]
async fn post_endpoint_with_body() {
    let iii = common::shared_iii();

    iii.register_function(
        RegisterFunctionMessage {
            id: "test.api.post.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        |input: Value| async move {
            let body = input.get("body").cloned().unwrap_or(Value::Null);
            Ok(json!({
                "status_code": 201,
                "body": {"received": body, "created": true},
            }))
        },
    );

    let _trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "http".to_string(),
            function_id: "test.api.post.rs".to_string(),
            config: json!({
                "api_path": "test/rs/items",
                "http_method": "POST",
            }),
        })
        .expect("register trigger");

    common::settle().await;
    sleep(Duration::from_millis(500)).await;

    let resp = common::http_client()
        .post(format!("{}/test/rs/items", common::engine_http_url()))
        .json(&json!({"name": "test item", "value": 123}))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status().as_u16(), 201);
    let data: Value = resp.json().await.expect("json parse");
    assert_eq!(data["created"], true);
    assert_eq!(data["received"]["name"], "test item");
}

#[tokio::test]
async fn path_parameters() {
    let iii = common::shared_iii();

    iii.register_function(
        RegisterFunctionMessage {
            id: "test.api.getbyid.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        |input: Value| async move {
            let id = input
                .get("path_params")
                .and_then(|p| p.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            Ok(json!({"status_code": 200, "body": {"id": id}}))
        },
    );

    let _trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "http".to_string(),
            function_id: "test.api.getbyid.rs".to_string(),
            config: json!({
                "api_path": "test/rs/items/:id",
                "http_method": "GET",
            }),
        })
        .expect("register trigger");

    common::settle().await;
    sleep(Duration::from_millis(500)).await;

    let resp = common::http_client()
        .get(format!(
            "{}/test/rs/items/abc123",
            common::engine_http_url()
        ))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status().as_u16(), 200);
    let data: Value = resp.json().await.expect("json parse");
    assert_eq!(data["id"], "abc123");
}

#[tokio::test]
async fn query_parameters() {
    let iii = common::shared_iii();

    iii.register_function(
        RegisterFunctionMessage {
            id: "test.api.search.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        |input: Value| async move {
            let qp = input.get("query_params").cloned().unwrap_or(json!({}));
            let q = qp.get("q").and_then(|v| v.as_str()).unwrap_or_default();
            let limit = qp.get("limit").and_then(|v| v.as_str()).unwrap_or_default();
            Ok(json!({"status_code": 200, "body": {"query": q, "limit": limit}}))
        },
    );

    let _trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "http".to_string(),
            function_id: "test.api.search.rs".to_string(),
            config: json!({
                "api_path": "test/rs/search",
                "http_method": "GET",
            }),
        })
        .expect("register trigger");

    common::settle().await;
    sleep(Duration::from_millis(500)).await;

    let resp = common::http_client()
        .get(format!(
            "{}/test/rs/search?q=hello&limit=10",
            common::engine_http_url()
        ))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status().as_u16(), 200);
    let data: Value = resp.json().await.expect("json parse");
    assert_eq!(data["query"], "hello");
    assert_eq!(data["limit"], "10");
}

#[tokio::test]
async fn custom_status_code() {
    let iii = common::shared_iii();

    iii.register_function(
        RegisterFunctionMessage {
            id: "test.api.notfound.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        |_input: Value| async move {
        Ok(json!({"status_code": 404, "body": {"error": "Not found"}}))
    });

    let _trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "http".to_string(),
            function_id: "test.api.notfound.rs".to_string(),
            config: json!({
                "api_path": "test/rs/missing",
                "http_method": "GET",
            }),
        })
        .expect("register trigger");

    common::settle().await;
    sleep(Duration::from_millis(500)).await;

    let resp = common::http_client()
        .get(format!("{}/test/rs/missing", common::engine_http_url()))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status().as_u16(), 404);
    let data: Value = resp.json().await.expect("json parse");
    assert_eq!(data, json!({"error": "Not found"}));
}

#[tokio::test]
async fn download_pdf_streaming() {
    let pdf_path = test_pdf_path();

    if !pdf_path.exists() {
        eprintln!("Skipping: handbook.pdf not found at {}", pdf_path.display());
        return;
    }

    let original_pdf = std::fs::read(&pdf_path).expect("read pdf");

    let iii = common::shared_iii();

    let pdf_data = original_pdf.clone();
    let iii_for_handler = iii.clone();
    iii.register_function(
        RegisterFunctionMessage {
            id: "test.api.download.pdf.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |input: Value| {
            let iii = iii_for_handler.clone();
            let pdf_data = pdf_data.clone();
            async move {
                let refs = iii_sdk::extract_channel_refs(&input);
                let writer_ref = refs
                    .iter()
                    .find(|(_, r)| matches!(r.direction, iii_sdk::ChannelDirection::Write))
                    .map(|(_, r)| r.clone())
                    .expect("missing writer ref");

                let writer = iii_sdk::ChannelWriter::new(iii.address(), &writer_ref);

                writer
                    .send_message(
                        &serde_json::to_string(&json!({
                            "type": "set_status", "status_code": 200
                        }))
                        .unwrap(),
                    )
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                writer
                    .send_message(
                        &serde_json::to_string(&json!({
                            "type": "set_headers", "headers": {"content-type": "application/pdf"}
                        }))
                        .unwrap(),
                    )
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                writer
                    .write(&pdf_data)
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;
                writer
                    .close()
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                Ok(Value::Null)
            }
        },
    );

    let _trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "http".to_string(),
            function_id: "test.api.download.pdf.rs".to_string(),
            config: json!({
                "api_path": "test/rs/download/pdf",
                "http_method": "GET",
            }),
        })
        .expect("register trigger");

    common::settle().await;
    sleep(Duration::from_millis(500)).await;

    let resp = common::http_client()
        .get(format!(
            "{}/test/rs/download/pdf",
            common::engine_http_url()
        ))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/pdf")
    );

    let downloaded = resp.bytes().await.expect("read body");
    assert_eq!(downloaded.len(), original_pdf.len());
    assert_eq!(downloaded.as_ref(), original_pdf.as_slice());
}

#[tokio::test]
async fn upload_pdf_streaming() {
    let pdf_path = test_pdf_path();

    if !pdf_path.exists() {
        eprintln!("Skipping: handbook.pdf not found at {}", pdf_path.display());
        return;
    }

    let original_pdf = std::fs::read(&pdf_path).expect("read pdf");

    let iii = common::shared_iii();

    let received: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();

    let iii_for_handler = iii.clone();
    iii.register_function(
        RegisterFunctionMessage {
            id: "test.api.upload.pdf.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |input: Value| {
            let iii = iii_for_handler.clone();
            let received = received_clone.clone();
            async move {
                let refs = iii_sdk::extract_channel_refs(&input);

                let writer_ref = refs
                    .iter()
                    .find(|(_, r)| matches!(r.direction, iii_sdk::ChannelDirection::Write))
                    .map(|(_, r)| r.clone())
                    .expect("missing writer ref");

                let reader_ref = refs
                    .iter()
                    .find(|(k, r)| {
                        k.contains("request_body")
                            && matches!(r.direction, iii_sdk::ChannelDirection::Read)
                    })
                    .map(|(_, r)| r.clone())
                    .expect("missing reader ref");

                let writer = iii_sdk::ChannelWriter::new(iii.address(), &writer_ref);
                let reader = iii_sdk::ChannelReader::new(iii.address(), &reader_ref);

                writer
                    .send_message(
                        &serde_json::to_string(&json!({
                            "type": "set_status", "status_code": 200
                        }))
                        .unwrap(),
                    )
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                writer
                    .send_message(
                        &serde_json::to_string(&json!({
                            "type": "set_headers", "headers": {"content-type": "application/json"}
                        }))
                        .unwrap(),
                    )
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                let data = reader
                    .read_all()
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;
                let len = data.len();
                *received.lock().await = data;

                let body = serde_json::to_vec(&json!({"received_size": len}))
                    .map_err(|e| IIIError::Handler(e.to_string()))?;
                writer
                    .write(&body)
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;
                writer
                    .close()
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                Ok(Value::Null)
            }
        },
    );

    let _trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "http".to_string(),
            function_id: "test.api.upload.pdf.rs".to_string(),
            config: json!({
                "api_path": "test/rs/upload/pdf",
                "http_method": "POST",
            }),
        })
        .expect("register trigger");

    common::settle().await;
    sleep(Duration::from_millis(500)).await;

    let resp = common::http_client()
        .post(format!("{}/test/rs/upload/pdf", common::engine_http_url()))
        .header("content-type", "application/octet-stream")
        .body(original_pdf.clone())
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status().as_u16(), 200);
    let data: Value = resp.json().await.expect("json parse");
    assert_eq!(data["received_size"], original_pdf.len());

    let recv = received.lock().await;
    assert_eq!(recv.len(), original_pdf.len());
    assert_eq!(recv.as_slice(), original_pdf.as_slice());
}

#[tokio::test]
async fn sse_streaming() {
    let iii = common::shared_iii();

    let events = vec![
        json!({"id": "1", "type": "message", "data": "Hello, world!"}),
        json!({"id": "2", "type": "update", "data": serde_json::to_string(&json!({"count": 42})).unwrap()}),
        json!({"id": "3", "type": "message", "data": "line one\nline two"}),
        json!({"id": "4", "type": "done", "data": "goodbye"}),
    ];

    let events_clone = events.clone();
    let iii_for_handler = iii.clone();
    iii.register_function(
        RegisterFunctionMessage {
            id: "test.api.sse.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |input: Value| {
            let iii = iii_for_handler.clone();
            let events = events_clone.clone();
            async move {
                let refs = iii_sdk::extract_channel_refs(&input);
                let writer_ref = refs
                    .iter()
                    .find(|(_, r)| matches!(r.direction, iii_sdk::ChannelDirection::Write))
                    .map(|(_, r)| r.clone())
                    .expect("missing writer ref");

                let writer = iii_sdk::ChannelWriter::new(iii.address(), &writer_ref);

                writer
                    .send_message(
                        &serde_json::to_string(&json!({
                            "type": "set_status", "status_code": 200
                        }))
                        .unwrap(),
                    )
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                writer
                    .send_message(
                        &serde_json::to_string(&json!({
                            "type": "set_headers", "headers": {
                                "content-type": "text/event-stream",
                                "cache-control": "no-cache",
                                "connection": "keep-alive",
                            }
                        }))
                        .unwrap(),
                    )
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                for event in &events {
                    let mut frame = String::new();
                    frame.push_str(&format!("id: {}\n", event["id"].as_str().unwrap()));
                    frame.push_str(&format!("event: {}\n", event["type"].as_str().unwrap()));
                    for line in event["data"].as_str().unwrap().split('\n') {
                        frame.push_str(&format!("data: {line}\n"));
                    }
                    frame.push('\n');

                    writer
                        .write(frame.as_bytes())
                        .await
                        .map_err(|e| IIIError::Handler(e.to_string()))?;
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }

                writer
                    .close()
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;
                Ok(Value::Null)
            }
        },
    );

    let _trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "http".to_string(),
            function_id: "test.api.sse.rs".to_string(),
            config: json!({
                "api_path": "test/rs/sse",
                "http_method": "GET",
            }),
        })
        .expect("register trigger");

    common::settle().await;
    sleep(Duration::from_millis(500)).await;

    let resp = common::http_client()
        .get(format!("{}/test/rs/sse", common::engine_http_url()))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );

    let body = resp.text().await.expect("read body");
    let mut received_events: Vec<(String, String, String)> = Vec::new();

    for block in body.split("\n\n") {
        if block.trim().is_empty() {
            continue;
        }
        let mut id = String::new();
        let mut event_type = String::new();
        let mut data_lines: Vec<String> = Vec::new();

        for line in block.split('\n') {
            if let Some(rest) = line.strip_prefix("id: ") {
                id = rest.to_string();
            } else if let Some(rest) = line.strip_prefix("event: ") {
                event_type = rest.to_string();
            } else if let Some(rest) = line.strip_prefix("data: ") {
                data_lines.push(rest.to_string());
            }
        }

        received_events.push((id, event_type, data_lines.join("\n")));
    }

    assert_eq!(received_events.len(), events.len());
    for (i, event) in events.iter().enumerate() {
        assert_eq!(received_events[i].0, event["id"].as_str().unwrap());
        assert_eq!(received_events[i].1, event["type"].as_str().unwrap());
        assert_eq!(received_events[i].2, event["data"].as_str().unwrap());
    }
}

#[tokio::test]
async fn urlencoded_form_data() {
    let iii = common::shared_iii();

    let iii_for_handler = iii.clone();
    iii.register_function(
        RegisterFunctionMessage {
            id: "test.api.form.urlencoded.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |input: Value| {
            let iii = iii_for_handler.clone();
            async move {
                let refs = iii_sdk::extract_channel_refs(&input);

                let writer_ref = refs
                    .iter()
                    .find(|(_, r)| matches!(r.direction, iii_sdk::ChannelDirection::Write))
                    .map(|(_, r)| r.clone())
                    .expect("missing writer ref");

                let reader_ref = refs
                    .iter()
                    .find(|(k, r)| {
                        k.contains("request_body")
                            && matches!(r.direction, iii_sdk::ChannelDirection::Read)
                    })
                    .map(|(_, r)| r.clone())
                    .expect("missing reader ref");

                let writer = iii_sdk::ChannelWriter::new(iii.address(), &writer_ref);
                let reader = iii_sdk::ChannelReader::new(iii.address(), &reader_ref);

                let raw = reader
                    .read_all()
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;
                let body = String::from_utf8_lossy(&raw);

                let params: std::collections::HashMap<String, String> = body
                    .split('&')
                    .filter_map(|pair| {
                        let mut parts = pair.splitn(2, '=');
                        let key = parts.next()?.to_string();
                        let value = parts.next().unwrap_or("").to_string();
                        let key = urlencoding_decode(&key);
                        let value = urlencoding_decode(&value);
                        Some((key, value))
                    })
                    .collect();

                writer
                    .send_message(
                        &serde_json::to_string(&json!({
                            "type": "set_status", "status_code": 200
                        }))
                        .unwrap(),
                    )
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                writer
                    .send_message(
                        &serde_json::to_string(&json!({
                            "type": "set_headers", "headers": {"content-type": "application/json"}
                        }))
                        .unwrap(),
                    )
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                let result = serde_json::to_vec(&json!({
                    "name": params.get("name"),
                    "email": params.get("email"),
                    "age": params.get("age"),
                }))
                .map_err(|e| IIIError::Handler(e.to_string()))?;

                writer
                    .write(&result)
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;
                writer
                    .close()
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                Ok(Value::Null)
            }
        },
    );

    let _trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "http".to_string(),
            function_id: "test.api.form.urlencoded.rs".to_string(),
            config: json!({
                "api_path": "test/rs/form/urlencoded",
                "http_method": "POST",
            }),
        })
        .expect("register trigger");

    common::settle().await;
    sleep(Duration::from_millis(500)).await;

    let resp = common::http_client()
        .post(format!(
            "{}/test/rs/form/urlencoded",
            common::engine_http_url()
        ))
        .header("content-type", "application/x-www-form-urlencoded")
        .body("name=John+Doe&email=john%40example.com&age=30")
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status().as_u16(), 200);
    let data: Value = resp.json().await.expect("json parse");
    assert_eq!(data["name"], "John Doe");
    assert_eq!(data["email"], "john@example.com");
    assert_eq!(data["age"], "30");
}

fn urlencoding_decode(s: &str) -> String {
    let s = s.replace('+', " ");
    let mut bytes: Vec<u8> = Vec::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hi = chars.next().unwrap_or('0');
            let lo = chars.next().unwrap_or('0');
            let byte = u8::from_str_radix(&format!("{hi}{lo}"), 16).unwrap_or(b'?');
            bytes.push(byte);
        } else {
            let mut buf = [0u8; 4];
            let enc = c.encode_utf8(&mut buf);
            bytes.extend_from_slice(enc.as_bytes());
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[tokio::test]
async fn multipart_form_data() {
    let pdf_path = test_pdf_path();

    if !pdf_path.exists() {
        eprintln!("Skipping: handbook.pdf not found at {}", pdf_path.display());
        return;
    }

    let original_pdf = std::fs::read(&pdf_path).expect("read pdf");

    let iii = common::shared_iii();

    let iii_for_handler = iii.clone();
    iii.register_function(
        RegisterFunctionMessage {
            id: "test.api.form.multipart.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |input: Value| {
            let iii = iii_for_handler.clone();
            async move {
                let refs = iii_sdk::extract_channel_refs(&input);

                let writer_ref = refs
                    .iter()
                    .find(|(_, r)| matches!(r.direction, iii_sdk::ChannelDirection::Write))
                    .map(|(_, r)| r.clone())
                    .expect("missing writer ref");

                let reader_ref = refs
                    .iter()
                    .find(|(k, r)| {
                        k.contains("request_body")
                            && matches!(r.direction, iii_sdk::ChannelDirection::Read)
                    })
                    .map(|(_, r)| r.clone())
                    .expect("missing reader ref");

                let writer = iii_sdk::ChannelWriter::new(iii.address(), &writer_ref);
                let reader = iii_sdk::ChannelReader::new(iii.address(), &reader_ref);

                let raw = reader
                    .read_all()
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                let content_type = input
                    .get("headers")
                    .and_then(|h| h.get("content-type"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let has_boundary = content_type
                    .split(';')
                    .any(|part| part.trim().starts_with("boundary="));

                let body_text = String::from_utf8_lossy(&raw);
                let has_title = body_text.contains("Test Document");
                let has_description = body_text.contains("A test upload");
                let has_filename = body_text.contains("filename=\"handbook.pdf\"");

                writer
                    .send_message(
                        &serde_json::to_string(&json!({
                            "type": "set_status", "status_code": 200
                        }))
                        .unwrap(),
                    )
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                writer
                    .send_message(
                        &serde_json::to_string(&json!({
                            "type": "set_headers", "headers": {"content-type": "application/json"}
                        }))
                        .unwrap(),
                    )
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                let result = serde_json::to_vec(&json!({
                    "has_boundary": has_boundary,
                    "has_title": has_title,
                    "has_description": has_description,
                    "has_filename": has_filename,
                    "body_size": raw.len(),
                }))
                .map_err(|e| IIIError::Handler(e.to_string()))?;

                writer
                    .write(&result)
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;
                writer
                    .close()
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                Ok(Value::Null)
            }
        },
    );

    let _trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "http".to_string(),
            function_id: "test.api.form.multipart.rs".to_string(),
            config: json!({
                "api_path": "test/rs/form/multipart",
                "http_method": "POST",
            }),
        })
        .expect("register trigger");

    common::settle().await;
    sleep(Duration::from_millis(500)).await;

    let form = reqwest::multipart::Form::new()
        .text("title", "Test Document")
        .text("description", "A test upload")
        .part(
            "file",
            reqwest::multipart::Part::bytes(original_pdf.clone())
                .file_name("handbook.pdf")
                .mime_str("application/pdf")
                .expect("mime"),
        );

    let resp = common::http_client()
        .post(format!(
            "{}/test/rs/form/multipart",
            common::engine_http_url()
        ))
        .multipart(form)
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status().as_u16(), 200);
    let data: Value = resp.json().await.expect("json parse");
    assert!(data["has_boundary"].as_bool().unwrap_or(false));
    assert!(data["has_title"].as_bool().unwrap_or(false));
    assert!(data["has_description"].as_bool().unwrap_or(false));
    assert!(data["has_filename"].as_bool().unwrap_or(false));
    assert!(data["body_size"].as_u64().unwrap_or(0) > original_pdf.len() as u64);
}
