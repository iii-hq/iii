//! The trigger APIs deprecated in 0.25.0 keep working and log one warning per
//! process per API. A test binary of its own: the "once" is process-wide.

#![allow(deprecated)]

use std::sync::{Arc, Mutex};

use serde_json::json;
use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

use iii_sdk::protocol::RegisterTriggerInput;
use iii_sdk::trigger::{TriggerConfig, TriggerHandler};
use iii_sdk::{Error, IIIClient, RegisterTriggerType};

/// Collects the message of every warning.
#[derive(Clone, Default)]
struct Warnings(Arc<Mutex<Vec<String>>>);

struct Message<'a>(&'a mut String);

impl Visit for Message<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            *self.0 = format!("{value:?}");
        }
    }
}

impl Subscriber for Warnings {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        *metadata.level() == tracing::Level::WARN
    }
    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut message = String::new();
        event.record(&mut Message(&mut message));
        self.0.lock().unwrap().push(message);
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

struct Noop;

#[async_trait::async_trait]
impl TriggerHandler for Noop {
    async fn register_trigger(&self, _: TriggerConfig) -> Result<(), Error> {
        Ok(())
    }
    async fn unregister_trigger(&self, _: TriggerConfig) -> Result<(), Error> {
        Ok(())
    }
}

#[test]
fn each_deprecated_api_warns_once_per_process_and_keeps_working() {
    let warnings = Warnings::default();
    tracing::subscriber::with_default(warnings.clone(), || {
        let iii = IIIClient::new("ws://127.0.0.1:1");
        for _ in 0..3 {
            let trigger = iii
                .register_trigger(RegisterTriggerInput::new("cron", "f::g", json!({})))
                .expect("still works");
            trigger.unregister();
        }
        for i in 0..2 {
            let typed = iii.register_trigger_type(RegisterTriggerType::new(
                format!("type-{i}"),
                "a type",
                Noop,
            ));
            typed
                .register_trigger("f::g", json!({ "n": i }))
                .expect("still works");
        }
    });
    let warnings = warnings.0.lock().unwrap().clone();
    let count = |needle: &str| warnings.iter().filter(|w| w.contains(needle)).count();
    assert_eq!(count("register_trigger is deprecated"), 1, "{warnings:#?}");
    assert_eq!(
        count("register_trigger_type is deprecated"),
        1,
        "{warnings:#?}"
    );
    assert_eq!(
        count("https://iii.dev/docs/upgrading/from-0-24-x"),
        2,
        "{warnings:#?}"
    );
}

/// iii's own crates register their remaining trigger types without the
/// user-facing runtime warning.
#[test]
fn the_first_party_entry_point_does_not_warn() {
    let warnings = Warnings::default();
    tracing::subscriber::with_default(warnings.clone(), || {
        let iii = IIIClient::new("ws://127.0.0.1:1");
        iii.register_trigger_type_without_warning(RegisterTriggerType::new(
            "first-party",
            "a type",
            Noop,
        ));
    });
    let warnings = warnings.0.lock().unwrap().clone();
    assert!(
        warnings.iter().all(|w| !w.contains("deprecated")),
        "{warnings:#?}"
    );
}
