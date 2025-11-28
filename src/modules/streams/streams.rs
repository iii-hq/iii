use std::{net::SocketAddr, pin::Pin, sync::Arc};

use axum::{
    Router,
    extract::{ConnectInfo, State, WebSocketUpgrade, ws::WebSocket},
    response::IntoResponse,
    routing::get,
};
use colored::Colorize;
use serde_json::Value;
use tokio::{net::TcpListener, sync::OnceCell};
use uuid::Uuid;

use crate::{
    engine::{Engine, EngineTrait, RegisterFunctionRequest},
    function::FunctionHandler,
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
    engine: Arc<Engine>,
    adapter: Arc<OnceCell<Arc<dyn StreamAdapter>>>,
}

async fn ws_handler(
    State(module): State<Arc<StreamSocketManager>>,
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    let module = module.clone();

    tracing::info!("WebSocket handler called");

    ws.on_upgrade(move |socket: WebSocket| async move {
        tracing::info!("WebSocket upgraded");

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

                    let adapter = Arc::clone(&self.adapter);
                    let _ = adapter
                        .get()
                        .unwrap()
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

                    let adapter = Arc::clone(&self.adapter);
                    let value = adapter
                        .get()
                        .unwrap()
                        .get(stream_name, group_id, item_id)
                        .await;

                    Ok(value)
                }

                "streams.delete" => {
                    let stream_name = input
                        .get("stream_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let group_id = input.get("group_id").and_then(|v| v.as_str()).unwrap_or("");
                    let item_id = input.get("item_id").and_then(|v| v.as_str()).unwrap_or("");

                    let adapter = Arc::clone(&self.adapter);
                    let _ = adapter
                        .get()
                        .unwrap()
                        .delete(stream_name, group_id, item_id)
                        .await;

                    Ok(None)
                }

                "streams.get_group" => {
                    let stream_name = input
                        .get("stream_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let group_id = input.get("group_id").and_then(|v| v.as_str()).unwrap_or("");

                    let adapter = Arc::clone(&self.adapter);
                    let values = adapter
                        .get()
                        .unwrap()
                        .get_group(stream_name, group_id)
                        .await;

                    Ok(Some(serde_json::to_value(values).unwrap()))
                }
                _ => Ok(None),
            }
        })
    }
}

#[async_trait::async_trait]
impl CoreModule for StreamCoreModule {
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

        // To register multiple similar functions more elegantly, we can use an array of tuples and iterate.
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

        let socket_manager = Arc::new(StreamSocketManager::new(adapter.clone()));
        let addr = "127.0.0.1:31112";
        let listener = TcpListener::bind(addr).await.unwrap();
        let app = Router::new()
            .route("/", get(ws_handler))
            .with_state(socket_manager);

        tokio::spawn(async move {
            tracing::info!("Stream API listening on address: {}", addr.purple());
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
            tracing::info!("Stream API stopped");
        });

        tokio::spawn(async move {
            adapter.watch_events().await;
        });

        Ok(())
    }
}

impl StreamCoreModule {
    pub fn new(engine: Arc<Engine>) -> Self {
        let adapter = Arc::new(OnceCell::new());

        Self { adapter, engine }
    }
}
