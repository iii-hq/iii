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

    bridge.register_function("example.on_event", |input| async move {
        println!("received event: {input}");
        Ok(json!({ "ok": true }))
    });

    let _subscription = bridge.register_trigger(
        "subscribe",
        "example.on_event",
        json!({ "topic": "demo.topic" }),
    )?;

    let publish_data = PubSubData {
        topic: "demo.topic".to_string(),
        data: json!({ "message": "hello from rust client" }),
    };
    let _ = bridge.invoke_function("publish", publish_data).await?;
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}
