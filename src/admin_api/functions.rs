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
    config::persistence::{
        HttpAuthRef, HttpFunctionConfig as KvHttpFunctionConfig, delete_http_function_from_kv,
        store_http_function_in_kv,
    },
    engine::Engine,
    function::{Function, RegistrationSource},
    invocation::method::{HttpAuth, HttpMethod, InvocationMethod},
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
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub auth: Option<HttpAuth>,
}

pub fn router(engine: Arc<Engine>) -> Router {
    use axum::extract::Extension;
    Router::new()
        .route("/admin/functions", post(register_function))
        .route("/admin/functions", get(list_functions))
        .route("/admin/functions/:path", put(update_function))
        .route("/admin/functions/:path", delete(unregister_function))
        .layer(middleware::from_fn(auth_middleware))
        .layer(Extension(engine))
}

pub async fn register_function(
    Extension(engine): Extension<Arc<Engine>>,
    Json(payload): Json<RegisterFunctionRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    engine
        .http_invoker
        .url_validator()
        .validate(&payload.invocation.url)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let auth_ref = payload
        .invocation
        .auth
        .as_ref()
        .map(|auth| auth_ref_from_auth(&payload.function_path, auth));

    let kv_config = KvHttpFunctionConfig {
        function_path: payload.function_path.clone(),
        url: payload.invocation.url.clone(),
        method: payload.invocation.method.clone(),
        timeout_ms: payload.invocation.timeout_ms,
        headers: payload.invocation.headers.clone(),
        auth: auth_ref,
        description: payload.description.clone(),
        request_format: payload.request_format.clone(),
        response_format: payload.response_format.clone(),
        metadata: payload.metadata.clone(),
        registered_at: Utc::now(),
    };

    store_http_function_in_kv(&engine, &kv_config)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.message))?;

    let invocation_method = InvocationMethod::Http {
        url: payload.invocation.url,
        method: payload.invocation.method,
        timeout_ms: payload.invocation.timeout_ms,
        headers: payload.invocation.headers,
        auth: payload.invocation.auth,
    };

    let function = Function {
        function_path: payload.function_path.clone(),
        description: payload.description,
        request_format: payload.request_format,
        response_format: payload.response_format,
        metadata: payload.metadata,
        invocation_method,
        registered_at: Utc::now(),
        registration_source: RegistrationSource::AdminApi,
        handler: None,
    };

    engine
        .service_registry
        .register_service_from_func_path(&payload.function_path)
        .await;
    engine
        .functions
        .register_function(payload.function_path.clone(), function);

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
    let mut function = engine
        .functions
        .get(&function_path)
        .ok_or((StatusCode::NOT_FOUND, "Function not found".to_string()))?;

    let index = format!("http_function:{}", function_path);
    let value = engine
        .kv_store
        .get(index.clone(), "config".to_string())
        .await
        .ok_or((StatusCode::NOT_FOUND, "KV config not found".to_string()))?;
    let mut kv_config: KvHttpFunctionConfig =
        serde_json::from_value(value).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    if let Some(description) = payload.description {
        function.description = Some(description.clone());
        kv_config.description = Some(description);
    }
    if let Some(request_format) = payload.request_format {
        function.request_format = Some(request_format.clone());
        kv_config.request_format = Some(request_format);
    }
    if let Some(response_format) = payload.response_format {
        function.response_format = Some(response_format.clone());
        kv_config.response_format = Some(response_format);
    }
    if let Some(metadata) = payload.metadata {
        function.metadata = Some(metadata.clone());
        kv_config.metadata = Some(metadata);
    }

    if let Some(invocation) = payload.invocation {
        engine
            .http_invoker
            .url_validator()
            .validate(&invocation.url)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

        let auth_ref = invocation
            .auth
            .as_ref()
            .map(|auth| auth_ref_from_auth(&function_path, auth));

        kv_config.url = invocation.url.clone();
        kv_config.method = invocation.method.clone();
        kv_config.timeout_ms = invocation.timeout_ms;
        kv_config.headers = invocation.headers.clone();
        kv_config.auth = auth_ref;
        kv_config.registered_at = Utc::now();

        function.invocation_method = InvocationMethod::Http {
            url: invocation.url,
            method: invocation.method,
            timeout_ms: invocation.timeout_ms,
            headers: invocation.headers,
            auth: invocation.auth,
        };
    }

    kv_config.registered_at = Utc::now();

    store_http_function_in_kv(&engine, &kv_config)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.message))?;

    function.registered_at = Utc::now();
    function.registration_source = RegistrationSource::AdminApi;

    engine
        .functions
        .register_function(function_path.clone(), function);

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
    delete_http_function_from_kv(&engine, &function_path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.message))?;

    engine.functions.remove(&function_path);
    engine
        .service_registry
        .remove_function_from_services(&function_path)
        .await;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_functions(
    Extension(engine): Extension<Arc<Engine>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let functions: Vec<Value> = engine
        .functions
        .iter()
        .map(|entry| {
            let function = entry.value();
            json!({
                "function_path": function.function_path,
                "invocation_type": match &function.invocation_method {
                    InvocationMethod::WebSocket { .. } => "websocket",
                    InvocationMethod::Http { .. } => "http",
                },
                "registered_at": function.registered_at,
                "registration_source": &function.registration_source,
            })
        })
        .collect();

    Ok(Json(json!({ "functions": functions })))
}

fn auth_ref_from_auth(function_path: &str, auth: &HttpAuth) -> HttpAuthRef {
    let prefix = function_path.to_uppercase().replace('.', "_");
    match auth {
        HttpAuth::Hmac { .. } => HttpAuthRef::Hmac {
            secret_key: format!("{}_HMAC_SECRET", prefix),
        },
        HttpAuth::Bearer { .. } => HttpAuthRef::Bearer {
            token_key: format!("{}_BEARER_TOKEN", prefix),
        },
        HttpAuth::ApiKey { header, .. } => HttpAuthRef::ApiKey {
            header: header.clone(),
            value_key: format!("{}_API_KEY", prefix),
        },
    }
}

fn default_method() -> HttpMethod {
    HttpMethod::Post
}

fn default_timeout_ms() -> u64 {
    30000
}
