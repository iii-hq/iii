// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashSet, pin::Pin, sync::Arc};

use colored::Colorize;
use dashmap::DashMap;
use futures::Future;
use serde_json::Value;
use uuid::Uuid;

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
    pub handler: Arc<HandlerFn>,
    pub _function_id: String,
    pub _description: Option<String>,
    pub request_format: Option<Value>,
    pub response_format: Option<Value>,
    pub metadata: Option<Value>,
}

impl Function {
    pub async fn call_handler(
        self,
        invocation_id: Option<Uuid>,
        data: Value,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        (self.handler)(invocation_id, data.clone()).await
    }
}

pub trait FunctionHandler {
    fn handle_function<'a>(
        &'a self,
        invocation_id: Option<Uuid>,
        function_id: String,
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

    pub fn register_function(&self, function_id: String, function: Function) {
        tracing::info!(
            "{} Function {}",
            "[REGISTERED]".green(),
            function_id.purple()
        );
        self.functions.insert(function_id, function);
    }

    pub fn remove(&self, function_id: &str) {
        self.functions.remove(function_id);
        tracing::info!("{} Function {}", "[REMOVED]".red(), function_id.purple());
    }

    pub fn get(&self, function_id: &str) -> Option<Function> {
        tracing::debug!("Searching for function path: {}", function_id);
        self.functions
            .get(function_id)
            .map(|entry| entry.value().clone())
    }

    pub fn iter(&self) -> dashmap::iter::Iter<'_, String, Function> {
        self.functions.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;
    use serde_json::json;

    #[test]
    fn test_functions_registry_new() {
        let registry = FunctionsRegistry::new();
        assert_eq!(registry.functions.len(), 0);
    }

    #[test]
    fn test_register_function() {
        let registry = FunctionsRegistry::new();
        let handler: Arc<HandlerFn> = Arc::new(|_invocation_id, _data| {
            async move { FunctionResult::Success(Some(json!({"result": "ok"}))) }
                .boxed()
        });
        let function = Function {
            handler,
            _function_id: "test.function".to_string(),
            _description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        };

        registry.register_function("test.function".to_string(), function);
        assert_eq!(registry.functions.len(), 1);
        assert!(registry.functions.contains_key("test.function"));
    }

    #[test]
    fn test_get_function() {
        let registry = FunctionsRegistry::new();
        let handler: Arc<HandlerFn> = Arc::new(|_invocation_id, _data| {
            async move { FunctionResult::Success(Some(json!({"result": "ok"}))) }
                .boxed()
        });
        let function = Function {
            handler: handler.clone(),
            _function_id: "test.function".to_string(),
            _description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        };

        registry.register_function("test.function".to_string(), function);
        let retrieved = registry.get("test.function");
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_get_function_nonexistent() {
        let registry = FunctionsRegistry::new();
        let retrieved = registry.get("nonexistent.function");
        assert!(retrieved.is_none());
    }

    #[test]
    fn test_remove_function() {
        let registry = FunctionsRegistry::new();
        let handler: Arc<HandlerFn> = Arc::new(|_invocation_id, _data| {
            async move { FunctionResult::Success(Some(json!({"result": "ok"}))) }
                .boxed()
        });
        let function = Function {
            handler,
            _function_id: "test.function".to_string(),
            _description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        };

        registry.register_function("test.function".to_string(), function);
        assert_eq!(registry.functions.len(), 1);

        registry.remove("test.function");
        assert_eq!(registry.functions.len(), 0);
        assert!(registry.get("test.function").is_none());
    }

    #[test]
    fn test_remove_function_nonexistent() {
        let registry = FunctionsRegistry::new();
        registry.remove("nonexistent.function");
        assert_eq!(registry.functions.len(), 0);
    }

    #[test]
    fn test_functions_hash_empty() {
        let registry = FunctionsRegistry::new();
        let hash = registry.functions_hash();
        assert_eq!(hash, "[]");
    }

    #[test]
    fn test_functions_hash_single() {
        let registry = FunctionsRegistry::new();
        let handler: Arc<HandlerFn> = Arc::new(|_invocation_id, _data| {
            async move { FunctionResult::Success(Some(json!({"result": "ok"}))) }
                .boxed()
        });
        let function = Function {
            handler,
            _function_id: "test.function".to_string(),
            _description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        };

        registry.register_function("test.function".to_string(), function);
        let hash = registry.functions_hash();
        assert!(hash.contains("test.function"));
    }

    #[test]
    fn test_functions_hash_stable_and_sorted() {
        let registry = FunctionsRegistry::new();
        let handler1: Arc<HandlerFn> = Arc::new(|_invocation_id, _data| {
            async move { FunctionResult::Success(Some(json!({"result": "ok"}))) }
                .boxed()
        });
        let handler2: Arc<HandlerFn> = Arc::new(|_invocation_id, _data| {
            async move { FunctionResult::Success(Some(json!({"result": "ok"}))) }
                .boxed()
        });
        let function1 = Function {
            handler: handler1,
            _function_id: "z.function".to_string(),
            _description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        };
        let function2 = Function {
            handler: handler2,
            _function_id: "a.function".to_string(),
            _description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        };

        registry.register_function("z.function".to_string(), function1);
        registry.register_function("a.function".to_string(), function2);

        let hash1 = registry.functions_hash();
        let hash2 = registry.functions_hash();
        assert_eq!(hash1, hash2);
        assert!(hash1.contains("a.function"));
        assert!(hash1.contains("z.function"));
        let a_pos = hash1.find("a.function").unwrap();
        let z_pos = hash1.find("z.function").unwrap();
        assert!(a_pos < z_pos);
    }

    #[test]
    fn test_functions_hash_changes_on_add() {
        let registry = FunctionsRegistry::new();
        let hash1 = registry.functions_hash();

        let handler: Arc<HandlerFn> = Arc::new(|_invocation_id, _data| {
            async move { FunctionResult::Success(Some(json!({"result": "ok"}))) }
                .boxed()
        });
        let function = Function {
            handler,
            _function_id: "test.function".to_string(),
            _description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        };

        registry.register_function("test.function".to_string(), function);
        let hash2 = registry.functions_hash();
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_functions_hash_changes_on_remove() {
        let registry = FunctionsRegistry::new();
        let handler: Arc<HandlerFn> = Arc::new(|_invocation_id, _data| {
            async move { FunctionResult::Success(Some(json!({"result": "ok"}))) }
                .boxed()
        });
        let function = Function {
            handler,
            _function_id: "test.function".to_string(),
            _description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        };

        registry.register_function("test.function".to_string(), function);
        let hash1 = registry.functions_hash();

        registry.remove("test.function");
        let hash2 = registry.functions_hash();
        assert_ne!(hash1, hash2);
        assert_eq!(hash2, "[]");
    }

    #[tokio::test]
    async fn test_function_call_handler_success() {
        let handler: Arc<HandlerFn> = Arc::new(|_invocation_id, _data| {
            async move { FunctionResult::Success(Some(json!({"result": "ok"}))) }
                .boxed()
        });
        let function = Function {
            handler,
            _function_id: "test.function".to_string(),
            _description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        };

        let result = function.call_handler(None, json!({"input": "data"})).await;
        match result {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value["result"], "ok");
            }
            _ => panic!("Expected Success"),
        }
    }

    #[tokio::test]
    async fn test_function_call_handler_failure() {
        let handler: Arc<HandlerFn> = Arc::new(|_invocation_id, _data| {
            async move {
                FunctionResult::Failure(ErrorBody {
                    code: "TEST_ERROR".to_string(),
                    message: "Test error".to_string(),
                })
            }
            .boxed()
        });
        let function = Function {
            handler,
            _function_id: "test.function".to_string(),
            _description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        };

        let result = function.call_handler(None, json!({"input": "data"})).await;
        match result {
            FunctionResult::Failure(error) => {
                assert_eq!(error.code, "TEST_ERROR");
                assert_eq!(error.message, "Test error");
            }
            _ => panic!("Expected Failure"),
        }
    }

    #[tokio::test]
    async fn test_function_call_handler_deferred() {
        let handler: Arc<HandlerFn> = Arc::new(|_invocation_id, _data| {
            async move { FunctionResult::Deferred }.boxed()
        });
        let function = Function {
            handler,
            _function_id: "test.function".to_string(),
            _description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        };

        let result = function.call_handler(None, json!({"input": "data"})).await;
        match result {
            FunctionResult::Deferred => {}
            _ => panic!("Expected Deferred"),
        }
    }

    #[tokio::test]
    async fn test_function_call_handler_no_result() {
        let handler: Arc<HandlerFn> = Arc::new(|_invocation_id, _data| {
            async move { FunctionResult::NoResult }.boxed()
        });
        let function = Function {
            handler,
            _function_id: "test.function".to_string(),
            _description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        };

        let result = function.call_handler(None, json!({"input": "data"})).await;
        match result {
            FunctionResult::NoResult => {}
            _ => panic!("Expected NoResult"),
        }
    }

    #[tokio::test]
    async fn test_function_call_handler_with_invocation_id() {
        let invocation_id = Uuid::new_v4();
        let handler: Arc<HandlerFn> = Arc::new(move |id, _data| {
            let expected_id = invocation_id;
            async move {
                assert_eq!(id, Some(expected_id));
                FunctionResult::Success(Some(json!({"result": "ok"})))
            }
            .boxed()
        });
        let function = Function {
            handler,
            _function_id: "test.function".to_string(),
            _description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        };

        let result = function
            .call_handler(Some(invocation_id), json!({"input": "data"}))
            .await;
        match result {
            FunctionResult::Success(_) => {}
            _ => panic!("Expected Success"),
        }
    }
}
