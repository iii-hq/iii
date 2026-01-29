// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashSet, pin::Pin, sync::Arc};

use chrono::{DateTime, Utc};
use colored::Colorize;
use dashmap::DashMap;
use futures::Future;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::invocation::method::InvocationMethod;
use crate::protocol::*;

pub enum FunctionResult<T, E> {
    Success(T),
    Failure(E),
    Deferred,
    NoResult,
}
type HandlerFuture = Pin<Box<dyn Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send>>;
pub type HandlerFn = dyn Fn(Option<Uuid>, Value) -> HandlerFuture + Send + Sync;

#[derive(Clone)]
pub struct Function {
    pub function_path: String,
    pub description: Option<String>,
    pub request_format: Option<Value>,
    pub response_format: Option<Value>,
    pub metadata: Option<Value>,
    pub invocation_method: InvocationMethod,
    pub registered_at: DateTime<Utc>,
    pub registration_source: RegistrationSource,
    pub handler: Option<Arc<HandlerFn>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RegistrationSource {
    Config,
    AdminApi,
    WebSocket { worker_id: Uuid },
}

impl Function {
    pub async fn call_handler(
        self,
        invocation_id: Option<Uuid>,
        data: Value,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let handler = match self.handler.as_ref() {
            Some(handler) => handler,
            None => {
                return FunctionResult::Failure(ErrorBody {
                    code: "handler_not_available".into(),
                    message: "Function handler not available".into(),
                });
            }
        };
        (handler)(invocation_id, data.clone()).await
    }
}

pub trait FunctionHandler {
    fn handle_function<'a>(
        &'a self,
        invocation_id: Option<Uuid>,
        function_path: String,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send + 'a>>;
}

#[derive(Default)]
pub struct FunctionsRegistry {
    pub functions: Arc<DashMap<String, Function>>,
}

impl FunctionsRegistry {
    pub fn new() -> Self {
        Self {
            functions: Arc::new(DashMap::new()),
        }
    }

    pub fn functions_hash(&self) -> String {
        let functions: HashSet<String> = self
            .functions
            .iter()
            .map(|entry| entry.key().clone())
            .collect();

        let mut function_hash = functions.iter().cloned().collect::<Vec<String>>();
        function_hash.sort();
        format!("{:?}", function_hash)
    }

    pub fn register_function(&self, function_path: String, function: Function) {
        tracing::info!(
            "{} Function {}",
            "[REGISTERED]".green(),
            function_path.purple()
        );
        self.functions.insert(function_path, function);
    }

    pub fn remove(&self, function_path: &str) {
        self.functions.remove(function_path);
        tracing::info!("{} Function {}", "[REMOVED]".red(), function_path.purple());
    }

    pub fn get(&self, function_path: &str) -> Option<Function> {
        tracing::debug!("Searching for function path: {}", function_path);
        self.functions
            .get(function_path)
            .map(|entry| entry.value().clone())
    }

    pub fn iter(&self) -> dashmap::iter::Iter<'_, String, Function> {
        self.functions.iter()
    }
}
