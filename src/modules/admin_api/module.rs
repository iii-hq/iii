// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use colored::Colorize;
use serde_json::Value;
use tokio::net::TcpListener;

use crate::{engine::Engine, modules::module::Module};

use super::{config::AdminApiConfig, create_router};

#[derive(Clone)]
pub struct AdminApiModule {
    engine: Arc<Engine>,
    config: AdminApiConfig,
    listener: Arc<tokio::sync::Mutex<Option<TcpListener>>>,
}

#[async_trait::async_trait]
impl Module for AdminApiModule {
    fn name(&self) -> &'static str {
        "AdminApiModule"
    }

    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
        let config: AdminApiConfig = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        Ok(Box::new(Self {
            engine,
            config,
            listener: Arc::new(tokio::sync::Mutex::new(None)),
        }))
    }

    fn register_functions(&self, _engine: Arc<Engine>) {}

    async fn initialize(&self) -> anyhow::Result<()> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        let listener = TcpListener::bind(&addr).await?;

        tracing::info!("Admin API bound to address: {}", addr.bright_cyan().bold());

        *self.listener.lock().await = Some(listener);

        Ok(())
    }

    async fn start_background_tasks(
        &self,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let listener = self
            .listener
            .lock()
            .await
            .take()
            .ok_or_else(|| anyhow::anyhow!("Listener not initialized"))?;

        let addr = listener.local_addr()?;
        let router = create_router(self.engine.clone());

        tokio::spawn(async move {
            tracing::info!(
                "Admin API endpoints available at http://{}",
                addr.to_string().purple()
            );
            tracing::info!("  POST   /admin/functions - Register HTTP function");
            tracing::info!("  GET    /admin/functions - List all functions");
            tracing::info!("  GET    /admin/functions/{{path}} - Get function details");
            tracing::info!("  PUT    /admin/functions/{{path}} - Update function");
            tracing::info!("  DELETE /admin/functions/{{path}} - Delete function");
            tracing::warn!(
                "⚠️  Secure this endpoint! Set {} environment variable",
                "III_ADMIN_TOKEN".yellow()
            );

            let server = axum::serve(listener, router).with_graceful_shutdown(async move {
                let _ = shutdown.changed().await;
                tracing::info!("Admin API server shutting down gracefully");
            });

            if let Err(e) = server.await {
                tracing::error!(
                    error = %e,
                    "Admin API server error"
                );
            }
        });

        Ok(())
    }
}

crate::register_module!("modules::admin_api::AdminApiModule", AdminApiModule);
