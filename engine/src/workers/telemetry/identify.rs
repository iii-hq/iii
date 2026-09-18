// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! An address the user gave us becomes a property on this device's person.
//!
//! A worker that captures an email (the onboarding signup box, and the
//! console's own prompt) publishes it on the `telemetry:identify` topic. The
//! engine subscribes, and reports one `user_identified` event carrying a
//! PostHog `$set`, so the address lands on the person keyed by THIS process's
//! `device_id`. The publishing worker never learns the `device_id`, and the
//! browser never talks to PostHog.
//!
//! The person keeps its `device_id` identity. Nothing is aliased and nothing
//! is merged, so the profile that has reported every event since install is
//! the one that gains an address, and every existing cohort still matches it.
//! The cost is that one human with two machines is two persons carrying the
//! same address, which a cohort or a `GROUP BY email` puts back together.
//!
//! TODO: Change this to PostHog's true `$identify` when iii cloud is available
//! and can provide stable IDs. A merge needs an identifier that means one
//! human across machines, and `device_id` is one per `~/.iii`. `$identify`
//! also spends the device's single allowed merge, so it wants an id we trust,
//! which is an account, not an address typed into a box.

use std::sync::Arc;

use serde_json::{Value, json};

use super::{TelemetryContext, harness::ReportedUsage, send_product_event};
use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    workers::telemetry::amplitude::{AmplitudeClient, PostHogClient},
};

/// Topic a worker publishes a captured address on.
pub const IDENTIFY_TOPIC: &str = "telemetry:identify";
pub const IDENTIFY_FN_ID: &str = "iii-telemetry::on-identify";
pub const IDENTIFY_TRIGGER_ID: &str = "iii-telemetry::identify-watch";

/// RFC 5321's ceiling for a whole address, the same bound the onboarding
/// worker's signup box enforces.
const MAX_EMAIL_LENGTH: usize = 254;

/// The properties of one identify event, or `None` for a message this handler
/// does not report.
///
/// The address is checked here as well as in the worker that captured it. The
/// topic is reachable by anything in the project, and this is the one place in
/// telemetry that sends something a person typed, so it does not send a
/// paragraph, an empty string, or a list of addresses.
pub fn identify_properties(payload: Value) -> Option<Value> {
    let Value::Object(fields) = payload else {
        return None;
    };
    let email = fields.get("email")?.as_str()?.trim();
    if email.is_empty() || email.len() > MAX_EMAIL_LENGTH {
        return None;
    }
    // One address: exactly one `@`, something either side, a dot in the
    // domain, and none of the characters that would make this a list or a
    // display name wrapping an address.
    if email.contains([' ', '\t', ',', ';', '<', '>']) {
        return None;
    }
    let (local, domain) = email.split_once('@')?;
    if local.is_empty()
        || domain.contains('@')
        || !domain.contains('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
    {
        return None;
    }

    let mut properties = json!({ "email": email });
    // Where the address came from: the onboarding box, the console prompt, or
    // whatever asks next. Free-form, and absent is fine.
    if let Some(source) = fields.get("source").and_then(Value::as_str) {
        properties["source"] = json!(source);
    }
    Some(properties)
}

/// Register the handler that turns one captured address into one identify
/// event.
pub(super) fn register_handler(
    engine: &Arc<Engine>,
    ctx: TelemetryContext,
    client: Arc<AmplitudeClient>,
    posthog_client: Option<Arc<PostHogClient>>,
) {
    let reported = Arc::new(ReportedUsage::default());
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: IDENTIFY_FN_ID.to_string(),
            description: Some("Report a captured email address".to_string()),
            request_format: None,
            response_format: None,
            metadata: Some(json!({ "internal": true })),
        },
        Handler::new(move |input: Value| {
            let ctx = ctx.clone();
            let client = Arc::clone(&client);
            let posthog_client = posthog_client.clone();
            let reported = Arc::clone(&reported);
            async move {
                // A rejected message still succeeds, or the queue redelivers
                // it forever.
                if let Some(properties) = identify_properties(input) {
                    // `$set` never changes, so the same address reports once
                    // per engine run. The address itself is the key: a person
                    // who submits twice is not two identifies.
                    let key = json!({ "dedupe_key": properties["email"].clone() });
                    if reported.claim(&key) {
                        let event =
                            ctx.build_event(super::amplitude::IDENTIFY_EVENT, properties, None);
                        send_product_event(&client, posthog_client.as_deref(), event).await;
                    }
                }
                FunctionResult::Success(Some(json!({})))
            }
        }),
    );
}

/// The same subscription with nothing behind it, for an engine the operator
/// opted out of telemetry on.
///
/// Without it the builtin queue would keep every address on a durable topic
/// nobody reads, and send them the day telemetry came back on. An opted-out
/// engine takes each message and drops it.
pub(super) fn register_drain(engine: &Arc<Engine>) {
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: IDENTIFY_FN_ID.to_string(),
            description: Some("Discard a captured email address (telemetry disabled)".to_string()),
            request_format: None,
            response_format: None,
            metadata: Some(json!({ "internal": true })),
        },
        Handler::new(move |_input: Value| async move { FunctionResult::Success(Some(json!({}))) }),
    );
}

/// Subscribe the handler to the identify topic in one namespace.
pub(super) async fn register_trigger_in(engine: &Arc<Engine>, namespace: &str) {
    let _ = engine
        .trigger_registry
        .register_trigger(super::topic_watch(
            IDENTIFY_TRIGGER_ID,
            IDENTIFY_FN_ID,
            IDENTIFY_TOPIC,
            namespace,
        ))
        .await;
}

/// Subscribe in the default namespace.
pub(super) async fn register_trigger(engine: &Arc<Engine>) {
    register_trigger_in(engine, crate::protocol::DEFAULT_NAMESPACE).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_becomes_properties() {
        let props = identify_properties(json!({
            "email": "  someone@example.com  ",
            "source": "console_prompt",
        }))
        .expect("reported");

        assert_eq!(props["email"], "someone@example.com");
        assert_eq!(props["source"], "console_prompt");
    }

    #[test]
    fn the_source_is_optional() {
        let props = identify_properties(json!({ "email": "a@b.co" })).expect("reported");
        assert_eq!(props["email"], "a@b.co");
        assert!(props.get("source").is_none());
    }

    #[test]
    fn nothing_that_is_not_one_address_reports() {
        for payload in [
            json!({}),
            json!({ "email": "" }),
            json!({ "email": "   " }),
            json!({ "email": "not-an-address" }),
            json!({ "email": "@example.com" }),
            json!({ "email": "someone@example" }),
            json!({ "email": "someone@.com" }),
            json!({ "email": "one@a.co, two@b.co" }),
            json!({ "email": "one@a.co two@b.co" }),
            json!({ "email": "Someone <someone@a.co>" }),
            json!({ "email": 7 }),
            json!("someone@example.com"),
            Value::Null,
        ] {
            assert!(
                identify_properties(payload.clone()).is_none(),
                "reported {payload}"
            );
        }
    }

    #[test]
    fn an_address_at_the_length_ceiling_reports_and_past_it_does_not() {
        let domain = "@example.com";
        let at_ceiling = "a".repeat(MAX_EMAIL_LENGTH - domain.len()) + domain;
        assert_eq!(at_ceiling.len(), MAX_EMAIL_LENGTH);
        assert!(identify_properties(json!({ "email": at_ceiling })).is_some());

        let past_ceiling = "a".repeat(MAX_EMAIL_LENGTH) + domain;
        assert!(identify_properties(json!({ "email": past_ceiling })).is_none());
    }
}
