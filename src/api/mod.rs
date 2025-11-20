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
    tracing::info!("Received dynamic request for path: {}, {}", path, method);
    if let Some(function_path) = engine.routers_registry.get_router("POST", &path).await {
        let invocation_id = Some(uuid::Uuid::new_v4());
        let function = engine.functions.get(function_path.as_str());
        if function.is_none() {
            return (StatusCode::NOT_FOUND, "Function Not Found").into_response();
        }
        let function = function.unwrap();
        let resp = (function.handler)(invocation_id, body).await;
        match resp {
            Ok(result) => (StatusCode::OK, Json(json!({"result": result}))).into_response(),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": err})),
            )
                .into_response(),
        }
    } else {
        (StatusCode::NOT_FOUND, "Not Found").into_response()
    }
}

pub fn api_endpoints() -> Router<Arc<Engine>> {
    Router::new().route("/dynamic/*path", any(dynamic_handler))
}
