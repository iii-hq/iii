use std::sync::Arc;

use async_trait::async_trait;
use function_macros::{function, service};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    builtins::BuiltinKvStore,
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::core_module::CoreModule,
    protocol::ErrorBody,
};

#[derive(Clone)]
pub struct KvServer {
    storage: Arc<BuiltinKvStore>,
}

#[async_trait]
impl CoreModule for KvServer {
    fn name(&self) -> &'static str {
        "KV Server"
    }

    async fn create(
        _engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn CoreModule>> {
        let storage = BuiltinKvStore::new(config);
        let storage = Arc::new(storage);
        Ok(Box::new(KvServer { storage }))
    }

    fn register_functions(&self, _engine: Arc<Engine>) {}

    async fn initialize(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Deserialize)]
pub struct KvSetInput {
    pub key: String,
    pub value: Value,
}

#[service(name = "kv_server")]
impl KvServer {
    #[function(
        name = "kv_server.get",
        description = "Get a value by key from the KV store"
    )]
    pub async fn get(&self, key: String) -> FunctionResult<Option<Value>, ErrorBody> {
        let result = self.storage.get(key).await;
        FunctionResult::Success(result)
    }

    #[function(
        name = "kv_server.set",
        description = "Set a value by key in the KV store"
    )]
    pub async fn set(&self, data: KvSetInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.storage.set(data.key, data.value).await;
        FunctionResult::NoResult
    }

    #[function(
        name = "kv_server.delete",
        description = "Delete a value by key from the KV store"
    )]
    pub async fn delete(&self, key: String) -> FunctionResult<Option<Value>, ErrorBody> {
        match self.storage.delete(key).await {
            Some(value) => FunctionResult::Success(Some(value)),
            None => FunctionResult::NoResult,
        }
    }
}

crate::register_module!(
    "modules::kv_server::KvServer",
    KvServer,
    enabled_by_default = false
);
