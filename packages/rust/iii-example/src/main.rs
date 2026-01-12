use iii_sdk::Bridge;
use serde_json::json;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bridge = Bridge::new("ws://127.0.0.1:49134");
    bridge.connect().await?;

    bridge.register_function("example.echo", |input| async move {
        Ok(json!({ "echo": input }))
    });

    let result: serde_json::Value = bridge
        .invoke_function("example.echo", json!({ "message": "hello" }))
        .await?;

    println!("result: {result}");

    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}
