use iii_sdk::IIIError;
use serde_json::{json, Value};

/// Maps a IIIError to an HTTP response format
pub fn error_response(error: IIIError) -> Value {
    let (status_code, message) = match error {
        IIIError::NotConnected => (503, "Bridge is not connected".to_string()),
        IIIError::Timeout => (504, "Invocation timed out".to_string()),
        IIIError::Remote { code, message, .. } => {
            (502, format!("Remote error ({}): {}", code, message))
        }
        IIIError::Handler(msg) => (500, format!("Handler error: {}", msg)),
        IIIError::Serde(msg) => (500, format!("Serialization error: {}", msg)),
        IIIError::WebSocket(msg) => (503, format!("WebSocket error: {}", msg)),
        IIIError::Runtime(msg) => (500, format!("Runtime error: {}", msg)),
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
