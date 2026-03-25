use iii_sdk::{RegisterFunctionMessage, TriggerRequest, III};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::bridge::error::{error_response, success_response};

/// State group ID used to persist console flow configurations.
const FLOW_CONFIG_GROUP: &str = "__console.flowConfigs";

fn validate_flow_id(id: &str) -> Result<String, Value> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(error_response(iii_sdk::IIIError::Handler(format!(
            "Invalid flow_id: {}",
            id
        ))));
    }
    Ok(id.to_string())
}

/// Parse a boolean parameter from query_params, handling string "true"/"false" coercion.
fn parse_bool_param(input: &Value, key: &str) -> bool {
    let params = input.get("query_params").unwrap_or(input);
    match params.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

async fn handle_health(bridge: &III) -> Value {
    match bridge
        .trigger(TriggerRequest {
            function_id: "engine::health::check".to_string(),
            payload: json!({}),
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(health_data) => success_response(health_data),
        Err(err) => error_response(err),
    }
}

async fn handle_workers(bridge: &III) -> Value {
    match bridge
        .trigger(TriggerRequest {
            function_id: "engine::workers::list".to_string(),
            payload: json!({}),
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(workers_data) => success_response(workers_data),
        Err(err) => error_response(err),
    }
}

async fn handle_triggers_list(bridge: &III, input: Value) -> Value {
    let include_internal = parse_bool_param(&input, "include_internal");
    let effective_input = json!({ "include_internal": include_internal });
    match bridge
        .trigger(TriggerRequest {
            function_id: "engine::triggers::list".to_string(),
            payload: effective_input,
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(triggers_data) => success_response(triggers_data),
        Err(err) => error_response(err),
    }
}

async fn handle_functions_list(bridge: &III, input: Value) -> Value {
    let include_internal = parse_bool_param(&input, "include_internal");
    let effective_input = json!({ "include_internal": include_internal });
    match bridge
        .trigger(TriggerRequest {
            function_id: "engine::functions::list".to_string(),
            payload: effective_input,
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(functions_data) => success_response(functions_data),
        Err(err) => error_response(err),
    }
}

async fn handle_status(bridge: &III) -> Value {
    let (workers_result, functions_result, metrics_result) = tokio::join!(
        bridge.trigger(TriggerRequest {
            function_id: "engine::workers::list".to_string(),
            payload: json!({}),
            action: None,
            timeout_ms: Some(5000),
        }),
        bridge.trigger(TriggerRequest {
            function_id: "engine::functions::list".to_string(),
            payload: json!({ "include_internal": true }),
            action: None,
            timeout_ms: Some(5000),
        }),
        bridge.trigger(TriggerRequest {
            function_id: "engine::metrics::list".to_string(),
            payload: json!({}),
            action: None,
            timeout_ms: Some(5000),
        })
    );

    let workers_count = workers_result
        .ok()
        .and_then(|v| {
            v.get("workers")
                .and_then(|w| w.as_array())
                .map(|arr| arr.len())
        })
        .unwrap_or(0);

    let functions_count = functions_result
        .ok()
        .and_then(|v| {
            v.get("functions")
                .and_then(|f| f.as_array())
                .map(|arr| arr.len())
        })
        .unwrap_or(0);

    let metrics_available = metrics_result.is_ok();

    success_response(json!({
        "workers": workers_count,
        "functions": functions_count,
        "status": "running",
        "metrics_available": metrics_available
    }))
}

async fn handle_trigger_types(bridge: &III) -> Value {
    let static_types = vec![
        "api",
        "event",
        "subscribe",
        "cron",
        "log",
        "stream::join",
        "stream::leave",
        "state",
        "engine::functions-available",
    ];

    match bridge
        .trigger(TriggerRequest {
            function_id: "engine::triggers::list".to_string(),
            payload: json!({ "include_internal": true }),
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(triggers_data) => {
            let mut types = HashSet::new();

            for t in &static_types {
                types.insert(t.to_string());
            }

            if let Some(triggers) = triggers_data.get("triggers").and_then(|v| v.as_array()) {
                for trigger in triggers {
                    if let Some(trigger_type) = trigger.get("trigger_type").and_then(|v| v.as_str())
                    {
                        types.insert(trigger_type.to_string());
                    }
                }
            }

            let mut types_vec: Vec<String> = types.into_iter().collect();
            types_vec.sort();

            success_response(json!({ "trigger_types": types_vec }))
        }
        Err(_) => {
            let mut types_vec: Vec<String> = static_types.iter().map(|s| s.to_string()).collect();
            types_vec.sort();
            success_response(json!({ "trigger_types": types_vec }))
        }
    }
}

async fn handle_alerts_list(bridge: &III) -> Value {
    match bridge
        .trigger(TriggerRequest {
            function_id: "engine::alerts::list".to_string(),
            payload: json!({}),
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(data) => success_response(data),
        Err(err) => error_response(err),
    }
}

async fn handle_sampling_rules(bridge: &III) -> Value {
    match bridge
        .trigger(TriggerRequest {
            function_id: "engine::sampling::rules".to_string(),
            payload: json!({}),
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(data) => success_response(data),
        Err(err) => error_response(err),
    }
}

async fn handle_otel_logs_list(bridge: &III, input: Value) -> Value {
    let effective_input = input.get("body").cloned().unwrap_or(input);
    match bridge
        .trigger(TriggerRequest {
            function_id: "engine::logs::list".to_string(),
            payload: effective_input,
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(data) => success_response(data),
        Err(err) => error_response(err),
    }
}

async fn handle_otel_logs_clear(bridge: &III) -> Value {
    match bridge
        .trigger(TriggerRequest {
            function_id: "engine::logs::clear".to_string(),
            payload: json!({}),
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(data) => success_response(data),
        Err(err) => error_response(err),
    }
}

async fn handle_otel_traces_list(bridge: &III, input: Value) -> Value {
    let effective_input = input.get("body").cloned().unwrap_or(input);
    match bridge
        .trigger(TriggerRequest {
            function_id: "engine::traces::list".to_string(),
            payload: effective_input,
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(data) => success_response(data),
        Err(err) => error_response(err),
    }
}

async fn handle_otel_traces_clear(bridge: &III) -> Value {
    match bridge
        .trigger(TriggerRequest {
            function_id: "engine::traces::clear".to_string(),
            payload: json!({}),
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(data) => success_response(data),
        Err(err) => error_response(err),
    }
}

async fn handle_otel_traces_tree(bridge: &III, input: Value) -> Value {
    // Extract trace_id from body wrapper or top-level input
    // API triggers wrap POST body inside a "body" field
    let trace_id = input
        .get("body")
        .and_then(|b| b.get("trace_id"))
        .and_then(|v| v.as_str())
        .or_else(|| input.get("trace_id").and_then(|v| v.as_str()));

    let trace_id = match trace_id {
        Some(id) => id.to_string(),
        None => {
            return error_response(iii_sdk::IIIError::Handler(
                "Missing trace_id in request".to_string(),
            ))
        }
    };

    let tree_input = json!({ "trace_id": trace_id });

    match bridge
        .trigger(TriggerRequest {
            function_id: "engine::traces::tree".to_string(),
            payload: tree_input,
            action: None,
            timeout_ms: Some(10000),
        })
        .await
    {
        Ok(data) => success_response(data),
        Err(err) => error_response(err),
    }
}

async fn handle_metrics_detailed(bridge: &III, input: Value) -> Value {
    let effective_input = input.get("body").cloned().unwrap_or(input);
    match bridge
        .trigger(TriggerRequest {
            function_id: "engine::metrics::list".to_string(),
            payload: effective_input,
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(data) => success_response(data),
        Err(err) => error_response(err),
    }
}

async fn handle_rollups_list(bridge: &III, input: Value) -> Value {
    let effective_input = input.get("body").cloned().unwrap_or(input);
    match bridge
        .trigger(TriggerRequest {
            function_id: "engine::rollups::list".to_string(),
            payload: effective_input,
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(data) => success_response(data),
        Err(err) => error_response(err),
    }
}

async fn handle_state_groups_list(bridge: &III, _input: Value) -> Value {
    // Always use state::list_groups - no filtering by stream_name needed
    match bridge
        .trigger(TriggerRequest {
            function_id: "state::list_groups".to_string(),
            payload: json!({}),
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(data) => {
            if let Some(groups) = data.get("groups").and_then(|g| g.as_array()) {
                let group_objects: Vec<_> = groups
                    .iter()
                    .filter_map(|g| g.as_str())
                    .map(|id| json!({ "id": id, "count": 0 }))
                    .collect();

                success_response(json!({
                    "groups": group_objects,
                    "count": group_objects.len()
                }))
            } else {
                success_response(json!({ "groups": [], "count": 0 }))
            }
        }
        Err(err) => error_response(err),
    }
}

async fn handle_state_group_items(bridge: &III, input: Value) -> Value {
    // Extract scope from body or top-level input
    let scope = input
        .get("body")
        .and_then(|b| b.get("scope"))
        .and_then(|v| v.as_str())
        .or_else(|| input.get("scope").and_then(|v| v.as_str()));

    match scope {
        Some(scope) => {
            let state_input = json!({ "scope": scope });

            match bridge
                .trigger(TriggerRequest {
                    function_id: "state::list".to_string(),
                    payload: state_input,
                    action: None,
                    timeout_ms: Some(5000),
                })
                .await
            {
                Ok(data) => {
                    // state::list returns an array of items directly
                    if let Some(items) = data.as_array() {
                        success_response(json!({
                            "items": items,
                            "count": items.len()
                        }))
                    } else {
                        success_response(json!({
                            "items": [],
                            "count": 0
                        }))
                    }
                }
                Err(err) => error_response(err),
            }
        }
        None => error_response(iii_sdk::IIIError::Handler(
            "Missing scope in request".to_string(),
        )),
    }
}

async fn handle_state_item_set(bridge: &III, input: Value) -> Value {
    // Extract path parameters (from URL: /states/:group/item)
    let path_params = input.get("path_params");
    let body = input.get("body");

    let group_id = path_params
        .and_then(|p| p.get("group"))
        .and_then(|v| v.as_str())
        .or_else(|| input.get("group").and_then(|v| v.as_str()));

    let group_id = match group_id {
        Some(id) => id.to_string(),
        None => {
            return error_response(iii_sdk::IIIError::Handler(
                "Missing group in path parameters".to_string(),
            ))
        }
    };

    // Extract key and value from body
    let item_id = body
        .and_then(|b| b.get("key"))
        .and_then(|v| v.as_str())
        .or_else(|| input.get("key").and_then(|v| v.as_str()));

    let item_id = match item_id {
        Some(id) => id.to_string(),
        None => {
            return error_response(iii_sdk::IIIError::Handler(
                "Missing key in request body".to_string(),
            ))
        }
    };

    let data = body
        .and_then(|b| b.get("value"))
        .or_else(|| input.get("value"));

    let data = match data {
        Some(value) => value.clone(),
        None => {
            return error_response(iii_sdk::IIIError::Handler(
                "Missing value in request body".to_string(),
            ))
        }
    };

    let state_input = json!({
        "scope": group_id,
        "key": item_id,
        "value": data
    });

    match bridge
        .trigger(TriggerRequest {
            function_id: "state::set".to_string(),
            payload: state_input,
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(data) => success_response(data),
        Err(err) => error_response(err),
    }
}

async fn handle_state_item_delete(bridge: &III, input: Value) -> Value {
    // Extract path parameters (from URL: /states/:group/item/:key)
    let path_params = input.get("path_params");

    tracing::debug!(path_params = ?path_params, "Received state item delete input");
    let group_id = path_params
        .and_then(|p| p.get("group"))
        .and_then(|v| v.as_str())
        .or_else(|| input.get("group").and_then(|v| v.as_str()));

    let group_id = match group_id {
        Some(id) => id.to_string(),
        None => {
            return error_response(iii_sdk::IIIError::Handler(
                "Missing group in path parameters".to_string(),
            ))
        }
    };

    let item_id = path_params
        .and_then(|p| p.get("key"))
        .and_then(|v| v.as_str())
        .or_else(|| input.get("key").and_then(|v| v.as_str()));

    let item_id = match item_id {
        Some(id) => id.to_string(),
        None => {
            return error_response(iii_sdk::IIIError::Handler(
                "Missing key in path parameters".to_string(),
            ))
        }
    };

    let state_input = json!({
        "scope": group_id,
        "key": item_id
    });

    match bridge
        .trigger(TriggerRequest {
            function_id: "state::delete".to_string(),
            payload: state_input,
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(data) => success_response(data),
        Err(err) => error_response(err),
    }
}

async fn handle_adapters(bridge: &III) -> Value {
    let (workers_result, triggers_result, health_result) = tokio::join!(
        bridge.trigger(TriggerRequest {
            function_id: "engine::workers::list".to_string(),
            payload: json!({}),
            action: None,
            timeout_ms: Some(5000),
        }),
        bridge.trigger(TriggerRequest {
            function_id: "engine::triggers::list".to_string(),
            payload: json!({ "include_internal": true }),
            action: None,
            timeout_ms: Some(5000),
        }),
        bridge.trigger(TriggerRequest {
            function_id: "engine::health::check".to_string(),
            payload: json!({}),
            action: None,
            timeout_ms: Some(5000),
        })
    );

    let mut adapters: Vec<Value> = Vec::new();

    // Derive modules from trigger types
    let health_status = health_result
        .as_ref()
        .ok()
        .and_then(|v| v.get("status"))
        .and_then(|s| s.as_str())
        .unwrap_or("unknown");

    let mut seen_modules = HashSet::new();

    if let Ok(triggers_data) = &triggers_result {
        if let Some(triggers) = triggers_data.get("triggers").and_then(|v| v.as_array()) {
            for trigger in triggers {
                let trigger_type = trigger
                    .get("trigger_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");

                // Derive module from trigger type
                let module_id = match trigger_type {
                    "api" => "rest_api",
                    "cron" => "cron",
                    "event" => "event",
                    t if t.starts_with("stream") => "streams",
                    "subscribe" => "streams",
                    _ => trigger_type,
                };

                if seen_modules.insert(module_id.to_string()) {
                    adapters.push(json!({
                        "id": module_id,
                        "type": "module",
                        "status": "active",
                        "health": if health_status == "healthy" { "healthy" } else { "degraded" },
                        "description": format!("{} module", module_id),
                        "internal": false
                    }));
                }
            }

            // Aggregate internal flag per trigger_type: true if ALL triggers are internal
            let mut trigger_internal: std::collections::HashMap<String, bool> =
                std::collections::HashMap::new();
            for trigger in triggers {
                let trigger_type = trigger
                    .get("trigger_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let function_id = trigger
                    .get("function_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let is_internal = function_id.starts_with("engine::");

                let entry = trigger_internal
                    .entry(trigger_type.to_string())
                    .or_insert(true);
                if !is_internal {
                    *entry = false;
                }
            }

            for (trigger_type, is_internal) in &trigger_internal {
                adapters.push(json!({
                    "id": trigger_type,
                    "type": "trigger",
                    "status": "active",
                    "health": "healthy",
                    "description": format!("{} trigger handler", trigger_type),
                    "internal": is_internal
                }));
            }
        }
    }

    // Always add devtools and observability modules (they're running if the console is connected)
    if seen_modules.insert("devtools".to_string()) {
        adapters.push(json!({
            "id": "devtools",
            "type": "module",
            "status": "active",
            "health": "healthy",
            "description": "devtools module",
            "internal": true
        }));
    }
    if seen_modules.insert("observability".to_string()) {
        adapters.push(json!({
            "id": "observability",
            "type": "module",
            "status": "active",
            "health": if health_status == "healthy" { "healthy" } else { "degraded" },
            "description": "observability module",
            "internal": true
        }));
    }

    // Add worker pools
    if let Ok(workers_data) = &workers_result {
        if let Some(workers) = workers_data.get("workers").and_then(|v| v.as_array()) {
            let mut pool_counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for worker in workers {
                let runtime = worker
                    .get("runtime")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                *pool_counts.entry(runtime.to_string()).or_insert(0) += 1;
            }

            for (pool_id, count) in &pool_counts {
                adapters.push(json!({
                    "id": pool_id,
                    "type": "worker_pool",
                    "status": "active",
                    "health": "healthy",
                    "description": format!("{} worker pool", pool_id),
                    "count": count,
                    "internal": false
                }));
            }
        }
    }

    let count = adapters.len();
    success_response(json!({
        "adapters": adapters,
        "count": count
    }))
}

async fn handle_streams_list(bridge: &III) -> Value {
    match bridge
        .trigger(TriggerRequest {
            function_id: "stream::list_all".to_string(),
            payload: json!({}),
            action: None,
            timeout_ms: Some(10000),
        })
        .await
    {
        Ok(data) => {
            // Transform to frontend format
            if let Some(streams) = data.get("stream").and_then(|s| s.as_array()) {
                let stream_objects: Vec<_> = streams
                    .iter()
                    .map(|stream| {
                        let id = stream.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let groups = stream
                            .get("groups")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|g| g.as_str())
                                    .map(String::from)
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        let is_internal = id.starts_with("iii.") || id.starts_with("iii:");

                        json!({
                            "id": id,
                            "type": if is_internal { "system" } else { "user" },
                            "description": format!("{} stream", id),
                            "groups": groups,
                            "status": "active",
                            "internal": is_internal
                        })
                    })
                    .collect();

                success_response(json!({
                    "streams": stream_objects,
                    "count": stream_objects.len(),
                    "websocket_port": 3112
                }))
            } else {
                success_response(json!({ "streams": [], "count": 0, "websocket_port": 3112 }))
            }
        }
        Err(err) => error_response(err),
    }
}

async fn handle_flow_config_get(bridge: &III, input: Value) -> Value {
    // Get flow_id from path_params or query_params
    let flow_id = input
        .get("path_params")
        .and_then(|p| p.get("flow_id"))
        .and_then(|v| v.as_str())
        .or_else(|| {
            input
                .get("query_params")
                .and_then(|p| p.get("flow_id"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| input.get("flow_id").and_then(|v| v.as_str()));

    let flow_id = match flow_id {
        Some(id) => id.to_string(),
        None => {
            return error_response(iii_sdk::IIIError::Handler(
                "Missing flow_id parameter".to_string(),
            ))
        }
    };

    let flow_id = match validate_flow_id(&flow_id) {
        Ok(id) => id,
        Err(err) => return err,
    };

    // Try to get config from the engine's state
    let state_input = json!({
        "scope": FLOW_CONFIG_GROUP,
        "key": flow_id
    });

    match bridge
        .trigger(TriggerRequest {
            function_id: "state::get".to_string(),
            payload: state_input,
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(data) => {
            if data.is_null() {
                success_response(json!({ "id": flow_id, "config": {} }))
            } else {
                success_response(data)
            }
        }
        Err(_) => {
            // Return empty config if state module doesn't have it
            success_response(json!({ "id": flow_id, "config": {} }))
        }
    }
}

async fn handle_invoke(bridge: &III, input: Value) -> Value {
    let body = input.get("body").unwrap_or(&input);

    let function_id = body
        .get("function_id")
        .and_then(|v| v.as_str())
        .or_else(|| input.get("function_id").and_then(|v| v.as_str()));

    let function_id = match function_id {
        Some(id) => id.to_string(),
        None => {
            return error_response(iii_sdk::IIIError::Handler(
                "Missing function_id in request".to_string(),
            ))
        }
    };

    let data = body
        .get("input")
        .or_else(|| input.get("input"))
        .cloned()
        .unwrap_or(json!({}));

    match bridge
        .trigger(TriggerRequest {
            function_id: function_id.clone(),
            payload: data,
            action: None,
            timeout_ms: Some(30000),
        })
        .await
    {
        Ok(result) => success_response(result),
        Err(err) => error_response(err),
    }
}

async fn handle_cron_trigger(bridge: &III, input: Value) -> Value {
    let body = input.get("body").unwrap_or(&input);

    let trigger_id = body
        .get("trigger_id")
        .and_then(|v| v.as_str())
        .or_else(|| input.get("trigger_id").and_then(|v| v.as_str()));

    let trigger_id = match trigger_id {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => {
            return error_response(iii_sdk::IIIError::Handler(
                "Missing trigger_id in request".to_string(),
            ))
        }
    };

    let provided_function_id = body
        .get("function_id")
        .and_then(|v| v.as_str())
        .or_else(|| input.get("function_id").and_then(|v| v.as_str()))
        .map(|v| v.to_string());

    let function_id = if let Some(function_id) = provided_function_id {
        function_id
    } else {
        let triggers_data = match bridge
            .trigger(TriggerRequest {
                function_id: "engine::triggers::list".to_string(),
                payload: json!({ "include_internal": true }),
                action: None,
                timeout_ms: Some(5000),
            })
            .await
        {
            Ok(data) => data,
            Err(err) => return error_response(err),
        };

        let trigger_match = triggers_data
            .get("triggers")
            .and_then(|v| v.as_array())
            .and_then(|triggers| {
                triggers.iter().find(|trigger| {
                    trigger
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(|id| id == trigger_id)
                        .unwrap_or(false)
                })
            });

        let trigger = match trigger_match {
            Some(trigger) => trigger,
            None => {
                return error_response(iii_sdk::IIIError::Handler(format!(
                    "Cron trigger '{}' not found",
                    trigger_id
                )))
            }
        };

        let trigger_type = trigger
            .get("trigger_type")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if trigger_type != "cron" {
            return error_response(iii_sdk::IIIError::Handler(format!(
                "Trigger '{}' is not a cron trigger",
                trigger_id
            )));
        }

        match trigger.get("function_id").and_then(|v| v.as_str()) {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => {
                return error_response(iii_sdk::IIIError::Handler(format!(
                    "Cron trigger '{}' has no function_id",
                    trigger_id
                )))
            }
        }
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());
    let payload = json!({
        "trigger": "cron",
        "job_id": trigger_id,
        "scheduled_time": now,
        "actual_time": now,
        "manual": true,
        "source": "console"
    });

    match bridge
        .trigger(TriggerRequest {
            function_id: function_id.clone(),
            payload,
            action: None,
            timeout_ms: Some(30000),
        })
        .await
    {
        Ok(result) => success_response(json!({
            "trigger_id": trigger_id,
            "function_id": function_id,
            "result": result
        })),
        Err(err) => error_response(err),
    }
}

async fn handle_flow_config_save(bridge: &III, input: Value) -> Value {
    let body = input.get("body").cloned().unwrap_or(input.clone());

    let flow_id = input
        .get("path_params")
        .and_then(|p| p.get("flow_id"))
        .and_then(|v| v.as_str())
        .or_else(|| body.get("id").and_then(|v| v.as_str()));

    let flow_id = match flow_id {
        Some(id) => id.to_string(),
        None => {
            return error_response(iii_sdk::IIIError::Handler(
                "Missing flow_id parameter".to_string(),
            ))
        }
    };

    let flow_id = match validate_flow_id(&flow_id) {
        Ok(id) => id,
        Err(err) => return err,
    };

    let config = body.get("config").cloned().unwrap_or(json!({}));
    let data = json!({ "id": flow_id, "config": config });

    let state_input = json!({
        "scope": FLOW_CONFIG_GROUP,
        "key": flow_id,
        "value": data
    });

    match bridge
        .trigger(TriggerRequest {
            function_id: "state::set".to_string(),
            payload: state_input,
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(_) => success_response(json!({ "message": "Flow config saved successfully" })),
        Err(err) => error_response(err),
    }
}

async fn handle_queues_list(bridge: &III) -> Value {
    match bridge
        .trigger(TriggerRequest {
            function_id: "engine::queue::list_topics".to_string(),
            payload: json!({}),
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(data) => success_response(json!({ "queues": data })),
        Err(err) => error_response(err),
    }
}

async fn handle_queue_detail(bridge: &III, input: Value) -> Value {
    let topic = input
        .get("path_params")
        .and_then(|p| p.get("topic"))
        .and_then(|v| v.as_str())
        .or_else(|| input.get("topic").and_then(|v| v.as_str()));

    let topic = match topic {
        Some(t) => t.to_string(),
        None => {
            return error_response(iii_sdk::IIIError::Handler(
                "Missing topic in path parameters".to_string(),
            ))
        }
    };

    match bridge
        .trigger(TriggerRequest {
            function_id: "engine::queue::topic_stats".to_string(),
            payload: json!({ "topic": topic }),
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(data) => success_response(json!({ "topic": topic, "stats": data })),
        Err(err) => error_response(err),
    }
}

async fn handle_queue_publish(bridge: &III, input: Value) -> Value {
    let topic = input
        .get("path_params")
        .and_then(|p| p.get("topic"))
        .and_then(|v| v.as_str())
        .or_else(|| input.get("topic").and_then(|v| v.as_str()));

    let topic = match topic {
        Some(t) => t.to_string(),
        None => {
            return error_response(iii_sdk::IIIError::Handler(
                "Missing topic in path parameters".to_string(),
            ))
        }
    };

    let body = input.get("body").unwrap_or(&input);
    let data = body.get("data").cloned().unwrap_or(json!({}));

    match bridge
        .trigger(TriggerRequest {
            function_id: "enqueue".to_string(),
            payload: json!({ "topic": topic, "data": data }),
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(_) => success_response(json!({ "message": "Message published", "topic": topic })),
        Err(err) => error_response(err),
    }
}

async fn handle_dlq_list(bridge: &III) -> Value {
    match bridge
        .trigger(TriggerRequest {
            function_id: "engine::queue::dlq_topics".to_string(),
            payload: json!({}),
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(data) => success_response(json!({ "topics": data })),
        Err(err) => error_response(err),
    }
}

async fn handle_dlq_messages(bridge: &III, input: Value) -> Value {
    let topic = input
        .get("path_params")
        .and_then(|p| p.get("topic"))
        .and_then(|v| v.as_str())
        .or_else(|| input.get("topic").and_then(|v| v.as_str()));

    let topic = match topic {
        Some(t) => t.to_string(),
        None => {
            return error_response(iii_sdk::IIIError::Handler(
                "Missing topic in path parameters".to_string(),
            ))
        }
    };

    let body = input.get("body").unwrap_or(&input);
    let offset = body.get("offset").and_then(|v| v.as_u64()).unwrap_or(0);
    let limit = body.get("limit").and_then(|v| v.as_u64()).unwrap_or(50);

    match bridge
        .trigger(TriggerRequest {
            function_id: "engine::queue::dlq_messages".to_string(),
            payload: json!({ "topic": topic, "offset": offset, "limit": limit }),
            action: None,
            timeout_ms: Some(5000),
        })
        .await
    {
        Ok(data) => success_response(json!({ "topic": topic, "messages": data })),
        Err(err) => error_response(err),
    }
}

async fn handle_dlq_redrive(bridge: &III, input: Value) -> Value {
    let topic = input
        .get("path_params")
        .and_then(|p| p.get("topic"))
        .and_then(|v| v.as_str())
        .or_else(|| input.get("topic").and_then(|v| v.as_str()));

    let topic = match topic {
        Some(t) => t.to_string(),
        None => {
            return error_response(iii_sdk::IIIError::Handler(
                "Missing topic in path parameters".to_string(),
            ))
        }
    };

    match bridge
        .trigger(TriggerRequest {
            function_id: "iii::queue::redrive".to_string(),
            payload: json!({ "queue": topic }),
            action: None,
            timeout_ms: Some(30000),
        })
        .await
    {
        Ok(data) => success_response(data),
        Err(err) => error_response(err),
    }
}

async fn handle_dlq_redrive_message(bridge: &III, input: Value) -> Value {
    let topic = input
        .get("path_params")
        .and_then(|p| p.get("topic"))
        .and_then(|v| v.as_str());

    let message_id = input
        .get("path_params")
        .and_then(|p| p.get("id"))
        .and_then(|v| v.as_str());

    let (topic, message_id) = match (topic, message_id) {
        (Some(t), Some(m)) => (t.to_string(), m.to_string()),
        _ => {
            return error_response(iii_sdk::IIIError::Handler(
                "Missing topic or message id in path parameters".to_string(),
            ))
        }
    };

    match bridge
        .trigger(TriggerRequest {
            function_id: "iii::queue::redrive_message".to_string(),
            payload: json!({ "queue": topic, "message_id": message_id }),
            action: None,
            timeout_ms: Some(30000),
        })
        .await
    {
        Ok(data) => success_response(data),
        Err(err) => error_response(err),
    }
}

async fn handle_dlq_discard_message(bridge: &III, input: Value) -> Value {
    let topic = input
        .get("path_params")
        .and_then(|p| p.get("topic"))
        .and_then(|v| v.as_str());

    let message_id = input
        .get("path_params")
        .and_then(|p| p.get("id"))
        .and_then(|v| v.as_str());

    let (topic, message_id) = match (topic, message_id) {
        (Some(t), Some(m)) => (t.to_string(), m.to_string()),
        _ => {
            return error_response(iii_sdk::IIIError::Handler(
                "Missing topic or message id in path parameters".to_string(),
            ))
        }
    };

    match bridge
        .trigger(TriggerRequest {
            function_id: "iii::queue::discard_message".to_string(),
            payload: json!({ "queue": topic, "message_id": message_id }),
            action: None,
            timeout_ms: Some(30000),
        })
        .await
    {
        Ok(data) => success_response(data),
        Err(err) => error_response(err),
    }
}

fn reg_fn_msg(id: &str) -> RegisterFunctionMessage {
    RegisterFunctionMessage {
        id: id.to_string(),
        description: None,
        request_format: None,
        response_format: None,
        metadata: None,
        invocation: None,
    }
}

pub fn register_functions(bridge: &III) {
    let b = bridge.clone();
    bridge.register_function_with(reg_fn_msg("engine::console::health"), move |_input| {
        let bridge = b.clone();
        async move { Ok(handle_health(&bridge).await) }
    });

    let b = bridge.clone();
    bridge.register_function_with(reg_fn_msg("engine::console::workers"), move |_input| {
        let bridge = b.clone();
        async move { Ok(handle_workers(&bridge).await) }
    });

    let b = bridge.clone();
    bridge.register_function_with(reg_fn_msg("engine::console::functions"), move |input| {
        let bridge = b.clone();
        async move { Ok(handle_functions_list(&bridge, input).await) }
    });

    let b = bridge.clone();
    bridge.register_function_with(reg_fn_msg("engine::console::triggers"), move |input| {
        let bridge = b.clone();
        async move { Ok(handle_triggers_list(&bridge, input).await) }
    });

    let b = bridge.clone();
    bridge.register_function_with(reg_fn_msg("engine::console::status"), move |_input| {
        let bridge = b.clone();
        async move { Ok(handle_status(&bridge).await) }
    });

    let b = bridge.clone();
    bridge.register_function_with(
        reg_fn_msg("engine::console::trigger_types"),
        move |_input| {
            let bridge = b.clone();
            async move { Ok(handle_trigger_types(&bridge).await) }
        },
    );

    let b = bridge.clone();
    bridge.register_function_with(reg_fn_msg("engine::console::alerts_list"), move |_input| {
        let bridge = b.clone();
        async move { Ok(handle_alerts_list(&bridge).await) }
    });

    let b = bridge.clone();
    bridge.register_function_with(
        reg_fn_msg("engine::console::sampling_rules"),
        move |_input| {
            let bridge = b.clone();
            async move { Ok(handle_sampling_rules(&bridge).await) }
        },
    );

    let b = bridge.clone();
    bridge.register_function_with(
        reg_fn_msg("engine::console::otel_logs_list"),
        move |input| {
            let bridge = b.clone();
            async move { Ok(handle_otel_logs_list(&bridge, input).await) }
        },
    );

    let b = bridge.clone();
    bridge.register_function_with(
        reg_fn_msg("engine::console::otel_logs_clear"),
        move |_input| {
            let bridge = b.clone();
            async move { Ok(handle_otel_logs_clear(&bridge).await) }
        },
    );

    let b = bridge.clone();
    bridge.register_function_with(
        reg_fn_msg("engine::console::otel_traces_list"),
        move |input| {
            let bridge = b.clone();
            async move { Ok(handle_otel_traces_list(&bridge, input).await) }
        },
    );

    let b = bridge.clone();
    bridge.register_function_with(
        reg_fn_msg("engine::console::otel_traces_clear"),
        move |_input| {
            let bridge = b.clone();
            async move { Ok(handle_otel_traces_clear(&bridge).await) }
        },
    );

    let b = bridge.clone();
    bridge.register_function_with(
        reg_fn_msg("engine::console::otel_traces_tree"),
        move |input| {
            let bridge = b.clone();
            async move { Ok(handle_otel_traces_tree(&bridge, input).await) }
        },
    );

    let b = bridge.clone();
    bridge.register_function_with(
        reg_fn_msg("engine::console::metrics_detailed"),
        move |input| {
            let bridge = b.clone();
            async move { Ok(handle_metrics_detailed(&bridge, input).await) }
        },
    );

    let b = bridge.clone();
    bridge.register_function_with(reg_fn_msg("engine::console::rollups_list"), move |input| {
        let bridge = b.clone();
        async move { Ok(handle_rollups_list(&bridge, input).await) }
    });

    let b = bridge.clone();
    bridge.register_function_with(
        reg_fn_msg("engine::console::state_groups_list"),
        move |input| {
            let bridge = b.clone();
            async move { Ok(handle_state_groups_list(&bridge, input).await) }
        },
    );

    let b = bridge.clone();
    bridge.register_function_with(
        reg_fn_msg("engine::console::state_group_items"),
        move |input| {
            let bridge = b.clone();
            async move { Ok(handle_state_group_items(&bridge, input).await) }
        },
    );

    let b = bridge.clone();
    bridge.register_function_with(
        reg_fn_msg("engine::console::state_item_set"),
        move |input| {
            let bridge = b.clone();
            async move { Ok(handle_state_item_set(&bridge, input).await) }
        },
    );

    let b = bridge.clone();
    bridge.register_function_with(
        reg_fn_msg("engine::console::state_item_delete"),
        move |input| {
            let bridge = b.clone();
            async move { Ok(handle_state_item_delete(&bridge, input).await) }
        },
    );

    let b = bridge.clone();
    bridge.register_function_with(reg_fn_msg("engine::console::adapters"), move |_input| {
        let bridge = b.clone();
        async move { Ok(handle_adapters(&bridge).await) }
    });

    let b = bridge.clone();
    bridge.register_function(
        (reg_fn_msg("engine::console::streams_list"), move |_input| {
            let bridge = b.clone();
            async move { Ok(handle_streams_list(&bridge).await) }
        }),
    );

    let b = bridge.clone();
    bridge.register_function_with(
        reg_fn_msg("engine::console::flow_config_get"),
        move |input| {
            let bridge = b.clone();
            async move { Ok(handle_flow_config_get(&bridge, input).await) }
        },
    );

    let b = bridge.clone();
    bridge.register_function_with(
        reg_fn_msg("engine::console::flow_config_save"),
        move |input| {
            let bridge = b.clone();
            async move { Ok(handle_flow_config_save(&bridge, input).await) }
        },
    );

    let b = bridge.clone();
    bridge.register_function_with(reg_fn_msg("engine::console::invoke"), move |input| {
        let bridge = b.clone();
        async move { Ok(handle_invoke(&bridge, input).await) }
    });

    let b = bridge.clone();
    bridge.register_function_with(reg_fn_msg("engine::console::cron_trigger"), move |input| {
        let bridge = b.clone();
        async move { Ok(handle_cron_trigger(&bridge, input).await) }
    });

    // Queue management
    let b = bridge.clone();
    bridge.register_function_with(reg_fn_msg("engine::console::queues_list"), move |_input| {
        let bridge = b.clone();
        async move { Ok(handle_queues_list(&bridge).await) }
    });

    let b = bridge.clone();
    bridge.register_function_with(reg_fn_msg("engine::console::queue_detail"), move |input| {
        let bridge = b.clone();
        async move { Ok(handle_queue_detail(&bridge, input).await) }
    });

    let b = bridge.clone();
    bridge.register_function_with(reg_fn_msg("engine::console::queue_publish"), move |input| {
        let bridge = b.clone();
        async move { Ok(handle_queue_publish(&bridge, input).await) }
    });

    // DLQ management
    let b = bridge.clone();
    bridge.register_function_with(reg_fn_msg("engine::console::dlq_list"), move |_input| {
        let bridge = b.clone();
        async move { Ok(handle_dlq_list(&bridge).await) }
    });

    let b = bridge.clone();
    bridge.register_function_with(reg_fn_msg("engine::console::dlq_messages"), move |input| {
        let bridge = b.clone();
        async move { Ok(handle_dlq_messages(&bridge, input).await) }
    });

    let b = bridge.clone();
    bridge.register_function_with(reg_fn_msg("engine::console::dlq_redrive"), move |input| {
        let bridge = b.clone();
        async move { Ok(handle_dlq_redrive(&bridge, input).await) }
    });

    let b = bridge.clone();
    bridge.register_function_with(
        reg_fn_msg("engine::console::dlq_redrive_message"),
        move |input| {
            let bridge = b.clone();
            async move { Ok(handle_dlq_redrive_message(&bridge, input).await) }
        },
    );

    let b = bridge.clone();
    bridge.register_function_with(
        reg_fn_msg("engine::console::dlq_discard_message"),
        move |input| {
            let bridge = b.clone();
            async move { Ok(handle_dlq_discard_message(&bridge, input).await) }
        },
    );
}
