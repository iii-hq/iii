use std::{collections::HashMap, sync::Arc};

use axum::{
    Json, Router,
    extract::{Extension, Path},
    http::StatusCode,
    middleware,
    routing::{delete, get, post, put},
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    engine::Engine,
    invocation::{auth::HttpAuthRef, http_function::HttpFunctionConfig, method::HttpMethod},
    modules::http_functions::HttpFunctionsModule,
};

use super::middleware::auth_middleware;

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
    pub auth: Option<HttpAuthRef>,
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

pub async fn register_function(
    Extension(engine): Extension<Arc<Engine>>,
    Json(payload): Json<RegisterFunctionRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let http_module = engine
        .service_registry
        .get_service::<HttpFunctionsModule>("http_functions")
        .ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "HTTP functions module not loaded".to_string(),
        ))?;

    // Validate function path
    validate_function_path(&payload.function_path).map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    // Check for duplicate registration
    if engine.functions.get(&payload.function_path).is_some() {
        return Err((
            StatusCode::CONFLICT,
            format!(
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
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    // Validate that environment variables exist for auth configuration
    if let Some(ref auth_ref) = payload.invocation.auth {
        validate_auth_env_vars(auth_ref).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
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
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.message))?;

    Ok(Json(json!({
        "status": "registered",
        "function_path": payload.function_path,
        "persisted": true
    })))
}

pub async fn update_function(
    Extension(engine): Extension<Arc<Engine>>,
    Path(function_path): Path<String>,
    Json(payload): Json<UpdateFunctionRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let http_module = engine
        .service_registry
        .get_service::<HttpFunctionsModule>("http_functions")
        .ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "HTTP functions module not loaded".to_string(),
        ))?;

    engine
        .functions
        .get(&function_path)
        .ok_or((StatusCode::NOT_FOUND, "Function not found".to_string()))?;

    let index = format!("http_function:{}", function_path);
    let value = http_module
        .kv_store()
        .get(index.clone(), "config".to_string())
        .await
        .ok_or((StatusCode::NOT_FOUND, "KV config not found".to_string()))?;
    let mut kv_config: HttpFunctionConfig =
        serde_json::from_value(value).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

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
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

        // Validate that environment variables exist for auth configuration
        if let Some(ref auth_ref) = invocation.auth {
            validate_auth_env_vars(auth_ref).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
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
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.message))?;

    Ok(Json(json!({
        "status": "updated",
        "function_path": function_path,
        "persisted": true
    })))
}

pub async fn unregister_function(
    Path(function_path): Path<String>,
    Extension(engine): Extension<Arc<Engine>>,
) -> Result<StatusCode, (StatusCode, String)> {
    let http_module = engine
        .service_registry
        .get_service::<HttpFunctionsModule>("http_functions")
        .ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "HTTP functions module not loaded".to_string(),
        ))?;

    http_module
        .unregister_http_function(&function_path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.message))?;

    engine
        .service_registry
        .remove_function_from_services(&function_path);

    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_functions(
    Extension(engine): Extension<Arc<Engine>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let http_module = engine
        .service_registry
        .get_service::<HttpFunctionsModule>("http_functions")
        .ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "HTTP functions module not loaded".to_string(),
        ))?;

    let kv_store = http_module.kv_store();
    let keys = kv_store
        .list_keys_with_prefix("http_function:".to_string())
        .await;

    let mut functions = Vec::new();
    for key in keys {
        if let Some(value) = kv_store.get(key.clone(), "config".to_string()).await {
            if let Ok(config) = serde_json::from_value::<HttpFunctionConfig>(value) {
                let mut func_json = json!({
                    "function_path": config.function_path,
                    "invocation_type": "http",
                    "url": config.url,
                    "method": format!("{:?}", config.method),
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
    }

    Ok(Json(json!({ "functions": functions })))
}

pub async fn get_function(
    Extension(engine): Extension<Arc<Engine>>,
    Path(function_path): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let http_module = engine
        .service_registry
        .get_service::<HttpFunctionsModule>("http_functions")
        .ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "HTTP functions module not loaded".to_string(),
        ))?;

    engine
        .functions
        .get(&function_path)
        .ok_or((StatusCode::NOT_FOUND, "Function not found".to_string()))?;

    let index = format!("http_function:{}", function_path);
    let value = http_module
        .kv_store()
        .get(index, "config".to_string())
        .await
        .ok_or((
            StatusCode::NOT_FOUND,
            "Function config not found in KV store".to_string(),
        ))?;

    let config: HttpFunctionConfig = serde_json::from_value(value)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut response = json!({
        "function_path": config.function_path,
        "invocation_type": "http",
        "url": config.url,
        "method": format!("{:?}", config.method),
        "timeout_ms": config.timeout_ms,
        "headers": config.headers,
        "registered_at": config.registered_at,
    });

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

    Ok(Json(response))
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

/// Validates that the environment variables referenced by the auth configuration exist.
/// Returns an error message if any required environment variables are missing.
fn validate_auth_env_vars(auth_ref: &HttpAuthRef) -> Result<(), String> {
    match auth_ref {
        HttpAuthRef::Hmac { secret_key } => {
            std::env::var(secret_key).map_err(|_| {
                format!(
                    "Missing environment variable '{}' for HMAC authentication. \
                     Please set this environment variable before registering the function.",
                    secret_key
                )
            })?;
        }
        HttpAuthRef::Bearer { token_key } => {
            std::env::var(token_key).map_err(|_| {
                format!(
                    "Missing environment variable '{}' for Bearer token authentication. \
                     Please set this environment variable before registering the function.",
                    token_key
                )
            })?;
        }
        HttpAuthRef::ApiKey { value_key, .. } => {
            std::env::var(value_key).map_err(|_| {
                format!(
                    "Missing environment variable '{}' for API key authentication. \
                     Please set this environment variable before registering the function.",
                    value_key
                )
            })?;
        }
    }
    Ok(())
}

/// Validates input sizes to prevent abuse and resource exhaustion.
fn validate_input_sizes(payload: &RegisterFunctionRequest) -> Result<(), (StatusCode, String)> {
    if let Some(ref desc) = payload.description {
        if desc.len() > 2000 {
            return Err((
                StatusCode::BAD_REQUEST,
                "Description cannot exceed 2000 characters".to_string(),
            ));
        }
    }

    if payload.invocation.headers.len() > 50 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Cannot specify more than 50 headers".to_string(),
        ));
    }

    for (key, value) in &payload.invocation.headers {
        if key.len() > 256 || value.len() > 4096 {
            return Err((
                StatusCode::BAD_REQUEST,
                "Header key cannot exceed 256 chars and value cannot exceed 4096 chars".to_string(),
            ));
        }
    }

    if let Some(ref metadata) = payload.metadata {
        let metadata_str = serde_json::to_string(metadata).unwrap_or_default();
        if metadata_str.len() > 10240 {
            return Err((
                StatusCode::BAD_REQUEST,
                "Metadata cannot exceed 10KB when serialized".to_string(),
            ));
        }
    }

    Ok(())
}

fn default_method() -> HttpMethod {
    HttpMethod::Post
}
