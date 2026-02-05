use serde::{Deserialize, Serialize};

use crate::{
    config::HttpFunctionConfig,
    engine::Engine,
    function::RegistrationSource,
    invocation::method::HttpAuth,
    protocol::ErrorBody,
};

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

        engine
            .register_http_function_from_persistence(
                config,
                RegistrationSource::AdminApi,
            )
            .await?;
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
