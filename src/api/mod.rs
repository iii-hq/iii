use std::{collections::HashMap, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{StatusCode, header::HeaderMap},
    response::IntoResponse,
    routing::any,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json, map::Values};

use crate::engine::Engine;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct APIrequest {
    pub query_params: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub path: String,
    pub method: String,
    pub body: Value,
}

impl APIrequest {
    pub fn new(
        query_params: HashMap<String, String>,
        headers: HeaderMap,
        path: String,
        method: String,
        body: Option<Json<Value>>,
    ) -> Self {
        // sometimes body can be empty, like in GET requests
        let body_value = body.map(|Json(v)| v).unwrap_or(serde_json::json!({}));
        APIrequest {
            query_params,
            headers: headers
                .iter()
                .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
                .collect(),
            path,
            method,
            body: body_value,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct APIresponse {
    pub status_code: u16,
    pub headers: Vec<String>,
    pub body: Value,
}

impl APIresponse {
    pub fn from_function_return(value: Value) -> Self {
        let status_code = value
            .get("status_code")
            .and_then(|v| v.as_u64())
            .unwrap_or(200) as u16;
        let headers = value
            .get("headers")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<String>>()
            })
            .unwrap_or(vec![]);
        let body = value.get("body").cloned().unwrap_or(json!({}));
        APIresponse {
            status_code,
            headers,
            body,
        }
    }
}

#[axum::debug_handler]
async fn dynamic_handler(
    method: axum::http::Method,
    headers: HeaderMap,
    State(engine): State<Arc<Engine>>,
    Path(path): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    body: Option<Json<Value>>,
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
        let api_request = APIrequest::new(
            params.clone(),
            headers,
            path.clone(),
            method.as_str().to_string(),
            body,
        );

        let api_request_value = serde_json::to_value(api_request).unwrap_or(serde_json::json!({}));

        let func_result = engine
            .non_worker_invocations
            .handle_invocation(api_request_value, function_handler)
            .await;

        return match func_result {
            Ok(Ok(result)) => {
                let result = result.unwrap_or(json!({}));
                let status_code = result
                    .get("status_code")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(200) as u16;
                let api_response = APIresponse::from_function_return(result);
                (
                    StatusCode::from_u16(status_code).unwrap_or(StatusCode::OK),
                    Json(api_response),
                )
                    .into_response()
            }
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
