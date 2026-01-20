use std::time::Duration;

use iii_sdk::Bridge;
use serde_json::json;

#[derive(serde::Serialize)]
struct PubSubData {
    topic: String,
    data: serde_json::Value,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iii_bridge_url = std::env::var("REMOTE_III_URL").unwrap_or("ws://127.0.0.1:49134".into());
    let bridge = Bridge::new(&iii_bridge_url);
    bridge.connect().await?;

    let bridge_for_handler = bridge.clone();
    bridge.register_function("example.increment", move |input| {
        let bridge = bridge_for_handler.clone();
        async move {
            let amount = input
                .get("amount")
                .and_then(|value| value.as_i64())
                .unwrap_or(1);

            let current_value = bridge
                .invoke_function("kv_server.get", json!({ "key": "counter" }))
                .await?;
            let current = current_value
                .as_i64()
                .or_else(|| {
                    current_value
                        .as_str()
                        .and_then(|value| value.parse::<i64>().ok())
                })
                .unwrap_or(0);
            let next = current + amount;

            let _ = bridge
                .invoke_function(
                    "kv_server.set",
                    json!({ "key": "counter", "value": json!(next) }),
                )
                .await?;

            Ok(json!({ "value": next }))
        }
    });

    let _subscription = bridge.register_trigger(
        "subscribe",
        "example.increment",
        json!({ "topic": "counter.tick" }),
    )?;

    for i in 0..5 {
        let publish_data = PubSubData {
            topic: "counter.tick".to_string(),
            data: json!({ "amount": 1, "seq": i }),
        };
        let _ = bridge.invoke_function("publish", publish_data).await?;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    let final_value = bridge
        .invoke_function("kv_server.get", json!({ "key": "counter" }))
        .await?;
    println!("final counter value: {final_value}");
    Ok(())
}
