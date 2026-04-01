use std::{thread::sleep, time::Duration};

use iii_sdk::{
    InitOptions, OtelConfig, RegisterFunction, TriggerRequest, UpdateBuilder, UpdateOp,
    register_worker,
};
use serde_json::json;

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct EchoInput {
    message: String,
    repeat: u32,
    uppercase: bool,
    prefix: String,
}

fn echo_message(input: EchoInput) -> Result<serde_json::Value, String> {
    let mut result = input.message.repeat(input.repeat as usize);
    if input.uppercase {
        result = result.to_uppercase();
    }
    Ok(json!({ "echo": format!("{}{}", input.prefix, result) }))
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct DelayEchoInput {
    message: String,
    delay_ms: u64,
    suffix: String,
}

async fn delay_echo(input: DelayEchoInput) -> Result<serde_json::Value, String> {
    tokio::time::sleep(Duration::from_millis(input.delay_ms)).await;
    Ok(
        json!({ "echo": format!("{}{}", input.message, input.suffix), "delayed_ms": input.delay_ms }),
    )
}

mod cron_trigger_example;
mod custom_trigger_example;
mod http_example;
mod logger_example;
mod trigger_type_example;

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

    // Logger demo (all log levels with structured data)
    logger_example::setup(&iii);

    // Register HTTP fetch API handlers (GET & POST http-fetch with OTel instrumentation)
    http_example::setup(&iii);

    // Custom webhook trigger type with typed config (compile-time safe)
    trigger_type_example::setup(&iii);

    // Built-in trigger types: cron and state (untyped config)
    cron_trigger_example::setup(&iii);

    // More custom triggers: schedule, file-watch (typed), custom-event (untyped fallback)
    custom_trigger_example::setup(&iii);

    // List all registered trigger types with their schemas
    trigger_type_example::list_trigger_types_example(&iii).await;

    iii.register_function(
        RegisterFunction::new("example::echo", echo_message)
            .description("Echo a message with repeat and formatting options"),
    );

    iii.register_function(
        RegisterFunction::new_async("example::delay_echo", delay_echo)
            .description("Echo with configurable delay"),
    );

    let result = iii
        .trigger(TriggerRequest {
            function_id: "example::echo".to_string(),
            payload: json!({"message": "hello", "repeat": 2, "uppercase": false, "prefix": "> "}),
            action: None,
            timeout_ms: None,
        })
        .await?;
    println!("Echo result: {result}");

    // Invoke logger demo to exercise all log levels
    let logger_result = iii
        .trigger(TriggerRequest {
            function_id: "example::logger_demo".to_string(),
            payload: json!({"test": true}),
            action: None,
            timeout_ms: None,
        })
        .await?;
    println!("Logger demo result: {logger_result}");

    // =========================================================================
    // Stream Atomic Update Examples
    // =========================================================================

    // Example 1: Using UpdateOp directly
    println!("\n--- Example 1: Direct UpdateOp ---");
    let result = iii
        .trigger(TriggerRequest {
            function_id: "stream::update".to_string(),
            payload: json!({
                "stream_name": "example",
                "group_id": "demo",
                "item_id": "counter-1",
                "ops": [
                    UpdateOp::set("name", json!("Counter Example")),
                    UpdateOp::set("counter", json!(0)),
                    UpdateOp::set("status", json!("initialized")),
                ],
            }),
            action: None,
            timeout_ms: None,
        })
        .await?;
    println!("Initial value: {:?}", result);

    // Example 2: Atomic increment
    println!("\n--- Example 2: Atomic Increment ---");
    let result = iii
        .trigger(TriggerRequest {
            function_id: "stream::update".to_string(),
            payload: json!({
                "stream_name": "example",
                "group_id": "demo",
                "item_id": "counter-1",
                "ops": [UpdateOp::increment("counter", 5)],
            }),
            action: None,
            timeout_ms: None,
        })
        .await?;
    println!("After increment by 5: {:?}", result);

    // Example 3: Multiple atomic operations in one call
    println!("\n--- Example 3: Multiple Operations ---");
    let result = iii
        .trigger(TriggerRequest {
            function_id: "stream::update".to_string(),
            payload: json!({
                "stream_name": "example",
                "group_id": "demo",
                "item_id": "counter-1",
                "ops": [
                    UpdateOp::increment("counter", 10),
                    UpdateOp::set("status", json!("active")),
                    UpdateOp::set("lastUpdated", json!("2024-01-21T12:00:00Z")),
                ],
            }),
            action: None,
            timeout_ms: None,
        })
        .await?;
    println!("After multiple ops: {:?}", result);

    // Example 4: Using UpdateBuilder pattern
    println!("\n--- Example 4: UpdateBuilder Pattern ---");
    let ops = UpdateBuilder::new()
        .increment("counter", 1)
        .set("status", json!("processing"))
        .set("metadata", json!({"source": "rust-sdk", "version": "1.0"}))
        .build();

    let result = iii
        .trigger(TriggerRequest {
            function_id: "stream::update".to_string(),
            payload: json!({
                "stream_name": "example",
                "group_id": "demo",
                "item_id": "counter-1",
                "ops": ops,
            }),
            action: None,
            timeout_ms: None,
        })
        .await?;
    println!("After builder ops: {:?}", result);

    // Example 5: Merge operation
    println!("\n--- Example 5: Merge Operation ---");
    let result = iii
        .trigger(TriggerRequest {
            function_id: "stream::update".to_string(),
            payload: json!({
                "stream_name": "example",
                "group_id": "demo",
                "item_id": "counter-1",
                "ops": [UpdateOp::merge(json!({
                    "extra_field": "added via merge",
                    "another_field": 42
                }))],
            }),
            action: None,
            timeout_ms: None,
        })
        .await?;
    println!("After merge: {:?}", result);

    // Example 6: Remove a field
    println!("\n--- Example 6: Remove Field ---");
    let result = iii
        .trigger(TriggerRequest {
            function_id: "stream::update".to_string(),
            payload: json!({
                "stream_name": "example",
                "group_id": "demo",
                "item_id": "counter-1",
                "ops": [UpdateOp::remove("extra_field")],
            }),
            action: None,
            timeout_ms: None,
        })
        .await?;
    println!("After removing extra_field: {:?}", result);

    // Example 7: Decrement
    println!("\n--- Example 7: Decrement ---");
    let result = iii
        .trigger(TriggerRequest {
            function_id: "stream::update".to_string(),
            payload: json!({
                "stream_name": "example",
                "group_id": "demo",
                "item_id": "counter-1",
                "ops": [UpdateOp::decrement("counter", 3)],
            }),
            action: None,
            timeout_ms: None,
        })
        .await?;
    println!("After decrement by 3: {:?}", result);

    // Example 8: Concurrent updates simulation
    println!("\n--- Example 8: Concurrent Updates ---");

    // Initialize
    iii.trigger(TriggerRequest {
        function_id: "stream::update".to_string(),
        payload: json!({
            "stream_name": "example",
            "group_id": "demo",
            "item_id": "concurrent-test",
            "ops": [UpdateOp::set("counter", json!(0))],
        }),
        action: None,
        timeout_ms: None,
    })
    .await?;

    // Spawn 10 concurrent increment tasks
    let mut handles = vec![];
    for i in 0..10 {
        let iii_clone = iii.clone();
        let handle = tokio::spawn(async move {
            for _ in 0..10 {
                let _ = iii_clone
                    .trigger(TriggerRequest {
                        function_id: "stream::update".to_string(),
                        payload: json!({
                            "stream_name": "example",
                            "group_id": "demo",
                            "item_id": "concurrent-test",
                            "ops": [UpdateOp::increment("counter", 1)],
                        }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await;
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
    let final_result = iii
        .trigger(TriggerRequest {
            function_id: "stream::update".to_string(),
            payload: json!({
                "stream_name": "example",
                "group_id": "demo",
                "item_id": "concurrent-test",
                "ops": [UpdateOp::increment("counter", 0)],
            }),
            action: None,
            timeout_ms: None,
        })
        .await?;
    println!(
        "Final counter after 100 concurrent increments: {}",
        final_result["new_value"]["counter"]
    );

    println!("\n--- All examples completed! Process stays alive via connection thread. ---");

    sleep(Duration::from_secs(10));
    println!("Finishing III");
    iii.shutdown();
    sleep(Duration::from_secs(10));

    Ok(())
}
