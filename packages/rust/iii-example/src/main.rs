use std::time::Duration;

use iii_sdk::{Bridge, Stream, UpdateBuilder, UpdateOp};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Serialize, Deserialize, Debug)]
struct KeyValueData {
    key: String,
    value: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iii_bridge_url = std::env::var("REMOTE_III_URL").unwrap_or("ws://127.0.0.1:49134".into());
    let bridge = Bridge::new(&iii_bridge_url);
    bridge.connect().await?;

    // Create a Stream instance for atomic updates
    let stream = Stream::new(bridge.clone());

    bridge.register_function("example.echo", |input| async move {
        Ok(json!({ "echo": input }))
    });

    let result = bridge
        .call("example.echo", json!({ "message": "hello" }))
        .await?;
    println!("Echo result: {result}");

    // =========================================================================
    // Stream Atomic Update Examples
    // =========================================================================

    let stream_key = "example::demo::counter-1";

    // Example 1: Using UpdateOp directly
    println!("\n--- Example 1: Direct UpdateOp ---");
    let result = stream
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
    let result = stream.increment(stream_key, "counter", 5).await?;
    println!(
        "After increment by 5: counter = {}",
        result.new_value["counter"]
    );

    // Example 3: Multiple atomic operations in one call
    println!("\n--- Example 3: Multiple Operations ---");
    let result = stream
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

    let result = stream.update(stream_key, ops).await?;
    println!("After builder ops: {:?}", result.new_value);

    // Example 5: Merge operation
    println!("\n--- Example 5: Merge Operation ---");
    let result = stream
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
    let result = stream.remove_field(stream_key, "extra_field").await?;
    println!("After removing extra_field: {:?}", result.new_value);

    // Example 7: Decrement
    println!("\n--- Example 7: Decrement ---");
    let result = stream.decrement(stream_key, "counter", 3).await?;
    println!(
        "After decrement by 3: counter = {}",
        result.new_value["counter"]
    );

    // Example 8: Concurrent updates simulation
    println!("\n--- Example 8: Concurrent Updates ---");
    let concurrent_key = "example::demo::concurrent-test";

    // Initialize
    stream
        .update(concurrent_key, vec![UpdateOp::set("counter", json!(0))])
        .await?;

    // Spawn 10 concurrent increment tasks
    let mut handles = vec![];
    for i in 0..10 {
        let stream_clone = stream.clone();
        let key = concurrent_key.to_string();
        let handle = tokio::spawn(async move {
            for _ in 0..10 {
                let _ = stream_clone.increment(&key, "counter", 1).await;
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
    let final_result = stream
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
