// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, sync::Arc};

use axum::{
    Json, Router,
    extract::{Extension, Path},
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    config::persistence::http_function_key,
    engine::Engine,
    invocation::{auth::HttpAuthConfig, http_function::HttpFunctionConfig, method::HttpMethod},
    modules::http_functions::HttpFunctionsModule,
};

use super::middleware::{auth_middleware, error_response};

type ApiResult = Result<Response, Response>;

#[derive(Debug, Deserialize)]
pub struct RegisterFunctionRequest {
    pub function_path: String,
    pub description: Option<String>,
    pub request_format: Option<Value>,
    pub response_format: Option<Value>,
    pub metadata: Option<Value>,
    pub invocation: HttpInvocationConfig,
}

#[derive(Debug, Deserialize)]
pub struct UpdateFunctionRequest {
    pub description: Option<String>,
    pub request_format: Option<Value>,
    pub response_format: Option<Value>,
    pub metadata: Option<Value>,
    pub invocation: Option<HttpInvocationConfig>,
}

#[derive(Debug, Deserialize)]
pub struct HttpInvocationConfig {
    pub url: String,
    #[serde(default = "default_method")]
    pub method: HttpMethod,
    /// Request timeout in milliseconds. If not specified, the invoker's default timeout will be used.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Authentication configuration using environment variable keys.
    /// Example: {"type": "hmac", "secret_key": "MY_FUNCTION_HMAC_SECRET"}
    pub auth: Option<HttpAuthConfig>,
}

pub fn router(engine: Arc<Engine>) -> Router {
    Router::new()
        .route("/admin/functions", post(register_function))
        .route("/admin/functions", get(list_functions))
        .route("/admin/functions/{path}", get(get_function))
        .route("/admin/functions/{path}", put(update_function))
        .route("/admin/functions/{path}", delete(unregister_function))
        .layer(middleware::from_fn(auth_middleware))
        .layer(Extension(engine))
}

#[allow(clippy::result_large_err)]
fn get_http_module(engine: &Engine) -> Result<Arc<HttpFunctionsModule>, Response> {
    engine
        .service_registry
        .get_service::<HttpFunctionsModule>("http_functions")
        .ok_or_else(|| {
            error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "module_unavailable",
                "HTTP functions module not loaded",
            )
        })
}

pub async fn register_function(
    Extension(engine): Extension<Arc<Engine>>,
    Json(payload): Json<RegisterFunctionRequest>,
) -> ApiResult {
    let http_module = get_http_module(&engine)?;

    // Validate function path
    validate_function_path(&payload.function_path).map_err(|e| {
        error_response(StatusCode::BAD_REQUEST, "invalid_function_path", &e)
    })?;

    // Check for duplicate registration
    if engine.functions.get(&payload.function_path).is_some() {
        return Err(error_response(
            StatusCode::CONFLICT,
            "function_exists",
            &format!(
                "Function '{}' already exists. Use PUT /admin/functions/{} to update.",
                payload.function_path, payload.function_path
            ),
        ));
    }

    // Validate input sizes
    validate_input_sizes(&payload)?;

    // Validate URL
    http_module
        .http_invoker()
        .url_validator()
        .validate(&payload.invocation.url)
        .await
        .map_err(|e| {
            error_response(StatusCode::BAD_REQUEST, "invalid_url", &e.to_string())
        })?;

    // Validate that environment variables exist for auth configuration
    if let Some(ref auth_config) = payload.invocation.auth {
        auth_config.validate().map_err(|e| {
            error_response(StatusCode::BAD_REQUEST, &e.code, &e.message)
        })?;
    }

    let kv_config = HttpFunctionConfig {
        function_path: payload.function_path.clone(),
        url: payload.invocation.url.clone(),
        method: payload.invocation.method.clone(),
        timeout_ms: payload.invocation.timeout_ms,
        headers: payload.invocation.headers.clone(),
        auth: payload.invocation.auth.clone(),
        description: payload.description.clone(),
        request_format: payload.request_format.clone(),
        response_format: payload.response_format.clone(),
        metadata: payload.metadata.clone(),
        registered_at: Some(Utc::now()),
        updated_at: None,
    };

    http_module
        .persist_and_register(kv_config)
        .await
        .map_err(|e| {
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "persist_failed",
                &e.message,
            )
        })?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "status": "registered",
            "function_path": payload.function_path,
            "persisted": true
        })),
    )
        .into_response())
}

pub async fn update_function(
    Extension(engine): Extension<Arc<Engine>>,
    Path(function_path): Path<String>,
    Json(payload): Json<UpdateFunctionRequest>,
) -> ApiResult {
    let http_module = get_http_module(&engine)?;

    engine.functions.get(&function_path).ok_or_else(|| {
        error_response(StatusCode::NOT_FOUND, "function_not_found", "Function not found")
    })?;

    let index = http_function_key(&function_path);
    let value = http_module
        .kv_store()
        .get(index.clone(), "config".to_string())
        .await
        .ok_or_else(|| {
            error_response(
                StatusCode::NOT_FOUND,
                "config_not_found",
                "KV config not found",
            )
        })?;
    let mut kv_config: HttpFunctionConfig = serde_json::from_value(value).map_err(|e| {
        error_response(StatusCode::BAD_REQUEST, "invalid_config", &e.to_string())
    })?;

    // Validate update input sizes
    validate_update_input_sizes(&payload)?;

    if let Some(description) = payload.description {
        kv_config.description = Some(description);
    }
    if let Some(request_format) = payload.request_format {
        kv_config.request_format = Some(request_format);
    }
    if let Some(response_format) = payload.response_format {
        kv_config.response_format = Some(response_format);
    }
    if let Some(metadata) = payload.metadata {
        kv_config.metadata = Some(metadata);
    }

    if let Some(invocation) = payload.invocation {
        http_module
            .http_invoker()
            .url_validator()
            .validate(&invocation.url)
            .await
            .map_err(|e| {
                error_response(StatusCode::BAD_REQUEST, "invalid_url", &e.to_string())
            })?;

        // Validate that environment variables exist for auth configuration
        if let Some(ref auth_config) = invocation.auth {
            auth_config.validate().map_err(|e| {
                error_response(StatusCode::BAD_REQUEST, &e.code, &e.message)
            })?;
        }

        kv_config.url = invocation.url;
        kv_config.method = invocation.method;
        kv_config.timeout_ms = invocation.timeout_ms;
        kv_config.headers = invocation.headers;
        kv_config.auth = invocation.auth;
    }

    // Set updated_at timestamp (don't modify registered_at)
    kv_config.updated_at = Some(Utc::now());

    http_module
        .persist_and_register(kv_config)
        .await
        .map_err(|e| {
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "persist_failed",
                &e.message,
            )
        })?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "status": "updated",
            "function_path": function_path,
            "persisted": true
        })),
    )
        .into_response())
}

pub async fn unregister_function(
    Path(function_path): Path<String>,
    Extension(engine): Extension<Arc<Engine>>,
) -> ApiResult {
    let http_module = get_http_module(&engine)?;

    http_module
        .unregister_http_function(&function_path)
        .await
        .map_err(|e| {
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "unregister_failed",
                &e.message,
            )
        })?;

    engine
        .service_registry
        .remove_function_from_services(&function_path);

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn list_functions(
    Extension(engine): Extension<Arc<Engine>>,
) -> ApiResult {
    let http_module = get_http_module(&engine)?;

    let kv_store = http_module.kv_store();
    let keys = kv_store
        .list_keys_with_prefix("http_function:".to_string())
        .await;

    let mut functions = Vec::new();
    for key in keys {
        if let Some(value) = kv_store.get(key.clone(), "config".to_string()).await
            && let Ok(config) = serde_json::from_value::<HttpFunctionConfig>(value)
        {
            let mut func_json = json!({
                "function_path": config.function_path,
                "invocation_type": "http",
                "url": config.url,
                "method": serde_json::to_value(&config.method).unwrap_or_default(),
                "registered_at": config.registered_at,
            });

            if let Some(ref description) = config.description {
                func_json["description"] = json!(description);
            }
            if let Some(ref metadata) = config.metadata {
                func_json["metadata"] = json!(metadata);
            }
            if let Some(ref updated_at) = config.updated_at {
                func_json["updated_at"] = json!(updated_at);
            }

            functions.push(func_json);
        }
    }

    Ok((StatusCode::OK, Json(json!({ "functions": functions }))).into_response())
}

pub async fn get_function(
    Extension(engine): Extension<Arc<Engine>>,
    Path(function_path): Path<String>,
) -> ApiResult {
    let http_module = get_http_module(&engine)?;

    engine.functions.get(&function_path).ok_or_else(|| {
        error_response(StatusCode::NOT_FOUND, "function_not_found", "Function not found")
    })?;

    let index = http_function_key(&function_path);
    let value = http_module
        .kv_store()
        .get(index, "config".to_string())
        .await
        .ok_or_else(|| {
            error_response(
                StatusCode::NOT_FOUND,
                "config_not_found",
                "Function config not found in KV store",
            )
        })?;

    let config: HttpFunctionConfig = serde_json::from_value(value).map_err(|e| {
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid_config",
            &e.to_string(),
        )
    })?;

    let mut response = json!({
        "function_path": config.function_path,
        "invocation_type": "http",
        "url": config.url,
        "method": serde_json::to_value(&config.method).unwrap_or_default(),
        "timeout_ms": config.timeout_ms,
        "headers": config.headers,
        "registered_at": config.registered_at,
    });

    // L6: Include auth type info without exposing secret env var keys
    if let Some(ref auth) = config.auth {
        response["auth_type"] = match auth {
            HttpAuthConfig::Hmac { .. } => json!("hmac"),
            HttpAuthConfig::Bearer { .. } => json!("bearer"),
            HttpAuthConfig::ApiKey { .. } => json!("api_key"),
        };
    }

    if let Some(ref description) = config.description {
        response["description"] = json!(description);
    }
    if let Some(ref metadata) = config.metadata {
        response["metadata"] = json!(metadata);
    }
    if let Some(ref request_format) = config.request_format {
        response["request_format"] = json!(request_format);
    }
    if let Some(ref response_format) = config.response_format {
        response["response_format"] = json!(response_format);
    }
    if let Some(ref updated_at) = config.updated_at {
        response["updated_at"] = json!(updated_at);
    }

    Ok((StatusCode::OK, Json(response)).into_response())
}

/// Validates that the function path is safe and follows naming conventions.
/// Function paths can only contain alphanumeric characters, dots, underscores, and hyphens.
/// They cannot start or end with a dot.
fn validate_function_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("Function path cannot be empty".to_string());
    }

    if path.len() > 255 {
        return Err("Function path cannot exceed 255 characters".to_string());
    }

    if !path
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err(
            "Function path can only contain alphanumeric characters, dots, underscores, and hyphens"
                .to_string(),
        );
    }

    if path.starts_with('.') || path.ends_with('.') {
        return Err("Function path cannot start or end with a dot".to_string());
    }

    if path.contains("..") {
        return Err("Function path cannot contain consecutive dots".to_string());
    }

    Ok(())
}

/// Validates input sizes to prevent abuse and resource exhaustion.
#[allow(clippy::result_large_err)]
fn validate_input_sizes(payload: &RegisterFunctionRequest) -> Result<(), Response> {
    if let Some(ref desc) = payload.description
        && desc.len() > 2000
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "description_too_large",
            "Description cannot exceed 2000 characters",
        ));
    }

    if payload.invocation.headers.len() > 50 {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "too_many_headers",
            "Cannot specify more than 50 headers",
        ));
    }

    for (key, value) in &payload.invocation.headers {
        if key.len() > 256 || value.len() > 4096 {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "header_too_large",
                "Header key cannot exceed 256 chars and value cannot exceed 4096 chars",
            ));
        }
    }

    if let Some(ref metadata) = payload.metadata {
        let metadata_str = serde_json::to_string(metadata).unwrap_or_default();
        if metadata_str.len() > 10240 {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "metadata_too_large",
                "Metadata cannot exceed 10KB when serialized",
            ));
        }
    }

    Ok(())
}

/// Validates input sizes for update requests.
#[allow(clippy::result_large_err)]
fn validate_update_input_sizes(payload: &UpdateFunctionRequest) -> Result<(), Response> {
    if let Some(ref desc) = payload.description
        && desc.len() > 2000
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "description_too_large",
            "Description cannot exceed 2000 characters",
        ));
    }

    if let Some(ref invocation) = payload.invocation {
        if invocation.headers.len() > 50 {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "too_many_headers",
                "Cannot specify more than 50 headers",
            ));
        }

        for (key, value) in &invocation.headers {
            if key.len() > 256 || value.len() > 4096 {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    "header_too_large",
                    "Header key cannot exceed 256 chars and value cannot exceed 4096 chars",
                ));
            }
        }
    }

    if let Some(ref metadata) = payload.metadata {
        let metadata_str = serde_json::to_string(metadata).unwrap_or_default();
        if metadata_str.len() > 10240 {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "metadata_too_large",
                "Metadata cannot exceed 10KB when serialized",
            ));
        }
    }

    Ok(())
}

fn default_method() -> HttpMethod {
    HttpMethod::Post
}
