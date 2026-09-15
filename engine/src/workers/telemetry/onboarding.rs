// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The onboarding tour reports its own progress.
//!
//! The `onboarding` worker publishes one message per closed tour step on the
//! `onboarding:steps:complete` topic. The engine subscribes to that topic
//! and reports each message as an `onboarding_step` product event, with the
//! payload the worker sent.
//!
//! The topic is a durable queue subscription, not fire-and-forget pub/sub: a
//! step closes once, and the message waits in the queue and is retried until
//! this handler takes it, rather than being dropped when nothing is listening
//! at that instant.
//!
//! One event per step, counted in memory only: the same step reported twice
//! (a redelivery, a reload, a second report from the agent) reports once,
//! while a restarted engine lets every step report again. Progress belongs to
//! the tour's own state, and an engine that persisted a shadow copy of it
//! would answer to nobody when the two disagreed.
//!
//! The `durable:subscriber` trigger type belongs to the `queue` worker. A
//! binding registered before its provider connects is parked and replayed
//! when the provider arrives (see `TriggerRegistry::register_trigger`), so
//! this registration is unconditional: a project without the tour, or without
//! `queue`, simply never fires it.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use serde_json::{Value, json};

use super::{TelemetryContext, send_product_event};
use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    trigger::Trigger,
    workers::telemetry::amplitude::{AmplitudeClient, PostHogClient},
};

/// Topic the `onboarding` worker publishes each closed step on.
pub const STEP_TOPIC: &str = "onboarding:steps:complete";
pub const STEP_FN_ID: &str = "iii-telemetry::on-onboarding-step";
pub const STEP_TRIGGER_ID: &str = "iii-telemetry::onboarding-step-watch";
pub const STEP_EVENT: &str = "onboarding_step";

/// The properties the event carries: the tour message as published, or — for
/// a payload that is not an object — the value under `payload`, so a
/// malformed publish is still visible rather than dropped.
pub fn step_properties(payload: Value) -> Value {
    match payload {
        Value::Object(_) => payload,
        other => json!({ "payload": other }),
    }
}

/// The steps already reported by this engine process.
///
/// Deliberately in memory and nowhere else: the set is a duplicate filter for
/// one run, not a record of what the operator has done.
#[derive(Default)]
pub struct ReportedSteps(Mutex<HashSet<(String, String)>>);

impl ReportedSteps {
    /// The step's identity: the tour and the step, as published. A message
    /// that names neither cannot be deduplicated and always reports — an
    /// unidentifiable step is rarer than a lost one is harmful.
    fn key(payload: &Value) -> Option<(String, String)> {
        let text = |field| payload.get(field)?.as_str().map(str::to_string);
        let tour = text("tour_id")?;
        let step = text("step_id")?;
        Some((tour, step))
    }

    /// Whether this message is the first report of its step. A poisoned lock
    /// reports rather than swallows: a duplicate event is the lesser fault.
    pub fn claim(&self, payload: &Value) -> bool {
        let Some(key) = Self::key(payload) else {
            return true;
        };
        match self.0.lock() {
            Ok(mut reported) => reported.insert(key),
            Err(_) => true,
        }
    }
}

/// Register the handler that turns one published step into one product event.
pub(super) fn register_handler(
    engine: &Arc<Engine>,
    ctx: TelemetryContext,
    client: Arc<AmplitudeClient>,
    posthog_client: Option<Arc<PostHogClient>>,
) {
    let reported = Arc::new(ReportedSteps::default());
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: STEP_FN_ID.to_string(),
            description: Some("Report a completed onboarding tour step".to_string()),
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
                // A duplicate still succeeds: the queue must see the message
                // taken, or it redelivers the one thing this filter exists to
                // suppress.
                if reported.claim(&input) {
                    let event = ctx.build_event(STEP_EVENT, step_properties(input), None);
                    send_product_event(&client, posthog_client.as_deref(), event).await;
                }
                FunctionResult::Success(Some(json!({})))
            }
        }),
    );
}

/// Subscribe the handler to the tour's topic. The deterministic trigger id
/// means a re-registration replaces rather than duplicates.
///
/// Fire and forget, like every other part of this worker: a subscription
/// this engine could not take is not the operator's problem and never
/// becomes theirs. The outcome is dropped — nothing is returned to a caller,
/// nothing is logged or traced here, and nothing about the tour changes.
pub(super) async fn register_trigger(engine: &Arc<Engine>) {
    let _ = engine
        .trigger_registry
        .register_trigger(Trigger {
            id: STEP_TRIGGER_ID.to_string(),
            trigger_type: "durable:subscriber".to_string(),
            function_id: STEP_FN_ID.to_string(),
            config: json!({ "topic": STEP_TOPIC }),
            worker_id: None,
            metadata: None,
            namespace: crate::protocol::default_namespace(),
            trigger_namespace: None,
            home_namespace: crate::protocol::default_namespace(),
            provider_namespace: crate::protocol::default_namespace(),
        })
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(tour: &str, id: &str) -> Value {
        json!({ "tour_id": tour, "step_id": id, "step_number": 1, "step_title": "T" })
    }

    #[test]
    fn an_object_payload_is_the_event_properties() {
        let props = step_properties(json!({
            "tour_id": "console-basics",
            "tour_title": "Find your way around",
            "step_number": 3,
            "step_id": "coder",
            "step_title": "CODER",
        }));
        assert_eq!(props["tour_id"], "console-basics");
        assert_eq!(props["step_number"], 3);
        assert_eq!(props["step_title"], "CODER");
    }

    #[test]
    fn a_non_object_payload_is_kept_under_payload() {
        assert_eq!(step_properties(json!("done")), json!({ "payload": "done" }));
        assert_eq!(step_properties(Value::Null), json!({ "payload": null }));
    }

    #[test]
    fn one_report_per_step_and_tour() {
        let reported = ReportedSteps::default();

        assert!(reported.claim(&step("console-basics", "coder")));
        assert!(!reported.claim(&step("console-basics", "coder")));

        // Another step of the same tour, and the same step id in another
        // tour, are both their own step.
        assert!(reported.claim(&step("console-basics", "tabs")));
        assert!(reported.claim(&step("second-tour", "coder")));

        // A fresh process starts over, which is what a restart gives us.
        assert!(ReportedSteps::default().claim(&step("console-basics", "coder")));
    }

    #[test]
    fn a_step_without_an_identity_always_reports() {
        let reported = ReportedSteps::default();
        assert!(reported.claim(&json!({ "step_id": "coder" })));
        assert!(reported.claim(&json!({ "step_id": "coder" })));
        assert!(reported.claim(&json!("done")));
    }
}
