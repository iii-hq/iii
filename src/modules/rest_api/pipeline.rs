// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use serde_json::{Map, Value};

use crate::engine::{Engine, EngineTrait};
use crate::modules::rest_api::types::{HttpResponse, MatchedRoute};
use crate::modules::rest_api::{MiddlewarePhase, MiddlewareRegistry};
use crate::protocol::ErrorBody;

fn merge_json(base: &mut Value, overlay: &Value) {
    if let (Some(base_obj), Some(overlay_obj)) = (base.as_object_mut(), overlay.as_object()) {
        for (k, v) in overlay_obj {
            if let Some(existing) = base_obj.get_mut(k) {
                merge_json(existing, v);
            } else {
                base_obj.insert(k.clone(), v.clone());
            }
        }
    } else if !overlay.is_null() {
        *base = overlay.clone();
    }
}

#[derive(Debug)]
pub enum PhaseResult {
    Continue { request: Value, context: Value },
    Respond(HttpResponse),
}

pub struct MiddlewarePipeline {
    engine: Arc<Engine>,
    registry: Arc<MiddlewareRegistry>,
}

impl MiddlewarePipeline {
    pub fn new(engine: Arc<Engine>, registry: Arc<MiddlewareRegistry>) -> Self {
        Self { engine, registry }
    }

    pub async fn run_phase(
        &self,
        phase: MiddlewarePhase,
        request: &Value,
        context: &Value,
        matched_route: Option<&MatchedRoute>,
        request_path: &str,
    ) -> Result<PhaseResult, ErrorBody> {
        let entries = self.registry.get_matching(phase, request_path);
        if entries.is_empty() {
            return Ok(PhaseResult::Continue {
                request: request.clone(),
                context: context.clone(),
            });
        }

        let mut current_request = request.clone();
        let mut current_context = context.clone();

        for entry in entries {
            let middleware_request = serde_json::json!({
                "phase": phase.as_str(),
                "request": current_request,
                "context": current_context,
                "matched_route": matched_route.map(|r| serde_json::json!({
                    "function_id": r.function_id,
                    "path_pattern": r.path_pattern
                }))
            });

            let result = self
                .engine
                .call(&entry.function_id, middleware_request)
                .await
                .map_err(|e| ErrorBody {
                    code: e.code,
                    message: e.message,
                })?;

            let Some(result_value) = result else {
                continue;
            };

            let action = result_value
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("continue");

            if action == "respond"
                && let Some(resp) = result_value.get("response")
            {
                let status_code = resp
                    .get("status_code")
                    .and_then(|v| v.as_u64())
                    .filter(|c| (100..=599).contains(c))
                    .map(|c| c as u16)
                    .unwrap_or(200);
                let headers = resp.get("headers").map_or_else(Vec::new, |h| {
                    if let Some(arr) = h.as_array() {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    } else if let Some(obj) = h.as_object() {
                        obj.iter()
                            .filter_map(|(k, v)| v.as_str().map(|v| format!("{}: {}", k, v)))
                            .collect()
                    } else {
                        Vec::new()
                    }
                });
                let body = resp
                    .get("body")
                    .cloned()
                    .unwrap_or(Value::Object(Map::new()));
                return Ok(PhaseResult::Respond(HttpResponse {
                    status_code,
                    headers,
                    body,
                }));
            }

            if let Some(req) = result_value.get("request") {
                merge_json(&mut current_request, req);
            }
            if let Some(ctx) = result_value.get("context") {
                merge_json(&mut current_context, ctx);
            }
        }

        Ok(PhaseResult::Continue {
            request: current_request,
            context: current_context,
        })
    }

    pub fn run_on_response_fire_and_forget(
        &self,
        request: &Value,
        context: &Value,
        response: &Value,
        request_path: &str,
    ) {
        let entries = self
            .registry
            .get_matching(MiddlewarePhase::OnResponse, request_path);
        if entries.is_empty() {
            return;
        }
        let engine = self.engine.clone();
        let request = request.clone();
        let context = context.clone();
        let response = response.clone();
        tokio::spawn(async move {
            for entry in entries {
                let middleware_request = serde_json::json!({
                    "phase": "onResponse",
                    "request": request,
                    "context": context,
                    "response": response,
                    "matched_route": Value::Null
                });
                let _ = engine.call(&entry.function_id, middleware_request).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};

    use crate::engine::{Engine, EngineTrait, Handler};
    use crate::function::FunctionResult;
    use crate::modules::observability::metrics::ensure_default_meter;
    use crate::modules::rest_api::{MiddlewareEntry, MiddlewarePhase, MiddlewareRegistry};
    use crate::protocol::ErrorBody;

    type HandlerFuture = std::pin::Pin<
        Box<dyn std::future::Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send>,
    >;

    fn make_handler(
        f: impl Fn(Value) -> Option<Value> + Send + Sync + 'static,
    ) -> Handler<impl Fn(Value) -> HandlerFuture> {
        Handler::new(move |input: Value| {
            let result = f(input);
            Box::pin(async move { FunctionResult::Success(result) })
                as std::pin::Pin<
                    Box<
                        dyn std::future::Future<Output = FunctionResult<Option<Value>, ErrorBody>>
                            + Send,
                    >,
                >
        })
    }

    #[tokio::test]
    async fn run_phase_empty_registry_returns_continue() {
        let engine = Arc::new(Engine::new());
        let registry = Arc::new(MiddlewareRegistry::new());
        let pipeline = super::MiddlewarePipeline::new(engine, registry);

        let request = json!({"path": "/api/foo"});
        let context = json!({});

        let result = pipeline
            .run_phase(
                MiddlewarePhase::OnRequest,
                &request,
                &context,
                None,
                "/api/foo",
            )
            .await
            .unwrap();

        match result {
            super::PhaseResult::Continue {
                request: r,
                context: c,
            } => {
                assert_eq!(r, request);
                assert_eq!(c, context);
            }
            super::PhaseResult::Respond(_) => panic!("expected Continue"),
        }
    }

    #[tokio::test]
    async fn run_phase_continue_merges_context() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "mw::logger".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            make_handler(|_| {
                Some(json!({
                    "action": "continue",
                    "context": {"seen": true}
                }))
            }),
        );

        let registry = Arc::new(MiddlewareRegistry::new());
        registry.register(MiddlewareEntry {
            middleware_id: "m1".to_string(),
            phase: MiddlewarePhase::OnRequest,
            scope: None,
            priority: 100,
            function_id: "mw::logger".to_string(),
            worker_id: uuid::Uuid::new_v4(),
        });

        let pipeline = super::MiddlewarePipeline::new(engine, registry);
        let request = json!({"path": "/api/foo"});
        let context = json!({});

        let result = pipeline
            .run_phase(
                MiddlewarePhase::OnRequest,
                &request,
                &context,
                None,
                "/api/foo",
            )
            .await
            .unwrap();

        match result {
            super::PhaseResult::Continue { context: c, .. } => {
                assert_eq!(c.get("seen").and_then(|v| v.as_bool()), Some(true));
            }
            super::PhaseResult::Respond(_) => panic!("expected Continue"),
        }
    }

    #[tokio::test]
    async fn run_phase_respond_short_circuits() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "mw::auth".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            make_handler(|_| {
                Some(json!({
                    "action": "respond",
                    "response": {
                        "status_code": 403,
                        "headers": ["x-denied: true"],
                        "body": {"reason": "forbidden"}
                    }
                }))
            }),
        );

        let registry = Arc::new(MiddlewareRegistry::new());
        registry.register(MiddlewareEntry {
            middleware_id: "m1".to_string(),
            phase: MiddlewarePhase::OnRequest,
            scope: None,
            priority: 100,
            function_id: "mw::auth".to_string(),
            worker_id: uuid::Uuid::new_v4(),
        });

        let pipeline = super::MiddlewarePipeline::new(engine, registry);
        let request = json!({"path": "/api/foo"});
        let context = json!({});

        let result = pipeline
            .run_phase(
                MiddlewarePhase::OnRequest,
                &request,
                &context,
                None,
                "/api/foo",
            )
            .await
            .unwrap();

        match result {
            super::PhaseResult::Respond(resp) => {
                assert_eq!(resp.status_code, 403);
                assert_eq!(resp.headers, vec!["x-denied: true"]);
                assert_eq!(
                    resp.body.get("reason").and_then(|v| v.as_str()),
                    Some("forbidden")
                );
            }
            super::PhaseResult::Continue { .. } => panic!("expected Respond"),
        }
    }

    #[tokio::test]
    async fn run_phase_error_propagates() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "mw::failing".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input: Value| {
                Box::pin(async move {
                    FunctionResult::Failure(ErrorBody {
                        code: "auth_failed".into(),
                        message: "invalid token".into(),
                    })
                }) as HandlerFuture
            }),
        );

        let registry = Arc::new(MiddlewareRegistry::new());
        registry.register(MiddlewareEntry {
            middleware_id: "m1".to_string(),
            phase: MiddlewarePhase::OnRequest,
            scope: None,
            priority: 100,
            function_id: "mw::failing".to_string(),
            worker_id: uuid::Uuid::new_v4(),
        });

        let pipeline = super::MiddlewarePipeline::new(engine, registry);
        let request = json!({"path": "/api/foo"});
        let context = json!({});

        let err = pipeline
            .run_phase(
                MiddlewarePhase::OnRequest,
                &request,
                &context,
                None,
                "/api/foo",
            )
            .await
            .unwrap_err();

        assert_eq!(err.code, "auth_failed");
        assert_eq!(err.message, "invalid token");
    }
}
