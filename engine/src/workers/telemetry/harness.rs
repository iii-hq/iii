// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The harness reports its own session usage.
//!
//! The `harness` worker publishes one message per reported moment on the
//! `harness:usage` topic (see the worker's `usage_report` module). The engine
//! subscribes to that topic and reports each message as one product event:
//! `harness_session_progress` at root turn 1, 2, 5, 10, 25 and 50, cumulative
//! so the last report supersedes the ones before it, and
//! `harness_turn_failed` once per session per outcome.
//!
//! The event name arrives in the message and is checked against the two names
//! above, so a stray publish on the topic cannot mint product events of its
//! own naming. Everything else in the message becomes the event properties.
//!
//! The topic is a durable queue subscription, not fire-and-forget pub/sub: a
//! turn ends once, and the message waits in the queue and is retried until
//! this handler takes it, rather than being dropped when nothing is listening
//! at that instant.
//!
//! The worker decides WHAT is worth reporting and reports each thing once,
//! durably. The `dedupe_key` it stamps on every message is the second line of
//! defence, counted in memory only: a redelivery inside one engine run
//! reports once, while a restarted engine may report a message again. Like
//! the tour's steps, progress belongs to the worker that owns it, and an
//! engine keeping a shadow copy would answer to nobody when the two
//! disagreed.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use serde_json::{Value, json};

use super::{TelemetryContext, send_product_event};
use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    workers::telemetry::posthog::PostHogClient,
};

/// Topic the `harness` worker publishes each reported moment on.
pub const USAGE_TOPIC: &str = "harness:usage";
pub const USAGE_FN_ID: &str = "iii-telemetry::on-harness-usage";
pub const USAGE_TRIGGER_ID: &str = "iii-telemetry::harness-usage-watch";

pub const PROGRESS_EVENT: &str = "harness_session_progress";
pub const FAILED_EVENT: &str = "harness_turn_failed";

/// The event name and its properties, or `None` for a message this handler
/// does not report: one that is not an object, names no event, or names one
/// outside the pair above.
///
/// `event` and `dedupe_key` are the envelope and are dropped; every other
/// field the worker sent becomes a property, so a new counter needs a change
/// in the worker alone.
pub fn usage_event(payload: Value) -> Option<(String, Value)> {
    let Value::Object(mut fields) = payload else {
        return None;
    };
    let event = fields.remove("event")?.as_str()?.to_string();
    if event != PROGRESS_EVENT && event != FAILED_EVENT {
        return None;
    }
    fields.remove("dedupe_key");
    Some((event, Value::Object(fields)))
}

/// The `dedupe_key`s already reported by this engine process.
///
/// Deliberately in memory and nowhere else: the set is a duplicate filter for
/// one run, not a record of what the operator has done.
#[derive(Default)]
pub struct ReportedUsage(Mutex<HashSet<String>>);

impl ReportedUsage {
    /// Whether this message is the first report of its key. A message with no
    /// key cannot be deduplicated and always reports — a duplicate event is a
    /// lesser fault than a lost one. A poisoned lock reports for the same
    /// reason.
    pub fn claim(&self, payload: &Value) -> bool {
        let Some(key) = payload.get("dedupe_key").and_then(Value::as_str) else {
            return true;
        };
        match self.0.lock() {
            Ok(mut reported) => reported.insert(key.to_string()),
            Err(_) => true,
        }
    }
}

/// Register the handler that turns one published message into one product event.
pub(super) fn register_handler(
    engine: &Arc<Engine>,
    ctx: TelemetryContext,
    posthog_client: Option<Arc<PostHogClient>>,
) {
    let reported = Arc::new(ReportedUsage::default());
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: USAGE_FN_ID.to_string(),
            description: Some("Report harness session usage".to_string()),
            request_format: None,
            response_format: None,
            metadata: Some(json!({ "internal": true })),
        },
        Handler::new(move |input: Value| {
            let ctx = ctx.clone();
            let posthog_client = posthog_client.clone();
            let reported = Arc::clone(&reported);
            async move {
                // A duplicate, and a message this handler does not report,
                // both still succeed: the queue must see the message taken, or
                // it redelivers the one thing this filter exists to suppress.
                if reported.claim(&input)
                    && let Some((name, properties)) = usage_event(input)
                {
                    let event = ctx.build_event(&name, properties, None);
                    send_product_event(posthog_client.as_deref(), event).await;
                }
                FunctionResult::Success(Some(json!({})))
            }
        }),
    );
}

/// The same subscription with nothing behind it, for an engine the operator
/// opted out of telemetry on.
///
/// The worker publishes on a DURABLE topic, and the builtin queue keeps a
/// message nobody subscribes to: without this drain an opted-out engine would
/// grow a file of reports it never sends, and send the backlog the day
/// telemetry came back on. So the disabled worker takes the messages and drops
/// them. Nothing is read, nothing is counted, nothing leaves the process.
pub(super) fn register_drain(engine: &Arc<Engine>) {
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: USAGE_FN_ID.to_string(),
            description: Some("Discard harness session usage (telemetry disabled)".to_string()),
            request_format: None,
            response_format: None,
            metadata: Some(json!({ "internal": true })),
        },
        Handler::new(move |_input: Value| async move { FunctionResult::Success(Some(json!({}))) }),
    );
}

/// Subscribe the handler to the harness's topic in one namespace. The
/// deterministic trigger id means a re-registration replaces rather than
/// duplicates.
///
/// Fire and forget, like every other part of this worker: a subscription this
/// engine could not take is not the operator's problem and never becomes
/// theirs. A binding registered before the `queue` worker connects is stored
/// and replayed when the provider arrives, so a project without the harness,
/// or without `queue`, simply never fires it.
pub(super) async fn register_trigger_in(engine: &Arc<Engine>, namespace: &str) {
    let _ = engine
        .trigger_registry
        .register_trigger(super::topic_watch(
            USAGE_TRIGGER_ID,
            USAGE_FN_ID,
            USAGE_TOPIC,
            namespace,
        ))
        .await;
}

/// Subscribe in the default namespace, which is where an unnamespaced project
/// and the engine's own providers live.
pub(super) async fn register_trigger(engine: &Arc<Engine>) {
    register_trigger_in(engine, crate::protocol::DEFAULT_NAMESPACE).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress() -> Value {
        json!({
            "event": PROGRESS_EVENT,
            "dedupe_key": "s1:progress:2",
            "turn_index": 2,
            "model": "claude-opus-5",
            "cost_usd": 0.12,
        })
    }

    #[test]
    fn the_envelope_is_dropped_and_the_rest_is_properties() {
        let (name, props) = usage_event(progress()).expect("reported");
        assert_eq!(name, PROGRESS_EVENT);
        assert_eq!(props["turn_index"], 2);
        assert_eq!(props["model"], "claude-opus-5");
        assert_eq!(props["cost_usd"], 0.12);
        assert!(props.get("event").is_none());
        assert!(props.get("dedupe_key").is_none());
    }

    #[test]
    fn a_failure_reports_its_bucket() {
        let (name, props) = usage_event(json!({
            "event": FAILED_EVENT,
            "dedupe_key": "s1:failed",
            "outcome": "failed",
            "error_kind": "llm.rate_limited",
        }))
        .expect("reported");
        assert_eq!(name, FAILED_EVENT);
        assert_eq!(props["error_kind"], "llm.rate_limited");
    }

    #[test]
    fn only_the_two_known_events_report() {
        assert!(usage_event(json!({ "event": "harness_secret_exfil" })).is_none());
        assert!(usage_event(json!({ "turn_index": 1 })).is_none());
        assert!(usage_event(json!({ "event": 7 })).is_none());
        assert!(usage_event(json!("done")).is_none());
        assert!(usage_event(Value::Null).is_none());
    }

    #[test]
    fn one_report_per_dedupe_key() {
        let reported = ReportedUsage::default();

        assert!(reported.claim(&progress()));
        assert!(!reported.claim(&progress()));

        // The next milestone of the same session is its own report.
        assert!(reported.claim(&json!({ "dedupe_key": "s1:progress:5" })));
        // As is the same milestone of another session.
        assert!(reported.claim(&json!({ "dedupe_key": "s2:progress:2" })));

        // A message with no key cannot be deduplicated.
        assert!(reported.claim(&json!({ "event": PROGRESS_EVENT })));
        assert!(reported.claim(&json!({ "event": PROGRESS_EVENT })));

        // A fresh process starts over, which is what a restart gives us.
        assert!(ReportedUsage::default().claim(&progress()));
    }
}
