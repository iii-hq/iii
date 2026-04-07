/// Pattern: State Management
/// Comparable to: Redis, DynamoDB, Memcached
///
/// Persistent key-value state scoped by namespace. Supports set, get,
/// list, delete, and partial update operations.

use iii_sdk::{
    register_worker, InitOptions, RegisterFunction, TriggerRequest, TriggerAction,
    builtin_triggers::*, IIITrigger, Logger,
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
struct AdjustStockInput {
    id: String,
    adjustment: i64,
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
    // state::update - Partial merge using ops array
    // Payload: { scope, key, ops }
    // ops: [{ type: "set", path, value }]
    // Use update instead of get-then-set for atomic partial changes.
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
    // Combining operations - inventory adjustment with update
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("products::adjust-stock", move |data: AdjustStockInput| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();

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

                let current_stock = product["stock"].as_i64().unwrap_or(0);
                let new_stock = current_stock + data.adjustment;

                if new_stock < 0 {
                    return Ok(json!({
                        "error": "Insufficient stock",
                        "current": current_stock,
                        "requested": data.adjustment,
                    }));
                }

                iii.trigger(TriggerRequest {
                    function_id: "state::update".into(),
                    payload: json!({
                        "scope": "products",
                        "key": data.id,
                        "ops": [
                            { "type": "set", "path": "stock", "value": new_stock },
                            { "type": "set", "path": "last_stock_change", "value": chrono::Utc::now().to_rfc3339() },
                        ],
                    }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                logger.info("Stock adjusted", &json!({ "id": data.id, "from": current_stock, "to": new_stock }));
                Ok(json!({ "id": data.id, "previousStock": current_stock, "newStock": new_stock }))
            }
        })
        .description("Adjust product stock"),
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
        IIITrigger::Http(HttpTriggerConfig::new("/products/:id/stock").method(HttpMethod::Post))
            .for_function("products::adjust-stock"),
    )
    .expect("failed");

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        tokio::signal::ctrl_c().await.ok();
    });
    iii.shutdown();
}
