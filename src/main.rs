use std::{net::SocketAddr, sync::Arc};

mod engine;
mod function;
mod invocation;
mod logging;
mod pending_invocations;
mod protocol;
mod services;
mod trigger;
mod workers;

use axum::{
    Router,
    extract::{ConnectInfo, State, ws::WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use tokio::net::TcpListener;
use uuid::Uuid;

use engine::{Engine, EngineTrait};

use crate::function::Function;

async fn ws_handler(
    State(engine): State<Arc<Engine>>,
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        let engine = engine.clone();
        async move {
            if let Err(err) = engine.handle_worker(socket, addr).await {
                tracing::error!(addr = %addr, error = ?err, "worker error");
            }
        }
    })
}

fn register_local_functions(engine: &Arc<Engine>) {
    let logging_function = Function {
        handler: Box::new(
            move |_invocation_id: Option<Uuid>, input: serde_json::Value| {
                Box::pin(async move {
                    tracing::info!(input = ?input, "logger.info invoked");
                    Ok(None)
                })
            },
        ),
        _function_path: "logger.info".to_string(),
        _description: Some("Log an info message".to_string()),
        request_format: Some(serde_json::json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" }
            },
            "required": ["message"]
        })),
        response_format: None,
    };

    engine.register_local_functions(vec![logging_function]);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logging::init_tracing();

    let engine = Arc::new(Engine::new());
    register_local_functions(&engine);
    let engine_clone = engine.clone();
    tokio::spawn(async move {
        engine_clone.notify_new_functions(5).await;
    });

    let app = Router::new()
        .route("/", get(ws_handler))
        .with_state(engine.clone());

    let addr = "127.0.0.1:49134";
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(address = addr, "Engine listening");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
