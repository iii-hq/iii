// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde_json::Value;

use crate::{engine::EngineTrait, protocol::ErrorBody};

/// Evaluates a condition function against the provided data.
///
/// Returns:
/// - `Ok(true)` — proceed with the handler (condition passed or returned no value)
/// - `Ok(false)` — skip the handler (condition explicitly returned `false`)
/// - `Err(ErrorBody)` — condition function invocation failed
pub async fn check_condition<E: EngineTrait>(
    engine: &E,
    condition_function_id: &str,
    data: Value,
) -> Result<bool, ErrorBody> {
    match engine.call(condition_function_id, data).await {
        Ok(Some(result)) => Ok(result.as_bool() != Some(false)),
        Ok(None) => {
            tracing::warn!(
                condition_function_id = %condition_function_id,
                "Condition function returned no result"
            );
            Ok(true)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::sync::Mutex;

    struct MockEngine {
        result: Mutex<Result<Option<Value>, ErrorBody>>,
    }

    impl MockEngine {
        fn returning_ok(val: Option<Value>) -> Self {
            Self {
                result: Mutex::new(Ok(val)),
            }
        }
        fn returning_err(err: ErrorBody) -> Self {
            Self {
                result: Mutex::new(Err(err)),
            }
        }
    }

    impl crate::engine::EngineTrait for MockEngine {
        async fn call(
            &self,
            _function_id: &str,
            _input: impl serde::Serialize + Send,
        ) -> Result<Option<Value>, ErrorBody> {
            self.result.lock().unwrap().clone()
        }
        async fn register_trigger_type(&self, _tt: crate::trigger::TriggerType) {}
        fn register_function(
            &self,
            _req: crate::engine::RegisterFunctionRequest,
            _handler: Box<dyn crate::function::FunctionHandler + Send + Sync>,
        ) {
        }
        fn register_function_handler<H, F>(
            &self,
            _req: crate::engine::RegisterFunctionRequest,
            _handler: crate::engine::Handler<H>,
        ) where
            H: crate::engine::HandlerFn<F>,
            F: std::future::Future<Output = crate::engine::HandlerOutput> + Send + 'static,
        {
        }
        fn register_function_handler_with_session<H, F>(
            &self,
            _req: crate::engine::RegisterFunctionRequest,
            _handler: crate::engine::SessionHandler<H>,
        ) where
            H: crate::engine::SessionHandlerFn<F>,
            F: std::future::Future<Output = crate::engine::HandlerOutput> + Send + 'static,
        {
        }
    }

    #[tokio::test]
    async fn check_condition_returns_true_when_result_is_true() {
        let engine = MockEngine::returning_ok(Some(json!(true)));
        let result = check_condition(&engine, "cond", json!({})).await.unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn check_condition_returns_false_when_result_is_false() {
        let engine = MockEngine::returning_ok(Some(json!(false)));
        let result = check_condition(&engine, "cond", json!({})).await.unwrap();
        assert!(!result);
    }

    #[tokio::test]
    async fn check_condition_returns_true_for_none() {
        let engine = MockEngine::returning_ok(None);
        let result = check_condition(&engine, "cond", json!({})).await.unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn check_condition_propagates_error() {
        let engine = MockEngine::returning_err(ErrorBody {
            code: "fail".into(),
            message: "boom".into(),
            stacktrace: None,
        });
        let result = check_condition(&engine, "cond", json!({})).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, "fail");
    }

    #[tokio::test]
    async fn check_condition_returns_true_for_non_bool_value() {
        let engine = MockEngine::returning_ok(Some(json!("hello")));
        let result = check_condition(&engine, "cond", json!({})).await.unwrap();
        assert!(result);
    }
}
