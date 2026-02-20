// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde_json::Value;
use std::sync::Arc;

use crate::engine::Engine;
use crate::protocol::{MatchedRoute, MiddlewareAction, MiddlewarePhase, MiddlewareRequest};

pub struct MiddlewarePipeline {
    engine: Arc<Engine>,
}

impl MiddlewarePipeline {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self { engine }
    }

    /// Executes a single middleware phase.
    /// Returns `Ok(Some(response))` if a middleware short-circuits via `Respond`.
    /// Returns `Ok(None)` if all middlewares `Continue`.
    /// Returns `Err(())` if a middleware fails or a worker disconnects.
    pub async fn execute_phase(
        &self,
        phase: MiddlewarePhase,
        request: &mut Value,
        context: &mut Value,
        matched_route: Option<MatchedRoute>,
    ) -> Result<Option<Value>, ()> {
        let mut entries = self.engine.middleware_registry.get_for_phase(&phase);
        // Ensure they are sorted by priority (lowest first)
        entries.sort_by_key(|e| e.priority);

        for entry in entries {
            // Check scope if present
            if let Some(scope) = &entry.scope {
                let req_path = request.get("path").and_then(|v| v.as_str()).unwrap_or("");
                if !match_path_pattern(&scope.path, req_path) {
                    continue;
                }
            }

            let mw_req = MiddlewareRequest {
                phase: phase.clone(),
                request: request.clone(),
                context: context.clone(),
                matched_route: matched_route.clone(),
            };

            match self
                .engine
                .invoke_middleware(
                    &entry.worker_id,
                    &entry.middleware_id,
                    phase.clone(),
                    mw_req,
                )
                .await
            {
                Ok(result) => {
                    match result.action {
                        MiddlewareAction::Respond => {
                            return Ok(Some(result.response.unwrap_or_default()));
                        }
                        MiddlewareAction::Continue => {
                            // Merge request and context
                            if let Some(req_mod) = result.request {
                                merge_values(request, req_mod);
                            }
                            if let Some(ctx_mod) = result.context {
                                merge_values(context, ctx_mod);
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Middleware error for {}: {:?}", entry.middleware_id, e);
                    return Err(());
                }
            }
        }

        Ok(None)
    }
}

/// Simple path pattern matcher supporting exact match and trailing wildcard `*`.
fn match_path_pattern(pattern: &str, path: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        path.starts_with(prefix)
    } else {
        pattern == path
    }
}

/// Deep merges `source` into `target`.
fn merge_values(target: &mut Value, source: Value) {
    if let (Some(t_obj), Some(s_obj)) = (target.as_object_mut(), source.as_object()) {
        for (k, v) in s_obj {
            if v.is_object() && t_obj.get(k).map(|t| t.is_object()).unwrap_or(false) {
                merge_values(t_obj.get_mut(k).unwrap(), v.clone());
            } else {
                t_obj.insert(k.clone(), v.clone());
            }
        }
    } else {
        *target = source;
    }
}
