use crate::{
    invocation::http_function::HttpFunctionConfig,
    engine::Engine,
    function::RegistrationSource,
    protocol::ErrorBody,
};

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
