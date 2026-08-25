use iii_sdk::protocol::RegisterTriggerInput;
use iii_sdk::{Error, IIIClient, RegisterFunction};
use serde_json::{json, Value};
use tracing::info;

/// Broadcast channel carrying serialized console events (currently only
/// `traces_changed` ticks) from the engine bridge to every browser connected
/// on `/ws/console-events`.
pub type ConsoleEvents = tokio::sync::broadcast::Sender<String>;

/// Register the trace-trigger handler and its `trace` trigger.
///
/// The engine coalesces span activity into one `{trace_ids}` tick per
/// window and invokes the handler fire-and-forget; the handler fans the tick
/// out to the browsers, which re-run their own filtered queries
/// (notify-then-query — the engine stays the single source of filter
/// semantics, and an idle engine produces no traffic at all). The trigger is
/// owned by this bridge connection, so the engine unregisters it when the
/// console disconnects.
pub fn register_trace_events(bridge: &IIIClient, events: ConsoleEvents) -> Result<(), Error> {
    let tick = events.clone();
    bridge.register_function(
        "engine::console::traces_changed",
        RegisterFunction::new_async(move |input: Value| {
            let events = tick.clone();
            async move {
                let trace_ids = input
                    .get("trace_ids")
                    .cloned()
                    .unwrap_or_else(|| Value::Array(Vec::new()));
                let frame = json!({ "type": "traces_changed", "trace_ids": trace_ids }).to_string();
                // No receiver just means no console tab is open right now.
                let _ = events.send(frame);
                Ok(json!({ "delivered": true }))
            }
        }),
    );

    info!("Registering trace trigger: engine::console::traces_changed");
    bridge.register_trigger(RegisterTriggerInput::new(
        "trace",
        "engine::console::traces_changed",
        json!({}),
    ))?;
    Ok(())
}
