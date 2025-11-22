use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::any,
};
use serde_json::{Value, json};

use crate::engine::Engine;

#[axum::debug_handler]
async fn dynamic_handler(
    method: axum::http::Method,
    State(engine): State<Arc<Engine>>,
    Path(path): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if let Some(function_path) = engine
        .routers_registry
        .get_router(method.as_str(), &path)
        .await
    {
        let function = engine.functions.get(function_path.as_str());
        if function.is_none() {
            return (StatusCode::NOT_FOUND, "Function Not Found").into_response();
        }
        let function_handler = function.expect("function existence checked");

        let func_result = engine
            .non_worker_invocations
            .handle_invocation(body, function_handler)
            .await;
        return match func_result {
            Ok(Ok(result)) => (StatusCode::OK, Json(json!({"result": result}))).into_response(),
            Ok(Err(err)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": err})),
            )
                .into_response(),
            Err(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Invocation timed out or channel closed"})),
            )
                .into_response(),
        };
    }
    (StatusCode::NOT_FOUND, "Not Found").into_response()
}

pub fn api_endpoints() -> Router<Arc<Engine>> {
    Router::new().route("/dynamic/*path", any(dynamic_handler))
}
