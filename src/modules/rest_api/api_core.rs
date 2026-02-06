// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{pin::Pin, sync::Arc};

use anyhow::anyhow;
use axum::{
    Router,
    extract::Extension,
    http::{Method, StatusCode},
};
use colored::Colorize;
use dashmap::DashMap;
use futures::Future;
use serde_json::Value;
use tokio::{net::TcpListener, sync::RwLock};
use tower::limit::ConcurrencyLimitLayer;
use tower_http::{
    cors::{Any as HTTP_Any, CorsLayer},
    timeout::TimeoutLayer,
};

use super::{
    config::RestApiConfig,
    hot_router::{HotRouter, MakeHotRouterService},
    views::dynamic_handler,
};
use crate::{
    engine::{Engine, EngineTrait},
    modules::module::Module,
    trigger::{Trigger, TriggerRegistrator, TriggerType},
};

#[derive(Debug)]
pub struct PathRouter {
    pub http_path: String,
    pub http_method: String,
    pub function_id: String,
    pub condition_function_id: Option<String>,
}

impl PathRouter {
    pub fn new(
        http_path: String,
        http_method: String,
        function_id: String,
        condition_function_id: Option<String>,
    ) -> Self {
        Self {
            http_path,
            http_method: http_method.to_uppercase(),
            function_id,
            condition_function_id,
        }
    }
}

#[derive(Clone)]
pub struct RestApiCoreModule {
    engine: Arc<Engine>,
    config: RestApiConfig,
    pub routers_registry: Arc<DashMap<String, PathRouter>>,
    shared_routers: Arc<RwLock<Router>>,
}

#[async_trait::async_trait]
impl Module for RestApiCoreModule {
    fn name(&self) -> &'static str {
        "RestApiCoreModule"
    }
    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
        let config: RestApiConfig = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        let routers_registry = Arc::new(DashMap::new());

        // Create an empty router initially, it will be updated when routes are registered
        let empty_router = Router::new();
        let shared_routers = Arc::new(RwLock::new(empty_router));

        Ok(Box::new(Self {
            engine,
            config,
            routers_registry,
            shared_routers,
        }))
    }

    fn register_functions(&self, _engine: Arc<Engine>) {}

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing API adapter on port {}", self.config.port);

        self.engine
            .clone()
            .register_trigger_type(TriggerType {
                id: "api".to_string(),
                _description: "HTTP API trigger".to_string(),
                registrator: Box::new(self.clone()),
                worker_id: None,
            })
            .await;

        let addr = format!("{}:{}", self.config.host, self.config.port);
        let listener = TcpListener::bind(&addr).await?;

        // Build initial router from registry
        self.update_routes().await?;

        let hot_router = HotRouter {
            inner: self.shared_routers.clone(),
            engine: self.engine.clone(),
        };

        tokio::spawn(async move {
            tracing::info!("API listening on address: {}", addr.purple());
            let make_service = MakeHotRouterService {
                router: hot_router.clone(),
            };
            axum::serve(listener, make_service).await.unwrap();
        });

        Ok(())
    }
}

impl RestApiCoreModule {
    /// Updates the router with all routes from the registry and configurations
    async fn update_routes(&self) -> anyhow::Result<()> {
        // Build CORS layer
        let cors_layer = self.build_cors_layer();

        // Read the routers_registry and build the router
        let mut new_router = Self::build_routers_from_routers_registry(
            self.engine.clone(),
            Arc::new(self.clone()),
            &self.routers_registry,
        );

        // Apply CORS layer to the router
        new_router = new_router.layer(cors_layer);

        // Apply timeout layer based on configuration
        new_router = new_router.layer(TimeoutLayer::with_status_code(
            StatusCode::GATEWAY_TIMEOUT,
            std::time::Duration::from_millis(self.config.default_timeout),
        ));

        new_router = new_router.layer(ConcurrencyLimitLayer::new(
            self.config.concurrency_request_limit,
        ));

        // Update the shared router
        let mut shared_router = self.shared_routers.write().await;
        *shared_router = new_router;

        tracing::debug!("Routes updated successfully");
        Ok(())
    }

    fn build_router_for_axum(path: &String) -> String {
        // Axum requires paths to start with a leading slash
        // and convert :param to {param}, since axum 0.8 changed the syntax

        // update for axum 0.8, replacing todo/:id to todo/{id}
        let axum_path = path
            .clone()
            .split('/')
            .map(|segment| {
                if segment.strip_prefix(':').is_some() {
                    format!("{{{}}}", &segment[1..])
                } else {
                    segment.to_string()
                }
            })
            .collect::<Vec<String>>()
            .join("/");

        if axum_path != *path {
            tracing::debug!(
                "Converted path from {} to {}",
                path.purple(),
                axum_path.purple()
            );
        }

        //ensure the path starts with a leading slash
        if !axum_path.starts_with('/') {
            format!("/{}", axum_path)
        } else {
            axum_path
        }
    }
    fn build_routers_from_routers_registry(
        engine: Arc<Engine>,
        api_handler: Arc<RestApiCoreModule>,
        routers_registry: &DashMap<String, PathRouter>,
    ) -> Router {
        use axum::routing::{delete, get, post, put};

        let mut router = Router::new();

        for entry in routers_registry.iter() {
            let path = Self::build_router_for_axum(&entry.http_path);

            let method = entry.http_method.clone();
            let path_for_extension = entry.http_path.clone();
            router = match method.as_str() {
                "GET" => router.route(
                    &path,
                    get(dynamic_handler).layer(Extension(path_for_extension)),
                ),
                "POST" => router.route(
                    &path,
                    post(dynamic_handler).layer(Extension(path_for_extension)),
                ),
                "PUT" => router.route(
                    &path,
                    put(dynamic_handler).layer(Extension(path_for_extension)),
                ),
                "DELETE" => router.route(
                    &path,
                    delete(dynamic_handler).layer(Extension(path_for_extension)),
                ),
                "PATCH" => router.route(
                    &path,
                    axum::routing::patch(dynamic_handler).layer(Extension(path_for_extension)),
                ),
                "HEAD" => router.route(
                    &path,
                    axum::routing::head(dynamic_handler).layer(Extension(path_for_extension)),
                ),
                "OPTIONS" => router.route(
                    &path,
                    axum::routing::options(dynamic_handler).layer(Extension(path_for_extension)),
                ),
                _ => {
                    tracing::warn!("Unsupported HTTP method: {}", method.purple());
                    router
                }
            };
        }

        router
            .layer(Extension(engine))
            .layer(Extension(api_handler))
    }
    /// Builds the CorsLayer based on configuration
    fn build_cors_layer(&self) -> CorsLayer {
        let Some(cors_config) = &self.config.cors else {
            return CorsLayer::permissive();
        };

        let mut cors = CorsLayer::new();

        // Origins
        if cors_config.allowed_origins.is_empty() {
            cors = cors.allow_origin(HTTP_Any);
        } else {
            let origins: Vec<_> = cors_config
                .allowed_origins
                .iter()
                .filter_map(|o| o.parse().ok())
                .collect();
            cors = cors.allow_origin(origins);
        }

        // Methods
        if cors_config.allowed_methods.is_empty() {
            cors = cors.allow_methods(HTTP_Any);
        } else {
            let methods: Vec<Method> = cors_config
                .allowed_methods
                .iter()
                .filter_map(|m| m.parse().ok())
                .collect();
            cors = cors.allow_methods(methods);
        }

        cors.allow_headers(HTTP_Any)
    }

    pub fn get_router(
        &self,
        http_method: &str,
        http_path: &str,
    ) -> Option<(String, Option<String>)> {
        let method = http_method.to_uppercase();
        let http_path = if http_path.starts_with('/') {
            tracing::debug!("Looking up router for path with leading slash");
            // need to check if we want to do this or return an error when registering with leading slash http_path
            http_path
                .to_string()
                .clone()
                .trim_start_matches('/')
                .to_string()
        } else {
            http_path.to_string()
        };
        let key = format!("{}:{}", method, http_path);
        tracing::debug!("Looking up router for key: {}", key);
        self.routers_registry
            .get(&key)
            .map(|r| (r.function_id.clone(), r.condition_function_id.clone()))
    }

    pub async fn register_router(&self, router: PathRouter) -> anyhow::Result<()> {
        let function_id = router.function_id.clone();
        let http_path = router.http_path.clone();
        let method = router.http_method.to_uppercase();
        let key = format!("{}:{}", method, router.http_path);
        tracing::debug!("Registering router {}", key.purple());
        self.routers_registry.insert(key, router);

        tracing::info!(
            "{} Endpoint {} → {}",
            "[REGISTERED]".green(),
            format!("{} /{}", method, http_path).bright_yellow().bold(),
            function_id.purple()
        );

        // Update routes after registering
        self.update_routes().await?;

        Ok(())
    }

    pub async fn unregister_router(
        &self,
        http_method: &str,
        http_path: &str,
    ) -> anyhow::Result<bool> {
        let key = format!("{}:{}", http_method.to_uppercase(), http_path);
        tracing::debug!("Unregistering router {}", key.purple());
        let removed = self.routers_registry.remove(&key).is_some();

        if removed {
            // Update routes after unregistering
            self.update_routes().await?;
        }

        Ok(removed)
    }
}

impl TriggerRegistrator for RestApiCoreModule {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let adapter = self.clone();

        Box::pin(async move {
            let api_path = trigger
                .config
                .get("api_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("api_path is required for api triggers"))?;

            let http_method = trigger
                .config
                .get("http_method")
                .and_then(|v| v.as_str())
                .unwrap_or("GET");

            let condition_function_id = trigger
                .config
                .get("_condition_path")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());

            let router = PathRouter::new(
                api_path.to_string(),
                http_method.to_string(),
                trigger.function_id.clone(),
                condition_function_id,
            );

            adapter.register_router(router).await?;
            Ok(())
        })
    }

    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let adapter = self.clone();

        Box::pin(async move {
            let api_path = trigger
                .config
                .get("api_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("api_path is required to unregister api triggers"))?;

            let http_method = trigger
                .config
                .get("http_method")
                .and_then(|v| v.as_str())
                .unwrap_or("GET");

            adapter.unregister_router(http_method, api_path).await?;
            Ok(())
        })
    }
}

crate::register_module!(
    "modules::api::RestApiModule",
    RestApiCoreModule,
    enabled_by_default = true
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trigger::TriggerRegistrator;

    #[tokio::test]
    async fn unregister_trigger_errors_on_missing_api_path() {
        let engine = Arc::new(Engine::default());
        let module = RestApiCoreModule {
            engine: engine.clone(),
            config: RestApiConfig::default(),
            routers_registry: Arc::new(DashMap::new()),
            shared_routers: Arc::new(RwLock::new(Router::new())),
        };

        let trigger = Trigger {
            id: "t1".to_string(),
            trigger_type: "api".to_string(),
            function_id: "func1".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
        };

        let result = module.unregister_trigger(trigger).await;
        assert!(
            result.is_err(),
            "must error when api_path is missing from config"
        );
    }

    #[tokio::test]
    async fn unregister_trigger_removes_registered_route() {
        let engine = Arc::new(Engine::default());
        let module = RestApiCoreModule {
            engine: engine.clone(),
            config: RestApiConfig::default(),
            routers_registry: Arc::new(DashMap::new()),
            shared_routers: Arc::new(RwLock::new(Router::new())),
        };

        let trigger = Trigger {
            id: "t1".to_string(),
            trigger_type: "api".to_string(),
            function_id: "func1".to_string(),
            config: serde_json::json!({"api_path": "users", "http_method": "GET"}),
            worker_id: None,
        };
        module.register_trigger(trigger.clone()).await.unwrap();
        assert!(module.routers_registry.get("GET:users").is_some());

        let result = module.unregister_trigger(trigger).await;
        assert!(result.is_ok());
        assert!(
            module.routers_registry.get("GET:users").is_none(),
            "route must be removed from registry"
        );
    }

    #[tokio::test]
    async fn register_trigger_normalizes_lowercase_method() {
        let engine = Arc::new(Engine::default());
        let module = RestApiCoreModule {
            engine: engine.clone(),
            config: RestApiConfig::default(),
            routers_registry: Arc::new(DashMap::new()),
            shared_routers: Arc::new(RwLock::new(Router::new())),
        };

        let trigger = Trigger {
            id: "t1".to_string(),
            trigger_type: "api".to_string(),
            function_id: "func1".to_string(),
            config: serde_json::json!({"api_path": "users", "http_method": "get"}),
            worker_id: None,
        };
        module.register_trigger(trigger).await.unwrap();

        let entry = module.routers_registry.get("GET:users").unwrap();
        assert_eq!(
            entry.http_method, "GET",
            "http_method must be normalized to uppercase"
        );
    }
}
