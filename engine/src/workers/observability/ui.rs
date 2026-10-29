// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Built-in Console Injection assets for `iii-observability`.
//!
//! The Console hosts `console::ui::asset`, observed with args `{ kind, path }`
//! for each asset it injects, and calls through each observer to load it.
//! The observability worker lives inside the engine rather than in a worker
//! process, so its observers are the engine's own: one per asset in every
//! namespace the Console registers the function in, all targeting the
//! built-in content function in `default`.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    observer::StandingObserver,
    protocol::{Deliver, ErrorBody},
};

/// The function the Console hosts for its injected UI assets.
pub const CONSOLE_ASSET_FUNCTION_ID: &str = "console::ui::asset";
pub const CONTENT_FUNCTION_ID: &str = "iii-observability::ui-content";
pub const PAGE_PATH: &str = "iii-observability/page.js";
pub const STYLES_PATH: &str = "iii-observability/styles.css";
pub const PAGE_OBSERVER_ID: &str = "iii-observability::ui-page";
pub const STYLES_OBSERVER_ID: &str = "iii-observability::ui-styles";

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

/// The engine's observers of the Console's asset function: the page script
/// and the stylesheet, in every namespace the Console registers it in (see
/// [`crate::observer::ObserverRegistry::observe_everywhere`]). The Console
/// calls through each with `{ path }` and gets `{ content, content_type }`.
pub fn observe_console_assets(engine: &Arc<Engine>) {
    for (id, kind, path) in [
        (PAGE_OBSERVER_ID, "script", PAGE_PATH),
        (STYLES_OBSERVER_ID, "style", STYLES_PATH),
    ] {
        engine.observers.observe_everywhere(StandingObserver {
            id: id.to_string(),
            function_id: CONSOLE_ASSET_FUNCTION_ID.to_string(),
            args: json!({ "kind": kind, "path": path }),
            target_id: CONTENT_FUNCTION_ID.to_string(),
            deliver: Deliver::Input,
            metadata: Some(json!({ "internal": true, "source": "builtin" })),
        });
    }
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

    /// The asset observers wait for the Console's function, and follow it
    /// into the namespace it registers in.
    #[tokio::test]
    async fn console_assets_are_observed_where_the_console_hosts_them() {
        crate::workers::observability::metrics::ensure_default_meter();
        let engine = Arc::new(Engine::new());
        register_function(&engine);
        observe_console_assets(&engine);
        assert!(engine.observers.get(PAGE_OBSERVER_ID).is_none());

        engine.register_function_handler_ns(
            "harness-ns",
            RegisterFunctionRequest {
                function_id: CONSOLE_ASSET_FUNCTION_ID.to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|input: Value| async move { FunctionResult::Success(Some(input)) }),
        );
        let page_id = format!("{PAGE_OBSERVER_ID}@harness-ns");
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while engine.observers.get(&page_id).is_none()
                || engine
                    .observers
                    .get(&format!("{STYLES_OBSERVER_ID}@harness-ns"))
                    .is_none()
            {
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("observers in the Console's namespace");
        let page = engine.observers.get(&page_id).unwrap();
        assert_eq!(page.args, json!({ "kind": "script", "path": PAGE_PATH }));
        assert_eq!(page.target.1, CONTENT_FUNCTION_ID);
        assert!(page.is_engine_owned());
    }
}
