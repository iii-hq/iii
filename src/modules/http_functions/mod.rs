// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

pub mod config;

use std::{pin::Pin, sync::Arc};

use dashmap::DashMap;
use futures::Future;
use serde_json::Value;

use crate::{
    engine::{Engine, EngineTrait, RegisterFunctionRequest},
    function::FunctionResult,
    invocation::{
        auth::resolve_auth_ref,
        http_function::HttpFunctionConfig,
        http_invoker::{HttpEndpointParams, HttpInvoker, HttpInvokerConfig},
        method::HttpAuth,
    },
    modules::module::Module,
    protocol::ErrorBody,
};

use config::HttpFunctionsConfig;

type HandlerFuture = Pin<Box<dyn Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send>>;

#[derive(Clone)]
pub struct HttpFunctionsModule {
    engine: Arc<Engine>,
    http_invoker: Arc<HttpInvoker>,
    http_functions: Arc<DashMap<String, HttpFunctionConfig>>,
    #[allow(dead_code)]
    config: HttpFunctionsConfig,
}

impl HttpFunctionsModule {
    fn create_handler_wrapper(
        &self,
        config: HttpFunctionConfig,
        auth: Option<HttpAuth>,
    ) -> crate::engine::Handler<impl Fn(Value) -> HandlerFuture + Send + Sync + 'static> {
        let invoker = self.http_invoker.clone();
        let function_path = config.function_path.clone();
        let url = config.url.clone();
        let method = config.method.clone();
        let timeout_ms = config.timeout_ms;
        let headers = config.headers.clone();

        crate::engine::Handler::new(move |input: Value| -> HandlerFuture {
            let invoker = invoker.clone();
            let function_path = function_path.clone();
            let url = url.clone();
            let method = method.clone();
            let headers = headers.clone();
            let auth = auth.clone();

            Box::pin(async move {
                let endpoint = HttpEndpointParams {
                    url: &url,
                    method: &method,
                    timeout_ms: &timeout_ms,
                    headers: &headers,
                    auth: &auth,
                };

                match invoker
                    .invoke_http(
                        &function_path,
                        &endpoint,
                        uuid::Uuid::new_v4(),
                        input,
                        None,
                        None,
                    )
                    .await
                {
                    Ok(result) => FunctionResult::Success(result),
                    Err(e) => FunctionResult::Failure(e),
                }
            })
        })
    }

    pub async fn register_http_function(
        &self,
        config: HttpFunctionConfig,
    ) -> Result<(), ErrorBody> {
        self.http_invoker
            .url_validator()
            .validate(&config.url)
            .await
            .map_err(|e| ErrorBody {
                code: "url_validation_failed".into(),
                message: e.to_string(),
            })?;

        let auth = config.auth.as_ref().map(resolve_auth_ref).transpose()?;

        let handler = self.create_handler_wrapper(config.clone(), auth);

        self.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: config.function_path.clone(),
                description: config.description.clone(),
                request_format: config.request_format.clone(),
                response_format: config.response_format.clone(),
                metadata: config.metadata.clone(),
            },
            handler,
        );

        self.http_functions
            .insert(config.function_path.clone(), config);

        Ok(())
    }

    pub async fn unregister_http_function(&self, function_path: &str) -> Result<(), ErrorBody> {
        self.http_functions.remove(function_path);
        self.engine.functions.remove(function_path);
        Ok(())
    }

    pub fn http_invoker(&self) -> &Arc<HttpInvoker> {
        &self.http_invoker
    }

    pub fn http_functions(&self) -> &Arc<DashMap<String, HttpFunctionConfig>> {
        &self.http_functions
    }
}

#[async_trait::async_trait]
impl Module for HttpFunctionsModule {
    fn name(&self) -> &'static str {
        "HttpFunctionsModule"
    }

    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
        let config: HttpFunctionsConfig = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        let http_invoker = Arc::new(HttpInvoker::new(HttpInvokerConfig {
            url_validator: config.security.clone().into(),
            ..HttpInvokerConfig::default()
        })?);

        let http_functions = Arc::new(DashMap::new());

        Ok(Box::new(Self {
            engine,
            http_invoker,
            http_functions,
            config,
        }))
    }

    fn register_functions(&self, _engine: Arc<Engine>) {}

    async fn initialize(&self) -> anyhow::Result<()> {
        self.engine
            .service_registry
            .register_service("http_functions", Arc::new(self.clone()));

        Ok(())
    }
}

crate::register_module!(
    "modules::http_functions::HttpFunctionsModule",
    HttpFunctionsModule
);

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use axum::{
        Json, Router,
        body::Bytes,
        extract::State,
        http::{HeaderMap, Method, StatusCode},
        routing::{get, post},
    };
    use serde_json::json;
    use serial_test::serial;
    use tokio::{net::TcpListener, task::JoinHandle};

    use super::*;
    use crate::{
        config::SecurityConfig,
        engine::EngineTrait,
        invocation::{auth::HttpAuthConfig, method::HttpMethod},
        modules::observability::metrics::ensure_default_meter,
    };

    #[derive(Clone, Default)]
    struct RequestCapture {
        headers: Arc<Mutex<HashMap<String, String>>>,
        method: Arc<Mutex<Option<String>>>,
        body: Arc<Mutex<Option<Value>>>,
    }

    async fn echo_handler(
        State(capture): State<RequestCapture>,
        method: Method,
        headers: HeaderMap,
        body: Bytes,
    ) -> (StatusCode, Json<Value>) {
        let mut header_map = HashMap::new();
        for (name, value) in &headers {
            header_map.insert(
                name.as_str().to_string(),
                value.to_str().unwrap_or_default().to_string(),
            );
        }

        *capture.method.lock().expect("lock request method") = Some(method.to_string());
        *capture.headers.lock().expect("lock request headers") = header_map.clone();
        *capture.body.lock().expect("lock request body") = Some(if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).expect("request body should be valid json")
        });

        (
            StatusCode::OK,
            Json(json!({
                "method": method.to_string(),
                "headers": header_map,
                "body": capture.body.lock().expect("lock request body").clone().unwrap_or(Value::Null),
            })),
        )
    }

    async fn empty_handler() -> StatusCode {
        StatusCode::NO_CONTENT
    }

    async fn invalid_handler() -> &'static str {
        "not-json"
    }

    async fn error_handler() -> (StatusCode, Json<Value>) {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": {
                    "code": "UPSTREAM_ERROR",
                    "message": "upstream failed"
                }
            })),
        )
    }

    async fn spawn_http_server() -> (String, RequestCapture, JoinHandle<()>) {
        let capture = RequestCapture::default();
        let app = Router::new()
            .route("/echo", get(echo_handler).post(echo_handler))
            .route("/empty", post(empty_handler))
            .route("/invalid", post(invalid_handler))
            .route("/error", post(error_handler))
            .with_state(capture.clone());

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind local http test server");
        let addr = listener.local_addr().expect("local addr");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve local http app");
        });

        (format!("http://{addr}"), capture, handle)
    }

    fn permissive_config() -> HttpFunctionsConfig {
        HttpFunctionsConfig {
            security: SecurityConfig {
                require_https: false,
                block_private_ips: false,
                url_allowlist: vec!["*".to_string()],
            },
        }
    }

    fn build_module(engine: Arc<crate::engine::Engine>) -> HttpFunctionsModule {
        let config = permissive_config();
        HttpFunctionsModule {
            engine,
            http_invoker: Arc::new(
                HttpInvoker::new(HttpInvokerConfig {
                    url_validator: config.security.clone().into(),
                    ..HttpInvokerConfig::default()
                })
                .expect("create http invoker"),
            ),
            http_functions: Arc::new(DashMap::new()),
            config,
        }
    }

    fn make_function_config(function_path: &str, url: String) -> HttpFunctionConfig {
        HttpFunctionConfig {
            function_path: function_path.to_string(),
            url,
            method: HttpMethod::Post,
            timeout_ms: Some(5_000),
            headers: HashMap::new(),
            auth: None,
            description: Some(format!("test function {function_path}")),
            request_format: None,
            response_format: None,
            metadata: None,
            registered_at: None,
            updated_at: None,
        }
    }

    #[tokio::test]
    async fn create_and_initialize_registers_http_functions_service() {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());
        let config = serde_json::to_value(permissive_config()).expect("serialize config");

        let module = HttpFunctionsModule::create(engine.clone(), Some(config))
            .await
            .expect("create http functions module");
        assert_eq!(module.name(), "HttpFunctionsModule");

        module.initialize().await.expect("initialize module");

        assert!(
            engine
                .service_registry
                .get_service::<HttpFunctionsModule>("http_functions")
                .is_some()
        );

        let module = engine
            .service_registry
            .get_service::<HttpFunctionsModule>("http_functions")
            .expect("http functions service");
        assert!(Arc::strong_count(module.http_invoker()) >= 1);
        assert!(module.http_functions().is_empty());
        module.register_functions(engine.clone());
    }

    #[tokio::test]
    async fn register_http_function_invokes_local_server_and_unregisters() {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());
        let module = build_module(engine.clone());
        let (base_url, capture, server) = spawn_http_server().await;

        let mut config = make_function_config("remote.echo", format!("{base_url}/echo"));
        config
            .headers
            .insert("x-custom".to_string(), "present".to_string());

        module
            .register_http_function(config.clone())
            .await
            .expect("register http function");
        assert!(module.http_functions.contains_key("remote.echo"));
        assert!(engine.functions.get("remote.echo").is_some());

        let result = engine
            .call("remote.echo", json!({ "hello": "world" }))
            .await
            .expect("invoke remote http function")
            .expect("http function should return a body");

        assert_eq!(result["body"], json!({ "hello": "world" }));
        assert_eq!(result["method"], "POST");

        let headers = capture.headers.lock().expect("lock captured headers");
        assert_eq!(headers.get("x-custom").map(String::as_str), Some("present"));
        assert_eq!(
            headers.get("x-iii-function-path").map(String::as_str),
            Some("remote.echo")
        );
        drop(headers);

        module
            .unregister_http_function("remote.echo")
            .await
            .expect("unregister http function");
        assert!(!module.http_functions.contains_key("remote.echo"));
        assert!(engine.functions.get("remote.echo").is_none());

        server.abort();
    }

    #[tokio::test]
    async fn invoke_http_handler_returns_invalid_response_and_upstream_errors() {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());
        let module = build_module(engine.clone());
        let (base_url, _capture, server) = spawn_http_server().await;

        module
            .register_http_function(make_function_config(
                "remote.invalid",
                format!("{base_url}/invalid"),
            ))
            .await
            .expect("register invalid response function");
        let invalid_result = engine.call("remote.invalid", json!({ "test": true })).await;
        match invalid_result {
            Err(err) => assert_eq!(err.code, "invalid_response"),
            _ => panic!("expected invalid_response error"),
        }

        module
            .register_http_function(make_function_config(
                "remote.error",
                format!("{base_url}/error"),
            ))
            .await
            .expect("register upstream error function");
        let error_result = engine.call("remote.error", json!({ "test": true })).await;
        match error_result {
            Err(err) => {
                assert_eq!(err.code, "UPSTREAM_ERROR");
                assert_eq!(err.message, "upstream failed");
            }
            _ => panic!("expected upstream error"),
        }

        module
            .register_http_function(make_function_config(
                "remote.empty",
                format!("{base_url}/empty"),
            ))
            .await
            .expect("register empty response function");
        let empty_result = engine
            .call("remote.empty", json!({ "test": true }))
            .await
            .expect("invoke empty response function");
        assert!(empty_result.is_none());

        server.abort();
    }

    #[tokio::test]
    async fn register_http_function_rejects_invalid_url() {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());
        let module = build_module(engine);

        let result = module
            .register_http_function(make_function_config("remote.bad", "://bad-url".to_string()))
            .await;

        match result {
            Err(err) => assert_eq!(err.code, "url_validation_failed"),
            _ => panic!("expected url validation failure"),
        }
        assert!(!module.http_functions.contains_key("remote.bad"));
    }

    #[tokio::test]
    #[serial]
    async fn register_http_function_resolves_bearer_auth_and_sends_header() {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());
        let module = build_module(engine.clone());
        let (base_url, capture, server) = spawn_http_server().await;

        // SAFETY: serialized test, no concurrent env mutation.
        unsafe { std::env::set_var("TEST_HTTP_TOKEN", "secret-token") };

        let mut config = make_function_config("remote.auth", format!("{base_url}/echo"));
        config.auth = Some(HttpAuthConfig::Bearer {
            token_key: "TEST_HTTP_TOKEN".to_string(),
        });

        module
            .register_http_function(config)
            .await
            .expect("register auth protected function");

        let result = engine
            .call("remote.auth", json!({ "secure": true }))
            .await
            .expect("invoke auth protected function");
        assert!(result.is_some());

        let headers = capture.headers.lock().expect("lock captured headers");
        assert_eq!(
            headers.get("authorization").map(String::as_str),
            Some("Bearer secret-token")
        );
        drop(headers);

        // SAFETY: serialized test, no concurrent env mutation.
        unsafe { std::env::remove_var("TEST_HTTP_TOKEN") };
        server.abort();
    }
}
