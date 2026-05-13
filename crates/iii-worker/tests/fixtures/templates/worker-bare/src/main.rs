use iii_sdk::{InitOptions, RegisterFunction, register_worker};
use serde_json::{Value, json};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let url = std::env::var("III_URL").unwrap_or_else(|_| "ws://localhost:49134".to_string());

    let iii = register_worker(&url, InitOptions::default());

    iii.register_function(RegisterFunction::new_async(
        "hello",
        |req: Value| async move {
            let name = req
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("world");
            Ok::<_, String>(json!({ "greeting": format!("hello, {name}") }))
        },
    ));

    println!("worker ready (engine: {url})");
    tokio::signal::ctrl_c().await?;
    iii.shutdown();
    Ok(())
}
