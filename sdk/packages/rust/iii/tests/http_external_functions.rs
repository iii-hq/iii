//! Integration tests for HTTP external function invocation.
//!
//! Requires a running III engine. Set III_URL or use ws://localhost:49134 default.

mod common;

use std::collections::HashMap;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use iii_sdk::{
    HttpInvocationConfig, HttpMethod, RegisterFunctionMessage, RegisterTriggerInput, TriggerRequest,
};

fn unique_function_id(prefix: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    format!("{}::{}::{}", prefix, ts, uuid::Uuid::new_v4().simple())
}

fn unique_topic(prefix: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    format!("{}.{}", prefix, ts)
}

#[derive(Debug, Clone)]
struct CapturedWebhook {
    method: String,
    url: String,
    headers: HashMap<String, String>,
    body: Option<Value>,
}

struct WebhookProbe {
    listener: TcpListener,
}

impl WebhookProbe {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind webhook server");
        Self { listener }
    }

    fn url(&self) -> String {
        let addr = self.listener.local_addr().expect("no local addr");
        format!("http://127.0.0.1:{}/webhook", addr.port())
    }

    async fn accept_one(&self) -> CapturedWebhook {
        let (mut stream, _) = self.listener.accept().await.expect("accept failed");

        let mut buf = vec![0u8; 8192];
        let n = stream.read(&mut buf).await.expect("read failed");
        let raw = String::from_utf8_lossy(&buf[..n]).to_string();

        let mut lines = raw.lines();
        let request_line = lines.next().unwrap_or("");
        let parts: Vec<&str> = request_line.splitn(3, ' ').collect();
        let method = parts.first().copied().unwrap_or("POST").to_string();
        let url = parts.get(1).copied().unwrap_or("/").to_string();
        let url = url.split('?').next().unwrap_or("/").to_string();

        let mut headers = HashMap::new();
        let raw_bytes = &buf[..n];

        let mut header_end = 0;
        for i in 0..n.saturating_sub(3) {
            if raw_bytes[i] == b'\r'
                && raw_bytes[i + 1] == b'\n'
                && raw_bytes[i + 2] == b'\r'
                && raw_bytes[i + 3] == b'\n'
            {
                header_end = i + 4;
                break;
            }
        }
        if header_end == 0 {
            for i in 0..n.saturating_sub(1) {
                if raw_bytes[i] == b'\n' && raw_bytes[i + 1] == b'\n' {
                    header_end = i + 2;
                    break;
                }
            }
        }
        let body_start = header_end;

        for line in raw.lines().skip(1) {
            if line.is_empty() {
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                headers.insert(k.trim().to_lowercase(), v.trim().to_string());
            }
        }

        let body_bytes = &raw_bytes[body_start..];
        let body = if body_bytes.is_empty() {
            None
        } else {
            serde_json::from_slice(body_bytes).ok()
        };

        let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 10\r\n\r\n{\"ok\":true}";
        let _ = stream.write_all(response).await;

        CapturedWebhook {
            method,
            url,
            headers,
            body,
        }
    }

    async fn wait_for_webhook(&self, timeout: Duration) -> Option<CapturedWebhook> {
        tokio::time::timeout(timeout, self.accept_one()).await.ok()
    }
}

#[tokio::test]
async fn delivers_queue_events_to_external_http_function() {
    let iii = common::shared_iii();

    let probe = WebhookProbe::start().await;
    let function_id = unique_function_id("test::http_external::target::rs");
    let topic = unique_topic("test.http_external.topic.rs");
    let payload = json!({"hello": "world", "count": 1});

    let http_fn = iii.register_function(
        RegisterFunctionMessage {
            id: function_id.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        HttpInvocationConfig {
            url: probe.url(),
            method: HttpMethod::Post,
            timeout_ms: Some(3000),
            headers: HashMap::new(),
            auth: None,
        },
    );
    common::settle().await;

    let _trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "queue".to_string(),
            function_id: function_id.clone(),
            config: json!({"topic": topic}),
        })
        .expect("register trigger");
    common::settle().await;

    iii.trigger(TriggerRequest {
        function_id: "enqueue".to_string(),
        payload: json!({"topic": topic, "data": payload}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("enqueue failed");

    let webhook = probe
        .wait_for_webhook(Duration::from_secs(7))
        .await
        .expect("no webhook received");

    assert_eq!(webhook.method, "POST");
    assert_eq!(webhook.url, "/webhook");
    assert_eq!(webhook.body.as_ref().unwrap()["hello"], "world");
    assert_eq!(webhook.body.as_ref().unwrap()["count"], 1);

    drop(_trigger);
    http_fn.unregister();
}

#[tokio::test]
async fn registers_and_unregisters_external_http_function() {
    let iii = common::shared_iii();

    let probe = WebhookProbe::start().await;
    let function_id = unique_function_id("test::http_external::reg_unreg::rs");

    let http_fn = iii.register_function(
        RegisterFunctionMessage {
            id: function_id.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        HttpInvocationConfig {
            url: probe.url(),
            method: HttpMethod::Post,
            timeout_ms: Some(3000),
            headers: HashMap::new(),
            auth: None,
        },
    );
    common::settle().await;

    let found = {
        let functions = iii.list_functions().await.expect("list_functions failed");
        functions.iter().any(|f| f.function_id == function_id)
    };
    assert!(found, "function should appear after registration");

    http_fn.unregister();
    common::settle().await;

    let gone = {
        let functions = iii.list_functions().await.expect("list_functions failed");
        !functions.iter().any(|f| f.function_id == function_id)
    };
    assert!(gone, "function should be absent after unregister");
}

#[tokio::test]
async fn delivers_events_with_custom_headers() {
    let iii = common::shared_iii();

    let probe = WebhookProbe::start().await;
    let function_id = unique_function_id("test::http_external::headers::rs");
    let topic = unique_topic("test.http_external.headers.rs");
    let payload = json!({"msg": "with-headers"});

    let mut custom_headers = HashMap::new();
    custom_headers.insert("x-custom-header".to_string(), "test-value".to_string());
    custom_headers.insert("x-another".to_string(), "123".to_string());

    let http_fn = iii.register_function(
        RegisterFunctionMessage {
            id: function_id.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        HttpInvocationConfig {
            url: probe.url(),
            method: HttpMethod::Post,
            timeout_ms: Some(3000),
            headers: custom_headers,
            auth: None,
        },
    );
    common::settle().await;

    let _trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "queue".to_string(),
            function_id: function_id.clone(),
            config: json!({"topic": topic}),
        })
        .expect("register trigger");
    common::settle().await;

    iii.trigger(TriggerRequest {
        function_id: "enqueue".to_string(),
        payload: json!({"topic": topic, "data": payload}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("enqueue failed");

    let webhook = probe
        .wait_for_webhook(Duration::from_secs(7))
        .await
        .expect("no webhook received");

    assert_eq!(webhook.method, "POST");
    assert_eq!(
        webhook.headers.get("x-custom-header").map(String::as_str),
        Some("test-value")
    );
    assert_eq!(
        webhook.headers.get("x-another").map(String::as_str),
        Some("123")
    );

    drop(_trigger);
    http_fn.unregister();
}

#[tokio::test]
async fn delivers_events_to_multiple_external_functions() {
    let iii = common::shared_iii();

    let probe_a = WebhookProbe::start().await;
    let probe_b = WebhookProbe::start().await;
    let function_id_a = unique_function_id("test::http_external::multi_a::rs");
    let function_id_b = unique_function_id("test::http_external::multi_b::rs");
    let topic_a = unique_topic("test.http_external.multi_a.rs");
    let topic_b = unique_topic("test.http_external.multi_b.rs");
    let payload_a = json!({"source": "topic-a", "value": 1});
    let payload_b = json!({"source": "topic-b", "value": 2});

    let http_fn_a = iii.register_function(
        RegisterFunctionMessage {
            id: function_id_a.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        HttpInvocationConfig {
            url: probe_a.url(),
            method: HttpMethod::Post,
            timeout_ms: Some(3000),
            headers: HashMap::new(),
            auth: None,
        },
    );
    let http_fn_b = iii.register_function(
        RegisterFunctionMessage {
            id: function_id_b.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        HttpInvocationConfig {
            url: probe_b.url(),
            method: HttpMethod::Post,
            timeout_ms: Some(3000),
            headers: HashMap::new(),
            auth: None,
        },
    );
    common::settle().await;

    let _trigger_a = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "queue".to_string(),
            function_id: function_id_a.clone(),
            config: json!({"topic": topic_a}),
        })
        .expect("register trigger a");
    let _trigger_b = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "queue".to_string(),
            function_id: function_id_b.clone(),
            config: json!({"topic": topic_b}),
        })
        .expect("register trigger b");
    common::settle().await;

    iii.trigger(TriggerRequest {
        function_id: "enqueue".to_string(),
        payload: json!({"topic": topic_a, "data": payload_a}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("enqueue a failed");
    iii.trigger(TriggerRequest {
        function_id: "enqueue".to_string(),
        payload: json!({"topic": topic_b, "data": payload_b}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("enqueue b failed");

    let webhook_a = probe_a
        .wait_for_webhook(Duration::from_secs(7))
        .await
        .expect("no webhook received for a");
    let webhook_b = probe_b
        .wait_for_webhook(Duration::from_secs(7))
        .await
        .expect("no webhook received for b");

    assert_eq!(webhook_a.body.as_ref().unwrap()["source"], "topic-a");
    assert_eq!(webhook_b.body.as_ref().unwrap()["source"], "topic-b");

    drop(_trigger_a);
    drop(_trigger_b);
    http_fn_a.unregister();
    http_fn_b.unregister();
}

#[tokio::test]
async fn stops_delivering_events_after_unregister() {
    let iii = common::shared_iii();

    let probe = WebhookProbe::start().await;
    let function_id = unique_function_id("test::http_external::stop::rs");
    let topic = unique_topic("test.http_external.stop.rs");
    let payload_before = json!({"phase": "before-unregister"});
    let payload_after = json!({"phase": "after-unregister"});

    let http_fn = iii.register_function(
        RegisterFunctionMessage {
            id: function_id.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        HttpInvocationConfig {
            url: probe.url(),
            method: HttpMethod::Post,
            timeout_ms: Some(3000),
            headers: HashMap::new(),
            auth: None,
        },
    );
    common::settle().await;

    let trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "queue".to_string(),
            function_id: function_id.clone(),
            config: json!({"topic": topic}),
        })
        .expect("register trigger");
    common::settle().await;

    iii.trigger(TriggerRequest {
        function_id: "enqueue".to_string(),
        payload: json!({"topic": topic, "data": payload_before}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("enqueue before failed");

    let webhook_before = probe
        .wait_for_webhook(Duration::from_secs(7))
        .await
        .expect("no webhook before unregister");
    assert_eq!(
        webhook_before.body.as_ref().unwrap()["phase"],
        "before-unregister"
    );

    drop(trigger);
    http_fn.unregister();
    tokio::time::sleep(Duration::from_millis(500)).await;

    iii.trigger(TriggerRequest {
        function_id: "enqueue".to_string(),
        payload: json!({"topic": topic, "data": payload_after}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("enqueue after failed");

    let received_after = probe
        .wait_for_webhook(Duration::from_secs(2))
        .await
        .is_some();
    assert!(
        !received_after,
        "should not receive webhook after unregister"
    );
}

#[tokio::test]
async fn delivers_events_using_put_method() {
    let iii = common::shared_iii();

    let probe = WebhookProbe::start().await;
    let function_id = unique_function_id("test::http_external::put_method::rs");
    let topic = unique_topic("test.http_external.put.rs");
    let payload = json!({"method_test": "put", "value": 42});

    let http_fn = iii.register_function(
        RegisterFunctionMessage {
            id: function_id.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        HttpInvocationConfig {
            url: probe.url(),
            method: HttpMethod::Put,
            timeout_ms: Some(3000),
            headers: HashMap::new(),
            auth: None,
        },
    );
    common::settle().await;

    let _trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "queue".to_string(),
            function_id: function_id.clone(),
            config: json!({"topic": topic}),
        })
        .expect("register trigger");
    common::settle().await;

    iii.trigger(TriggerRequest {
        function_id: "enqueue".to_string(),
        payload: json!({"topic": topic, "data": payload}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("enqueue failed");

    let webhook = probe
        .wait_for_webhook(Duration::from_secs(7))
        .await
        .expect("no webhook received");

    assert_eq!(webhook.method, "PUT");
    assert_eq!(webhook.body.as_ref().unwrap()["method_test"], "put");
    assert_eq!(webhook.body.as_ref().unwrap()["value"], 42);

    drop(_trigger);
    http_fn.unregister();
}
