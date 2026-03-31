/// Pattern: HTTP Endpoints
/// Comparable to: Express, Fastify, Actix-web
///
/// Exposes RESTful HTTP endpoints backed by iii functions.
/// Each handler receives an ApiRequest object and returns
/// { status_code, body, headers }.

use iii_sdk::{
    register_worker, InitOptions, RegisterFunction, TriggerRequest, TriggerAction,
    builtin_triggers::*, IIITrigger, Logger, ApiRequest,
};
use serde_json::json;

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct CreateUserBody {
    name: String,
    email: String,
}

fn main() {
    let url = std::env::var("III_ENGINE_URL").unwrap_or("ws://127.0.0.1:49134".into());
    let iii = register_worker(&url, InitOptions::default());

    // ---
    // POST /users - Create a new user
    // ApiRequest: { body, path_params, headers, method }
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("users::create", move |req: ApiRequest<CreateUserBody>| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();
                let id = format!("usr-{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());

                let user = json!({
                    "id": id,
                    "name": req.body.name,
                    "email": req.body.email,
                    "created_at": chrono::Utc::now().to_rfc3339(),
                });

                iii.trigger(TriggerRequest {
                    function_id: "state::set".into(),
                    payload: json!({ "scope": "users", "key": id, "value": user }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                logger.info("User created", &json!({ "event": "user_created", "id": id }));

                Ok(json!({
                    "status_code": 201,
                    "body": user,
                    "headers": { "Content-Type": "application/json" },
                }))
            }
        })
        .description("Create a new user"),
    );

    // ---
    // GET /users/:id - Retrieve a user by path parameter
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("users::get-by-id", move |req: ApiRequest| {
            let iii = iii_clone.clone();
            async move {
                let id = req.path_params.get("id").cloned().unwrap_or_default();

                let user = iii
                    .trigger(TriggerRequest {
                        function_id: "state::get".into(),
                        payload: json!({ "scope": "users", "key": id }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                if user.is_null() {
                    return Ok(json!({ "status_code": 404, "body": { "error": "User not found" } }));
                }

                Ok(json!({ "status_code": 200, "body": user }))
            }
        })
        .description("Get user by ID"),
    );

    // ---
    // GET /users - List all users
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("users::list", move |_: serde_json::Value| {
            let iii = iii_clone.clone();
            async move {
                let users = iii
                    .trigger(TriggerRequest {
                        function_id: "state::list".into(),
                        payload: json!({ "scope": "users" }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                Ok(json!({ "status_code": 200, "body": users }))
            }
        })
        .description("List all users"),
    );

    // ---
    // PUT /users/:id - Update an existing user
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("users::update", move |req: ApiRequest| {
            let iii = iii_clone.clone();
            async move {
                let id = req.path_params.get("id").cloned().unwrap_or_default();
                let updates = req.body;

                let obj = match updates.as_object() {
                    Some(o) => o,
                    None => return Ok(json!({ "status_code": 400, "body": { "error": "Request body must be a JSON object" } })),
                };

                let existing = iii
                    .trigger(TriggerRequest {
                        function_id: "state::get".into(),
                        payload: json!({ "scope": "users", "key": id }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                if existing.is_null() {
                    return Ok(json!({ "status_code": 404, "body": { "error": "User not found" } }));
                }

                const IMMUTABLE_FIELDS: &[&str] = &["id", "created_at"];
                let mut ops: Vec<serde_json::Value> = obj
                    .iter()
                    .filter(|(path, _)| !IMMUTABLE_FIELDS.contains(&path.as_str()))
                    .map(|(path, value)| json!({ "type": "set", "path": path, "value": value }))
                    .collect();

                ops.push(json!({ "type": "set", "path": "updated_at", "value": chrono::Utc::now().to_rfc3339() }));

                iii.trigger(TriggerRequest {
                    function_id: "state::update".into(),
                    payload: json!({ "scope": "users", "key": id, "ops": ops }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                Ok(json!({ "status_code": 200, "body": { "id": id } }))
            }
        })
        .description("Update a user"),
    );

    // ---
    // DELETE /users/:id - Remove a user
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("users::delete", move |req: ApiRequest| {
            let iii = iii_clone.clone();
            async move {
                let id = req.path_params.get("id").cloned().unwrap_or_default();

                let existing = iii
                    .trigger(TriggerRequest {
                        function_id: "state::get".into(),
                        payload: json!({ "scope": "users", "key": id }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                if existing.is_null() {
                    return Ok(json!({ "status_code": 404, "body": { "error": "User not found" } }));
                }

                iii.trigger(TriggerRequest {
                    function_id: "state::delete".into(),
                    payload: json!({ "scope": "users", "key": id }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                Ok(json!({ "status_code": 204, "body": null }))
            }
        })
        .description("Delete a user"),
    );

    // ---
    // HTTP trigger registrations
    // ---
    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/users").method(HttpMethod::Post))
            .for_function("users::create"),
    )
    .expect("failed");

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/users/:id").method(HttpMethod::Get))
            .for_function("users::get-by-id"),
    )
    .expect("failed");

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/users").method(HttpMethod::Get))
            .for_function("users::list"),
    )
    .expect("failed");

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/users/:id").method(HttpMethod::Put))
            .for_function("users::update"),
    )
    .expect("failed");

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/users/:id").method(HttpMethod::Delete))
            .for_function("users::delete"),
    )
    .expect("failed");

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        tokio::signal::ctrl_c().await.ok();
    });
    iii.shutdown();
}
