use std::sync::Arc;
pub mod adapters;
pub mod structs;

pub use self::structs::{KvDeleteInput, KvGetInput, KvSetInput};

use async_trait::async_trait;
use function_macros::{function, service};
use serde_json::Value;

use crate::{
    builtins::BuiltinKvStore,
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::{
        kv_server::{
            adapters::KVStoreAdapter,
            structs::{KvListInput, KvListKeysWithPrefixInput},
        },
        module::Module,
    },
    protocol::ErrorBody,
};

#[derive(Clone)]
pub struct KvServer {
    storage: Arc<dyn KVStoreAdapter>,
}

#[async_trait]
impl Module for KvServer {
    fn name(&self) -> &'static str {
        "KV Server"
    }

    async fn create(
        _engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn Module>> {
        let storage = BuiltinKvStore::new(config);
        let storage = Arc::new(storage);
        Ok(Box::new(KvServer { storage }))
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[service(name = "kv_server")]
impl KvServer {
    #[function(
        name = "kv_server.get",
        description = "Get a value by key from the KV store"
    )]
    pub async fn get(&self, data: KvGetInput) -> FunctionResult<Option<Value>, ErrorBody> {
        tracing::debug!(key = %data.key, "Getting value from KV store");
        let result = self.storage.get(data.key, None).await;
        FunctionResult::Success(result)
    }

    #[function(
        name = "kv_server.set",
        description = "Set a value by key in the KV store"
    )]
    pub async fn set(&self, data: KvSetInput) -> FunctionResult<Option<Value>, ErrorBody> {
        tracing::debug!(key = %data.key, "Setting value in KV store");
        let result = self.storage.set(data.key, data.value, None).await;

        FunctionResult::Success(Some(serde_json::to_value(result).unwrap_or_else(|e| {
            tracing::error!(error = %e, "Failed to serialize set result");
            Value::Null
        })))
    }

    #[function(
        name = "kv_server.delete",
        description = "Delete a value by key from the KV store"
    )]
    pub async fn delete(&self, data: KvDeleteInput) -> FunctionResult<Option<Value>, ErrorBody> {
        tracing::debug!(key = %data.key, "Deleting value from KV store");

        match self.storage.delete(data.key).await {
            Some(value) => FunctionResult::Success(Some(value)),
            None => FunctionResult::NoResult,
        }
    }

    #[function(
        name = "kv_server.list_keys_with_prefix",
        description = "List all keys with a prefix in the KV store"
    )]
    pub async fn list_keys_with_prefix(
        &self,
        data: KvListKeysWithPrefixInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let result = self.storage.list_keys_with_prefix(data.prefix).await;

        match serde_json::to_value(result) {
            Ok(value) => FunctionResult::Success(Some(value)),
            Err(err) => FunctionResult::Failure(ErrorBody {
                code: "serialization_error".into(),
                message: format!("Failed to serialize result: {}", err),
            }),
        }
    }

    #[function(
        name = "kv_server.list",
        description = "List all values in the KV store"
    )]
    pub async fn list(&self, data: KvListInput) -> FunctionResult<Option<Value>, ErrorBody> {
        let result = self.storage.list(data.key.clone()).await;
        tracing::debug!(key = %data.key.clone(), result = %result.len(), "Listing values from KV store");

        match serde_json::to_value(result) {
            Ok(value) => FunctionResult::Success(Some(value)),
            Err(err) => FunctionResult::Failure(ErrorBody {
                code: "serialization_error".into(),
                message: format!("Failed to serialize result: {}", err),
            }),
        }
    }
}

crate::register_module!(
    "modules::kv_server::KvServer",
    KvServer,
    enabled_by_default = true
);
