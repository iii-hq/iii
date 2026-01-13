use std::time::Duration;

use iii_sdk::Bridge;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Serialize, Deserialize, Debug)]
struct KeyValueData {
    key: String,
    value: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iii_bridge_url = std::env::var("III_BRIDGE_URL").unwrap_or("ws://127.0.0.1:49134".into());
    let bridge = Bridge::new(&iii_bridge_url);
    bridge.connect().await?;

    bridge.register_function("example.echo", |input| async move {
        Ok(json!({ "echo": input }))
    });

    let result: serde_json::Value = bridge
        .invoke_function("example.echo", json!({ "message": "hello" }))
        .await?;

    println!("result: {result}");
    let list_functions = bridge.list_functions().await?;
    println!("registered functions: {list_functions:#?}");

    let data = KeyValueData {
        key: "my_key".to_string(),
        value: "new_value".to_string(),
    };
    // Set a key-value pair
    let _: serde_json::Value = bridge.invoke_function("remote.kv.set", data).await?;

    // Get the value by key
    let item: serde_json::Value = bridge
        .invoke_function("remote.kv.get", "my_key".to_string())
        .await?;
    println!("got item: {item}");

    let deleted_item: serde_json::Value = bridge
        .invoke_function("remote.kv.delete", "my_key".to_string())
        .await?;

    println!("deleted item: {deleted_item}");
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}
