use std::{collections::HashMap, sync::Arc};

use axum::{
    Json,
    body::Bytes,
    extract::{Extension, Query},
    http::{StatusCode, Uri, header::HeaderMap},
    response::IntoResponse,
};
use serde_json::{Value, json};

use super::{
    RestApiCoreModule,
    types::{APIrequest, APIresponse},
};
use crate::engine::{Engine, EngineTrait};

// Helper function to extract all path parameters from a route pattern and actual path
// Returns a HashMap<String, String> where keys are parameter names (without ':') and values are their corresponding values
// Example:
//   registered_path = "/users/:id/posts/:post_id"
//   actual_path = "/users/123/posts/456"
//   Returns: {"id": "123", "post_id": "456"}
fn extract_path_params(registered_path: &str, actual_path: &str) -> HashMap<String, String> {
    let registered_segments: Vec<&str> = registered_path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let actual_segments: Vec<&str> = actual_path.split('/').filter(|s| !s.is_empty()).collect();

    let mut params = HashMap::new();

    // Only proceed if we have the same number of segments
    if registered_segments.len() != actual_segments.len() {
        return params;
    }

    // Match segments and extract parameters (segments starting with :)
    for (i, registered_seg) in registered_segments.iter().enumerate() {
        if registered_seg.starts_with(':') {
            // Extract parameter name (remove the ':')
            let param_name = registered_seg
                .strip_prefix(':')
                .unwrap_or(registered_seg)
                .to_string();

            // Extract the corresponding value from actual path
            if let Some(actual_value) = actual_segments.get(i) {
                params.insert(param_name, actual_value.to_string());
            }
        }
    }

    params
}
#[allow(clippy::too_many_arguments)]
#[axum::debug_handler]
pub async fn dynamic_handler(
    method: axum::http::Method,
    uri: Uri,
    headers: HeaderMap,
    Extension(engine): Extension<Arc<Engine>>,
    Extension(api_handler): Extension<Arc<RestApiCoreModule>>,
    Extension(registered_path): Extension<String>,
    Query(query_params): Query<HashMap<String, String>>,
    body: Bytes,
) -> impl IntoResponse {
    let actual_path = uri.path();

    tracing::debug!("Registered route path: {}", registered_path);
    tracing::debug!("Actual path: {}", actual_path);
    tracing::debug!("HTTP Method: {}", method);
    tracing::debug!("Query parameters: {:?}", query_params);
    tracing::debug!("Headers: {:?}", headers);
    let path_parameters: HashMap<String, String> =
        extract_path_params(&registered_path, actual_path);

    if let Some((function_path, condition_function_path)) =
        api_handler.get_router(method.as_str(), &registered_path)
    {
        let parsed_body = if body.is_empty() {
            None
        } else {
            match serde_json::from_slice::<Value>(&body) {
                Ok(json) => Some(Json(json)),
                Err(e) => {
                    tracing::error!("Failed to parse request body as JSON: {}", e);
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"error": format!("Failed to parse the request body as JSON: {}", e)})),
                    )
                        .into_response();
                }
            }
        };

        let api_request = APIrequest::new(
            query_params.clone(),
            path_parameters.clone(),
            headers,
            registered_path.clone(),
            method.as_str().to_string(),
            parsed_body,
        );

        let api_request_value = serde_json::to_value(api_request).unwrap_or(serde_json::json!({}));

        if let Some(condition_function_path) = condition_function_path.as_ref() {
            tracing::debug!(
                condition_function_path = %condition_function_path,
                "Checking trigger conditions"
            );

            match engine
                .invoke_function(condition_function_path, api_request_value.clone())
                .await
            {
                Ok(Some(result)) => {
                    if let Some(passed) = result.as_bool()
                        && !passed
                    {
                        tracing::debug!(
                            function_path = %function_path,
                            "Condition check failed, skipping handler"
                        );
                        return (
                            StatusCode::UNPROCESSABLE_ENTITY,
                            Json(json!({"error": "Request condition not met", "skipped": true})),
                        )
                            .into_response();
                    }
                }
                Ok(None) => {
                    tracing::warn!(
                        condition_function_path = %condition_function_path,
                        "Condition function returned no result"
                    );
                }
                Err(err) => {
                    tracing::error!(
                        condition_function_path = %condition_function_path,
                        error = ?err,
                        "Error invoking condition function"
                    );
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": "Condition check failed"})),
                    )
                        .into_response();
                }
            }
        }

        let func_result = engine
            .invoke_function(&function_path, api_request_value)
            .await;

        return match func_result {
            Ok(result) => {
                let result = result.unwrap_or(json!({}));
                let status_code = result
                    .get("status_code")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(200) as u16;
                let api_response = APIresponse::from_function_return(result);
                (
                    StatusCode::from_u16(status_code).unwrap_or(StatusCode::OK),
                    Json(api_response.body),
                )
                    .into_response()
            }
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": err})),
            )
                .into_response(),
        };
    }
    (StatusCode::NOT_FOUND, "Not Found").into_response()
}
