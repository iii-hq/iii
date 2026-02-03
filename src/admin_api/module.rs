// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use colored::Colorize;
use serde::Deserialize;
use serde_json::Value;
use tokio::net::TcpListener;

use crate::{engine::Engine, modules::module::Module};

use super::create_router;

const DEFAULT_ADMIN_PORT: u16 = 49135;
const DEFAULT_ADMIN_HOST: &str = "127.0.0.1";

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AdminApiConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_host")]
    pub host: String,
}

fn default_port() -> u16 {
    DEFAULT_ADMIN_PORT
}

fn default_host() -> String {
    DEFAULT_ADMIN_HOST.to_string()
}

impl Default for AdminApiConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_ADMIN_PORT,
            host: DEFAULT_ADMIN_HOST.to_string(),
        }
    }
}

#[derive(Clone)]
pub struct AdminApiModule {
    engine: Arc<Engine>,
    config: AdminApiConfig,
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

        Ok(Box::new(Self { engine, config }))
    }

    fn register_functions(&self, _engine: Arc<Engine>) {}

    async fn initialize(&self) -> anyhow::Result<()> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        let listener = TcpListener::bind(&addr).await?;

        tracing::info!(
            "Admin API listening on address: {}",
            addr.bright_cyan().bold()
        );

        let router = create_router(self.engine.clone());

        tokio::spawn(async move {
            tracing::info!(
                "Admin API endpoints available at http://{}",
                addr.purple()
            );
            tracing::info!("  POST   /admin/functions - Register HTTP function");
            tracing::info!("  GET    /admin/functions - List all functions");
            tracing::info!("  PUT    /admin/functions/:path - Update function");
            tracing::info!("  DELETE /admin/functions/:path - Delete function");
            tracing::warn!(
                "⚠️  Secure this endpoint! Set {} environment variable",
                "III_ADMIN_TOKEN".yellow()
            );

            if let Err(e) = axum::serve(listener, router).await {
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
