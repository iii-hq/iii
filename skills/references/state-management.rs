/// Pattern: State Management
/// Comparable to: Redis, DynamoDB, Memcached
///
/// Persistent key-value state scoped by namespace. Supports set, get,
/// list, delete, and atomic update operations (set, merge, append,
/// increment, decrement, remove). The merge op accepts a nested-segment
/// path for shallow-merging into auto-created intermediates.

use iii_sdk::{
    register_worker, InitOptions, RegisterFunction, TriggerRequest, TriggerAction,
    builtin_triggers::*, IIITrigger,
};
use serde_json::json;

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct CreateProductInput {
    name: String,
    price: f64,
    category: String,
    stock: Option<i64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct GetProductInput {
    id: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct RemoveProductInput {
    id: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct UpdatePriceInput {
    id: String,
    #[serde(rename = "newPrice")]
    new_price: f64,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct RecordChunkInput {
    #[serde(rename = "sessionId")]
    session_id: String,
    chunk: String,
    author: String,
}

fn main() {
    let url = std::env::var("III_ENGINE_URL").unwrap_or("ws://127.0.0.1:49134".into());
    let iii = register_worker(&url, InitOptions::default());

    // ---
    // state::set - Store a value under a scoped key
    // Payload: { scope, key, value }
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("products::create", move |data: CreateProductInput| {
            let iii = iii_clone.clone();
            async move {
                let id = format!("prod-{}", chrono::Utc::now().timestamp_millis());
                let product = json!({
                    "id": id,
                    "name": data.name,
                    "price": data.price,
                    "category": data.category,
                    "stock": data.stock.unwrap_or(0),
                    "created_at": chrono::Utc::now().to_rfc3339(),
                });

                iii.trigger(TriggerRequest {
                    function_id: "state::set".into(),
                    payload: json!({ "scope": "products", "key": id, "value": product }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                Ok(product)
            }
        })
        .description("Create a new product"),
    );

    // ---
    // state::get - Retrieve a value by scope and key
    // Payload: { scope, key }
    // Returns null if the key does not exist - always guard for null.
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("products::get", move |data: GetProductInput| {
            let iii = iii_clone.clone();
            async move {
                let product = iii
                    .trigger(TriggerRequest {
                        function_id: "state::get".into(),
                        payload: json!({ "scope": "products", "key": data.id }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                if product.is_null() {
                    return Ok(json!({ "error": "Product not found", "id": data.id }));
                }

                Ok(product)
            }
        })
        .description("Get a product by ID"),
    );

    // ---
    // state::list - Retrieve all values in a scope
    // Payload: { scope }
    // Returns an array of all stored values.
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("products::list-all", move |_: serde_json::Value| {
            let iii = iii_clone.clone();
            async move {
                let products = iii
                    .trigger(TriggerRequest {
                        function_id: "state::list".into(),
                        payload: json!({ "scope": "products" }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                let arr = products.as_array().cloned().unwrap_or_default();
                Ok(json!({ "count": arr.len(), "products": arr }))
            }
        })
        .description("List all products"),
    );

    // ---
    // state::delete - Remove a key from a scope
    // Payload: { scope, key }
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("products::remove", move |data: RemoveProductInput| {
            let iii = iii_clone.clone();
            async move {
                let existing = iii
                    .trigger(TriggerRequest {
                        function_id: "state::get".into(),
                        payload: json!({ "scope": "products", "key": data.id }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                if existing.is_null() {
                    return Ok(json!({ "error": "Product not found", "id": data.id }));
                }

                iii.trigger(TriggerRequest {
                    function_id: "state::delete".into(),
                    payload: json!({ "scope": "products", "key": data.id }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                Ok(json!({ "deleted": data.id }))
            }
        })
        .description("Remove a product by ID"),
    );

    // ---
    // state::update - Atomic ops over a record
    // Payload: { scope, key, ops }
    // ops: [{ type: "set" | "merge" | "append" | "increment" | "decrement" | "remove", path, value?, by? }]
    // Use update instead of get-then-set for atomic partial changes.
    // Returns { old_value, new_value, errors? } - failed ops surface
    // structured entries in `errors` while later valid ops still apply.
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("products::update-price", move |data: UpdatePriceInput| {
            let iii = iii_clone.clone();
            async move {
                let existing = iii
                    .trigger(TriggerRequest {
                        function_id: "state::get".into(),
                        payload: json!({ "scope": "products", "key": data.id }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                if existing.is_null() {
                    return Ok(json!({ "error": "Product not found", "id": data.id }));
                }

                iii.trigger(TriggerRequest {
                    function_id: "state::update".into(),
                    payload: json!({
                        "scope": "products",
                        "key": data.id,
                        "ops": [
                            { "type": "set", "path": "price", "value": data.new_price },
                            { "type": "set", "path": "updated_at", "value": chrono::Utc::now().to_rfc3339() },
                        ],
                    }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                Ok(json!({ "id": data.id, "price": data.new_price }))
            }
        })
        .description("Update product price"),
    );

    // ---
    // state::update with merge - Nested shallow-merge for per-session structured state
    // `merge.path` accepts a string (first-level field) or an array of literal
    // segments. The engine walks the segments, auto-creating each intermediate
    // object. Sibling keys at every level are preserved.
    // Each segment is a literal key - ["a.b"] writes one key named "a.b",
    // not a -> b.
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("transcripts::record-chunk", move |data: RecordChunkInput| {
            let iii = iii_clone.clone();
            async move {
                let timestamp = chrono::Utc::now().timestamp_millis().to_string();

                // Build `{ "<timestamp>": "<chunk>" }` for the timestamped merge,
                // since serde_json::json! object keys must be string literals.
                let mut chunk_obj = serde_json::Map::new();
                chunk_obj.insert(timestamp.clone(), serde_json::Value::String(data.chunk));

                iii.trigger(TriggerRequest {
                    function_id: "state::update".into(),
                    payload: json!({
                        "scope": "audio::transcripts",
                        "key": data.session_id,
                        "ops": [
                            // Nested path: walks session_id -> "metadata", auto-creating
                            // each intermediate object if it doesn't exist yet.
                            { "type": "merge", "path": [&data.session_id, "metadata"], "value": { "author": data.author } },
                            // First-level form (sugar for path: [session_id]).
                            { "type": "merge", "path": &data.session_id, "value": chunk_obj },
                        ],
                    }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                Ok(json!({ "sessionId": data.session_id, "timestamp": timestamp }))
            }
        })
        .description("Record a transcript chunk under a session"),
    );

    // ---
    // HTTP triggers
    // ---
    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/products").method(HttpMethod::Post))
            .for_function("products::create"),
    )
    .expect("failed");

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/products/:id").method(HttpMethod::Get))
            .for_function("products::get"),
    )
    .expect("failed");

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/products").method(HttpMethod::Get))
            .for_function("products::list-all"),
    )
    .expect("failed");

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/products/:id").method(HttpMethod::Delete))
            .for_function("products::remove"),
    )
    .expect("failed");

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/products/:id/price").method(HttpMethod::Put))
            .for_function("products::update-price"),
    )
    .expect("failed");

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/transcripts/:sessionId/chunks").method(HttpMethod::Post))
            .for_function("transcripts::record-chunk"),
    )
    .expect("failed");

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        tokio::signal::ctrl_c().await.ok();
    });
    iii.shutdown();
}
