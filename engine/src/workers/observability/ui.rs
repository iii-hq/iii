// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Built-in Console Injection assets for `iii-observability`.
//!
//! The observability worker lives inside the engine rather than in a worker
//! process, so it owns its Console UI bindings directly in the engine trigger
//! registry. Console providers are namespace-scoped, while the built-in
//! content function lives in `default`; one binding is therefore installed
//! for every namespace that provides `console:script` / `console:style`.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    protocol::{DEFAULT_NAMESPACE, ErrorBody},
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

fn asset_for_trigger_type(trigger_type: &str) -> Option<(&'static str, &'static str)> {
    match trigger_type {
        "console:script" => Some((PAGE_TRIGGER_ID, PAGE_PATH)),
        "console:style" => Some((STYLES_TRIGGER_ID, STYLES_PATH)),
        _ => None,
    }
}

fn namespaced_trigger_id(base_id: &str, provider_namespace: &str) -> String {
    if provider_namespace == DEFAULT_NAMESPACE {
        base_id.to_string()
    } else {
        format!("{base_id}@{provider_namespace}")
    }
}

/// Ensure the engine-owned asset binding exists for one Console provider.
///
/// This runs immediately before a Console trigger type is registered. On the
/// first connection the binding parks briefly and is drained by that type
/// registration; on reconnect it already exists and the trigger registry
/// replays it to the replacement provider.
pub(crate) async fn register_trigger_for_provider(
    engine: &Engine,
    trigger_type: &str,
    provider_namespace: &str,
) -> anyhow::Result<()> {
    let Some((base_id, path)) = asset_for_trigger_type(trigger_type) else {
        return Ok(());
    };
    let id = namespaced_trigger_id(base_id, provider_namespace);

    if engine.trigger_registry.triggers.contains_key(&id)
        || engine.trigger_registry.pending_triggers.contains_key(&id)
    {
        return Ok(());
    }

    engine
        .trigger_registry
        .register_trigger(Trigger {
            id,
            trigger_type: trigger_type.to_string(),
            function_id: CONTENT_FUNCTION_ID.to_string(),
            config: json!({ "path": path }),
            worker_id: None,
            metadata: Some(json!({
                "internal": true,
                "source": "builtin",
            })),
            // The content function is engine-owned and remains in `default`.
            namespace: crate::protocol::default_namespace(),
            // The provider is strict so one project's asset cannot be sent to
            // another project's Console when both share an engine.
            trigger_namespace: Some(provider_namespace.to_string()),
            home_namespace: provider_namespace.to_string(),
            provider_namespace: provider_namespace.to_string(),
        })
        .await
        .map_err(|error| {
            anyhow::anyhow!(
                "failed to register Console UI asset {path} in namespace {provider_namespace}: {error:?}"
            )
        })?;

    Ok(())
}

/// Register bindings for Console providers that already exist (for example
/// when the observability worker is hot-reloaded after Console connected).
pub async fn register_triggers(engine: &Engine) -> anyhow::Result<()> {
    let providers: Vec<(String, String)> = engine
        .trigger_registry
        .trigger_types
        .iter()
        .filter(|entry| asset_for_trigger_type(&entry.value().id).is_some())
        .map(|entry| (entry.value().id.clone(), entry.value().namespace.clone()))
        .collect();

    for (trigger_type, provider_namespace) in providers {
        register_trigger_for_provider(engine, &trigger_type, &provider_namespace).await?;
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
    async fn trigger_registration_waits_until_console_provider_exists() {
        let engine = Engine::new();
        register_triggers(&engine)
            .await
            .expect("missing Console provider should be a no-op");
        assert!(engine.trigger_registry.triggers.is_empty());
        assert!(engine.trigger_registry.pending_triggers.is_empty());
    }

    #[test]
    fn namespaced_ids_preserve_the_default_ids_and_isolate_projects() {
        assert_eq!(
            namespaced_trigger_id(PAGE_TRIGGER_ID, DEFAULT_NAMESPACE),
            PAGE_TRIGGER_ID
        );
        assert_eq!(
            namespaced_trigger_id(PAGE_TRIGGER_ID, "harness-ns"),
            "iii-observability::ui-page@harness-ns"
        );
    }
}
