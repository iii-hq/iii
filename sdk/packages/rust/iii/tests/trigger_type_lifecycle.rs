//! Two-worker integration tests for custom trigger type lifecycle.

mod common;

use std::sync::{Arc, Mutex};

use serial_test::serial;

use async_trait::async_trait;
use iii_sdk::protocol::{RegisterTriggerInput, TriggerRequest};
use iii_sdk::runtime::IIIConnectionState;
use iii_sdk::trigger::{TriggerConfig, TriggerHandler};
use iii_sdk::{Error, InitOptions, RegisterFunction, register_worker};
use serde_json::{Value, json};
use tokio::time::Duration;

const TRIGGER_TYPE_ID: &str = "test.tt-lifecycle.rust";
const CONSUMER_FN: &str = "test.tt-lifecycle.rust.consumer";
const FIRE_FN: &str = "test.tt-lifecycle.rust.fire";

#[derive(Clone, Default)]
struct LifecycleState {
    bindings: Arc<Mutex<Vec<TriggerConfig>>>,
    register_calls: Arc<Mutex<Vec<TriggerConfig>>>,
    unregister_calls: Arc<Mutex<Vec<TriggerConfig>>>,
    handler_calls: Arc<Mutex<Vec<Value>>>,
}

struct LifecycleTriggerHandler {
    state: LifecycleState,
}

#[async_trait]
impl TriggerHandler for LifecycleTriggerHandler {
    async fn register_trigger(&self, config: TriggerConfig) -> Result<(), Error> {
        self.state.bindings.lock().unwrap().push(config.clone());
        self.state.register_calls.lock().unwrap().push(config);
        Ok(())
    }

    async fn unregister_trigger(&self, config: TriggerConfig) -> Result<(), Error> {
        let stored = {
            let mut bindings = self.state.bindings.lock().unwrap();
            let idx = bindings.iter().position(|b| b.id == config.id);
            idx.map(|i| bindings.remove(i))
        };
        self.state
            .unregister_calls
            .lock()
            .unwrap()
            .push(stored.unwrap_or(config));
        Ok(())
    }
}

async fn wait_connected(iii: &iii_sdk::IIIClient) {
    for _ in 0..50 {
        if iii.get_connection_state() == IIIConnectionState::Connected {
            tokio::time::sleep(Duration::from_millis(100)).await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("worker did not connect");
}

async fn wait_register_calls(state: &LifecycleState, at_least: usize) {
    for _ in 0..50 {
        if state.register_calls.lock().unwrap().len() >= at_least {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for register_trigger callbacks");
}

async fn wait_handler_calls(state: &LifecycleState, at_least: usize) {
    for _ in 0..50 {
        if state.handler_calls.lock().unwrap().len() >= at_least {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for handler invocations");
}

/// Named worker options.
///
/// Provider and consumer are connected at the same time, so the SDK's default
/// `{hostname}:{pid}` name would put both in the same
/// `(namespace, worker_name)` lease — the engine fatally rejects the second
/// claimant. The names are stable per role: `provider_reconnect_rebinds_trigger`
/// re-creates the provider under the same identity after shutting the first one
/// down, which is the reconnect the test is asserting.
fn named_options(worker_name: &str) -> InitOptions {
    InitOptions {
        metadata: Some(iii_sdk::runtime::WorkerMetadata {
            name: worker_name.to_string(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

async fn create_provider(state: &LifecycleState) -> iii_sdk::IIIClient {
    let handler_state = state.clone();
    let iii = register_worker(
        &common::engine_ws_url(),
        named_options("rust-sdk-lifecycle-provider"),
    );
    wait_connected(&iii).await;

    let fire_state = state.clone();
    let fire_iii = iii.clone();

    iii.register_trigger_type(iii_sdk::RegisterTriggerType::new(
        TRIGGER_TYPE_ID,
        "Rust SDK lifecycle test trigger type",
        LifecycleTriggerHandler {
            state: handler_state,
        },
    ));

    iii.register_function(
        FIRE_FN,
        RegisterFunction::new_async(move |payload: Value| {
            let fire_state = fire_state.clone();
            let fire_iii = fire_iii.clone();
            async move {
                let bindings: Vec<TriggerConfig> = fire_state.bindings.lock().unwrap().clone();
                for binding in bindings {
                    let _ = fire_iii
                        .trigger(TriggerRequest {
                            function_id: binding.function_id,
                            payload: payload.clone(),
                            action: None,
                            timeout_ms: Some(5000),
                        })
                        .await;
                }
                Ok(json!({ "fired": fire_state.bindings.lock().unwrap().len() }))
            }
        }),
    );

    iii
}

async fn create_consumer(state: &LifecycleState) -> iii_sdk::IIIClient {
    let handler_state = state.clone();
    let iii = register_worker(
        &common::engine_ws_url(),
        named_options("rust-sdk-lifecycle-consumer"),
    );
    wait_connected(&iii).await;

    iii.register_function(
        CONSUMER_FN,
        RegisterFunction::new_async(move |payload: Value| {
            let handler_state = handler_state.clone();
            async move {
                handler_state.handler_calls.lock().unwrap().push(payload);
                Ok(json!({ "ok": true }))
            }
        }),
    );

    iii.register_trigger(RegisterTriggerInput {
        trigger_type: TRIGGER_TYPE_ID.to_string(),
        function_id: CONSUMER_FN.to_string(),
        config: json!({ "tag": "test" }),
        metadata: None,
        namespace: None,
    })
    .expect("register trigger");

    wait_register_calls(state, 1).await;
    iii
}

#[tokio::test]
#[serial]
async fn fire_invokes_bound_function() {
    let state = LifecycleState::default();
    let provider = create_provider(&state).await;

    let consumer = create_consumer(&state).await;

    assert_eq!(state.bindings.lock().unwrap().len(), 1);
    assert_eq!(state.register_calls.lock().unwrap().len(), 1);
    assert_eq!(
        state.register_calls.lock().unwrap()[0].function_id,
        CONSUMER_FN
    );

    let result = provider
        .trigger(TriggerRequest {
            function_id: FIRE_FN.to_string(),
            payload: json!({ "n": 1 }),
            action: None,
            timeout_ms: None,
        })
        .await
        .expect("fire");
    assert_eq!(result.get("fired"), Some(&json!(1)));

    wait_handler_calls(&state, 1).await;

    let calls = state.handler_calls.lock().unwrap();
    assert_eq!(calls[0].get("n"), Some(&json!(1)));

    consumer.shutdown();
    provider.shutdown();
}

#[tokio::test]
#[serial]
async fn provider_reconnect_rebinds_trigger() {
    let state = LifecycleState::default();
    let provider = create_provider(&state).await;

    let consumer = create_consumer(&state).await;

    let bound_trigger_id = state.register_calls.lock().unwrap()[0].id.clone();
    state.register_calls.lock().unwrap().clear();

    provider.shutdown();
    tokio::time::sleep(Duration::from_millis(400)).await;

    let provider = create_provider(&state).await;
    wait_register_calls(&state, 1).await;

    {
        let register_calls = state.register_calls.lock().unwrap();
        assert!(
            register_calls
                .iter()
                .any(|c| c.id == bound_trigger_id && c.function_id == CONSUMER_FN),
            "expected re-bind for trigger {bound_trigger_id}"
        );
    }

    state.handler_calls.lock().unwrap().clear();

    provider
        .trigger(TriggerRequest {
            function_id: FIRE_FN.to_string(),
            payload: json!({ "n": 2 }),
            action: None,
            timeout_ms: None,
        })
        .await
        .expect("fire");

    wait_handler_calls(&state, 1).await;

    let calls = state.handler_calls.lock().unwrap();
    assert_eq!(calls.last().unwrap().get("n"), Some(&json!(2)));

    consumer.shutdown();
    provider.shutdown();
}

#[tokio::test]
#[serial]
async fn consumer_disconnect_invokes_unregister_trigger() {
    let state = LifecycleState::default();
    let provider = create_provider(&state).await;

    let consumer = create_consumer(&state).await;

    state.unregister_calls.lock().unwrap().clear();

    consumer.shutdown();
    tokio::time::sleep(Duration::from_millis(600)).await;

    let unregister_calls = state.unregister_calls.lock().unwrap();
    assert_eq!(unregister_calls.len(), 1);
    assert_eq!(unregister_calls[0].function_id, CONSUMER_FN);
    assert_eq!(unregister_calls[0].config, json!({ "tag": "test" }));

    provider.shutdown();
}
