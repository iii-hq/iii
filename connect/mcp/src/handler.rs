use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use iii_sdk::{
    FunctionInfo, FunctionsAvailableGuard, III, RegisterFunctionMessage, RegisterTriggerInput,
    Trigger, TriggerAction, TriggerRequest, Value,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{Mutex, mpsc};

use crate::prompts;
use crate::worker_manager::{WorkerCreateParams, WorkerManager, WorkerStopParams};

const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
const INTERNAL_ERROR: i32 = -32603;
const INVALID_PARAMS: i32 = -32602;
const METHOD_NOT_FOUND: i32 = -32601;

fn has_metadata_flag(f: &FunctionInfo, key: &str) -> bool {
    f.metadata
        .as_ref()
        .and_then(|m| m.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

impl JsonRpcResponse {
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

pub struct McpHandler {
    initialized: AtomicBool,
    iii: III,
    expose_all: bool,
    worker_manager: WorkerManager,
    triggers: Mutex<HashMap<String, Trigger>>,
    notification_rx: tokio::sync::Mutex<mpsc::Receiver<String>>,
    _functions_guard: FunctionsAvailableGuard,
}

impl McpHandler {
    pub fn new(iii: III, engine_url: String, expose_all: bool) -> Self {
        let (tx, notification_rx) = mpsc::channel(16);

        let guard = iii.on_functions_available(move |_| {
            let n = json!({ "jsonrpc": "2.0", "method": "notifications/tools/list_changed" });
            if let Ok(json) = serde_json::to_string(&n) {
                if tx.try_send(json).is_err() {
                    tracing::warn!("Notification channel full, tools/list_changed dropped");
                }
            }
        });

        Self {
            initialized: AtomicBool::new(false),
            iii,
            expose_all,
            worker_manager: WorkerManager::new(engine_url),
            triggers: Mutex::new(HashMap::new()),
            notification_rx: tokio::sync::Mutex::new(notification_rx),
            _functions_guard: guard,
        }
    }

    pub async fn take_notification(&self) -> Option<String> {
        self.notification_rx.lock().await.try_recv().ok()
    }

    pub async fn handle(&self, body: Value) -> Option<Value> {
        let method = body.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let id = body.get("id").cloned();

        if method.starts_with("notifications/") {
            if method == "notifications/initialized" {
                self.initialized.store(true, Ordering::SeqCst);
            }
            return None;
        }

        if method == "initialize" {
            self.initialized.store(true, Ordering::SeqCst);
        } else if !self.initialized.load(Ordering::SeqCst) && method != "ping" {
            return Some(json!(JsonRpcResponse::error(
                id,
                INTERNAL_ERROR,
                "Not initialized"
            )));
        }

        Some(self.dispatch(&body).await)
    }

    async fn dispatch(&self, body: &Value) -> Value {
        let method = body.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let id = body.get("id").cloned();
        let params = body.get("params").cloned();

        let result = match method {
            "initialize" => Ok(initialize_result()),
            "ping" => Ok(json!({})),
            "tools/list" => self.tools_list().await,
            "tools/call" => self.tools_call(params).await,
            "resources/list" => Ok(self.resources_list()),
            "resources/read" => self.resources_read(params).await,
            "resources/templates/list" => Ok(json!({ "resourceTemplates": [] })),
            "prompts/list" => Ok(prompts::list()),
            "prompts/get" => Ok(prompts::get(params)),
            _ => Err(format!("Unknown method: {}", method)),
        };

        json!(match result {
            Ok(value) => JsonRpcResponse::success(id, value),
            Err(err) => JsonRpcResponse::error(id, METHOD_NOT_FOUND, err),
        })
    }

    async fn tools_list(&self) -> Result<Value, String> {
        let mut tools = builtin_tools();
        if let Ok(functions) = self.iii.list_functions().await {
            tools.extend(
                functions
                    .iter()
                    .filter(|f| self.expose_all || has_metadata_flag(f, "mcp.expose"))
                    .map(function_to_tool),
            );
        }
        Ok(json!({ "tools": tools }))
    }

    async fn tools_call(&self, params: Option<Value>) -> Result<Value, String> {
        let params: CallParams = parse(params)?;

        match params.name.as_str() {
            "iii_worker_register" => {
                let p: WorkerCreateParams = parse(Some(params.arguments))?;
                return match self.worker_manager.create_worker(p).await {
                    Ok(r) => Ok(tool_json(&serde_json::to_value(&r).unwrap_or_default())),
                    Err(e) => Ok(tool_error(&e)),
                };
            }
            "iii_worker_stop" => {
                let p: WorkerStopParams = parse(Some(params.arguments))?;
                return match self.worker_manager.stop_worker(p).await {
                    Ok(r) => Ok(tool_json(&serde_json::to_value(&r).unwrap_or_default())),
                    Err(e) => Ok(tool_error(&e)),
                };
            }
            "iii_trigger_register" => return Ok(self.trigger_register(params.arguments).await),
            "iii_trigger_unregister" => return Ok(self.trigger_unregister(params.arguments).await),
            "iii_trigger_void" => {
                let fid = str_field(&params.arguments, "function_id");
                if fid.is_empty() {
                    return Ok(tool_error("Missing required field: function_id"));
                }
                let payload = params
                    .arguments
                    .get("payload")
                    .cloned()
                    .unwrap_or(json!({}));
                return match self
                    .iii
                    .trigger(TriggerRequest {
                        function_id: fid.clone(),
                        payload,
                        action: Some(TriggerAction::Void),
                        timeout_ms: None,
                    })
                    .await
                {
                    Ok(_) => Ok(tool_result(&format!("Triggered (void): {}", fid))),
                    Err(e) => Ok(tool_error(&format!("Error: {}", e))),
                };
            }
            "iii_trigger_enqueue" => {
                let fid = str_field(&params.arguments, "function_id");
                if fid.is_empty() {
                    return Ok(tool_error("Missing required field: function_id"));
                }
                let payload = params
                    .arguments
                    .get("payload")
                    .cloned()
                    .unwrap_or(json!({}));
                let queue = str_field_or(&params.arguments, "queue", "default");
                return match self
                    .iii
                    .trigger(TriggerRequest {
                        function_id: fid,
                        payload,
                        action: Some(TriggerAction::Enqueue { queue }),
                        timeout_ms: None,
                    })
                    .await
                {
                    Ok(r) => Ok(tool_json(&r)),
                    Err(e) => Ok(tool_error(&format!("Error: {}", e))),
                };
            }
            _ => {}
        }

        let function_id = params.name.replace("__", "::");
        match self
            .iii
            .trigger(TriggerRequest {
                function_id: function_id.clone(),
                payload: params.arguments,
                action: None,
                timeout_ms: None,
            })
            .await
        {
            Ok(result) => Ok(tool_json(&result)),
            Err(err) => {
                tracing::error!(function_id = %function_id, error = %err, "Trigger failed");
                Ok(tool_error(&format!("Error: {}", err)))
            }
        }
    }

    fn resources_list(&self) -> Value {
        json!({ "resources": [
            { "uri": "iii://functions", "name": "Functions", "mimeType": "application/json" },
            { "uri": "iii://workers", "name": "Workers", "mimeType": "application/json" },
            { "uri": "iii://triggers", "name": "Triggers", "mimeType": "application/json" },
            { "uri": "iii://context", "name": "Context", "mimeType": "application/json" },
        ]})
    }

    async fn resources_read(&self, params: Option<Value>) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct P {
            uri: String,
        }
        let p: P = parse(params)?;
        let (text, mime) = match p.uri.as_str() {
            "iii://functions" => {
                let v = self
                    .iii
                    .list_functions()
                    .await
                    .map_err(|e| format!("{}", e))?;
                (
                    serde_json::to_string_pretty(&v).unwrap_or_else(|_| "[]".into()),
                    "application/json",
                )
            }
            "iii://workers" => {
                let v = self
                    .iii
                    .list_workers()
                    .await
                    .map_err(|e| format!("{}", e))?;
                (
                    serde_json::to_string_pretty(&v).unwrap_or_else(|_| "[]".into()),
                    "application/json",
                )
            }
            "iii://triggers" => {
                let v = self
                    .iii
                    .list_triggers(true)
                    .await
                    .map_err(|e| format!("{}", e))?;
                (
                    serde_json::to_string_pretty(&v).unwrap_or_else(|_| "[]".into()),
                    "application/json",
                )
            }
            "iii://context" => (
                serde_json::to_string_pretty(&json!({
                    "sdk_version": "0.10.0",
                    "function_id_delimiter": "::",
                    "metadata_filtering": { "mcp.expose": true, "a2a.expose": true }
                }))
                .unwrap(),
                "application/json",
            ),
            _ => return Err(format!("Resource not found: {}", p.uri)),
        };
        Ok(json!({ "contents": [{ "uri": p.uri, "mimeType": mime, "text": text }] }))
    }

    async fn trigger_register(&self, args: Value) -> Value {
        #[derive(Deserialize)]
        struct P {
            trigger_type: String,
            function_id: String,
            config: Value,
        }
        let p: P = match parse(Some(args)) {
            Ok(p) => p,
            Err(e) => return tool_error(&e),
        };
        match self.iii.register_trigger(RegisterTriggerInput {
            trigger_type: p.trigger_type.clone(),
            function_id: p.function_id.clone(),
            config: p.config,
        }) {
            Ok(trigger) => {
                let id = uuid::Uuid::new_v4().to_string();
                self.triggers.lock().await.insert(id.clone(), trigger);
                tool_json(
                    &json!({ "id": id, "trigger_type": p.trigger_type, "function_id": p.function_id }),
                )
            }
            Err(e) => tool_error(&format!("Error: {}", e)),
        }
    }

    async fn trigger_unregister(&self, args: Value) -> Value {
        #[derive(Deserialize)]
        struct P {
            id: String,
        }
        let p: P = match parse(Some(args)) {
            Ok(p) => p,
            Err(e) => return tool_error(&e),
        };
        match self.triggers.lock().await.remove(&p.id) {
            Some(trigger) => {
                trigger.unregister();
                tool_json(&json!({ "id": p.id, "message": "Unregistered" }))
            }
            None => tool_error(&format!("Trigger not found: {}", p.id)),
        }
    }
}

pub fn register_http(iii: &III, expose_all: bool) {
    let iii_fn = iii.clone();
    iii.register_function_with(
        RegisterFunctionMessage {
            id: "mcp::handler".to_string(),
            description: Some("MCP JSON-RPC handler".to_string()),
            request_format: Some(json!({ "type": "object", "properties": { "body": { "type": "object" } } })),
            response_format: None, metadata: None, invocation: None,
        },
        move |input: Value| {
            let iii_inner = iii_fn.clone();
            async move {
                let body = input.get("body").cloned().unwrap_or(input);
                let response = dispatch_http(&iii_inner, &body, expose_all).await;
                Ok(json!({ "status_code": 200, "headers": { "content-type": "application/json" }, "body": response }))
            }
        },
    );

    if let Err(e) = iii.register_trigger(RegisterTriggerInput {
        trigger_type: "http".to_string(),
        function_id: "mcp::handler".to_string(),
        config: json!({ "api_path": "/mcp", "http_method": "POST" }),
    }) {
        tracing::error!(error = %e, "Failed to register MCP HTTP trigger");
    } else {
        tracing::info!("MCP Streamable HTTP registered: POST /mcp");
    }
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": { "tools": { "listChanged": true }, "resources": { "subscribe": false, "listChanged": true }, "prompts": { "listChanged": false } },
        "serverInfo": { "name": "iii-mcp", "version": env!("CARGO_PKG_VERSION") },
        "instructions": "iii-engine MCP server (SDK v0.10). Functions with metadata mcp.expose: true are exposed as tools."
    })
}

async fn dispatch_http(iii: &III, body: &Value, expose_all: bool) -> Value {
    let method = body.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let id = body.get("id").cloned();
    let params = body.get("params").cloned();

    if method.starts_with("notifications/") {
        return json!(null);
    }

    let result = match method {
        "initialize" => Ok(initialize_result()),
        "ping" => Ok(json!({})),
        "tools/list" => {
            let mut tools = Vec::new();
            if let Ok(fns) = iii.list_functions().await {
                tools.extend(
                    fns.iter()
                        .filter(|f| expose_all || has_metadata_flag(f, "mcp.expose"))
                        .map(function_to_tool),
                );
            }
            Ok(json!({ "tools": tools }))
        }
        "tools/call" => {
            let p: CallParams = match params {
                Some(p) => match serde_json::from_value(p) {
                    Ok(p) => p,
                    Err(e) => {
                        return json!(JsonRpcResponse::error(id, INVALID_PARAMS, format!("{}", e)));
                    }
                },
                None => return json!(JsonRpcResponse::error(id, INVALID_PARAMS, "Missing params")),
            };
            let function_id = p.name.replace("__", "::");
            match iii
                .trigger(TriggerRequest {
                    function_id,
                    payload: p.arguments,
                    action: None,
                    timeout_ms: None,
                })
                .await
            {
                Ok(r) => Ok(tool_json(&r)),
                Err(e) => Ok(tool_error(&format!("Error: {}", e))),
            }
        }
        "resources/list" => Ok(json!({ "resources": [
            { "uri": "iii://functions", "name": "Functions", "mimeType": "application/json" },
            { "uri": "iii://workers", "name": "Workers", "mimeType": "application/json" },
            { "uri": "iii://triggers", "name": "Triggers", "mimeType": "application/json" },
        ]})),
        "prompts/list" => Ok(prompts::list()),
        "prompts/get" => Ok(prompts::get(params)),
        _ => Err(format!("Unknown method: {}", method)),
    };

    json!(match result {
        Ok(v) => JsonRpcResponse::success(id, v),
        Err(e) => JsonRpcResponse::error(id, METHOD_NOT_FOUND, e),
    })
}

fn parse<T: serde::de::DeserializeOwned>(params: Option<Value>) -> Result<T, String> {
    match params {
        Some(p) => serde_json::from_value(p).map_err(|e| format!("Invalid params: {}", e)),
        None => Err("Missing params".to_string()),
    }
}

fn function_to_tool(f: &FunctionInfo) -> McpTool {
    McpTool {
        name: f.function_id.replace("::", "__"),
        description: f.description.clone(),
        input_schema: f
            .request_format
            .clone()
            .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
    }
}

fn builtin_tools() -> Vec<McpTool> {
    vec![
        McpTool {
            name: "iii_worker_register".into(),
            description: Some("Register a new worker (Node.js or Python)".into()),
            input_schema: json!({ "type": "object", "properties": { "language": { "type": "string", "enum": ["node", "python"] }, "code": { "type": "string" }, "function_name": { "type": "string" }, "description": { "type": "string" } }, "required": ["language", "code", "function_name"] }),
        },
        McpTool {
            name: "iii_worker_stop".into(),
            description: Some("Stop a worker".into()),
            input_schema: json!({ "type": "object", "properties": { "id": { "type": "string" } }, "required": ["id"] }),
        },
        McpTool {
            name: "iii_trigger_register".into(),
            description: Some("Attach an http/cron/queue trigger to a function".into()),
            input_schema: json!({ "type": "object", "properties": { "trigger_type": { "type": "string" }, "function_id": { "type": "string" }, "config": { "type": "object" } }, "required": ["trigger_type", "function_id", "config"] }),
        },
        McpTool {
            name: "iii_trigger_unregister".into(),
            description: Some("Unregister a trigger".into()),
            input_schema: json!({ "type": "object", "properties": { "id": { "type": "string" } }, "required": ["id"] }),
        },
        McpTool {
            name: "iii_trigger_void".into(),
            description: Some("Fire-and-forget function invocation".into()),
            input_schema: json!({ "type": "object", "properties": { "function_id": { "type": "string" }, "payload": { "type": "object" } }, "required": ["function_id", "payload"] }),
        },
        McpTool {
            name: "iii_trigger_enqueue".into(),
            description: Some("Enqueue to named queue".into()),
            input_schema: json!({ "type": "object", "properties": { "function_id": { "type": "string" }, "payload": { "type": "object" }, "queue": { "type": "string" } }, "required": ["function_id", "payload"] }),
        },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpTool {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    input_schema: Value,
}

#[derive(Deserialize)]
struct CallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

fn tool_result(text: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": false })
}
fn tool_error(msg: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": msg }], "isError": true })
}
fn tool_json(v: &Value) -> Value {
    tool_result(&serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string()))
}
fn str_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}
fn str_field_or(v: &Value, key: &str, default: &str) -> String {
    v.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or(default)
        .to_string()
}
