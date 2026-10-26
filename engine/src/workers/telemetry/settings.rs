// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Whether this engine's anonymous telemetry is on, for a worker to ask.
//!
//! The opt-out is decided by [`super::check_disabled`], which reads the
//! engine's config, environment and `~/.iii`. A worker sees none of those, so
//! it asks the engine instead of copying the checks.

use std::sync::Arc;

use serde_json::{Value, json};

use super::{TelemetryConfig, TelemetryContext, resolve_posthog_api_key, resolve_posthog_host};
use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
};

pub const SETTINGS_FN_ID: &str = "iii-telemetry::settings";

/// Answer with this engine's PostHog settings (telemetry on).
pub(super) fn register_handler(
    engine: &Arc<Engine>,
    ctx: &TelemetryContext,
    config: &TelemetryConfig,
) {
    register(
        engine,
        json!({
            "enabled": true,
            "distinct_id": ctx.device_id,
            "api_key": resolve_posthog_api_key(config),
            "host": resolve_posthog_host(config),
        }),
    );
}

/// Answer `{"enabled": false}` and nothing else (telemetry off, any reason).
/// The reason, the device id and the key stay inside the engine.
pub(super) fn register_disabled(engine: &Arc<Engine>) {
    register(engine, json!({ "enabled": false }));
}

fn register(engine: &Arc<Engine>, answer: Value) {
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: SETTINGS_FN_ID.to_string(),
            description: Some("Whether anonymous telemetry is on, and where it goes".to_string()),
            request_format: None,
            response_format: None,
            metadata: Some(json!({ "internal": true })),
        },
        Handler::new(move |_input: Value| {
            let answer = answer.clone();
            async move { FunctionResult::Success(Some(answer)) }
        }),
    );
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::workers::telemetry::environment::EnvironmentInfo;

    fn ctx() -> TelemetryContext {
        TelemetryContext {
            device_id: "device-123".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "machine-1".to_string(),
                iii_execution_context: "user".to_string(),
                timezone: "UTC".to_string(),
                cpu_cores: 4,
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
                host_user_id: None,
            },
        }
    }

    async fn ask(engine: &Arc<Engine>) -> Value {
        crate::workers::observability::metrics::ensure_default_meter();
        engine
            .call(SETTINGS_FN_ID, json!({ "ignored": true }))
            .await
            .expect("settings call succeeds")
            .expect("settings returns a value")
    }

    #[tokio::test]
    async fn enabled_answer_carries_all_four_fields() {
        let engine = Arc::new(Engine::new());
        let config = TelemetryConfig {
            posthog_api_key: Some("phc_test_key".to_string()),
            posthog_host: Some("http://127.0.0.1:9".to_string()),
            ..TelemetryConfig::default()
        };
        register_handler(&engine, &ctx(), &config);

        assert_eq!(
            ask(&engine).await,
            json!({
                "enabled": true,
                "distinct_id": "device-123",
                "api_key": "phc_test_key",
                "host": "http://127.0.0.1:9",
            })
        );
    }

    #[tokio::test]
    async fn enabled_answer_has_null_api_key_when_none_resolves() {
        // A blank configured key resolves to no key at all.
        let engine = Arc::new(Engine::new());
        let config = TelemetryConfig {
            posthog_api_key: Some("  ".to_string()),
            posthog_host: Some("http://127.0.0.1:9".to_string()),
            ..TelemetryConfig::default()
        };
        register_handler(&engine, &ctx(), &config);

        let answer = ask(&engine).await;
        assert_eq!(answer["enabled"], true);
        assert!(answer.get("api_key").is_some_and(Value::is_null));
    }

    #[tokio::test]
    async fn disabled_answer_is_exactly_enabled_false() {
        let engine = Arc::new(Engine::new());
        register_disabled(&engine);

        assert_eq!(ask(&engine).await, json!({ "enabled": false }));
    }

    #[tokio::test]
    async fn settings_is_tagged_internal() {
        let engine = Arc::new(Engine::new());
        register_disabled(&engine);

        let metadata = engine
            .functions
            .get(crate::protocol::DEFAULT_NAMESPACE, SETTINGS_FN_ID)
            .and_then(|f| f.metadata.clone());
        assert_eq!(metadata, Some(json!({ "internal": true })));
    }
}
