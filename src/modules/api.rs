use std::{collections::HashMap, pin::Pin, sync::Arc};

use anyhow::anyhow;
use axum::{
    Json, Router,
    extract::{Extension, Path, Query, State},
    http::{StatusCode, header::HeaderMap},
    response::IntoResponse,
    routing::any,
};
use colored::Colorize;
use dashmap::DashMap;
use futures::Future;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::RwLock;

use crate::{
    engine::{Engine, EngineTrait},
    modules::logger::{LogLevel, log},
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
    pub headers: HashMap<String, String>,
    pub path: String,
    pub method: String,
    pub body: Value,
}

impl APIrequest {
    pub fn new(
        query_params: HashMap<String, String>,
        headers: HeaderMap,
        path: String,
        method: String,
        body: Option<Json<Value>>,
    ) -> Self {
        // sometimes body can be empty, like in GET requests
        let body_value = body.map(|Json(v)| v).unwrap_or(serde_json::json!({}));
        APIrequest {
            query_params,
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
pub struct ApiAdapter {
    pub routers_registry: Arc<RwLock<DashMap<String, PathRouter>>>,
}

impl ApiAdapter {
    pub fn new() -> Self {
        Self {
            routers_registry: Arc::new(RwLock::new(DashMap::new())),
        }
    }

    pub async fn initialize(&self, engine: &Arc<Engine>) {
        engine
            .register_trigger_type(TriggerType {
                id: "api".to_string(),
                _description: "HTTP API trigger".to_string(),
                registrator: Box::new(self.clone()),
                worker_id: None,
            })
            .await;
    }

    pub fn api_endpoints(self: Arc<Self>) -> Router<Arc<Engine>> {
        Router::new()
            .route("/dynamic/*path", any(dynamic_handler))
            .layer(Extension(self))
    }

    pub async fn get_router(&self, http_method: &str, http_path: &str) -> Option<String> {
        let method = http_method.to_uppercase();
        let key = format!("{}:{}", method, http_path);
        tracing::debug!("Looking up router for key: {}", key);
        let routers = self.routers_registry.read().await;
        let router = routers.get(&key);
        match router {
            Some(r) => Some(r.function_path.clone()),
            None => None,
        }
    }

    pub async fn register_router(&self, router: PathRouter) {
        let function_path = router.function_path.clone();
        let http_path = router.http_path.clone();
        let method = router.http_method.to_uppercase();
        let key = format!("{}:{}", method, router.http_path);
        tracing::debug!("Registering router: {}", key);
        self.routers_registry.write().await.insert(key, router);

        log(
            LogLevel::Info,
            "core::ApiCoreModule",
            &format!(
                "{} ENDPOINT {} → {}",
                "[REGISTERED]".green(),
                &format!("{} /{}", method, http_path).bright_yellow().bold(),
                function_path.purple()
            ),
            None,
            None,
        );
    }

    pub async fn unregister_router(&self, http_method: &str, http_path: &str) -> bool {
        let key = format!("{}:{}", http_method.to_uppercase(), http_path);
        tracing::debug!("Unregistering router: {}", key);
        self.routers_registry.write().await.remove(&key).is_some()
    }
}

impl TriggerRegistrator for ApiAdapter {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let adapter = self.clone();

        Box::pin(async move {
            let api_path = trigger
                .config
                .get("apiPath")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("apiPath is required for api triggers"))?;

            let http_method = trigger
                .config
                .get("httpMethod")
                .and_then(|v| v.as_str())
                .unwrap_or("GET");

            let router = PathRouter::new(
                api_path.to_string(),
                http_method.to_string(),
                trigger.function_path.clone(),
            );

            adapter.register_router(router).await;
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
                .get("apiPath")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            let http_method = trigger
                .config
                .get("httpMethod")
                .and_then(|v| v.as_str())
                .unwrap_or("GET");

            adapter.unregister_router(http_method, api_path).await;
            Ok(())
        })
    }
}

#[axum::debug_handler]
async fn dynamic_handler(
    method: axum::http::Method,
    headers: HeaderMap,
    State(engine): State<Arc<Engine>>,
    Extension(api_handler): Extension<Arc<ApiAdapter>>,
    Path(path): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    body: Option<Json<Value>>,
) -> impl IntoResponse {
    if let Some(function_path) = api_handler.get_router(method.as_str(), &path).await {
        let function = engine.functions.get(function_path.as_str());
        if function.is_none() {
            return (StatusCode::NOT_FOUND, "Function Not Found").into_response();
        }
        let function_handler = function.expect("function existence checked");
        let api_request = APIrequest::new(
            params.clone(),
            headers,
            path.clone(),
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
                    Json(api_response),
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
