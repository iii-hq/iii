use std::{collections::HashMap, future::Future, net::SocketAddr, pin::Pin, sync::Arc};

use async_trait::async_trait;
use axum::{
    extract::{ConnectInfo, State, WebSocketUpgrade, ws::WebSocket},
    response::IntoResponse,
    Router,
    routing::get,
};
use colored::Colorize;
use serde_json::Value;
use tokio::net::TcpListener;
use uuid::Uuid;

use crate::{
    engine::{Engine, EngineTrait, RegisterFunctionRequest},
    function::FunctionHandler,
    modules::{
        core_module::{ConfigurableModule, CoreModule},
        streams::{
            adapters::{RedisAdapter, StreamAdapter},
            config::StreamModuleConfig,
            StreamSocketManager,
        },
    },
    protocol::ErrorBody,
};

#[derive(Clone)]
pub struct StreamCoreModule {
    engine: Arc<Engine>,
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

impl FunctionHandler for StreamCoreModule {
    fn handle_function<'a>(
        &'a self,
        _invocation_id: Option<Uuid>,
        function_path: String,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Value>, ErrorBody>> + Send + 'a>> {
        Box::pin(async move {
            match function_path.as_str() {
                "streams.set" => {
                    let stream_name = input
                        .get("stream_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let group_id = input.get("group_id").and_then(|v| v.as_str()).unwrap_or("");
                    let item_id = input.get("item_id").and_then(|v| v.as_str()).unwrap_or("");
                    let data = input.get("data").cloned().unwrap_or(Value::Null);

                    let _ = self
                        .adapter
                        .set(stream_name, group_id, item_id, data.clone())
                        .await;

                    Ok(Some(data))
                }

                "streams.get" => {
                    let stream_name = input
                        .get("stream_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let group_id = input.get("group_id").and_then(|v| v.as_str()).unwrap_or("");
                    let item_id = input.get("item_id").and_then(|v| v.as_str()).unwrap_or("");

                    let value = self.adapter.get(stream_name, group_id, item_id).await;

                    Ok(value)
                }

                "streams.delete" => {
                    let stream_name = input
                        .get("stream_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let group_id = input.get("group_id").and_then(|v| v.as_str()).unwrap_or("");
                    let item_id = input.get("item_id").and_then(|v| v.as_str()).unwrap_or("");

                    let _ = self.adapter.delete(stream_name, group_id, item_id).await;

                    Ok(Some(Value::Null))
                }

                "streams.get_group" => {
                    let stream_name = input
                        .get("stream_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let group_id = input.get("group_id").and_then(|v| v.as_str()).unwrap_or("");

                    let values = self.adapter.get_group(stream_name, group_id).await;

                    Ok(Some(serde_json::to_value(values).unwrap()))
                }
                _ => Ok(None),
            }
        })
    }
}

#[async_trait]
impl CoreModule for StreamCoreModule {
    async fn create(
        engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn CoreModule>> {
        use crate::modules::core_module::AdapterFactory;
        let mut adapter_factories: HashMap<String, AdapterFactory<dyn StreamAdapter>> =
            HashMap::new();

        adapter_factories.insert(
            "modules::streams::RedisAdapter".to_string(),
            Box::new(|_engine: Arc<Engine>, config: Option<Value>| {
                Box::pin(async move {
                    let redis_url = config
                        .as_ref()
                        .and_then(|v| v.get("redis_url").and_then(|u| u.as_str()))
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "redis://localhost:6379".to_string());

                    let adapter = RedisAdapter::new(redis_url).await?;
                    Ok(Arc::new(adapter) as Arc<dyn StreamAdapter>)
                })
            }),
        );

        Self::create_with_adapters(engine, config, adapter_factories).await
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing StreamCoreModule");

        let functions = [
            ("streams.set", "Set a value in a stream"),
            ("streams.get", "Get a value from a stream"),
            ("streams.delete", "Delete a value from a stream"),
            ("streams.get_group", "Get a group of values from a stream"),
        ];

        for (function_path, description) in functions.iter() {
            let _ = self.engine.register_function(
                RegisterFunctionRequest {
                    function_path: function_path.to_string(),
                    description: Some(description.to_string()),
                    request_format: None,
                    response_format: None,
                },
                Box::new(self.clone()),
            );
        }

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

#[async_trait]
impl ConfigurableModule for StreamCoreModule {
    type Config = StreamModuleConfig;
    type Adapter = dyn StreamAdapter;

    fn build(engine: Arc<Engine>, config: Self::Config, adapter: Arc<Self::Adapter>) -> Self {
        Self {
            engine,
            config,
            adapter,
        }
    }

    fn default_adapter_class() -> &'static str {
        "modules::streams::RedisAdapter"
    }

    fn adapter_class_from_config(config: &Self::Config) -> Option<String> {
        config.adapter.as_ref().map(|a| a.class.clone())
    }

    fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
        config.adapter.as_ref().and_then(|a| a.config.clone())
    }
}
