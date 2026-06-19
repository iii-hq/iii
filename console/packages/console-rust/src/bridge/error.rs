use iii_sdk::Error;
use serde_json::{json, Value};

/// Maps a Error to an HTTP response format
pub fn error_response(error: Error) -> Value {
    let (status_code, message) = match error {
        Error::NotConnected => (503, "Bridge is not connected".to_string()),
        Error::Timeout => (504, "Invocation timed out".to_string()),
        Error::Remote { code, message, .. } => {
            (502, format!("Remote error ({}): {}", code, message))
        }
        Error::Handler(msg) => (500, format!("Handler error: {}", msg)),
        Error::Serde(msg) => (500, format!("Serialization error: {}", msg)),
        Error::WebSocket(msg) => (503, format!("WebSocket error: {}", msg)),
        Error::Runtime(msg) => (500, format!("Runtime error: {}", msg)),
    };

    json!({
        "status_code": status_code,
        "headers": [],
        "body": {
            "error": message
        }
    })
}

/// Wraps a successful response in the standard HTTP response format
pub fn success_response(body: Value) -> Value {
    json!({
        "status_code": 200,
        "headers": [],
        "body": body
    })
}
