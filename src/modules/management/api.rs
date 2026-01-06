use std::sync::Arc;
use axum::{
    Router,
    extract::Extension,
    response::IntoResponse,
    routing::get,
    Json,
};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tower_http::cors::{CorsLayer, Any};

use crate::{
    engine::Engine,
    modules::core_module::CoreModule,
};

use super::config::ManagementConfig;

#[derive(Clone)]
pub struct ManagementModule {
    engine: Arc<Engine>,
    config: ManagementConfig,
}

#[async_trait::async_trait]
impl CoreModule for ManagementModule {
    fn name(&self) -> &'static str {
        "ManagementModule"
    }

    async fn create(
        engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn CoreModule>> {
        let config: ManagementConfig = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        Ok(Box::new(Self {
            engine,
            config,
        }))
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        if !self.config.enabled {
            tracing::info!("Management API is disabled");
            return Ok(());
        }

        let addr = format!("{}:{}", self.config.host, self.config.port);
        tracing::info!("Initializing Management API on {}", addr);

        let app = Router::new()
            .route("/_/api/status", get(get_status))
            .route("/_/api/config", get(get_config))
            .route("/_/api/triggers", get(get_triggers))
            .route("/_/api/queues", get(get_queues))
            .route("/_/api/logs", get(get_logs))
            .layer(
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods(Any)
                    .allow_headers(Any),
            )
            .layer(Extension(self.engine.clone()))
            .layer(Extension(self.config.clone()));

        let listener = TcpListener::bind(&addr).await?;

        tokio::spawn(async move {
            tracing::info!("Management API listening on address: {}", addr);
            axum::serve(listener, app).await.unwrap();
        });

        Ok(())
    }
}

// Handlers

async fn get_status(Extension(engine): Extension<Arc<Engine>>) -> impl IntoResponse {
    let worker_count = engine.worker_registry.workers.read().await.len();
    let function_count = engine.functions.iter().count();
    
    Json(json!({
        "status": "ok",
        "uptime": "TODO", 
        "workers": worker_count,
        "functions": function_count,
        "version": env!("CARGO_PKG_VERSION")
    }))
}

async fn get_config(Extension(config): Extension<ManagementConfig>) -> impl IntoResponse {
    // Redact sensitive info if needed
    Json(json!({
        "management": config
    }))
}

async fn get_triggers(Extension(engine): Extension<Arc<Engine>>) -> impl IntoResponse {
    let triggers = engine.trigger_registry.triggers.read().await;
    let list: Vec<Value> = triggers.iter().map(|pair| {
        let t = pair.value();
        json!({
            "id": t.id,
            "type": t.trigger_type,
            "function_path": t.function_path,
            "config": t.config,
            "worker_id": t.worker_id
        })
    }).collect();
    
    Json(json!({ "triggers": list }))
}

async fn get_queues() -> impl IntoResponse {
    // TODO: Implement stream introspection
    Json(json!({ "queues": [] }))
}

async fn get_logs() -> impl IntoResponse {
    // TODO: Implement log querying
    Json(json!({ "logs": [] }))
}
