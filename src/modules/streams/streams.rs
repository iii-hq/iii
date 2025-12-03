use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    extract::{ConnectInfo, State, WebSocketUpgrade, ws::WebSocket},
    response::IntoResponse,
    routing::get,
};
use colored::Colorize;
use function_macros::{function, service};
use serde_json::Value;
use tokio::{net::TcpListener, sync::OnceCell};

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    modules::{
        configurable::Configurable,
        core_module::CoreModule,
        streams::{
            StreamSocketManager,
            adapters::{RedisAdapter, StreamAdapter},
            config::WebSocketConfig,
        },
    },
    protocol::ErrorBody,
};

#[derive(Clone)]
pub struct StreamCoreModule {
    config: WebSocketConfig,
    adapter: Arc<OnceCell<Arc<dyn StreamAdapter>>>,
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

#[async_trait::async_trait]
impl CoreModule for StreamCoreModule {
    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> Result<(), anyhow::Error> {
        tracing::info!("Initializing StreamCoreModule");

        let adapter = match tokio::time::timeout(
            std::time::Duration::from_secs(3),
            RedisAdapter::new("redis://localhost:6379".to_string()),
        )
        .await
        {
            Ok(Ok(adapter)) => Arc::new(adapter),
            Ok(Err(e)) => return Err(anyhow::anyhow!("Failed to connect to Redis: {}", e)),
            Err(_) => return Err(anyhow::anyhow!("Timed out while connecting to Redis")),
        };

        self.adapter
            .set(adapter.clone())
            .map_err(|_| anyhow::anyhow!("Failed to set StreamAdapter"))?;

        let socket_manager = Arc::new(StreamSocketManager::new(adapter.clone()));

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

        tokio::spawn(async move {
            adapter.watch_events().await;
        });

        Ok(())
    }
}

#[service(name = "streams")]
impl StreamCoreModule {
    pub fn new() -> Self {
        let adapter = Arc::new(OnceCell::new());
        let config = WebSocketConfig::default();

        Self { adapter, config }
    }

    #[function(name = "streams.set", description = "Set a value in a stream")]
    pub async fn set(&self, input: Value) -> Result<Option<Value>, ErrorBody> {
        let adapter = self.adapter.get().unwrap();
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

        let adapter = Arc::clone(&self.adapter);
        let adapter = adapter.get().unwrap();
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

        let adapter = Arc::clone(&self.adapter);
        let adapter = adapter.get().unwrap();
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

        let adapter = Arc::clone(&self.adapter);
        let adapter = adapter.get().unwrap();
        let values = adapter.get_group(stream_name, group_id).await;

        Ok(Some(serde_json::to_value(values).unwrap()))
    }
}

impl Configurable for StreamCoreModule {
    type Config = WebSocketConfig;

    fn with_config(_engine: Arc<Engine>, config: Self::Config) -> Self {
        Self {
            config,
            adapter: Arc::new(OnceCell::new()),
        }
    }
}
