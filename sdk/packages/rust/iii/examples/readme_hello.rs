//! Canonical source-of-truth for the Rust SDK README "Hello World".
//!
//! The README embeds this file's contents; CI verifies it compiles with
//! `cargo check --example readme_hello`. Do not duplicate the code into the
//! README by hand — update this file and re-run the docs generator.

use iii_sdk::builtin_triggers::{HttpMethod, HttpTriggerConfig};
use iii_sdk::{
    IIITrigger, InitOptions, RegisterFunction, TriggerRequest, register_worker,
};
use serde_json::{Value, json};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // register_worker returns `III` directly (not `Result`); it spawns a
    // background runtime and auto-connects to the engine over WebSocket.
    let iii = register_worker("ws://localhost:49134", InitOptions::default());

    iii.register_function(RegisterFunction::new_async("greet", |input: Value| async move {
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("world");
        Ok::<Value, iii_sdk::IIIError>(json!({ "message": format!("Hello, {name}!") }))
    }));

    // Recommended: the typed IIITrigger builder.
    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/greet").method(HttpMethod::Post))
            .for_function("greet"),
    )?;

    // Invoke the function. `TriggerRequest::new` covers the 80% case;
    // `.with_action(TriggerAction::Void)` / `.with_timeout_ms(..)` handle the rest.
    let result: Value = iii
        .trigger(TriggerRequest::new("greet", json!({ "name": "world" })))
        .await?;

    println!("result: {result}");
    iii.shutdown();
    Ok(())
}
