// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{pin::Pin, sync::Arc};

use anyhow::anyhow;
use axum::{
    Router,
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
            http_method,
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
                id: "http".to_string(),
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

const ALLOW_ORIGIN_ANY: &str = "*";

impl RestApiCoreModule {
    #[cfg(test)]
    pub(crate) fn new_for_tests(engine: Arc<Engine>, config: RestApiConfig) -> Self {
        Self {
            engine,
            config,
            routers_registry: Arc::new(DashMap::new()),
            shared_routers: Arc::new(RwLock::new(Router::new())),
        }
    }

    fn normalize_http_path_for_key(http_path: &str) -> String {
        if http_path == "/" {
            "/".to_string()
        } else {
            http_path.trim_start_matches('/').to_string()
        }
    }

    fn build_router_key(http_method: &str, http_path: &str) -> String {
        let method = http_method.to_uppercase();
        let path = Self::normalize_http_path_for_key(http_path);
        format!("{}:{}", method, path)
    }

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

        new_router = new_router.layer(cors_layer);

        new_router = new_router.layer(TimeoutLayer::with_status_code(
            StatusCode::GATEWAY_TIMEOUT,
            std::time::Duration::from_millis(self.config.default_timeout),
        ));

        new_router = new_router.layer(ConcurrencyLimitLayer::new(
            self.config.concurrency_request_limit,
        ));

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
        use axum::{
            extract::Extension,
            routing::{delete, get, post, put},
        };

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
        let has_any_sentinel = cors_config
            .allowed_origins
            .iter()
            .any(|o| o == ALLOW_ORIGIN_ANY);

        if cors_config.allowed_origins.is_empty() || has_any_sentinel {
            if has_any_sentinel && cors_config.allowed_origins.len() > 1 {
                tracing::warn!(
                    "CORS config contains '{}' alongside explicit origins; all origins will be allowed",
                    ALLOW_ORIGIN_ANY
                );
            }
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
        let key = Self::build_router_key(http_method, http_path);
        tracing::debug!("Looking up router for key: {}", key);
        self.routers_registry
            .get(&key)
            .map(|r| (r.function_id.clone(), r.condition_function_id.clone()))
    }

    pub async fn register_router(&self, router: PathRouter) -> anyhow::Result<()> {
        let mut router = router;
        let function_id = router.function_id.clone();
        let http_path = router.http_path.clone();
        let method = router.http_method.to_uppercase();
        router.http_method = method.clone();
        let key = Self::build_router_key(&method, &router.http_path);
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
        let key = Self::build_router_key(http_method, http_path);
        tracing::info!("Unregistering router {}", key.purple());
        let removed = self.routers_registry.remove(&key).is_some();

        if removed {
            // Update routes after unregistering
            self.update_routes().await?;
        } else {
            tracing::warn!("No router found for key: {}", key.purple());
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
                .ok_or_else(|| anyhow!("api_path is required for http triggers"))?;

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
                .unwrap_or_default();

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
    use super::super::config::{CorsConfig, RestApiConfig};
    use super::*;
    use crate::modules::observability::metrics::ensure_default_meter;
    use serde_json::json;

    #[test]
    fn build_router_key_normalizes_leading_slash() {
        let a = RestApiCoreModule::build_router_key("GET", "users/:id");
        let b = RestApiCoreModule::build_router_key("get", "/users/:id");
        assert_eq!(a, b);
        assert_eq!(a, "GET:users/:id");
    }

    #[test]
    fn build_router_key_keeps_root_path() {
        let key = RestApiCoreModule::build_router_key("GET", "/");
        assert_eq!(key, "GET:/");
    }

    #[test]
    fn allow_origin_any_sentinel_is_recognized() {
        let origins = ["*".to_string()];
        assert!(origins.iter().any(|o| o == super::ALLOW_ORIGIN_ANY));
    }

    #[test]
    fn regular_origins_are_not_sentinel() {
        let origins = ["http://localhost:3000".to_string()];
        assert!(!origins.iter().any(|o| o == super::ALLOW_ORIGIN_ANY));
    }

    #[test]
    fn mixed_sentinel_and_explicit_origins_detected() {
        let origins = ["*".to_string(), "http://localhost:3000".to_string()];
        let has_any_sentinel = origins.iter().any(|o| o == super::ALLOW_ORIGIN_ANY);
        assert!(has_any_sentinel);
        assert!(origins.len() > 1);
    }

    // ---- normalize_http_path_for_key tests ----

    #[test]
    fn normalize_root_path_stays_root() {
        assert_eq!(RestApiCoreModule::normalize_http_path_for_key("/"), "/");
    }

    #[test]
    fn normalize_strips_leading_slash() {
        assert_eq!(
            RestApiCoreModule::normalize_http_path_for_key("/users"),
            "users"
        );
    }

    #[test]
    fn normalize_no_leading_slash_unchanged() {
        assert_eq!(
            RestApiCoreModule::normalize_http_path_for_key("users"),
            "users"
        );
    }

    #[test]
    fn normalize_nested_path() {
        assert_eq!(
            RestApiCoreModule::normalize_http_path_for_key("/api/v1/users"),
            "api/v1/users"
        );
    }

    #[test]
    fn normalize_path_with_params() {
        assert_eq!(
            RestApiCoreModule::normalize_http_path_for_key("/users/:id/posts"),
            "users/:id/posts"
        );
    }

    // ---- build_router_key tests ----

    #[test]
    fn build_router_key_uppercases_method() {
        let key = RestApiCoreModule::build_router_key("post", "items");
        assert_eq!(key, "POST:items");
    }

    #[test]
    fn build_router_key_various_methods() {
        assert_eq!(
            RestApiCoreModule::build_router_key("delete", "/resource"),
            "DELETE:resource"
        );
        assert_eq!(
            RestApiCoreModule::build_router_key("PUT", "/resource"),
            "PUT:resource"
        );
        assert_eq!(
            RestApiCoreModule::build_router_key("patch", "resource"),
            "PATCH:resource"
        );
    }

    #[test]
    fn build_router_key_root_with_different_methods() {
        assert_eq!(RestApiCoreModule::build_router_key("GET", "/"), "GET:/");
        assert_eq!(RestApiCoreModule::build_router_key("POST", "/"), "POST:/");
    }

    // ---- build_router_for_axum tests ----

    #[test]
    fn build_router_for_axum_converts_colon_params() {
        let result = RestApiCoreModule::build_router_for_axum(&"todo/:id".to_string());
        assert_eq!(result, "/todo/{id}");
    }

    #[test]
    fn build_router_for_axum_multiple_params() {
        let result =
            RestApiCoreModule::build_router_for_axum(&"users/:user_id/posts/:post_id".to_string());
        assert_eq!(result, "/users/{user_id}/posts/{post_id}");
    }

    #[test]
    fn build_router_for_axum_no_params() {
        let result = RestApiCoreModule::build_router_for_axum(&"api/v1/health".to_string());
        assert_eq!(result, "/api/v1/health");
    }

    #[test]
    fn build_router_for_axum_already_has_leading_slash() {
        let result = RestApiCoreModule::build_router_for_axum(&"/api/items".to_string());
        assert_eq!(result, "/api/items");
    }

    #[test]
    fn build_router_for_axum_root_path() {
        let result = RestApiCoreModule::build_router_for_axum(&"/".to_string());
        assert_eq!(result, "/");
    }

    #[test]
    fn build_router_for_axum_param_at_root() {
        let result = RestApiCoreModule::build_router_for_axum(&":id".to_string());
        assert_eq!(result, "/{id}");
    }

    #[test]
    fn build_router_for_axum_leading_slash_with_param() {
        let result = RestApiCoreModule::build_router_for_axum(&"/:id".to_string());
        assert_eq!(result, "/{id}");
    }

    // ---- build_cors_layer tests ----

    fn make_module_with_cors(cors: Option<CorsConfig>) -> RestApiCoreModule {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());
        let config = RestApiConfig {
            cors,
            ..RestApiConfig::default()
        };
        RestApiCoreModule {
            engine,
            config,
            routers_registry: Arc::new(DashMap::new()),
            shared_routers: Arc::new(RwLock::new(Router::new())),
        }
    }

    #[test]
    fn build_cors_layer_no_config_returns_permissive() {
        let module = make_module_with_cors(None);
        // Should not panic, returns a permissive CORS layer
        let _cors = module.build_cors_layer();
    }

    #[test]
    fn build_cors_layer_empty_origins_allows_any() {
        let module = make_module_with_cors(Some(CorsConfig {
            allowed_origins: vec![],
            allowed_methods: vec![],
        }));
        let _cors = module.build_cors_layer();
    }

    #[test]
    fn build_cors_layer_wildcard_origin() {
        let module = make_module_with_cors(Some(CorsConfig {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec!["GET".to_string()],
        }));
        let _cors = module.build_cors_layer();
    }

    #[test]
    fn build_cors_layer_specific_origins() {
        let module = make_module_with_cors(Some(CorsConfig {
            allowed_origins: vec![
                "http://localhost:3000".to_string(),
                "https://example.com".to_string(),
            ],
            allowed_methods: vec!["GET".to_string(), "POST".to_string()],
        }));
        let _cors = module.build_cors_layer();
    }

    #[test]
    fn build_cors_layer_mixed_wildcard_and_explicit() {
        let module = make_module_with_cors(Some(CorsConfig {
            allowed_origins: vec!["*".to_string(), "http://localhost:3000".to_string()],
            allowed_methods: vec![],
        }));
        // Should not panic. The wildcard takes precedence.
        let _cors = module.build_cors_layer();
    }

    #[test]
    fn build_cors_layer_with_all_methods() {
        let module = make_module_with_cors(Some(CorsConfig {
            allowed_origins: vec!["http://localhost:3000".to_string()],
            allowed_methods: vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "DELETE".to_string(),
                "PATCH".to_string(),
                "HEAD".to_string(),
                "OPTIONS".to_string(),
            ],
        }));
        let _cors = module.build_cors_layer();
    }

    // ---- build_routers_from_routers_registry tests ----

    #[test]
    fn build_routers_from_empty_registry() {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());
        let module = make_module_with_cors(None);
        let registry = DashMap::new();
        let _router = RestApiCoreModule::build_routers_from_routers_registry(
            engine,
            Arc::new(module),
            &registry,
        );
    }

    #[test]
    fn build_routers_from_registry_with_entries() {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());
        let module = make_module_with_cors(None);
        let registry = DashMap::new();

        registry.insert(
            "GET:users".to_string(),
            PathRouter::new(
                "users".to_string(),
                "GET".to_string(),
                "fn::list_users".to_string(),
                None,
            ),
        );
        registry.insert(
            "POST:users".to_string(),
            PathRouter::new(
                "users".to_string(),
                "POST".to_string(),
                "fn::create_user".to_string(),
                None,
            ),
        );
        registry.insert(
            "PUT:users/:id".to_string(),
            PathRouter::new(
                "users/:id".to_string(),
                "PUT".to_string(),
                "fn::update_user".to_string(),
                None,
            ),
        );
        registry.insert(
            "DELETE:users/:id".to_string(),
            PathRouter::new(
                "users/:id".to_string(),
                "DELETE".to_string(),
                "fn::delete_user".to_string(),
                None,
            ),
        );
        registry.insert(
            "PATCH:users/:id".to_string(),
            PathRouter::new(
                "users/:id".to_string(),
                "PATCH".to_string(),
                "fn::patch_user".to_string(),
                None,
            ),
        );
        registry.insert(
            "HEAD:health".to_string(),
            PathRouter::new(
                "health".to_string(),
                "HEAD".to_string(),
                "fn::health_check".to_string(),
                None,
            ),
        );
        registry.insert(
            "OPTIONS:cors".to_string(),
            PathRouter::new(
                "cors".to_string(),
                "OPTIONS".to_string(),
                "fn::cors_preflight".to_string(),
                None,
            ),
        );

        let _router = RestApiCoreModule::build_routers_from_routers_registry(
            engine,
            Arc::new(module),
            &registry,
        );
    }

    #[test]
    fn build_routers_unsupported_method_does_not_panic() {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());
        let module = make_module_with_cors(None);
        let registry = DashMap::new();

        registry.insert(
            "TRACE:debug".to_string(),
            PathRouter::new(
                "debug".to_string(),
                "TRACE".to_string(),
                "fn::trace_handler".to_string(),
                None,
            ),
        );

        let _router = RestApiCoreModule::build_routers_from_routers_registry(
            engine,
            Arc::new(module),
            &registry,
        );
    }

    // ---- get_router tests ----

    #[tokio::test]
    async fn test_get_router_found() {
        let module = make_module_with_cors(None);

        module.routers_registry.insert(
            "GET:users/:id".to_string(),
            PathRouter::new(
                "users/:id".to_string(),
                "GET".to_string(),
                "fn::get_user".to_string(),
                Some("fn::auth_check".to_string()),
            ),
        );

        let result = module.get_router("GET", "users/:id");
        assert!(result.is_some());
        let (function_id, condition) = result.unwrap();
        assert_eq!(function_id, "fn::get_user");
        assert_eq!(condition.as_deref(), Some("fn::auth_check"));
    }

    #[tokio::test]
    async fn test_get_router_not_found() {
        let module = make_module_with_cors(None);
        let result = module.get_router("GET", "nonexistent");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_get_router_normalizes_path() {
        let module = make_module_with_cors(None);

        module.routers_registry.insert(
            "POST:items".to_string(),
            PathRouter::new(
                "items".to_string(),
                "POST".to_string(),
                "fn::create_item".to_string(),
                None,
            ),
        );

        // Query with leading slash -- should be normalized
        let result = module.get_router("POST", "/items");
        assert!(result.is_some());
        let (function_id, _) = result.unwrap();
        assert_eq!(function_id, "fn::create_item");
    }

    #[tokio::test]
    async fn test_register_router_normalizes_method_before_storing() {
        let module = make_module_with_cors(None);

        module
            .register_router(PathRouter::new(
                "/health".to_string(),
                "get".to_string(),
                "fn::health".to_string(),
                None,
            ))
            .await
            .expect("register should succeed");

        let router = module
            .routers_registry
            .get("GET:health")
            .expect("router should be stored");
        assert_eq!(router.http_method, "GET");
    }

    // ---- PathRouter tests ----

    #[test]
    fn path_router_new() {
        let router = PathRouter::new(
            "/api/test".to_string(),
            "GET".to_string(),
            "fn::handler".to_string(),
            Some("fn::condition".to_string()),
        );
        assert_eq!(router.http_path, "/api/test");
        assert_eq!(router.http_method, "GET");
        assert_eq!(router.function_id, "fn::handler");
        assert_eq!(
            router.condition_function_id.as_deref(),
            Some("fn::condition")
        );
    }

    #[test]
    fn path_router_no_condition() {
        let router = PathRouter::new(
            "items".to_string(),
            "POST".to_string(),
            "fn::create".to_string(),
            None,
        );
        assert!(router.condition_function_id.is_none());
    }

    #[tokio::test]
    async fn create_name_register_functions_and_initialize_work() {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());

        let module = <RestApiCoreModule as Module>::create(
            engine.clone(),
            Some(json!({
                "host": "127.0.0.1",
                "port": 0,
                "default_timeout": 250,
                "concurrency_request_limit": 8
            })),
        )
        .await
        .expect("module should be created");

        assert_eq!(module.name(), "RestApiCoreModule");

        module.register_functions(engine.clone());
        module.initialize().await.expect("module should initialize");

        assert!(engine.trigger_registry.trigger_types.contains_key("http"));
    }

    #[tokio::test]
    async fn register_trigger_and_unregister_trigger_manage_routes() {
        let module = make_module_with_cors(None);
        let trigger = Trigger {
            id: "http-users".to_string(),
            trigger_type: "http".to_string(),
            function_id: "fn::users".to_string(),
            config: json!({
                "api_path": "users/:id",
                "http_method": "post",
                "_condition_path": "fn::condition"
            }),
            worker_id: None,
        };

        module
            .register_trigger(trigger.clone())
            .await
            .expect("trigger registration should succeed");

        let registered = module
            .get_router("POST", "/users/:id")
            .expect("router should exist");
        assert_eq!(registered.0, "fn::users");
        assert_eq!(registered.1.as_deref(), Some("fn::condition"));

        module
            .unregister_trigger(trigger)
            .await
            .expect("trigger unregistration should succeed");

        assert!(module.get_router("POST", "/users/:id").is_none());
    }

    #[tokio::test]
    async fn register_trigger_requires_api_path() {
        let module = make_module_with_cors(None);
        let trigger = Trigger {
            id: "http-missing-path".to_string(),
            trigger_type: "http".to_string(),
            function_id: "fn::users".to_string(),
            config: json!({ "http_method": "GET" }),
            worker_id: None,
        };

        let err = module
            .register_trigger(trigger)
            .await
            .expect_err("api_path should be required");
        assert!(err.to_string().contains("api_path is required"));
    }

    #[tokio::test]
    async fn unregister_router_returns_false_when_missing() {
        let module = make_module_with_cors(None);

        let removed = module
            .unregister_router("GET", "/missing")
            .await
            .expect("unregister should succeed");

        assert!(!removed);
    }
}
