// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Built-in Console Injection assets for `iii-observability`.
//!
//! The observability worker lives inside the engine rather than in a worker
//! process, so it owns its Console UI bindings directly in the engine trigger
//! registry. The Console trigger provider may connect before or after this
//! module; the registry parks the bindings until `console:script` and
//! `console:style` become available.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    protocol::ErrorBody,
    trigger::Trigger,
};

pub const CONTENT_FUNCTION_ID: &str = "iii-observability::ui-content";
pub const PAGE_PATH: &str = "iii-observability/page.js";
pub const STYLES_PATH: &str = "iii-observability/styles.css";
pub const PAGE_TRIGGER_ID: &str = "iii-observability::ui-page";
pub const STYLES_TRIGGER_ID: &str = "iii-observability::ui-styles";

const PAGE_JS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/workers/observability/ui/dist/page.js"
));
const STYLES_CSS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/workers/observability/ui/dist/styles.css"
));

/// Register the content function that serves both Console assets.
pub fn register_function(engine: &Arc<Engine>) {
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: CONTENT_FUNCTION_ID.to_string(),
            description: Some(
                "Serve the built-in iii-observability Console configuration UI assets."
                    .to_string(),
            ),
            request_format: None,
            response_format: None,
            metadata: Some(json!({
                "internal": true,
                "source": "builtin",
            })),
        },
        Handler::new(|input: Value| async move {
            let path = input.get("path").and_then(Value::as_str).ok_or_else(|| {
                ErrorBody::new("INVALID_UI_ASSET", "Console UI asset request requires a path")
            });

            let path = match path {
                Ok(path) => path,
                Err(error) => return FunctionResult::Failure(error),
            };

            let (content, content_type) = match path {
                PAGE_PATH => (PAGE_JS, "text/javascript; charset=utf-8"),
                STYLES_PATH => (STYLES_CSS, "text/css; charset=utf-8"),
                other => {
                    return FunctionResult::Failure(ErrorBody::new(
                        "UNKNOWN_UI_ASSET",
                        format!(
                            "Unknown iii-observability Console UI asset '{other}' (expected '{PAGE_PATH}' or '{STYLES_PATH}')"
                        ),
                    ));
                }
            };

            FunctionResult::Success(Some(json!({
                "content": content,
                "content_type": content_type,
            })))
        }),
    );
}

/// Register the engine-owned asset bindings.
pub async fn register_triggers(engine: &Engine) -> anyhow::Result<()> {
    for (id, trigger_type, path) in [
        (PAGE_TRIGGER_ID, "console:script", PAGE_PATH),
        (STYLES_TRIGGER_ID, "console:style", STYLES_PATH),
    ] {
        let (trigger_namespace, home_namespace, provider_namespace) =
            Trigger::internal_namespaces();

        engine
            .trigger_registry
            .register_trigger(Trigger {
                id: id.to_string(),
                trigger_type: trigger_type.to_string(),
                function_id: CONTENT_FUNCTION_ID.to_string(),
                config: json!({ "path": path }),
                worker_id: None,
                metadata: Some(json!({
                    "internal": true,
                    "source": "builtin",
                })),
                namespace: crate::protocol::default_namespace(),
                trigger_namespace,
                home_namespace,
                provider_namespace,
            })
            .await
            .map_err(|error| {
                anyhow::anyhow!("failed to register Console UI asset {path}: {error:?}")
            })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_page_is_nonempty_esm() {
        assert!(!PAGE_JS.trim().is_empty());
        assert!(PAGE_JS.contains("export"));
    }

    #[test]
    fn embedded_styles_are_scoped() {
        assert!(
            STYLES_CSS.contains(r#"[data-iii-ui="iii-observability"]"#)
                || STYLES_CSS.contains("[data-iii-ui=iii-observability]")
        );
    }

    #[tokio::test]
    async fn content_function_dispatches_only_known_assets() {
        crate::workers::observability::metrics::ensure_default_meter();
        let engine = Arc::new(Engine::new());
        register_function(&engine);

        let page = engine
            .call(CONTENT_FUNCTION_ID, json!({ "path": PAGE_PATH }))
            .await
            .expect("page asset should be served")
            .expect("page result should have a body");
        assert_eq!(page["content_type"], "text/javascript; charset=utf-8");
        assert_eq!(page["content"], PAGE_JS);

        let unknown = engine
            .call(CONTENT_FUNCTION_ID, json!({ "path": "other/page.js" }))
            .await
            .expect_err("unknown asset must be rejected");
        assert_eq!(unknown.code, "UNKNOWN_UI_ASSET");
    }

    #[tokio::test]
    async fn trigger_registration_is_deferred_until_console_provider_exists() {
        let engine = Engine::new();
        register_triggers(&engine)
            .await
            .expect("missing Console provider should park, not fail");
    }
}
