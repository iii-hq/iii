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
        tracing::info!(
            "{} Function {}",
            "[UNREGISTERED]".red(),
            function_id.purple()
        );
    }

    pub fn get(&self, function_id: &str) -> Option<Function> {
        tracing::debug!("Searching for function: {}", function_id);
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
    use serde_json::json;
    use std::sync::Arc;

    /// Helper: create a dummy function with a simple handler
    fn make_function(id: &str) -> Function {
        Function {
            handler: Arc::new(|_invocation_id, _input| {
                Box::pin(async { FunctionResult::Success(Some(serde_json::json!({"ok": true}))) })
            }),
            _function_id: id.to_string(),
            _description: Some("test function".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        }
    }

    // =========================================================================
    // FunctionResult variants
    // =========================================================================

    #[test]
    fn function_result_success_variant() {
        let result: FunctionResult<i32, String> = FunctionResult::Success(42);
        match result {
            FunctionResult::Success(v) => assert_eq!(v, 42),
            _ => panic!("expected Success"),
        }
    }

    #[test]
    fn function_result_failure_variant() {
        let result: FunctionResult<i32, String> = FunctionResult::Failure("err".to_string());
        match result {
            FunctionResult::Failure(e) => assert_eq!(e, "err"),
            _ => panic!("expected Failure"),
        }
    }

    #[test]
    fn function_result_deferred_variant() {
        let result: FunctionResult<i32, String> = FunctionResult::Deferred;
        match result {
            FunctionResult::Deferred => {}
            _ => panic!("expected Deferred"),
        }
    }

    #[test]
    fn function_result_no_result_variant() {
        let result: FunctionResult<i32, String> = FunctionResult::NoResult;
        match result {
            FunctionResult::NoResult => {}
            _ => panic!("expected NoResult"),
        }
    }

    // =========================================================================
    // FunctionsRegistry
    // =========================================================================

    #[test]
    fn registry_new_is_empty() {
        let reg = FunctionsRegistry::new();
        assert!(reg.get("anything").is_none());
    }

    #[test]
    fn registry_default_is_empty() {
        let reg = FunctionsRegistry::default();
        assert!(reg.get("anything").is_none());
    }

    #[test]
    fn registry_register_and_get() {
        let reg = FunctionsRegistry::new();
        let func = make_function("my_fn");
        reg.register_function("my_fn".to_string(), func);
        let retrieved = reg.get("my_fn");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap()._function_id, "my_fn");
    }

    #[test]
    fn registry_get_nonexistent_returns_none() {
        let reg = FunctionsRegistry::new();
        assert!(reg.get("does_not_exist").is_none());
    }

    #[test]
    fn registry_remove_function() {
        let reg = FunctionsRegistry::new();
        reg.register_function("fn_to_remove".to_string(), make_function("fn_to_remove"));
        assert!(reg.get("fn_to_remove").is_some());
        reg.remove("fn_to_remove");
        assert!(reg.get("fn_to_remove").is_none());
    }

    #[test]
    fn registry_remove_nonexistent_does_not_panic() {
        let reg = FunctionsRegistry::new();
        reg.remove("no_such_fn"); // should not panic
    }

    #[test]
    fn registry_functions_hash_deterministic() {
        let reg = FunctionsRegistry::new();
        reg.register_function("b".to_string(), make_function("b"));
        reg.register_function("a".to_string(), make_function("a"));

        let hash1 = reg.functions_hash();
        let hash2 = reg.functions_hash();
        assert_eq!(hash1, hash2, "hash should be deterministic");
    }

    #[test]
    fn registry_functions_hash_changes_on_add() {
        let reg = FunctionsRegistry::new();
        let hash_empty = reg.functions_hash();
        reg.register_function("fn1".to_string(), make_function("fn1"));
        let hash_one = reg.functions_hash();
        assert_ne!(hash_empty, hash_one);
    }

    #[test]
    fn registry_functions_hash_sorted() {
        let reg = FunctionsRegistry::new();
        reg.register_function("z_func".to_string(), make_function("z_func"));
        reg.register_function("a_func".to_string(), make_function("a_func"));
        let hash = reg.functions_hash();
        // The hash is a debug representation of a sorted vec
        assert!(hash.contains("a_func"));
        assert!(hash.contains("z_func"));
        // 'a_func' should appear before 'z_func' since it's sorted
        let a_pos = hash.find("a_func").unwrap();
        let z_pos = hash.find("z_func").unwrap();
        assert!(a_pos < z_pos, "functions should be sorted alphabetically");
    }

    #[test]
    fn registry_iter() {
        let reg = FunctionsRegistry::new();
        reg.register_function("fn1".to_string(), make_function("fn1"));
        reg.register_function("fn2".to_string(), make_function("fn2"));

        let mut keys: Vec<String> = reg.iter().map(|entry| entry.key().clone()).collect();
        keys.sort();
        assert_eq!(keys, vec!["fn1", "fn2"]);
    }

    #[test]
    fn registry_overwrite_existing_function() {
        let reg = FunctionsRegistry::new();
        let func1 = Function {
            handler: Arc::new(|_, _| Box::pin(async { FunctionResult::Success(None) })),
            _function_id: "fn".to_string(),
            _description: Some("version 1".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        };
        let func2 = Function {
            handler: Arc::new(|_, _| Box::pin(async { FunctionResult::Success(None) })),
            _function_id: "fn".to_string(),
            _description: Some("version 2".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        };
        reg.register_function("fn".to_string(), func1);
        reg.register_function("fn".to_string(), func2);
        let retrieved = reg.get("fn").unwrap();
        assert_eq!(retrieved._description, Some("version 2".to_string()));
    }

    #[test]
    fn function_metadata_and_formats() {
        let func = Function {
            handler: Arc::new(|_, _| Box::pin(async { FunctionResult::Success(None) })),
            _function_id: "fn".to_string(),
            _description: None,
            request_format: Some(json!({"type": "object"})),
            response_format: Some(json!({"type": "string"})),
            metadata: Some(json!({"version": 1})),
        };
        assert!(func._description.is_none());
        assert_eq!(func.request_format, Some(json!({"type": "object"})));
        assert_eq!(func.response_format, Some(json!({"type": "string"})));
        assert_eq!(func.metadata, Some(json!({"version": 1})));
    }

    // =========================================================================
    // Function::call_handler
    // =========================================================================

    #[tokio::test]
    async fn call_handler_returns_success() {
        let func = make_function("test");
        let result = func.call_handler(None, json!({})).await;
        match result {
            FunctionResult::Success(Some(val)) => {
                assert_eq!(val, json!({"ok": true}));
            }
            _ => panic!("expected Success with value"),
        }
    }

    #[tokio::test]
    async fn call_handler_with_invocation_id() {
        let invocation_id = Uuid::new_v4();
        let func = Function {
            handler: Arc::new(move |inv_id, _input| {
                let inv_id = inv_id.clone();
                Box::pin(async move {
                    if inv_id.is_some() {
                        FunctionResult::Success(Some(json!({"has_id": true})))
                    } else {
                        FunctionResult::Success(Some(json!({"has_id": false})))
                    }
                })
            }),
            _function_id: "test_with_id".to_string(),
            _description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        };
        let result = func.call_handler(Some(invocation_id), json!({})).await;
        match result {
            FunctionResult::Success(Some(val)) => {
                assert_eq!(val["has_id"], true);
            }
            _ => panic!("expected Success"),
        }
    }

    #[tokio::test]
    async fn call_handler_failure() {
        let func = Function {
            handler: Arc::new(|_, _| {
                Box::pin(async {
                    FunctionResult::Failure(ErrorBody {
                        code: "test_error".to_string(),
                        message: "something failed".to_string(),
                    })
                })
            }),
            _function_id: "failing_fn".to_string(),
            _description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        };
        let result = func.call_handler(None, json!({})).await;
        match result {
            FunctionResult::Failure(e) => {
                assert_eq!(e.code, "test_error");
                assert_eq!(e.message, "something failed");
            }
            _ => panic!("expected Failure"),
        }
    }
}
