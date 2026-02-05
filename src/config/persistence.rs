use std::sync::Arc;

use crate::{
    builtins::kv::BuiltinKvStore,
    engine::Engine,
    invocation::http_function::HttpFunctionConfig,
    protocol::ErrorBody,
};

const HTTP_FUNCTION_PREFIX: &str = "http_function:";

pub async fn load_http_functions_from_kv(
    kv_store: &Arc<BuiltinKvStore>,
    engine: &Arc<Engine>,
    http_module: &crate::modules::http_functions::HttpFunctionsModule,
) -> Result<(), ErrorBody> {
    let keys = kv_store
        .list_keys_with_prefix(HTTP_FUNCTION_PREFIX.to_string())
        .await;

    for index in keys {
        let value = kv_store
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

        http_module.register_http_function(config).await?;
    }

    Ok(())
}

pub async fn store_http_function_in_kv(
    kv_store: &Arc<BuiltinKvStore>,
    config: &HttpFunctionConfig,
) -> Result<(), ErrorBody> {
    let index = format!("{}{}", HTTP_FUNCTION_PREFIX, config.function_path);
    let value = serde_json::to_value(config).map_err(|err| ErrorBody {
        code: "kv_encode_failed".into(),
        message: err.to_string(),
    })?;

    kv_store.set(index, "config".to_string(), value).await;

    Ok(())
}

pub async fn delete_http_function_from_kv(
    kv_store: &Arc<BuiltinKvStore>,
    function_path: &str,
) -> Result<(), ErrorBody> {
    let index = format!("{}{}", HTTP_FUNCTION_PREFIX, function_path);
    kv_store.delete(index, "config".to_string()).await;
    Ok(())
}
