// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

#![cfg(feature = "rabbitmq")]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use lapin::{Connection, ConnectionProperties, options::*};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use uuid::Uuid;

use iii::{
    engine::Engine,
    function::{Function, FunctionResult},
    modules::{module::Module, queue::QueueCoreModule},
};

use common::queue_helpers::{
    dlq_count, enqueue, register_counting_function, register_failing_function,
    register_failing_function_with_timestamps, register_group_order_recording_function,
    register_order_recording_function, register_panicking_function,
    register_payload_capturing_function, register_slow_function,
};
use common::rabbitmq_helpers::{
    get_rabbitmq, rabbitmq_queue_config, rabbitmq_queue_config_custom, test_prefix,
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn full_roundtrip_enqueue_consume_invoke() {
    let ctx = get_rabbitmq().await;
    let prefix = test_prefix();

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let call_count = Arc::new(AtomicU64::new(0));
    register_counting_function(&engine, "test::rmq_handler", call_count.clone());

    let module = QueueCoreModule::create(
        engine.clone(),
        Some(rabbitmq_queue_config(&ctx.amqp_url, &prefix)),
    )
    .await
    .expect("QueueCoreModule::create should succeed");

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    enqueue(
        &engine,
        &format!("{prefix}-default"),
        "test::rmq_handler",
        json!({"task": "process_order", "order_id": 42}),
    )
    .await
    .expect("Enqueue should succeed");

    // RabbitMQ has real network I/O -- use generous timeout
    tokio::time::sleep(Duration::from_secs(5)).await;

    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "The registered function should have been invoked exactly once"
    );
}

#[tokio::test]
async fn full_roundtrip_fifo_preserves_order() {
    let ctx = get_rabbitmq().await;
    let prefix = test_prefix();

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let invocation_order: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    register_order_recording_function(&engine, "test::rmq_fifo", "seq", invocation_order.clone());

    let module = QueueCoreModule::create(
        engine.clone(),
        Some(rabbitmq_queue_config(&ctx.amqp_url, &prefix)),
    )
    .await
    .expect("QueueCoreModule::create should succeed");

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    let message_count: usize = 5;
    for i in 0..message_count {
        enqueue(
            &engine,
            &format!("{prefix}-payment"),
            "test::rmq_fifo",
            json!({
                "transaction_id": "txn-abc",
                "seq": i,
            }),
        )
        .await
        .expect("Enqueue should succeed");
    }

    tokio::time::sleep(Duration::from_secs(5)).await;

    let recorded = invocation_order.lock().await;
    assert_eq!(
        recorded.len(),
        message_count,
        "All {message_count} messages should have been processed, but got {}",
        recorded.len()
    );

    let expected: Vec<Value> = (0..message_count as i64).map(|i| json!(i)).collect();
    assert_eq!(
        *recorded, expected,
        "FIFO queue should preserve insertion order"
    );
}

#[tokio::test]
async fn retry_behavior_with_rabbitmq() {
    let ctx = get_rabbitmq().await;
    let prefix = test_prefix();

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let call_count = Arc::new(AtomicU64::new(0));
    register_failing_function(&engine, "test::rmq_retry", call_count.clone());

    let module = QueueCoreModule::create(
        engine.clone(),
        Some(rabbitmq_queue_config(&ctx.amqp_url, &prefix)),
    )
    .await
    .expect("QueueCoreModule::create should succeed");

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    enqueue(
        &engine,
        &format!("{prefix}-default"),
        "test::rmq_retry",
        json!({"key": "should_exhaust"}),
    )
    .await
    .expect("Enqueue should succeed");

    // max_retries=2, backoff_ms=200 -> 3 attempts with TTL-based retry through RabbitMQ
    tokio::time::sleep(Duration::from_secs(10)).await;

    let total_calls = call_count.load(Ordering::SeqCst);
    assert!(
        total_calls >= 1 && total_calls <= 3,
        "Expected 1-3 invocations (1 initial + up to 2 retries), got {total_calls}"
    );

    // Confirm no further redeliveries
    let calls_before = total_calls;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let calls_after = call_count.load(Ordering::SeqCst);
    assert_eq!(
        calls_before, calls_after,
        "No further invocations should occur after retry exhaustion, \
         but got {calls_after} (was {calls_before})"
    );
}

#[tokio::test]
async fn exhausted_message_lands_in_dlq() {
    let ctx = get_rabbitmq().await;
    let prefix = test_prefix();

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let call_count = Arc::new(AtomicU64::new(0));
    register_failing_function(&engine, "test::rmq_dlq", call_count.clone());

    let module = QueueCoreModule::create(
        engine.clone(),
        Some(rabbitmq_queue_config(&ctx.amqp_url, &prefix)),
    )
    .await
    .expect("QueueCoreModule::create should succeed");

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    assert_eq!(
        dlq_count(&engine, &format!("{prefix}-default")).await,
        0,
        "DLQ should start empty"
    );

    enqueue(
        &engine,
        &format!("{prefix}-default"),
        "test::rmq_dlq",
        json!({"should_land_in": "dlq"}),
    )
    .await
    .expect("Enqueue should succeed");

    // Wait for retries to exhaust and message to land in DLQ
    tokio::time::sleep(Duration::from_secs(10)).await;

    let count = dlq_count(&engine, &format!("{prefix}-default")).await;
    assert_eq!(
        count, 1,
        "Exactly one message should be in the DLQ after retry exhaustion, got {count}"
    );
}

#[tokio::test]
async fn concurrent_processing() {
    let ctx = get_rabbitmq().await;
    let prefix = test_prefix();

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let timestamps: Arc<Mutex<Vec<std::time::Instant>>> = Arc::new(Mutex::new(Vec::new()));
    register_slow_function(
        &engine,
        "test::rmq_slow",
        Duration::from_millis(200),
        timestamps.clone(),
    );

    let module = QueueCoreModule::create(
        engine.clone(),
        Some(rabbitmq_queue_config(&ctx.amqp_url, &prefix)),
    )
    .await
    .expect("QueueCoreModule::create should succeed");

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    let start = std::time::Instant::now();

    for i in 0..3 {
        enqueue(
            &engine,
            &format!("{prefix}-default"),
            "test::rmq_slow",
            json!({"idx": i}),
        )
        .await
        .expect("Enqueue should succeed");
    }

    tokio::time::sleep(Duration::from_secs(5)).await;

    let ts = timestamps.lock().await;
    assert_eq!(ts.len(), 3, "All 3 messages should have been processed");

    let first_start = *ts.iter().min().unwrap();
    let last_start = *ts.iter().max().unwrap();
    let spread = last_start.duration_since(first_start);

    assert!(
        spread < Duration::from_millis(1000),
        "Concurrent handlers should start close together, but spread was {:?} \
         (start timestamps relative to test start: {:?})",
        spread,
        ts.iter()
            .map(|t| t.duration_since(start))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn multiple_queues_operate_independently() {
    let ctx = get_rabbitmq().await;
    let prefix = test_prefix();

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let default_count = Arc::new(AtomicU64::new(0));
    let payment_count = Arc::new(AtomicU64::new(0));
    register_counting_function(&engine, "test::rmq_default", default_count.clone());
    register_counting_function(&engine, "test::rmq_payment", payment_count.clone());

    let module = QueueCoreModule::create(
        engine.clone(),
        Some(rabbitmq_queue_config(&ctx.amqp_url, &prefix)),
    )
    .await
    .expect("QueueCoreModule::create should succeed");

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    for i in 0..3 {
        enqueue(
            &engine,
            &format!("{prefix}-default"),
            "test::rmq_default",
            json!({"idx": i}),
        )
        .await
        .expect("Enqueue to default should succeed");

        enqueue(
            &engine,
            &format!("{prefix}-payment"),
            "test::rmq_payment",
            json!({"transaction_id": format!("txn-{i}"), "idx": i}),
        )
        .await
        .expect("Enqueue to payment should succeed");
    }

    tokio::time::sleep(Duration::from_secs(5)).await;

    let dc = default_count.load(Ordering::SeqCst);
    let pc = payment_count.load(Ordering::SeqCst);

    assert_eq!(
        dc, 3,
        "Default queue should have processed 3 messages, got {dc}"
    );
    assert_eq!(
        pc, 3,
        "Payment queue should have processed 3 messages, got {pc}"
    );
}

#[tokio::test]
async fn message_id_stamped_as_amqp_property() {
    let ctx = get_rabbitmq().await;
    let prefix = test_prefix();

    // We need to publish a message and inspect it BEFORE the consumer picks it up.
    // Use the RabbitMQ adapter directly: set up topology, publish, then basic_get.
    let conn = Connection::connect(&ctx.amqp_url, ConnectionProperties::default())
        .await
        .expect("Should connect to RabbitMQ");
    let channel = conn.create_channel().await.expect("Should create channel");

    let queue_name = format!("{prefix}-msgid");
    let rmq_queue_name = format!("iii.__fn_queue::{queue_name}.queue");
    let rmq_exchange = format!("iii.__fn_queue::{queue_name}");

    // Create minimal topology: exchange + queue + binding
    channel
        .exchange_declare(
            &rmq_exchange,
            lapin::ExchangeKind::Direct,
            ExchangeDeclareOptions {
                durable: true,
                ..Default::default()
            },
            lapin::types::FieldTable::default(),
        )
        .await
        .expect("exchange_declare");

    channel
        .queue_declare(
            &rmq_queue_name,
            QueueDeclareOptions {
                durable: true,
                ..Default::default()
            },
            lapin::types::FieldTable::default(),
        )
        .await
        .expect("queue_declare");

    channel
        .queue_bind(
            &rmq_queue_name,
            &rmq_exchange,
            &queue_name,
            QueueBindOptions::default(),
            lapin::types::FieldTable::default(),
        )
        .await
        .expect("queue_bind");

    // Publish a message with a known message_id
    let known_message_id = Uuid::new_v4().to_string();
    let payload = json!({
        "function_id": "test::msg_id_check",
        "data": {"check": "message_id"},
    });

    let properties = lapin::BasicProperties::default()
        .with_content_type("application/json".into())
        .with_delivery_mode(2)
        .with_message_id(known_message_id.clone().into());

    channel
        .basic_publish(
            &rmq_exchange,
            &queue_name,
            BasicPublishOptions::default(),
            serde_json::to_vec(&payload).unwrap().as_slice(),
            properties,
        )
        .await
        .expect("basic_publish")
        .await
        .expect("publish confirm");

    // basic_get to read the raw message and verify AMQP message_id property
    let delivery = channel
        .basic_get(&rmq_queue_name, BasicGetOptions { no_ack: true })
        .await
        .expect("basic_get should succeed");

    if let Some(msg) = delivery {
        let amqp_message_id = msg.properties.message_id().as_ref().map(|s| s.to_string());

        assert_eq!(
            amqp_message_id,
            Some(known_message_id),
            "AMQP message_id property should match the publisher-assigned message_id"
        );
    } else {
        panic!("Expected a message in the queue but found none");
    }

    // Cleanup inline topology
    let _ = channel
        .queue_delete(&rmq_queue_name, QueueDeleteOptions::default())
        .await;
    let _ = channel
        .exchange_delete(&rmq_exchange, ExchangeDeleteOptions::default())
        .await;
}

#[tokio::test]
async fn rmq_enqueue_process_ack_preserves_payload() {
    let ctx = get_rabbitmq().await;
    let prefix = test_prefix();

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let captured: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    register_payload_capturing_function(&engine, "test::rmq_payload", captured.clone());

    let module = QueueCoreModule::create(
        engine.clone(),
        Some(rabbitmq_queue_config(&ctx.amqp_url, &prefix)),
    )
    .await
    .expect("QueueCoreModule::create should succeed");

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    let sent_payload = json!({
        "order_id": 42,
        "items": [{"sku": "ABC-123", "qty": 3}],
        "metadata": {"source": "test", "timestamp": 1234567890},
        "nested": {"deep": {"value": true}}
    });

    enqueue(
        &engine,
        &format!("{prefix}-default"),
        "test::rmq_payload",
        sent_payload.clone(),
    )
    .await
    .expect("Enqueue should succeed");

    // RabbitMQ has real network I/O -- use generous timeout
    tokio::time::sleep(Duration::from_secs(5)).await;

    let received = captured.lock().await;
    assert_eq!(
        received.len(),
        1,
        "Exactly one message should have been captured, got {}",
        received.len()
    );
    assert_eq!(
        received[0], sent_payload,
        "Received payload must match sent payload byte-for-byte (serde_json structural equality)"
    );
}

#[tokio::test]
async fn rmq_topology_matches_expected_configuration() {
    let ctx = get_rabbitmq().await;
    let prefix = test_prefix();

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    // Register dummy handlers for both configured queues
    let default_count = Arc::new(AtomicU64::new(0));
    let payment_count = Arc::new(AtomicU64::new(0));
    register_counting_function(&engine, "test::rmq_topo_default", default_count);
    register_counting_function(&engine, "test::rmq_topo_payment", payment_count);

    let module = QueueCoreModule::create(
        engine.clone(),
        Some(rabbitmq_queue_config(&ctx.amqp_url, &prefix)),
    )
    .await
    .expect("QueueCoreModule::create should succeed");

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    // Open a separate connection for verification (do not reuse the adapter's internal channel)
    let verify_conn = Connection::connect(&ctx.amqp_url, ConnectionProperties::default())
        .await
        .expect("Should connect to RabbitMQ for verification");

    let queue_names = [format!("{prefix}-default"), format!("{prefix}-payment")];

    for queue_name in &queue_names {
        // Derive the 6 expected entity names using the naming convention
        let main_exchange = format!("iii.__fn_queue::{queue_name}");
        let main_queue = format!("iii.__fn_queue::{queue_name}.queue");
        let retry_exchange = format!("iii.__fn_queue::{queue_name}::retry");
        let retry_queue = format!("iii.__fn_queue::{queue_name}::retry.queue");
        let dlq_exchange = format!("iii.__fn_queue::{queue_name}::dlq");
        let dlq_queue = format!("iii.__fn_queue::{queue_name}::dlq.queue");

        // Verify all 3 exchanges exist via passive declare
        // Each passive declare needs a fresh channel because a failed declare closes the channel
        for exchange_name in [&main_exchange, &retry_exchange, &dlq_exchange] {
            let ch = verify_conn
                .create_channel()
                .await
                .expect("Should create verification channel");
            ch.exchange_declare(
                exchange_name,
                lapin::ExchangeKind::Direct,
                ExchangeDeclareOptions {
                    passive: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await
            .unwrap_or_else(|e| panic!("Exchange '{exchange_name}' should exist: {e}"));
        }

        // Verify all 3 queues exist via passive declare
        for q_name in [&main_queue, &retry_queue, &dlq_queue] {
            let ch = verify_conn
                .create_channel()
                .await
                .expect("Should create verification channel");
            ch.queue_declare(
                q_name,
                QueueDeclareOptions {
                    passive: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await
            .unwrap_or_else(|e| panic!("Queue '{q_name}' should exist: {e}"));
        }

        // Verify queue arguments via Management HTTP API
        let client = reqwest::Client::new();

        // URL-encode queue names (they contain :: and . characters)
        let encoded_main_queue = main_queue.replace("/", "%2F");
        let encoded_retry_queue = retry_queue.replace("/", "%2F");

        // Verify main queue arguments: x-dead-letter-exchange -> DLQ exchange
        let main_resp: Value = client
            .get(format!(
                "{}/api/queues/%2f/{}",
                ctx.mgmt_url, encoded_main_queue
            ))
            .basic_auth("guest", Some("guest"))
            .send()
            .await
            .expect("Management API request for main queue should succeed")
            .json()
            .await
            .expect("Management API response should be valid JSON");

        let main_args = &main_resp["arguments"];
        assert_eq!(
            main_args["x-dead-letter-exchange"].as_str(),
            Some(dlq_exchange.as_str()),
            "Main queue '{main_queue}' should have x-dead-letter-exchange pointing to DLQ exchange '{dlq_exchange}'"
        );

        // Verify retry queue arguments: x-message-ttl, x-dead-letter-exchange, x-dead-letter-routing-key
        let retry_resp: Value = client
            .get(format!(
                "{}/api/queues/%2f/{}",
                ctx.mgmt_url, encoded_retry_queue
            ))
            .basic_auth("guest", Some("guest"))
            .send()
            .await
            .expect("Management API request for retry queue should succeed")
            .json()
            .await
            .expect("Management API response should be valid JSON");

        let retry_args = &retry_resp["arguments"];
        assert_eq!(
            retry_args["x-message-ttl"].as_u64(),
            Some(200),
            "Retry queue '{retry_queue}' should have x-message-ttl=200 (backoff_ms)"
        );
        assert_eq!(
            retry_args["x-dead-letter-exchange"].as_str(),
            Some(main_exchange.as_str()),
            "Retry queue '{retry_queue}' should have x-dead-letter-exchange pointing to main exchange '{main_exchange}'"
        );
        assert_eq!(
            retry_args["x-dead-letter-routing-key"].as_str(),
            Some(queue_name.as_str()),
            "Retry queue '{retry_queue}' should have x-dead-letter-routing-key='{queue_name}'"
        );
    }
}

#[tokio::test]
async fn rmq_retry_backoff_timing_is_flat() {
    let ctx = get_rabbitmq().await;
    let prefix = test_prefix();
    let backoff_ms: u64 = 500;
    let max_retries: u32 = 3;

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let call_count = Arc::new(AtomicU64::new(0));
    let timestamps: Arc<Mutex<Vec<std::time::Instant>>> = Arc::new(Mutex::new(Vec::new()));
    register_failing_function_with_timestamps(
        &engine,
        "test::rmq_flat_backoff",
        call_count.clone(),
        timestamps.clone(),
    );

    let config = rabbitmq_queue_config_custom(&ctx.amqp_url, &prefix, max_retries, backoff_ms);
    let module = QueueCoreModule::create(engine.clone(), Some(config))
        .await
        .expect("QueueCoreModule::create should succeed");

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    enqueue(
        &engine,
        &format!("{prefix}-test"),
        "test::rmq_flat_backoff",
        json!({"key": "flat_backoff_test"}),
    )
    .await
    .expect("Enqueue should succeed");

    // max_retries=3 means attempts 0,1,2 retry and attempt 3 goes to DLQ = 4 total invocations
    // 3 retry intervals * 500ms + generous overhead
    tokio::time::sleep(Duration::from_secs(15)).await;

    let total_calls = call_count.load(Ordering::SeqCst);
    assert_eq!(
        total_calls, 4,
        "Expected 4 handler invocations (1 initial + 3 retries), got {total_calls}"
    );

    let ts = timestamps.lock().await;
    assert_eq!(
        ts.len(),
        4,
        "Expected 4 timestamps recorded, got {}",
        ts.len()
    );

    // Verify FLAT backoff: all 3 retry intervals should be approximately equal (~500ms)
    // NOT exponentially growing. Use tolerant bounds for CI stability.
    for i in 0..3 {
        let interval = ts[i + 1].duration_since(ts[i]);
        assert!(
            interval >= Duration::from_millis(400) && interval <= Duration::from_millis(3000),
            "Retry interval {i} should be ~{backoff_ms}ms (flat TTL backoff), got {:?}",
            interval
        );
    }
}

#[tokio::test]
async fn rmq_dlq_exhaustion_with_content_verification() {
    let ctx = get_rabbitmq().await;
    let prefix = test_prefix();

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let call_count = Arc::new(AtomicU64::new(0));
    register_failing_function(&engine, "test::rmq_dlq_content", call_count.clone());

    let config = rabbitmq_queue_config_custom(&ctx.amqp_url, &prefix, 2, 300);
    let module = QueueCoreModule::create(engine.clone(), Some(config))
        .await
        .expect("QueueCoreModule::create should succeed");

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    let sent_payload = json!({"order_id": 42, "action": "verify_dlq_content"});

    enqueue(
        &engine,
        &format!("{prefix}-test"),
        "test::rmq_dlq_content",
        sent_payload.clone(),
    )
    .await
    .expect("Enqueue should succeed");

    // max_retries=2 means attempts 0,1 retry and attempt 2 goes to DLQ = 3 total invocations
    tokio::time::sleep(Duration::from_secs(12)).await;

    let total_calls = call_count.load(Ordering::SeqCst);
    assert_eq!(
        total_calls, 3,
        "Expected 3 handler invocations (1 initial + 2 retries), got {total_calls}"
    );

    let count = dlq_count(&engine, &format!("{prefix}-test")).await;
    assert_eq!(
        count, 1,
        "Exactly one message should be in the DLQ after retry exhaustion, got {count}"
    );

    // Open a SEPARATE AMQP connection for DLQ content inspection
    let conn = Connection::connect(&ctx.amqp_url, ConnectionProperties::default())
        .await
        .expect("Should connect to RabbitMQ for DLQ inspection");
    let channel = conn.create_channel().await.expect("Should create channel");

    let dlq_queue_name = format!("iii.__fn_queue::{}::dlq.queue", format!("{prefix}-test"));

    let delivery = channel
        .basic_get(&dlq_queue_name, BasicGetOptions { no_ack: true })
        .await
        .expect("basic_get should succeed");

    let msg = delivery.expect("DLQ should have one message");

    // Verify raw payload matches the original sent data (NOT a structured envelope)
    let payload: Value =
        serde_json::from_slice(&msg.delivery.data).expect("DLQ message should be valid JSON");
    assert_eq!(
        payload, sent_payload,
        "DLQ message body should be the raw original payload"
    );

    // Verify x-death header presence (RabbitMQ dead-letter mechanism proof)
    let headers = msg
        .delivery
        .properties
        .headers()
        .as_ref()
        .expect("DLQ message should have headers");
    let x_death = headers.inner().get("x-death");
    assert!(
        x_death.is_some(),
        "x-death header should be present (proves RabbitMQ dead-letter mechanism was used)"
    );

    // Verify x-attempt header presence (message went through retry path with max_retries=2)
    let x_attempt = headers.inner().get("x-attempt");
    assert!(
        x_attempt.is_some(),
        "x-attempt header should be present (message went through retry path)"
    );
}

#[tokio::test]
async fn rmq_max_retries_zero_sends_directly_to_dlq() {
    let ctx = get_rabbitmq().await;
    let prefix = test_prefix();

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let call_count = Arc::new(AtomicU64::new(0));
    register_failing_function(&engine, "test::rmq_zero_retry", call_count.clone());

    let config = rabbitmq_queue_config_custom(&ctx.amqp_url, &prefix, 0, 200);
    let module = QueueCoreModule::create(engine.clone(), Some(config))
        .await
        .expect("QueueCoreModule::create should succeed");

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    let sent_payload = json!({"zero_retry": true});

    enqueue(
        &engine,
        &format!("{prefix}-test"),
        "test::rmq_zero_retry",
        sent_payload.clone(),
    )
    .await
    .expect("Enqueue should succeed");

    // With max_retries=0, handler called once then message goes to DLQ
    tokio::time::sleep(Duration::from_secs(5)).await;

    let total_calls = call_count.load(Ordering::SeqCst);
    assert_eq!(
        total_calls, 1,
        "Expected exactly 1 handler invocation (no retries with max_retries=0), got {total_calls}"
    );

    let count = dlq_count(&engine, &format!("{prefix}-test")).await;
    assert_eq!(
        count, 1,
        "Exactly one message should be in the DLQ, got {count}"
    );

    // Wait additional time and verify no further retries happen
    let calls_before = call_count.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_secs(3)).await;
    let calls_after = call_count.load(Ordering::SeqCst);
    assert_eq!(
        calls_before, calls_after,
        "No further invocations should occur after DLQ routing, \
         but got {calls_after} (was {calls_before})"
    );

    // Open a separate AMQP connection for DLQ content verification
    let conn = Connection::connect(&ctx.amqp_url, ConnectionProperties::default())
        .await
        .expect("Should connect to RabbitMQ for DLQ inspection");
    let channel = conn.create_channel().await.expect("Should create channel");

    let dlq_queue_name = format!("iii.__fn_queue::{}::dlq.queue", format!("{prefix}-test"));

    let delivery = channel
        .basic_get(&dlq_queue_name, BasicGetOptions { no_ack: true })
        .await
        .expect("basic_get should succeed");

    let msg = delivery.expect("DLQ should have one message");

    // Verify raw payload matches the sent data
    let payload: Value =
        serde_json::from_slice(&msg.delivery.data).expect("DLQ message should be valid JSON");
    assert_eq!(
        payload, sent_payload,
        "DLQ message body should be the raw original payload"
    );

    // Do NOT assert x-attempt header for max_retries=0:
    // The message goes directly to DLQ via nack without being republished
    // to the retry exchange, so no x-attempt header is set.
}

#[tokio::test]
async fn rmq_fifo_multi_group_ordering() {
    let ctx = get_rabbitmq().await;
    let prefix = test_prefix();

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let records: Arc<Mutex<Vec<(String, i64)>>> = Arc::new(Mutex::new(Vec::new()));
    register_group_order_recording_function(
        &engine,
        "test::rmq_fifo_group",
        "group",
        "seq",
        records.clone(),
        Duration::from_millis(0),
    );

    // Build FIFO queue config inline (single queue, not the two-queue helper)
    let config = json!({
        "adapter": {
            "class": "modules::queue::RabbitMQAdapter",
            "config": { "amqp_url": ctx.amqp_url }
        },
        "queue_configs": {
            format!("{prefix}-fifo"): {
                "type": "fifo",
                "message_group_field": "group",
                "concurrency": 1,
                "max_retries": 2,
                "backoff_ms": 200,
                "poll_interval_ms": 100
            }
        }
    });

    let module = QueueCoreModule::create(engine.clone(), Some(config))
        .await
        .expect("QueueCoreModule::create should succeed");

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    // Enqueue 3 groups x 5 messages INTERLEAVED to stress ordering guarantee
    let groups = ["A", "B", "C"];
    let messages_per_group = 5;
    for seq in 0..messages_per_group {
        for group in &groups {
            enqueue(
                &engine,
                &format!("{prefix}-fifo"),
                "test::rmq_fifo_group",
                json!({"group": group, "seq": seq}),
            )
            .await
            .expect("Enqueue should succeed");
        }
    }

    // 15 messages sequential with RabbitMQ I/O overhead
    tokio::time::sleep(Duration::from_secs(10)).await;

    let recs = records.lock().await;
    assert_eq!(
        recs.len(),
        15,
        "All 15 messages should have been processed, got {}",
        recs.len()
    );

    // Verify per-group FIFO ordering: sequence numbers must be strictly increasing
    for group in &groups {
        let group_seqs: Vec<i64> = recs
            .iter()
            .filter(|(g, _)| g == group)
            .map(|(_, s)| *s)
            .collect();
        assert_eq!(
            group_seqs.len(),
            messages_per_group as usize,
            "Group {} should have {} messages, got {}",
            group,
            messages_per_group,
            group_seqs.len()
        );
        for window in group_seqs.windows(2) {
            assert!(
                window[0] < window[1],
                "FIFO violation in group {}: seq {} came before seq {}",
                group,
                window[0],
                window[1]
            );
        }
    }
}

#[tokio::test]
async fn rmq_handler_panic_recovery() {
    let ctx = get_rabbitmq().await;
    let prefix = test_prefix();

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let success_count = Arc::new(AtomicU64::new(0));
    register_panicking_function(&engine, "test::rmq_panic", success_count.clone());

    let config = rabbitmq_queue_config_custom(&ctx.amqp_url, &prefix, 2, 200);
    let module = QueueCoreModule::create(engine.clone(), Some(config))
        .await
        .expect("QueueCoreModule::create should succeed");

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    // Enqueue a panicking message
    enqueue(
        &engine,
        &format!("{prefix}-test"),
        "test::rmq_panic",
        json!({"panic": true}),
    )
    .await
    .expect("Enqueue panicking message should succeed");

    // Let first panic attempt fire
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Enqueue a normal message to prove consumer survived the panic
    enqueue(
        &engine,
        &format!("{prefix}-test"),
        "test::rmq_panic",
        json!({"panic": false}),
    )
    .await
    .expect("Enqueue normal message should succeed");

    // Wait for retry exhaustion (max_retries=2 with 200ms TTL per retry + broker overhead)
    tokio::time::sleep(Duration::from_secs(12)).await;

    // Consumer survived: normal message was processed
    assert!(
        success_count.load(Ordering::SeqCst) >= 1,
        "At least one normal message should have been processed after the panic, got {}",
        success_count.load(Ordering::SeqCst)
    );

    // Panicked message reached DLQ after retry exhaustion
    let dlq = dlq_count(&engine, &format!("{prefix}-test")).await;
    assert!(
        dlq >= 1,
        "Panicked message should land in DLQ after exhausting retries, got {} DLQ entries",
        dlq
    );

    // Verify DLQ content via direct AMQP basic_get (following Phase 8 pattern)
    let conn = Connection::connect(&ctx.amqp_url, ConnectionProperties::default())
        .await
        .expect("Should connect to RabbitMQ for DLQ inspection");
    let channel = conn.create_channel().await.expect("Should create channel");

    let dlq_queue_name = format!("iii.__fn_queue::{}::dlq.queue", format!("{prefix}-test"));

    let delivery = channel
        .basic_get(&dlq_queue_name, BasicGetOptions { no_ack: true })
        .await
        .expect("basic_get should succeed");

    let msg = delivery.expect("DLQ should have at least one message");

    let payload: Value =
        serde_json::from_slice(&msg.delivery.data).expect("DLQ message should be valid JSON");
    assert_eq!(
        payload.get("panic").and_then(|v| v.as_bool()),
        Some(true),
        "DLQ message should contain the original panicking payload with panic: true"
    );
}

// ---------------------------------------------------------------------------
// Fan-out per function tests (real RabbitMQ)
// ---------------------------------------------------------------------------

use common::queue_helpers::enqueue_to_topic;
use iii::trigger::Trigger;

fn register_rmq_capturing_function(
    engine: &Arc<Engine>,
    function_id: &str,
) -> Arc<Mutex<Vec<Value>>> {
    let captured: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let cap = captured.clone();
    let function = Function {
        handler: Arc::new(move |_invocation_id, input, _session| {
            let rec = cap.clone();
            Box::pin(async move {
                rec.lock().await.push(input);
                FunctionResult::Success(Some(json!({ "ok": true })))
            })
        }),
        _function_id: function_id.to_string(),
        _description: Some("rmq capturing handler".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
    };
    engine
        .functions
        .register_function(function_id.to_string(), function);
    captured
}

fn register_rmq_counting_fn(engine: &Arc<Engine>, function_id: &str) -> Arc<AtomicU64> {
    let counter = Arc::new(AtomicU64::new(0));
    let cnt = counter.clone();
    let function = Function {
        handler: Arc::new(move |_invocation_id, _input, _session| {
            let c = cnt.clone();
            Box::pin(async move {
                c.fetch_add(1, Ordering::SeqCst);
                FunctionResult::Success(Some(json!({ "ok": true })))
            })
        }),
        _function_id: function_id.to_string(),
        _description: Some("rmq counting handler".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
    };
    engine
        .functions
        .register_function(function_id.to_string(), function);
    counter
}

#[tokio::test]
async fn rmq_fanout_two_functions_same_topic_both_receive() {
    let ctx = get_rabbitmq().await;
    let prefix = test_prefix();
    let topic = format!("{prefix}-fanout-both");

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let cap_a = register_rmq_capturing_function(&engine, "test::rmq_fan_a");
    let cap_b = register_rmq_capturing_function(&engine, "test::rmq_fan_b");

    let module = QueueCoreModule::create(
        engine.clone(),
        Some(rabbitmq_queue_config(&ctx.amqp_url, &prefix)),
    )
    .await
    .expect("QueueCoreModule::create should succeed");
    module.register_functions(engine.clone());
    module.initialize().await.expect("init should succeed");

    engine
        .trigger_registry
        .register_trigger(Trigger {
            id: format!("trig-{}", Uuid::new_v4()),
            trigger_type: "queue".to_string(),
            function_id: "test::rmq_fan_a".to_string(),
            config: json!({ "topic": &topic }),
            worker_id: None,
            metadata: None,
        })
        .await
        .expect("register trigger A");

    engine
        .trigger_registry
        .register_trigger(Trigger {
            id: format!("trig-{}", Uuid::new_v4()),
            trigger_type: "queue".to_string(),
            function_id: "test::rmq_fan_b".to_string(),
            config: json!({ "topic": &topic }),
            worker_id: None,
            metadata: None,
        })
        .await
        .expect("register trigger B");

    tokio::time::sleep(Duration::from_secs(2)).await;

    enqueue_to_topic(&engine, &topic, json!({"msg": "fanout-test"}))
        .await
        .expect("enqueue should succeed");

    tokio::time::sleep(Duration::from_secs(10)).await;

    let a = cap_a.lock().await;
    let b = cap_b.lock().await;
    assert_eq!(a.len(), 1, "fn_a should receive 1 message, got {}", a.len());
    assert_eq!(b.len(), 1, "fn_b should receive 1 message, got {}", b.len());
    assert_eq!(a[0]["msg"], "fanout-test");
    assert_eq!(b[0]["msg"], "fanout-test");
}

#[tokio::test]
async fn rmq_fanout_replicas_compete_within_function() {
    let ctx = get_rabbitmq().await;
    let prefix = test_prefix();
    let topic = format!("{prefix}-fanout-replica");

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let counter = register_rmq_counting_fn(&engine, "test::rmq_replica_fn");

    let module = QueueCoreModule::create(
        engine.clone(),
        Some(rabbitmq_queue_config(&ctx.amqp_url, &prefix)),
    )
    .await
    .expect("QueueCoreModule::create should succeed");
    module.register_functions(engine.clone());
    module.initialize().await.expect("init should succeed");

    for _ in 0..2 {
        engine
            .trigger_registry
            .register_trigger(Trigger {
                id: format!("trig-{}", Uuid::new_v4()),
                trigger_type: "queue".to_string(),
                function_id: "test::rmq_replica_fn".to_string(),
                config: json!({ "topic": &topic }),
                worker_id: None,
                metadata: None,
            })
            .await
            .expect("register trigger");
    }

    tokio::time::sleep(Duration::from_secs(2)).await;

    enqueue_to_topic(&engine, &topic, json!({"msg": "single"}))
        .await
        .expect("enqueue should succeed");

    tokio::time::sleep(Duration::from_secs(10)).await;

    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "only one replica should process the message"
    );
}

#[tokio::test]
async fn rmq_fanout_dlq_per_function() {
    let ctx = get_rabbitmq().await;
    let prefix = test_prefix();
    let topic = format!("{prefix}-fanout-dlq");

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let cap_success = register_rmq_capturing_function(&engine, "test::rmq_dlq_success");

    let fail_count = Arc::new(AtomicU64::new(0));
    register_failing_function(&engine, "test::rmq_dlq_fail", fail_count.clone());

    let module = QueueCoreModule::create(
        engine.clone(),
        Some(rabbitmq_queue_config(&ctx.amqp_url, &prefix)),
    )
    .await
    .expect("QueueCoreModule::create should succeed");
    module.register_functions(engine.clone());
    module.initialize().await.expect("init should succeed");

    engine
        .trigger_registry
        .register_trigger(Trigger {
            id: format!("trig-{}", Uuid::new_v4()),
            trigger_type: "queue".to_string(),
            function_id: "test::rmq_dlq_success".to_string(),
            config: json!({ "topic": &topic }),
            worker_id: None,
            metadata: None,
        })
        .await
        .expect("register success trigger");

    engine
        .trigger_registry
        .register_trigger(Trigger {
            id: format!("trig-{}", Uuid::new_v4()),
            trigger_type: "queue".to_string(),
            function_id: "test::rmq_dlq_fail".to_string(),
            config: json!({ "topic": &topic }),
            worker_id: None,
            metadata: None,
        })
        .await
        .expect("register fail trigger");

    tokio::time::sleep(Duration::from_secs(2)).await;

    enqueue_to_topic(&engine, &topic, json!({"data": "dlq-test"}))
        .await
        .expect("enqueue should succeed");

    tokio::time::sleep(Duration::from_secs(15)).await;

    let success = cap_success.lock().await;
    assert_eq!(
        success.len(),
        1,
        "success function should receive the message"
    );
    assert_eq!(success[0]["data"], "dlq-test");

    assert!(
        fail_count.load(Ordering::SeqCst) >= 1,
        "failing function should have been invoked at least once"
    );
}
