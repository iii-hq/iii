use serde_json::{Value, json};

pub fn list() -> Value {
    json!({
        "prompts": [
            { "name": "register-function", "description": "Guide to register a function", "arguments": [
                { "name": "language", "description": "node or python", "required": true },
                { "name": "function_id", "description": "e.g. myservice::process", "required": true }
            ]},
            { "name": "build-api", "description": "Expose a function as HTTP endpoint", "arguments": [
                { "name": "method", "description": "GET, POST, PUT, DELETE", "required": true },
                { "name": "path", "description": "e.g. /users", "required": true }
            ]},
            { "name": "setup-cron", "description": "Set up a scheduled cron job", "arguments": [
                { "name": "schedule", "description": "Cron expression", "required": true }
            ]},
            { "name": "event-pipeline", "description": "Build an event-driven pipeline", "arguments": [] }
        ]
    })
}

pub fn get(params: Option<Value>) -> Value {
    let name = params
        .as_ref()
        .and_then(|p| p.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let args = params
        .as_ref()
        .and_then(|p| p.get("arguments"))
        .cloned()
        .unwrap_or(json!({}));

    let text = match name {
        "register-function" => {
            let lang = args.get("language").and_then(|v| v.as_str()).unwrap_or("node");
            let fid = args.get("function_id").and_then(|v| v.as_str()).unwrap_or("myservice::handler");
            if lang == "python" {
                format!("Register Python function `{fid}`:\n1. `iii_worker_register` with language='python'\n2. Code: `async def handler(input): ...`\n3. Wire trigger via `iii_trigger_register`\n\n```python\nfrom iii_sdk import register_worker, Logger\niii = register_worker('ws://localhost:49134')\niii.register_function('{fid}', handler)\n```")
            } else {
                format!("Register Node.js function `{fid}`:\n1. `iii_worker_register` with language='node'\n2. Code: `async (input) => {{ ... }}`\n3. Wire trigger via `iii_trigger_register`\n\n```js\nimport {{ registerWorker, Logger }} from 'iii-sdk'\nconst iii = registerWorker('ws://localhost:49134')\niii.registerFunction({{ id: '{fid}' }}, handler)\n```")
            }
        }
        "build-api" => {
            let method = args.get("method").and_then(|v| v.as_str()).unwrap_or("GET");
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("/hello");
            format!("Expose HTTP {method} {path}:\n1. Register function\n2. Register trigger:\n```json\n{{ \"trigger_type\": \"http\", \"function_id\": \"api::handler\", \"config\": {{ \"api_path\": \"{path}\", \"http_method\": \"{method}\" }} }}\n```\n3. Input: {{ body, query_params, path_params, headers }}\n4. Return: {{ status_code, headers, body }}")
        }
        "setup-cron" => {
            let schedule = args.get("schedule").and_then(|v| v.as_str()).unwrap_or("0 * * * *");
            format!("Cron `{schedule}`:\n1. Register function\n2. Register trigger:\n```json\n{{ \"trigger_type\": \"cron\", \"function_id\": \"jobs::task\", \"config\": {{ \"expression\": \"{schedule}\" }} }}\n```\n3. `iii_trigger_unregister` to stop")
        }
        "event-pipeline" => "Event pipeline:\n```\nHTTP → Fn A → emit('order.created') → Fn B → emit('done') → Fn C\n```\n1. Register functions per stage\n2. Wire HTTP trigger to entry\n3. Queue triggers: `{{ \"trigger_type\": \"queue\", \"config\": {{ \"topic\": \"order.created\" }} }}`\n4. `iii_trigger_enqueue` for async, `iii_trigger_void` for fire-and-forget".to_string(),
        _ => format!("Unknown prompt: {name}"),
    };

    json!({ "messages": [{ "role": "user", "content": { "type": "text", "text": text } }] })
}
