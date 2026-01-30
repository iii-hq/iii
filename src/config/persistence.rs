use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    engine::Engine,
    function::{Function, RegistrationSource},
    invocation::method::{HttpAuth, HttpMethod, InvocationMethod},
    protocol::ErrorBody,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpFunctionConfig {
    pub function_path: String,
    pub url: String,
    pub method: HttpMethod,
    pub timeout_ms: u64,
    pub headers: HashMap<String, String>,
    pub auth: Option<HttpAuthRef>,
    pub description: Option<String>,
    pub request_format: Option<Value>,
    pub response_format: Option<Value>,
    pub metadata: Option<Value>,
    pub registered_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HttpAuthRef {
    Hmac { secret_key: String },
    Bearer { token_key: String },
    ApiKey { header: String, value_key: String },
}

const HTTP_FUNCTION_PREFIX: &str = "http_function:";

pub async fn load_http_functions_from_kv(engine: &Engine) -> Result<(), ErrorBody> {
    let keys = engine
        .kv_store
        .list_keys_with_prefix(HTTP_FUNCTION_PREFIX.to_string())
        .await;

    for index in keys {
        let value = engine
            .kv_store
            .get(index.clone(), "config".to_string())
            .await
            .ok_or_else(|| ErrorBody {
                code: "kv_missing".into(),
                message: format!("Missing KV entry for {}", index),
            })?;

        let config: HttpFunctionConfig =
            serde_json::from_value(value).map_err(|err| ErrorBody {
                code: "kv_decode_failed".into(),
                message: err.to_string(),
            })?;

        if engine.functions.get(&config.function_path).is_some() {
            continue;
        }

        let auth = config.auth.as_ref().map(resolve_auth_ref).transpose()?;

        let invocation_method = InvocationMethod::Http {
            url: config.url,
            method: config.method,
            timeout_ms: config.timeout_ms,
            headers: config.headers,
            auth,
        };

        let function = Function {
            function_path: config.function_path.clone(),
            description: config.description,
            request_format: config.request_format,
            response_format: config.response_format,
            metadata: config.metadata,
            invocation_method,
            registered_at: config.registered_at,
            registration_source: RegistrationSource::AdminApi,
            handler: None,
        };

        engine
            .service_registry
            .register_service_from_func_path(&config.function_path)
            .await;
        engine
            .functions
            .register_function(config.function_path.clone(), function);
    }

    Ok(())
}

pub async fn store_http_function_in_kv(
    engine: &Engine,
    config: &HttpFunctionConfig,
) -> Result<(), ErrorBody> {
    let index = format!("{}{}", HTTP_FUNCTION_PREFIX, config.function_path);
    let value = serde_json::to_value(config).map_err(|err| ErrorBody {
        code: "kv_encode_failed".into(),
        message: err.to_string(),
    })?;

    engine
        .kv_store
        .set(index, "config".to_string(), value)
        .await;

    Ok(())
}

pub async fn delete_http_function_from_kv(
    engine: &Engine,
    function_path: &str,
) -> Result<(), ErrorBody> {
    let index = format!("{}{}", HTTP_FUNCTION_PREFIX, function_path);
    engine.kv_store.delete(index, "config".to_string()).await;
    Ok(())
}

pub fn resolve_auth_ref(auth_ref: &HttpAuthRef) -> Result<HttpAuth, ErrorBody> {
    match auth_ref {
        HttpAuthRef::Hmac { secret_key } => {
            let secret = std::env::var(secret_key).map_err(|_| ErrorBody {
                code: "secret_not_found".into(),
                message: format!("Secret not found: {}", secret_key),
            })?;
            Ok(HttpAuth::Hmac { secret })
        }
        HttpAuthRef::Bearer { token_key } => {
            let token = std::env::var(token_key).map_err(|_| ErrorBody {
                code: "token_not_found".into(),
                message: format!("Token not found: {}", token_key),
            })?;
            Ok(HttpAuth::Bearer { token })
        }
        HttpAuthRef::ApiKey { header, value_key } => {
            let value = std::env::var(value_key).map_err(|_| ErrorBody {
                code: "api_key_not_found".into(),
                message: format!("API key not found: {}", value_key),
            })?;
            Ok(HttpAuth::ApiKey {
                header: header.clone(),
                value,
            })
        }
    }
}
