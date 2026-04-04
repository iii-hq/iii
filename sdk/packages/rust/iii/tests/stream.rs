//! Integration tests for stream operations.
//!
//! Requires a running III engine. Set III_URL or use ws://localhost:49134 default.

mod common;

use serde_json::{Value, json};

use iii_sdk::TriggerRequest;

const STREAM_NAME: &str = "test-stream-rs";
const GROUP_ID: &str = "test-group";

fn unique_item(prefix: &str) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    format!("{prefix}-{ts}")
}

#[tokio::test]
async fn stream_set_new_item() {
    let item_id = unique_item("set-new");
    let iii = common::shared_iii();

    let test_data = json!({"name": "Test Item", "value": 42});

    let result = iii
        .trigger(TriggerRequest {
            function_id: "stream::set".to_string(),
            payload: json!({
                "stream_name": STREAM_NAME,
                "group_id": GROUP_ID,
                "item_id": item_id,
                "data": test_data,
            }),
            action: None,
            timeout_ms: None,
        })
        .await
        .expect("stream::set");

    assert_eq!(result["old_value"], Value::Null);
    assert_eq!(result["new_value"], test_data);
}

#[tokio::test]
async fn stream_set_overwrite() {
    let item_id = unique_item("set-overwrite");
    let iii = common::shared_iii();

    let initial_data = json!({"value": 1});
    let updated_data = json!({"value": 2, "updated": true});

    iii.trigger(TriggerRequest {
        function_id: "stream::set".to_string(),
        payload: json!({"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": item_id, "data": initial_data}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("stream::set initial");

    let result = iii
        .trigger(TriggerRequest {
            function_id: "stream::set".to_string(),
            payload: json!({"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": item_id, "data": updated_data}),
            action: None,
            timeout_ms: None,
        })
        .await
        .expect("stream::set overwrite");

    assert_eq!(result["old_value"], initial_data);
    assert_eq!(result["new_value"], updated_data);
}

#[tokio::test]
async fn stream_get_existing_item() {
    let item_id = unique_item("get-existing");
    let iii = common::shared_iii();

    let test_data = json!({"name": "Test", "value": 100});

    iii.trigger(TriggerRequest {
        function_id: "stream::set".to_string(),
        payload: json!({"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": item_id, "data": test_data}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("stream::set");

    let result = iii
        .trigger(TriggerRequest {
            function_id: "stream::get".to_string(),
            payload: json!({"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": item_id}),
            action: None,
            timeout_ms: None,
        })
        .await
        .expect("stream::get");

    assert_eq!(result, test_data);
}

#[tokio::test]
async fn stream_get_non_existent_item() {
    let iii = common::shared_iii();

    let result = iii
        .trigger(TriggerRequest {
            function_id: "stream::get".to_string(),
            payload: json!({"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": "non-existent-item"}),
            action: None,
            timeout_ms: None,
        })
        .await
        .expect("stream::get non-existent");

    assert!(result.is_null());
}

#[tokio::test]
async fn stream_delete_existing_item() {
    let item_id = unique_item("delete-existing");
    let iii = common::shared_iii();

    iii.trigger(TriggerRequest {
        function_id: "stream::set".to_string(),
        payload: json!({"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": item_id, "data": {"test": true}}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("stream::set");

    iii.trigger(TriggerRequest {
        function_id: "stream::delete".to_string(),
        payload: json!({"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": item_id}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("stream::delete");

    let result = iii
        .trigger(TriggerRequest {
            function_id: "stream::get".to_string(),
            payload: json!({"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": item_id}),
            action: None,
            timeout_ms: None,
        })
        .await
        .expect("stream::get after delete");

    assert!(result.is_null());
}

#[tokio::test]
async fn stream_delete_non_existent_item() {
    let iii = common::shared_iii();

    iii.trigger(TriggerRequest {
        function_id: "stream::delete".to_string(),
        payload: json!({"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": "non-existent"}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("stream::delete non-existent should not error");
}

#[tokio::test]
async fn stream_list_items_in_group() {
    let iii = common::shared_iii();

    let group_id = format!(
        "stream-rs-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );

    let items = vec![
        json!({"id": "stream-item1", "value": 1}),
        json!({"id": "stream-item2", "value": 2}),
        json!({"id": "stream-item3", "value": 3}),
    ];

    for item in &items {
        iii.trigger(TriggerRequest {
            function_id: "stream::set".to_string(),
            payload: json!({
                "stream_name": STREAM_NAME,
                "group_id": group_id,
                "item_id": item["id"],
                "data": item,
            }),
            action: None,
            timeout_ms: None,
        })
        .await
        .expect("stream::set");
    }

    let result = iii
        .trigger(TriggerRequest {
            function_id: "stream::list".to_string(),
            payload: json!({"stream_name": STREAM_NAME, "group_id": group_id}),
            action: None,
            timeout_ms: None,
        })
        .await
        .expect("stream::list");

    let arr = result.as_array().expect("result should be array");
    assert!(arr.len() >= items.len());

    let mut result_sorted = arr.clone();
    result_sorted.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));

    let mut items_sorted = items.clone();
    items_sorted.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));

    assert_eq!(result_sorted, items_sorted);
}
