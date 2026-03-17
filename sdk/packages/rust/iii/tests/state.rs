//! Integration tests for state operations.
//!
//! Requires a running III engine. Set III_URL or use ws://localhost:49134 default.

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::Mutex;

use iii_sdk::{III, RegisterFunctionMessage, RegisterTriggerInput, TriggerRequest};

const SCOPE: &str = "test-scope-rs";

fn unique_key(test_name: &str) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    format!("{test_name}-{ts}")
}

async fn delete_state(iii: &III, key: &str) {
    let _ = iii
        .trigger(TriggerRequest {
            function_id: "state::delete".to_string(),
            payload: json!({"scope": SCOPE, "key": key}),
            action: None,
            timeout_ms: None,
        })
        .await;
}

#[tokio::test]
async fn state_set_new_item() {
    let key = unique_key("set-new");
    let iii = common::shared_iii();
    delete_state(iii, &key).await;

    let test_data = json!({"name": "Test Item", "value": 42});

    let result = iii
        .trigger(TriggerRequest {
            function_id: "state::set".to_string(),
            payload: json!({"scope": SCOPE, "key": key, "value": test_data}),
            action: None,
            timeout_ms: None,
        })
        .await
        .expect("state::set");

    assert_eq!(result["old_value"], Value::Null);
    assert_eq!(result["new_value"], test_data);

    delete_state(iii, &key).await;
}

#[tokio::test]
async fn state_set_overwrite() {
    let key = unique_key("set-overwrite");
    let iii = common::shared_iii();
    delete_state(iii, &key).await;

    let initial_data = json!({"value": 1});
    let updated_data = json!({"value": 2, "updated": true});

    iii.trigger(TriggerRequest {
        function_id: "state::set".to_string(),
        payload: json!({"scope": SCOPE, "key": key, "value": initial_data}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("state::set initial");

    let result = iii
        .trigger(TriggerRequest {
            function_id: "state::set".to_string(),
            payload: json!({"scope": SCOPE, "key": key, "value": updated_data}),
            action: None,
            timeout_ms: None,
        })
        .await
        .expect("state::set overwrite");

    assert_eq!(result["old_value"], initial_data);
    assert_eq!(result["new_value"], updated_data);

    delete_state(iii, &key).await;
}

#[tokio::test]
async fn state_get_existing_item() {
    let key = unique_key("get-existing");
    let iii = common::shared_iii();
    delete_state(iii, &key).await;

    let data = json!({"name": "Test", "value": 100});

    iii.trigger(TriggerRequest {
        function_id: "state::set".to_string(),
        payload: json!({"scope": SCOPE, "key": key, "value": data}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("state::set");

    let result = iii
        .trigger(TriggerRequest {
            function_id: "state::get".to_string(),
            payload: json!({"scope": SCOPE, "key": key}),
            action: None,
            timeout_ms: None,
        })
        .await
        .expect("state::get");

    assert_eq!(result, data);

    delete_state(iii, &key).await;
}

#[tokio::test]
async fn state_get_non_existent_item() {
    let iii = common::shared_iii();

    let result = iii
        .trigger(TriggerRequest {
            function_id: "state::get".to_string(),
            payload: json!({"scope": SCOPE, "key": "non-existent-item"}),
            action: None,
            timeout_ms: None,
        })
        .await
        .expect("state::get non-existent");

    assert!(result.is_null());
}

#[tokio::test]
async fn state_delete_existing_item() {
    let key = unique_key("delete-existing");
    let iii = common::shared_iii();
    delete_state(iii, &key).await;

    iii.trigger(TriggerRequest {
        function_id: "state::set".to_string(),
        payload: json!({"scope": SCOPE, "key": key, "value": {"test": true}}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("state::set");

    iii.trigger(TriggerRequest {
        function_id: "state::delete".to_string(),
        payload: json!({"scope": SCOPE, "key": key}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("state::delete");

    let result = iii
        .trigger(TriggerRequest {
            function_id: "state::get".to_string(),
            payload: json!({"scope": SCOPE, "key": key}),
            action: None,
            timeout_ms: None,
        })
        .await
        .expect("state::get after delete");

    assert!(result.is_null());
}

#[tokio::test]
async fn state_delete_non_existent_item() {
    let iii = common::shared_iii();

    iii.trigger(TriggerRequest {
        function_id: "state::delete".to_string(),
        payload: json!({"scope": SCOPE, "key": "non-existent"}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("state::delete non-existent should not error");
}

#[tokio::test]
async fn state_list_all_items_in_scope() {
    let iii = common::shared_iii();

    let scope = format!(
        "state-rs-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );

    let items = vec![
        json!({"id": "state-item1", "value": 1}),
        json!({"id": "state-item2", "value": 2}),
        json!({"id": "state-item3", "value": 3}),
    ];

    for item in &items {
        iii.trigger(TriggerRequest {
            function_id: "state::set".to_string(),
            payload: json!({"scope": scope, "key": item["id"], "value": item}),
            action: None,
            timeout_ms: None,
        })
        .await
        .expect("state::set");
    }
    let result = iii
        .trigger(TriggerRequest {
            function_id: "state::list".to_string(),
            payload: json!({"scope": scope}),
            action: None,
            timeout_ms: None,
        })
        .await
        .expect("state::list");

    let arr = result.as_array().expect("result should be array");
    assert!(arr.len() >= items.len());

    let mut result_sorted = arr.clone();
    result_sorted.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));

    let mut items_sorted = items.clone();
    items_sorted.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));

    assert_eq!(result_sorted, items_sorted);
}

#[tokio::test]
async fn reactive_state() {
    let key = unique_key("reactive");
    let iii = common::shared_iii();
    delete_state(iii, &key).await;

    let data = json!({"name": "Test", "value": 100});
    let updated_data = json!({"name": "New Test Data", "value": 200});

    iii.trigger(TriggerRequest {
        function_id: "state::set".to_string(),
        payload: json!({"scope": SCOPE, "key": key, "value": data}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("state::set initial");

    let reactive_data: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));
    let reactive_data_clone = reactive_data.clone();

    let fn_ref = iii.register_function(
        RegisterFunctionMessage {
            id: "test.state.rs.updated".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |event: Value| {
            let reactive_data = reactive_data_clone.clone();
            async move {
                if event.get("type").and_then(|v| v.as_str()) == Some("state")
                    && event.get("event_type").and_then(|v| v.as_str()) == Some("state:updated")
                {
                    *reactive_data.lock().await = event.get("new_value").cloned();
                }
                Ok(json!({}))
            }
        },
    );

    let key_clone = key.clone();
    let trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "state".to_string(),
            function_id: fn_ref.id.clone(),
            config: json!({"scope": SCOPE, "key": key_clone}),
        })
        .expect("register state trigger");

    let expected = Some(json!({"name": "New Test Data", "value": 200}));
    for attempt in 0..100 {
        iii.trigger(TriggerRequest {
            function_id: "state::set".to_string(),
            payload: json!({"scope": SCOPE, "key": key, "value": updated_data}),
            action: None,
            timeout_ms: None,
        })
        .await
        .expect("state::set updated");

        let captured = reactive_data.lock().await.clone();
        if captured == expected {
            break;
        }
        if attempt == 99 {
            panic!(
                "reactive state not updated after 100 attempts: got {:?}, expected {:?}",
                captured, expected
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    trigger.unregister();
    fn_ref.unregister();
    delete_state(iii, &key).await;
}
