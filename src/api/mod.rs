use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::any,
};
use dashmap::DashMap;
use serde_json::{Value, json};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::{engine::Engine, protocol::ErrorBody};

#[derive(Default)]
pub struct HttpInvocations {
    invocations: Arc<DashMap<Uuid, oneshot::Sender<Result<Option<Value>, ErrorBody>>>>,
}
impl HttpInvocations {
    pub fn new() -> Self {
        Self {
            invocations: Arc::new(DashMap::new()),
        }
    }
    pub fn insert(
        &self,
        invocation_id: Uuid,
        sender: oneshot::Sender<Result<Option<Value>, ErrorBody>>,
    ) {
        self.invocations.insert(invocation_id, sender);
    }

    pub fn remove(
        &self,
        invocation_id: &Uuid,
    ) -> Option<oneshot::Sender<Result<Option<Value>, ErrorBody>>> {
        self.invocations
            .remove(invocation_id)
            .map(|(_, sender)| sender)
    }
}

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
        let invocation_id = Some(uuid::Uuid::new_v4());
        let function = engine.functions.get(function_path.as_str());
        if function.is_none() {
            return (StatusCode::NOT_FOUND, "Function Not Found").into_response();
        }
        let function = function.expect("function existence checked");

        let (sender, receiver) = tokio::sync::oneshot::channel();
        let invocation_id = invocation_id.unwrap(); // We know it is Some
        engine.http_invocations.insert(invocation_id, sender);

        let _ = (function.handler)(Some(invocation_id), body).await;

        // Wait for the result
        match receiver.await {
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
        }
    } else {
        (StatusCode::NOT_FOUND, "Not Found").into_response()
    }
}

pub fn api_endpoints() -> Router<Arc<Engine>> {
    Router::new().route("/dynamic/*path", any(dynamic_handler))
}
