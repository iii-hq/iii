use std::time::Duration;

use iii_sdk::{
    IIIError, InitOptions, OtelConfig, RegisterFunctionMessage, RegisterTriggerInput, Streams,
    TriggerRequest, UpdateBuilder, UpdateOp, register_worker,
};
use serde_json::json;

mod http_example;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iii_iii_url = std::env::var("REMOTE_III_URL").unwrap_or("ws://127.0.0.1:49134".into());
    let iii = register_worker(
        &iii_iii_url,
        InitOptions {
            otel: Some(OtelConfig::default()),
            ..Default::default()
        },
    );

    // Register HTTP fetch API handlers (GET & POST http-fetch with OTel instrumentation)
    http_example::setup(&iii);

    iii.register_function(
        RegisterFunctionMessage {
            id: "api::get::error-test".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        |_input| async move {
            Err(IIIError::Handler(
                "Intentional error for OTEL stacktrace testing".into(),
            ))
        },
    );
    iii.register_trigger(RegisterTriggerInput {
        trigger_type: "http".to_string(),
        function_id: "api::get::error-test".to_string(),
        config: json!({
            "api_path": "error-test",
            "http_method": "GET",
            "description": "Returns an error to test OTEL stack traces",
        }),
    })
    .expect("failed to register error-test trigger");

    // Create a Streams instance for atomic updates
    let streams = Streams::new(iii.clone());

    iii.register_function(
        RegisterFunctionMessage {
            id: "example.echo".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        |input| async move { Ok(json!({ "echo": input })) },
    );

    let result = iii
        .trigger(TriggerRequest {
            function_id: "example.echo".to_string(),
            payload: json!({ "message": "hello" }),
            action: None,
            timeout_ms: None,
        })
        .await?;
    println!("Echo result: {result}");

    // =========================================================================
    // Stream Atomic Update Examples
    // =========================================================================

    let stream_key = "example::demo::counter-1";

    // Example 1: Using UpdateOp directly
    println!("\n--- Example 1: Direct UpdateOp ---");
    let result = streams
        .update(
            stream_key,
            vec![
                UpdateOp::set("name", json!("Counter Example")),
                UpdateOp::set("counter", json!(0)),
                UpdateOp::set("status", json!("initialized")),
            ],
        )
        .await?;
    println!("Initial value: {:?}", result.new_value);

    // Example 2: Atomic increment
    println!("\n--- Example 2: Atomic Increment ---");
    let result = streams.increment(stream_key, "counter", 5).await?;
    println!(
        "After increment by 5: counter = {}",
        result.new_value["counter"]
    );

    // Example 3: Multiple atomic operations in one call
    println!("\n--- Example 3: Multiple Operations ---");
    let result = streams
        .update(
            stream_key,
            vec![
                UpdateOp::increment("counter", 10),
                UpdateOp::set("status", json!("active")),
                UpdateOp::set("lastUpdated", json!("2024-01-21T12:00:00Z")),
            ],
        )
        .await?;
    println!("After multiple ops: {:?}", result.new_value);

    // Example 4: Using UpdateBuilder pattern
    println!("\n--- Example 4: UpdateBuilder Pattern ---");
    let ops = UpdateBuilder::new()
        .increment("counter", 1)
        .set("status", json!("processing"))
        .set("metadata", json!({"source": "rust-sdk", "version": "1.0"}))
        .build();

    let result = streams.update(stream_key, ops).await?;
    println!("After builder ops: {:?}", result.new_value);

    // Example 5: Merge operation
    println!("\n--- Example 5: Merge Operation ---");
    let result = streams
        .merge(
            stream_key,
            json!({
                "extra_field": "added via merge",
                "another_field": 42
            }),
        )
        .await?;
    println!("After merge: {:?}", result.new_value);

    // Example 6: Remove a field
    println!("\n--- Example 6: Remove Field ---");
    let result = streams.remove_field(stream_key, "extra_field").await?;
    println!("After removing extra_field: {:?}", result.new_value);

    // Example 7: Decrement
    println!("\n--- Example 7: Decrement ---");
    let result = streams.decrement(stream_key, "counter", 3).await?;
    println!(
        "After decrement by 3: counter = {}",
        result.new_value["counter"]
    );

    // Example 8: Concurrent updates simulation
    println!("\n--- Example 8: Concurrent Updates ---");
    let concurrent_key = "example::demo::concurrent-test";

    // Initialize
    streams
        .update(concurrent_key, vec![UpdateOp::set("counter", json!(0))])
        .await?;

    // Spawn 10 concurrent increment tasks
    let mut handles = vec![];
    for i in 0..10 {
        let streams_clone = streams.clone();
        let key = concurrent_key.to_string();
        let handle = tokio::spawn(async move {
            for _ in 0..10 {
                let _ = streams_clone.increment(&key, "counter", 1).await;
            }
            println!("Task {} completed 10 increments", i);
        });
        handles.push(handle);
    }

    // Wait for all tasks
    for handle in handles {
        handle.await?;
    }

    // Check final value (should be 100 with atomic updates)
    let final_result = streams
        .update(concurrent_key, vec![UpdateOp::increment("counter", 0)])
        .await?;
    println!(
        "Final counter after 100 concurrent increments: {}",
        final_result.new_value["counter"]
    );

    println!("\n--- All examples completed! Waiting... ---");
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}
