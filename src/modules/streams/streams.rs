use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, RwLock},
};

use axum::{
    Router,
    extract::{ConnectInfo, State, WebSocketUpgrade, ws::WebSocket},
    response::IntoResponse,
    routing::get,
};
use colored::Colorize;
use function_macros::{function, service};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::net::TcpListener;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::{
        core_module::{AdapterFactory, ConfigurableModule, CoreModule},
        streams::{
            StreamSocketManager,
            adapters::{RedisAdapter, StreamAdapter},
            config::AdapterEntry,
            structs::{StreamDeleteInput, StreamGetGroupInput, StreamGetInput, StreamSetInput},
        },
    },
    protocol::ErrorBody,
};

#[derive(Clone)]
pub struct StreamCoreModule {
    config: StreamModuleConfig,
    adapter: Arc<dyn StreamAdapter>,
}

async fn ws_handler(
    State(module): State<Arc<StreamSocketManager>>,
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    let module = module.clone();

    ws.on_upgrade(move |socket: WebSocket| async move {
        if let Err(err) = module.socket_handler(socket, addr).await {
            tracing::error!(addr = %addr, error = ?err, "stream socket error");
        }
    })
}

// #[derive(Debug, Clone, Deserialize, Default)]
// pub struct StreamModuleConfig {
//     pub port: u16,
//     #[serde(default)]
//     pub adapter: StreamAdapterConfig,
// }

// #[derive(Debug, Clone, Deserialize)]
// #[serde(tag = "type")]
// pub enum StreamAdapterConfig {
//     #[serde(rename = "redis")]
//     Redis { redis_url: String },
//     // #[serde(rename = "memory")]
//     // InMemory,
// }

// impl Default for StreamAdapterConfig {
//     fn default() -> Self {
//         Self::Redis {
//             redis_url: "redis://localhost:6379".to_string(),
//         }
//     }
// }

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct StreamModuleConfig {
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
    port: u16,
}

#[async_trait::async_trait]
impl CoreModule for StreamCoreModule {
    async fn create(
        engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn CoreModule>> {
        Self::create_with_adapters(engine, config).await
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing StreamCoreModule");

        let socket_manager = Arc::new(StreamSocketManager::new(self.adapter.clone()));
        let addr = format!("127.0.0.1:{}", self.config.port)
            .parse::<SocketAddr>()
            .unwrap();
        let listener = TcpListener::bind(addr).await.unwrap();
        let app = Router::new()
            .route("/", get(ws_handler))
            .with_state(socket_manager);

        tokio::spawn(async move {
            tracing::info!(
                "Stream API listening on address: {}",
                addr.to_string().purple()
            );

            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });

        let adapter = self.adapter.clone();
        tokio::spawn(async move {
            adapter.watch_events().await;
        });

        Ok(())
    }
}

#[async_trait::async_trait]
impl ConfigurableModule for StreamCoreModule {
    type Config = StreamModuleConfig;
    type Adapter = dyn StreamAdapter;
    const DEFAULT_ADAPTER_CLASS: &'static str = "modules::streams::adapters::RedisAdapter";

    fn build_registry() -> HashMap<String, AdapterFactory<Self::Adapter>> {
        let mut registry = HashMap::new();

        registry.insert(
            StreamCoreModule::DEFAULT_ADAPTER_CLASS.to_string(),
            StreamCoreModule::make_adapter_factory(
                |_engine: Arc<Engine>, config: Option<Value>| async move {
                    let redis_url = config
                        .as_ref()
                        .and_then(|c| c.get("redis_url"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("redis://localhost:6379")
                        .to_string();
                    Ok(Arc::new(RedisAdapter::new(redis_url).await?) as Arc<dyn StreamAdapter>)
                },
            ),
        );

        registry
    }
    async fn registry() -> &'static RwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<RwLock<HashMap<String, AdapterFactory<dyn StreamAdapter>>>> =
            Lazy::new(|| RwLock::new(StreamCoreModule::build_registry()));
        &REGISTRY
    }

    fn build(_engine: Arc<Engine>, config: Self::Config, adapter: Arc<Self::Adapter>) -> Self {
        Self { config, adapter }
    }

    fn adapter_class_from_config(config: &Self::Config) -> Option<String> {
        config.adapter.as_ref().map(|a| a.class.clone())
    }

    fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
        config.adapter.as_ref().and_then(|a| a.config.clone())
    }
}

#[service(name = "streams")]
impl StreamCoreModule {
    #[function(name = "streams.set", description = "Set a value in a stream")]
    pub async fn set(&self, input: StreamSetInput) -> FunctionResult<Option<Value>, ErrorBody> {
        let adapter = self.adapter.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;
        let item_id = input.item_id;
        let data = input.data;

        let _ = adapter
            .set(&stream_name, &group_id, &item_id, data.clone())
            .await;

        FunctionResult::Success(Some(data))
    }

    #[function(name = "streams.get", description = "Get a value from a stream")]
    pub async fn get(&self, input: StreamGetInput) -> FunctionResult<Option<Value>, ErrorBody> {
        let stream_name = input.stream_name;
        let group_id = input.group_id;
        let item_id = input.item_id;

        let adapter = self.adapter.clone();
        let data = adapter.get(&stream_name, &group_id, &item_id).await;

        FunctionResult::Success(data)
    }

    #[function(name = "streams.delete", description = "Delete a value from a stream")]
    pub async fn delete(
        &self,
        input: StreamDeleteInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let stream_name = input.stream_name;
        let group_id = input.group_id;
        let item_id = input.item_id;

        let adapter = self.adapter.clone();
        let data = adapter.get(&stream_name, &group_id, &item_id).await;
        let _ = adapter.delete(&stream_name, &group_id, &item_id).await;

        FunctionResult::Success(data)
    }

    #[function(name = "streams.getGroup", description = "Get a group from a stream")]
    pub async fn get_group(
        &self,
        input: StreamGetGroupInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let stream_name = input.stream_name;
        let group_id = input.group_id;

        let adapter = self.adapter.clone();
        let values = adapter.get_group(&stream_name, &group_id).await;

        FunctionResult::Success(serde_json::to_value(values).ok())
    }
}
