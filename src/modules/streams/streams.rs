use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    extract::{ConnectInfo, State, WebSocketUpgrade, ws::WebSocket},
    response::IntoResponse,
    routing::get,
};
use colored::Colorize;
use function_macros::{function, service};
use serde::Deserialize;
use serde_json::Value;
use tokio::net::TcpListener;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    modules::{
        core_module::CoreModule,
        streams::{
            StreamSocketManager,
            adapters::{RedisAdapter, StreamAdapter},
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

#[derive(Debug, Clone, Deserialize, Default)]
pub struct StreamModuleConfig {
    pub port: u16,
    #[serde(default)]
    pub adapter: StreamAdapterConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum StreamAdapterConfig {
    #[serde(rename = "redis")]
    Redis { redis_url: String },
    // #[serde(rename = "memory")]
    // InMemory,
}

impl Default for StreamAdapterConfig {
    fn default() -> Self {
        Self::Redis {
            redis_url: "redis://localhost:6379".to_string(),
        }
    }
}

impl StreamAdapterConfig {
    async fn build_adapter(&self, _engine: Arc<Engine>) -> anyhow::Result<Arc<dyn StreamAdapter>> {
        match self {
            StreamAdapterConfig::Redis { redis_url } => {
                let adapter = match tokio::time::timeout(
                    std::time::Duration::from_secs(3),
                    RedisAdapter::new(redis_url.clone()),
                )
                .await
                {
                    Ok(Ok(adapter)) => Arc::new(adapter),
                    Ok(Err(e)) => return Err(anyhow::anyhow!("Failed to connect to Redis: {}", e)),
                    Err(_) => return Err(anyhow::anyhow!("Timed out while connecting to Redis")),
                };
                Ok(adapter)
            }
        }
    }
}

#[async_trait::async_trait]
impl CoreModule for StreamCoreModule {
    async fn create(
        engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn CoreModule>> {
        let module_config: StreamModuleConfig = config
            .clone()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        let adapter_config: StreamAdapterConfig = config
            .and_then(|v| v.get("adapter").cloned())
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        let adapter = adapter_config.build_adapter(engine.clone()).await?;

        Ok(Box::new(Self {
            config: module_config,
            adapter,
        }))
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

#[service(name = "streams")]
impl StreamCoreModule {
    #[function(name = "streams.set", description = "Set a value in a stream")]
    pub async fn set(&self, input: Value) -> Result<Option<Value>, ErrorBody> {
        let adapter = self.adapter.clone();
        let stream_name = input
            .get("stream_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let group_id = input.get("group_id").and_then(|v| v.as_str()).unwrap_or("");
        let item_id = input.get("item_id").and_then(|v| v.as_str()).unwrap_or("");
        let data = input.get("data").cloned().unwrap_or(Value::Null);

        let _ = adapter
            .set(stream_name, group_id, item_id, data.clone())
            .await;

        Ok(Some(data))
    }

    #[function(name = "streams.get", description = "Get a value from a stream")]
    pub async fn get(&self, input: Value) -> Result<Option<Value>, ErrorBody> {
        let stream_name = input
            .get("stream_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let group_id = input.get("group_id").and_then(|v| v.as_str()).unwrap_or("");
        let item_id = input.get("item_id").and_then(|v| v.as_str()).unwrap_or("");
        let data = input.get("data").cloned().unwrap_or(Value::Null);

        let adapter = self.adapter.clone();
        let _ = adapter
            .set(stream_name, group_id, item_id, data.clone())
            .await;

        Ok(Some(data))
    }

    #[function(name = "streams.delete", description = "Delete a value from a stream")]
    pub async fn delete(&self, input: Value) -> Result<Option<Value>, ErrorBody> {
        let stream_name = input
            .get("stream_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let group_id = input.get("group_id").and_then(|v| v.as_str()).unwrap_or("");
        let item_id = input.get("item_id").and_then(|v| v.as_str()).unwrap_or("");

        let adapter = self.adapter.clone();
        let _ = adapter.delete(stream_name, group_id, item_id).await;

        Ok(Some(Value::Null))
    }

    #[function(name = "streams.getGroup", description = "Get a group from a stream")]
    pub async fn get_group(&self, input: Value) -> Result<Option<Value>, ErrorBody> {
        let stream_name = input
            .get("stream_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let group_id = input.get("group_id").and_then(|v| v.as_str()).unwrap_or("");

        let adapter = self.adapter.clone();
        let values = adapter.get_group(stream_name, group_id).await;

        Ok(Some(serde_json::to_value(values).unwrap()))
    }
}
