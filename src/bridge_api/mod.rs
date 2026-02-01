pub mod token;

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::Extension,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use uuid::Uuid;

use crate::{
    engine::{Engine, EngineTrait},
    protocol::Message,
};

use self::token::BridgeTokenRegistry;

pub fn router(engine: Arc<Engine>, token_registry: Arc<BridgeTokenRegistry>) -> Router {
    Router::new()
        .route("/bridge", post(handle_callback))
        .layer(Extension(engine))
        .layer(Extension(token_registry))
}

async fn handle_callback(
    Extension(token_registry): Extension<Arc<BridgeTokenRegistry>>,
    Extension(engine): Extension<Arc<Engine>>,
    headers: HeaderMap,
    Json(message): Json<Message>,
) -> Result<Json<Message>, (StatusCode, String)> {
    let token = extract_bearer_token(&headers)
        .map_err(|e| (StatusCode::UNAUTHORIZED, e))?;

    let token_data = token_registry
        .validate_token(&token)
        .ok_or((StatusCode::UNAUTHORIZED, "Invalid or expired token".to_string()))?;

    tracing::debug!(
        invocation_id = %token_data.invocation_id,
        trace_id = %token_data.trace_id,
        "Callback request received"
    );

    match message {
        Message::InvokeFunction {
            invocation_id,
            function_path,
            data,
        } => {
            tracing::debug!(
                function_path = %function_path,
                "Handling InvokeFunction callback"
            );

            let result = engine.invoke_function(&function_path, data).await;

            let response_message = match result {
                Ok(result_value) => Message::InvocationResult {
                    invocation_id: invocation_id.unwrap_or_else(Uuid::new_v4),
                    function_path,
                    result: result_value,
                    error: None,
                },
                Err(error) => Message::InvocationResult {
                    invocation_id: invocation_id.unwrap_or_else(Uuid::new_v4),
                    function_path,
                    result: None,
                    error: Some(error),
                },
            };

            Ok(Json(response_message))
        }
        _ => Err((
            StatusCode::BAD_REQUEST,
            "Only InvokeFunction messages are supported for callback".to_string(),
        )),
    }
}

fn extract_bearer_token(headers: &HeaderMap) -> Result<String, String> {
    let auth_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or("Missing Authorization header")?;

    if !auth_header.starts_with("Bearer ") {
        return Err("Invalid Authorization header format".to_string());
    }

    Ok(auth_header["Bearer ".len()..].to_string())
}
