use std::sync::Arc;
pub mod structs;

pub use self::structs::{
    KvDeleteInput, KvGetInput, KvLlenInput, KvLpushInput, KvLremInput, KvRpopInput, KvSetInput,
    KvZaddInput, KvZrangebyscoreInput, KvZremInput,
};

use async_trait::async_trait;
use function_macros::{function, service};
use serde_json::Value;

use crate::{
    builtins::kv::BuiltinKvStore,
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::{
        kv_server::structs::{KvListInput, KvListKeysWithPrefixInput, KvUpdateInput},
        module::Module,
    },
    protocol::ErrorBody,
};

#[derive(Clone)]
pub struct KvServer {
    storage: Arc<BuiltinKvStore>,
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
        tracing::debug!(index = %data.index, key = %data.key, "Getting value from KV store");
        let result = self.storage.get(data.index.clone(), data.key.clone()).await;
        FunctionResult::Success(result)
    }

    #[function(
        name = "kv_server.set",
        description = "Set a value by key in the KV store"
    )]
    pub async fn set(&self, data: KvSetInput) -> FunctionResult<Option<Value>, ErrorBody> {
        tracing::debug!(index = %data.index, key = %data.key, "Setting value in KV store");
        let result = self
            .storage
            .set(data.index.clone(), data.key.clone(), data.value.clone())
            .await;

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

        match self
            .storage
            .delete(data.index.clone(), data.key.clone())
            .await
        {
            Some(value) => FunctionResult::Success(Some(value)),
            None => FunctionResult::NoResult,
        }
    }

    #[function(
        name = "kv_server.update",
        description = "Update a value by key in the KV store"
    )]
    pub async fn update(&self, data: KvUpdateInput) -> FunctionResult<Option<Value>, ErrorBody> {
        tracing::debug!(index = %data.index, key = %data.key, ops_count = data.ops.len(), "Updating value in KV store");
        let result = self.storage.update(data.index, data.key, data.ops).await;
        FunctionResult::Success(Some(serde_json::to_value(result).unwrap_or_else(|e| {
            tracing::error!(error = %e, "Failed to serialize update result");
            Value::Null
        })))
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
        let result = self.storage.list(data.index.clone()).await;
        tracing::debug!(result = %result.len(), "Listing values from KV store");

        match serde_json::to_value(result) {
            Ok(value) => FunctionResult::Success(Some(value)),
            Err(err) => FunctionResult::Failure(ErrorBody {
                code: "serialization_error".into(),
                message: format!("Failed to serialize result: {}", err),
            }),
        }
    }

    #[function(
        name = "kv_server.lpush",
        description = "Push a value to the head of a list"
    )]
    pub async fn lpush(&self, data: KvLpushInput) -> FunctionResult<Option<Value>, ErrorBody> {
        tracing::debug!(key = %data.key, "Pushing value to list");
        self.storage.lpush(&data.key, data.value).await;
        FunctionResult::Success(None)
    }

    #[function(
        name = "kv_server.rpop",
        description = "Pop a value from the tail of a list"
    )]
    pub async fn rpop(&self, data: KvRpopInput) -> FunctionResult<Option<Value>, ErrorBody> {
        tracing::debug!(key = %data.key, "Popping value from list");
        let result = self.storage.rpop(&data.key).await;
        match result {
            Some(value) => FunctionResult::Success(Some(Value::String(value))),
            None => FunctionResult::NoResult,
        }
    }

    #[function(name = "kv_server.lrem", description = "Remove elements from a list")]
    pub async fn lrem(&self, data: KvLremInput) -> FunctionResult<Option<Value>, ErrorBody> {
        tracing::debug!(key = %data.key, count = data.count, "Removing elements from list");
        let removed = self.storage.lrem(&data.key, data.count, &data.value).await;
        FunctionResult::Success(Some(Value::Number(serde_json::Number::from(removed))))
    }

    #[function(name = "kv_server.llen", description = "Get the length of a list")]
    pub async fn llen(&self, data: KvLlenInput) -> FunctionResult<Option<Value>, ErrorBody> {
        tracing::debug!(key = %data.key, "Getting list length");
        let len = self.storage.llen(&data.key).await;
        FunctionResult::Success(Some(Value::Number(serde_json::Number::from(len))))
    }

    #[function(
        name = "kv_server.zadd",
        description = "Add a member with score to a sorted set"
    )]
    pub async fn zadd(&self, data: KvZaddInput) -> FunctionResult<Option<Value>, ErrorBody> {
        tracing::debug!(key = %data.key, score = data.score, "Adding member to sorted set");
        self.storage.zadd(&data.key, data.score, data.member).await;
        FunctionResult::Success(None)
    }

    #[function(
        name = "kv_server.zrem",
        description = "Remove a member from a sorted set"
    )]
    pub async fn zrem(&self, data: KvZremInput) -> FunctionResult<Option<Value>, ErrorBody> {
        tracing::debug!(key = %data.key, "Removing member from sorted set");
        let removed = self.storage.zrem(&data.key, &data.member).await;
        FunctionResult::Success(Some(Value::Bool(removed)))
    }

    #[function(
        name = "kv_server.zrangebyscore",
        description = "Get members from a sorted set within a score range"
    )]
    pub async fn zrangebyscore(
        &self,
        data: KvZrangebyscoreInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        tracing::debug!(key = %data.key, min = data.min, max = data.max, "Getting members by score range");
        let result = self
            .storage
            .zrangebyscore(&data.key, data.min, data.max)
            .await;
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
