//! Which client a received stream opens its socket on: the handler's, the
//! only client alive, or the one it is bound to. One test, so that the
//! number of live clients is the test's own.
//!
//! Requires a running III engine. Set III_URL, or use the default.

mod common;

use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use common::engine_ws_url;
use iii_helpers::observability::OtelConfig;
use iii_sdk::runtime::WorkerMetadata;
use iii_sdk::{
    CallRequest, Dispatch, Error, IIIClient, InitOptions, InputStream, RegisterFunction,
    WorkerIdentityMode, register_worker,
};

async fn client(name: &str) -> IIIClient {
    let iii = register_worker(
        &engine_ws_url(),
        InitOptions {
            metadata: Some(WorkerMetadata {
                name: name.to_string(),
                ..WorkerMetadata::default()
            }),
            otel: Some(OtelConfig {
                enabled: Some(false),
                ..OtelConfig::default()
            }),
            identity: WorkerIdentityMode::Explicit,
            ..InitOptions::default()
        },
    );
    iii.wait_until_registered(Duration::from_secs(10))
        .await
        .expect("engine reachable");
    iii
}

#[derive(Deserialize, schemars::JsonSchema)]
struct Upload {
    body: InputStream,
}

#[tokio::test]
async fn a_received_stream_binds_to_the_handler_the_only_client_or_bind() {
    let id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    let a = client(&format!("bind-a-{id}")).await;
    let export = format!("bind{id}::export");
    let read = format!("bind{id}::read");
    let f1 = a.register_function(
        &export,
        RegisterFunction::new_async(|input: Value| async move {
            let text = input["text"].as_str().unwrap_or_default().to_string();
            Ok::<_, Error>(json!({ "body": InputStream::from_bytes(text) }))
        }),
    );
    let f2 = a.register_function(
        &read,
        RegisterFunction::new_async(|upload: Upload| async move {
            Ok::<_, Error>(json!({ "text": upload.body.text().await? }))
        }),
    );
    for f in [&f1, &f2] {
        while !f.local_dispatch_active() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    let export_call = |client: &IIIClient, text: &str| {
        let client = client.clone();
        let request = CallRequest::new(&export, json!({ "text": text })).dispatch(Dispatch::Remote);
        async move {
            let result = client.call(request).await.unwrap();
            serde_json::from_value::<InputStream>(result["body"].clone()).unwrap()
        }
    };

    // One client alive: the stream binds to it on its first read.
    let body = export_call(&a, "only one").await;
    assert_eq!(body.text().await.unwrap(), "only one");

    // Two alive: a stream received outside a handler must be bound.
    let b = client(&format!("bind-b-{id}")).await;
    let body = export_call(&a, "two").await;
    let error = body.read_all().await.unwrap_err();
    assert!(error.to_string().contains("bind(&iii)"), "{error}");
    let body = export_call(&a, "bound").await;
    assert_eq!(body.bind(&b).text().await.unwrap(), "bound");

    // In a handler, the stream binds to the handler's client.
    let out = b
        .call(
            CallRequest::new(
                &read,
                json!({ "body": InputStream::from_bytes(&b"handler"[..]) }),
            )
            .dispatch(Dispatch::Remote),
        )
        .await
        .unwrap();
    assert_eq!(out["text"], "handler");

    // Shut down, a client no longer counts.
    b.shutdown_async().await;
    let body = export_call(&a, "one again").await;
    assert_eq!(body.text().await.unwrap(), "one again");
    a.shutdown_async().await;
}
