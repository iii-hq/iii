mod config;

use std::{
    collections::HashMap,
    convert::Infallible,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use anyhow::anyhow;
use axum::{
    Json, Router,
    body::Body,
    extract::{Extension, Query},
    http::{Method, Request, Response, StatusCode, Uri, header::HeaderMap},
    response::IntoResponse,
    serve::IncomingStream,
};
use colored::Colorize;
pub use config::RestApiConfig;
use dashmap::DashMap;
use futures::Future;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{net::TcpListener, sync::RwLock};
use tower::Service;
use tower_http::cors::{Any as HTTP_Any, CorsLayer};

use crate::{
    engine::{Engine, EngineTrait},
    modules::core_module::CoreModule,
    trigger::{Trigger, TriggerRegistrator, TriggerType},
};

#[derive(Debug)]
pub struct PathRouter {
    pub http_path: String,
    pub http_method: String,
    pub function_path: String,
}

impl PathRouter {
    pub fn new(http_path: String, http_method: String, function_path: String) -> Self {
        Self {
            http_path,
            http_method,
            function_path,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct APIrequest {
    pub query_params: HashMap<String, String>,
    pub path_params: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub path: String,
    pub method: String,
    pub body: Value,
}

impl APIrequest {
    pub fn new(
        query_params: HashMap<String, String>,
        path_params: HashMap<String, String>,
        headers: HeaderMap,
        path: String,
        method: String,
        body: Option<Json<Value>>,
    ) -> Self {
        // sometimes body can be empty, like in GET requests
        let body_value = body.map(|Json(v)| v).unwrap_or(serde_json::json!({}));
        APIrequest {
            query_params,
            path_params,
            headers: headers
                .iter()
                .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
                .collect(),
            path,
            method,
            body: body_value,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct APIresponse {
    pub status_code: u16,
    pub headers: Vec<String>,
    pub body: Value,
}

impl APIresponse {
    pub fn from_function_return(value: Value) -> Self {
        let status_code = value
            .get("status_code")
            .and_then(|v| v.as_u64())
            .unwrap_or(200) as u16;
        let headers = value
            .get("headers")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<String>>()
            })
            .unwrap_or(vec![]);
        let body = value.get("body").cloned().unwrap_or(json!({}));
        APIresponse {
            status_code,
            headers,
            body,
        }
    }
}

#[derive(Clone)]
pub struct RestApiCoreModule {
    engine: Arc<Engine>,
    config: RestApiConfig,
    pub routers_registry: Arc<RwLock<DashMap<String, PathRouter>>>,
    shared_routers: Arc<RwLock<Router>>,
}

#[derive(Clone)]
struct HotRouter {
    inner: Arc<RwLock<Router>>,
    engine: Arc<Engine>,
}

impl Service<Request<Body>> for HotRouter {
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let router_arc = self.inner.clone();
        let engine = self.engine.clone();
        Box::pin(async move {
            let router = router_arc.read().await;
            // Clone the router and add the engine as Extension
            // This allows using the router without state directly as Service
            let router_with_extension = router.clone().layer(Extension(engine));
            let mut svc = router_with_extension;
            // The Router without state implements Service<Request<Body>> directly
            match svc.call(req).await {
                Ok(response) => Ok(response),
                Err(_) => {
                    tracing::error!("Error handling request in HotRouter");
                    let error_response = Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from("Internal Server Error"))
                        .unwrap();
                    Ok(error_response)
                }
            }
        })
    }
}

struct MakeHotRouterService {
    router: HotRouter,
}

impl<'a> tower::Service<IncomingStream<'a>> for MakeHotRouterService {
    type Response = HotRouter;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: IncomingStream<'a>) -> Self::Future {
        let router = self.router.clone();
        Box::pin(async move { Ok(router) })
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
        let mut path = entry.http_path.clone();
        if !path.starts_with('/') {
            // need to check if we want to do this or return an error when registering with leading slash
            path = format!("/{}", path);
        }
        let method = entry.http_method.clone();
        let path_for_extension = path.clone();
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

#[async_trait::async_trait]
impl CoreModule for RestApiCoreModule {
    async fn create(
        engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn CoreModule>> {
        let config: RestApiConfig = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        let routers_registry = Arc::new(RwLock::new(DashMap::new()));

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
    /// Updates the router with all routes from the registry
    async fn update_routes(&self) -> anyhow::Result<()> {
        // Build CORS layer
        let cors_layer = self.build_cors_layer();

        // Read the routers_registry and build the router
        let routers_registry_guard = self.routers_registry.read().await;
        let mut new_router = build_routers_from_routers_registry(
            self.engine.clone(),
            Arc::new(self.clone()),
            &routers_registry_guard,
        );
        drop(routers_registry_guard); // Release the lock explicitly

        // Apply CORS layer to the router
        new_router = new_router.layer(cors_layer);

        // Update the shared router
        let mut shared_router = self.shared_routers.write().await;
        *shared_router = new_router;

        tracing::info!("Routes updated successfully");
        Ok(())
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

    pub async fn get_router(&self, http_method: &str, http_path: &str) -> Option<String> {
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
        let routers = self.routers_registry.read().await;
        let router = routers.get(&key);
        router.map(|r| r.function_path.clone())
    }

    pub async fn register_router(&self, router: PathRouter) -> anyhow::Result<()> {
        let function_path = router.function_path.clone();
        let http_path = router.http_path.clone();
        let method = router.http_method.to_uppercase();
        let key = format!("{}:{}", method, router.http_path);
        tracing::debug!("Registering router: {}", key);
        self.routers_registry.write().await.insert(key, router);

        tracing::info!(
            "{} Endpoint {} → {}",
            "[REGISTERED]".green(),
            format!("{} /{}", method, http_path).bright_yellow().bold(),
            function_path.purple()
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
        tracing::debug!("Unregistering router: {}", key);
        let removed = self.routers_registry.write().await.remove(&key).is_some();

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

            let router = PathRouter::new(
                api_path.to_string(),
                http_method.to_string(),
                trigger.function_path.clone(),
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

// Helper function to extract all path parameters from a route pattern and actual path
// Returns a HashMap<String, String> where keys are parameter names (without ':') and values are their corresponding values
// Example:
//   registered_path = "/users/:id/posts/:post_id"
//   actual_path = "/users/123/posts/456"
//   Returns: {"id": "123", "post_id": "456"}
fn extract_path_params(registered_path: &str, actual_path: &str) -> HashMap<String, String> {
    let registered_segments: Vec<&str> = registered_path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let actual_segments: Vec<&str> = actual_path.split('/').filter(|s| !s.is_empty()).collect();

    let mut params = HashMap::new();

    // Only proceed if we have the same number of segments
    if registered_segments.len() != actual_segments.len() {
        return params;
    }

    // Match segments and extract parameters (segments starting with :)
    for (i, registered_seg) in registered_segments.iter().enumerate() {
        if registered_seg.starts_with(':') {
            // Extract parameter name (remove the ':')
            let param_name = registered_seg
                .strip_prefix(':')
                .unwrap_or(registered_seg)
                .to_string();

            // Extract the corresponding value from actual path
            if let Some(actual_value) = actual_segments.get(i) {
                params.insert(param_name, actual_value.to_string());
            }
        }
    }

    params
}

#[axum::debug_handler]
async fn dynamic_handler(
    method: axum::http::Method,
    uri: Uri,
    headers: HeaderMap,
    Extension(engine): Extension<Arc<Engine>>,
    Extension(api_handler): Extension<Arc<RestApiCoreModule>>,
    Extension(registered_path): Extension<String>,
    Query(query_params): Query<HashMap<String, String>>,
    body: Option<Json<Value>>,
) -> impl IntoResponse {
    let actual_path = uri.path();

    tracing::debug!("Registered route path: {}", registered_path);
    tracing::debug!("Actual path: {}", actual_path);
    tracing::debug!("HTTP Method: {}", method);
    tracing::debug!("Query parameters: {:?}", query_params);
    tracing::debug!("Headers: {:?}", headers);
    let path_parameters: HashMap<String, String> =
        extract_path_params(&registered_path, actual_path);

    if let Some(function_path) = api_handler
        .get_router(method.as_str(), &registered_path)
        .await
    {
        let function = engine.functions.get(function_path.as_str());
        if function.is_none() {
            return (StatusCode::NOT_FOUND, "Function Not Found").into_response();
        }
        let function_handler = function.expect("function existence checked");
        let api_request = APIrequest::new(
            query_params.clone(),
            path_parameters.clone(),
            headers,
            registered_path.clone(),
            method.as_str().to_string(),
            body,
        );

        let api_request_value = serde_json::to_value(api_request).unwrap_or(serde_json::json!({}));

        let func_result = engine
            .non_worker_invocations
            .handle_invocation(api_request_value, function_handler)
            .await;

        return match func_result {
            Ok(Ok(result)) => {
                let result = result.unwrap_or(json!({}));
                let status_code = result
                    .get("status_code")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(200) as u16;
                let api_response = APIresponse::from_function_return(result);
                (
                    StatusCode::from_u16(status_code).unwrap_or(StatusCode::OK),
                    Json(api_response.body),
                )
                    .into_response()
            }
            Ok(Err(err)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": err})),
            )
                .into_response(),
            Err(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Invocation timed out or channel closed"})),
            )
                .into_response(),
        };
    }
    (StatusCode::NOT_FOUND, "Not Found").into_response()
}
