use futures::Future;
use iii::engine::{Engine, EngineTrait, RegisterFunctionRequest};
use iii::function::{FunctionHandler, FunctionResult};
use iii::modules::event::{EventAdapter, SubscriberQueueConfig};
use iii::modules::event::adapters::rabbitmq::{
    RabbitMQAdapter,
    types::{QueueMode, RabbitMQConfig},
};
use iii::protocol::ErrorBody;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;
use uuid::Uuid;

const TEST_AMQP_URL: &str = "amqp://motia:motia123@localhost:5672";

struct TestHandler {
    received: Arc<Mutex<Vec<Value>>>,
    fail_count: Arc<Mutex<HashMap<String, u32>>>,
    max_failures: u32,
}

impl TestHandler {
    fn new(received: Arc<Mutex<Vec<Value>>>) -> Self {
        Self {
            received,
            fail_count: Arc::new(Mutex::new(HashMap::new())),
            max_failures: 0,
        }
    }

    fn with_failures(received: Arc<Mutex<Vec<Value>>>, max_failures: u32) -> Self {
        Self {
            received,
            fail_count: Arc::new(Mutex::new(HashMap::new())),
            max_failures,
        }
    }
}

impl FunctionHandler for TestHandler {
    fn handle_function<'a>(
        &'a self,
        _invocation_id: Option<Uuid>,
        _function_path: String,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send + 'a>> {
        Box::pin(async move {
            if self.max_failures > 0 {
                let job_id = input
                    .get("test")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                let mut counts = self.fail_count.lock().await;
                let count = counts.entry(job_id.clone()).or_insert(0);

                if *count < self.max_failures {
                    *count += 1;
                    return FunctionResult::Failure(ErrorBody {
                        code: "test_error".to_string(),
                        message: format!(
                            "Simulated failure {}/{} for job {}",
                            *count, self.max_failures, job_id
                        ),
                    });
                }
            }

            let mut received = self.received.lock().await;
            received.push(input);
            FunctionResult::Success(None)
        })
    }
}

fn setup_engine() -> Arc<Engine> {
    Arc::new(Engine::new())
}

async fn setup_adapter(engine: Arc<Engine>) -> Arc<dyn EventAdapter> {
    setup_adapter_with_config(engine, RabbitMQConfig::default()).await
}

async fn setup_adapter_with_config(
    engine: Arc<Engine>,
    mut config: RabbitMQConfig,
) -> Arc<dyn EventAdapter> {
    config.amqp_url = TEST_AMQP_URL.to_string();
    let adapter = RabbitMQAdapter::new(config, engine)
        .await
        .expect("Failed to create adapter");
    Arc::new(adapter) as Arc<dyn EventAdapter>
}

async fn cleanup_topic(_adapter: &Arc<dyn EventAdapter>, _topic: &str) {
    sleep(Duration::from_millis(100)).await;
}

fn unique_topic() -> String {
    format!("test-{}", uuid::Uuid::new_v4())
}

#[tokio::test]
async fn test_emit_and_subscribe() {
    let engine = setup_engine();
    let adapter = setup_adapter(Arc::clone(&engine)).await;
    let topic = unique_topic();

    let received = Arc::new(Mutex::new(Vec::new()));
    let handler = Box::new(TestHandler::new(Arc::clone(&received)));

    let function_path = format!("test.handler.{}", topic);
    engine.register_function(
        RegisterFunctionRequest {
            function_path: function_path.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        handler,
    );

    adapter
        .subscribe(&topic, "test-sub-1", &function_path, None, None)
        .await;

    sleep(Duration::from_millis(500)).await;

    let test_data = json!({"message": "Hello RabbitMQ", "count": 1});
    adapter.emit(&topic, test_data.clone()).await;

    sleep(Duration::from_millis(2000)).await;

    let received_msgs = received.lock().await;
    assert_eq!(received_msgs.len(), 1, "Should receive exactly one message");
    assert_eq!(received_msgs[0], test_data, "Message content should match");

    cleanup_topic(&adapter, &topic).await;
}

#[tokio::test]
async fn test_multiple_subscribers() {
    let engine = setup_engine();
    let adapter = setup_adapter(Arc::clone(&engine)).await;
    let topic = unique_topic();

    let received1 = Arc::new(Mutex::new(Vec::new()));
    let received2 = Arc::new(Mutex::new(Vec::new()));

    let handler1 = Box::new(TestHandler::new(Arc::clone(&received1)));
    let handler2 = Box::new(TestHandler::new(Arc::clone(&received2)));

    let function_path1 = format!("test.handler1.{}", topic);
    let function_path2 = format!("test.handler2.{}", topic);

    engine.register_function(
        RegisterFunctionRequest {
            function_path: function_path1.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        handler1,
    );

    engine.register_function(
        RegisterFunctionRequest {
            function_path: function_path2.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        handler2,
    );

    adapter
        .subscribe(&topic, "test-sub-1", &function_path1, None, None)
        .await;
    adapter
        .subscribe(&topic, "test-sub-2", &function_path2, None, None)
        .await;

    sleep(Duration::from_millis(500)).await;

    let test_data = json!({"message": "Broadcast message", "id": 42});
    adapter.emit(&topic, test_data.clone()).await;

    sleep(Duration::from_millis(2000)).await;

    let received1_msgs = received1.lock().await;
    let received2_msgs = received2.lock().await;

    assert_eq!(
        received1_msgs.len() + received2_msgs.len(),
        1,
        "Exactly one subscriber should receive the message"
    );

    cleanup_topic(&adapter, &topic).await;
}

#[tokio::test]
async fn test_subscribe_unsubscribe() {
    let engine = setup_engine();
    let adapter = setup_adapter(Arc::clone(&engine)).await;
    let topic = unique_topic();

    let received = Arc::new(Mutex::new(Vec::new()));
    let handler = Box::new(TestHandler::new(Arc::clone(&received)));

    let function_path = format!("test.handler.{}", topic);
    engine.register_function(
        RegisterFunctionRequest {
            function_path: function_path.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        handler,
    );

    adapter
        .subscribe(&topic, "test-sub-1", &function_path, None, None)
        .await;
    sleep(Duration::from_millis(500)).await;

    adapter.emit(&topic, json!({"message": "First"})).await;
    sleep(Duration::from_millis(1000)).await;

    adapter.unsubscribe(&topic, "test-sub-1").await;
    sleep(Duration::from_millis(500)).await;

    adapter.emit(&topic, json!({"message": "Second"})).await;
    sleep(Duration::from_millis(1000)).await;

    let received_msgs = received.lock().await;
    assert_eq!(
        received_msgs.len(),
        1,
        "Should only receive message before unsubscribe"
    );

    cleanup_topic(&adapter, &topic).await;
}

#[tokio::test]
async fn test_standard_mode_concurrency() {
    let engine = setup_engine();
    let config = RabbitMQConfig {
        amqp_url: TEST_AMQP_URL.to_string(),
        max_attempts: 3,
        prefetch_count: 5,
        queue_mode: QueueMode::Standard,
    };
    let adapter = setup_adapter_with_config(Arc::clone(&engine), config).await;
    let topic = unique_topic();

    let received = Arc::new(Mutex::new(Vec::new()));
    let handler = Box::new(TestHandler::new(Arc::clone(&received)));

    let function_path = format!("test.handler.{}", topic);
    engine.register_function(
        RegisterFunctionRequest {
            function_path: function_path.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        handler,
    );

    adapter
        .subscribe(&topic, "test-sub-1", &function_path, None, None)
        .await;
    sleep(Duration::from_millis(500)).await;

    for i in 0..10 {
        adapter.emit(&topic, json!({"index": i})).await;
    }

    sleep(Duration::from_millis(3000)).await;

    let received_msgs = received.lock().await;
    assert_eq!(received_msgs.len(), 10, "Should receive all messages");

    cleanup_topic(&adapter, &topic).await;
}

#[tokio::test]
async fn test_fifo_mode_ordering() {
    let engine = setup_engine();
    let config = RabbitMQConfig {
        amqp_url: TEST_AMQP_URL.to_string(),
        max_attempts: 3,
        prefetch_count: 1,
        queue_mode: QueueMode::Fifo,
    };
    let adapter = setup_adapter_with_config(Arc::clone(&engine), config).await;
    let topic = unique_topic();

    let received = Arc::new(Mutex::new(Vec::new()));
    let handler = Box::new(TestHandler::new(Arc::clone(&received)));

    let function_path = format!("test.handler.{}", topic);
    engine.register_function(
        RegisterFunctionRequest {
            function_path: function_path.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        handler,
    );

    let queue_config = SubscriberQueueConfig {
        queue_mode: Some("fifo".to_string()),
        concurrency: Some(1),
        max_retries: None,
        visibility_timeout: None,
        delay_seconds: None,
        backoff_type: None,
        backoff_delay_ms: None,
    };

    adapter
        .subscribe(
            &topic,
            "test-sub-1",
            &function_path,
            None,
            Some(queue_config),
        )
        .await;
    sleep(Duration::from_millis(500)).await;

    for i in 0..5 {
        adapter.emit(&topic, json!({"order": i})).await;
    }

    sleep(Duration::from_millis(3000)).await;

    let received_msgs = received.lock().await;
    assert_eq!(received_msgs.len(), 5, "Should receive all messages");

    for (i, msg) in received_msgs.iter().enumerate() {
        assert_eq!(msg["order"], i, "Messages should be in FIFO order");
    }

    cleanup_topic(&adapter, &topic).await;
}

#[tokio::test]
async fn test_retry_on_failure() {
    let engine = setup_engine();
    let config = RabbitMQConfig {
        amqp_url: TEST_AMQP_URL.to_string(),
        max_attempts: 3,
        prefetch_count: 10,
        queue_mode: QueueMode::Standard,
    };
    let adapter = setup_adapter_with_config(Arc::clone(&engine), config).await;
    let topic = unique_topic();

    let received = Arc::new(Mutex::new(Vec::new()));
    let handler = Box::new(TestHandler::with_failures(Arc::clone(&received), 2));

    let function_path = format!("test.handler.{}", topic);
    engine.register_function(
        RegisterFunctionRequest {
            function_path: function_path.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        handler,
    );

    adapter
        .subscribe(&topic, "test-sub-1", &function_path, None, None)
        .await;
    sleep(Duration::from_millis(500)).await;

    adapter.emit(&topic, json!({"test": "retry"})).await;

    sleep(Duration::from_millis(2000)).await;

    let received_msgs = received.lock().await;
    assert_eq!(
        received_msgs.len(),
        1,
        "Should eventually succeed after immediate retries"
    );

    cleanup_topic(&adapter, &topic).await;
}

#[tokio::test]
async fn test_max_attempts_dlq() {
    let engine = setup_engine();
    let config = RabbitMQConfig {
        amqp_url: TEST_AMQP_URL.to_string(),
        max_attempts: 2,
        prefetch_count: 10,
        queue_mode: QueueMode::Standard,
    };
    let adapter = setup_adapter_with_config(Arc::clone(&engine), config).await;
    let topic = unique_topic();

    let received = Arc::new(Mutex::new(Vec::new()));
    let handler = Box::new(TestHandler::with_failures(Arc::clone(&received), 10));

    let function_path = format!("test.handler.{}", topic);
    engine.register_function(
        RegisterFunctionRequest {
            function_path: function_path.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        handler,
    );

    adapter
        .subscribe(&topic, "test-sub-1", &function_path, None, None)
        .await;
    sleep(Duration::from_millis(500)).await;

    let initial_count = adapter.dlq_count(&topic).await.unwrap_or(0);

    adapter.emit(&topic, json!({"test": "dlq"})).await;

    sleep(Duration::from_millis(3000)).await;

    let final_count = adapter
        .dlq_count(&topic)
        .await
        .expect("Failed to get DLQ count");
    assert_eq!(
        final_count - initial_count,
        1,
        "Message should be in DLQ after max attempts"
    );

    cleanup_topic(&adapter, &topic).await;
}

#[tokio::test]
async fn test_dlq_count() {
    let engine = setup_engine();
    let config = RabbitMQConfig {
        amqp_url: TEST_AMQP_URL.to_string(),
        max_attempts: 1,
        prefetch_count: 10,
        queue_mode: QueueMode::Standard,
    };
    let adapter = setup_adapter_with_config(Arc::clone(&engine), config).await;
    let topic = unique_topic();

    let received = Arc::new(Mutex::new(Vec::new()));
    let handler = Box::new(TestHandler::with_failures(Arc::clone(&received), 10));

    let function_path = format!("test.handler.{}", topic);
    engine.register_function(
        RegisterFunctionRequest {
            function_path: function_path.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        handler,
    );

    adapter
        .subscribe(&topic, "test-sub-1", &function_path, None, None)
        .await;
    sleep(Duration::from_millis(500)).await;

    let initial_count = adapter.dlq_count(&topic).await.unwrap_or(0);

    adapter.emit(&topic, json!({"test": "dlq_count_1"})).await;
    adapter.emit(&topic, json!({"test": "dlq_count_2"})).await;

    sleep(Duration::from_millis(2000)).await;

    let final_count = adapter
        .dlq_count(&topic)
        .await
        .expect("Failed to get final DLQ count");
    assert_eq!(
        final_count - initial_count,
        2,
        "Should have 2 messages in DLQ"
    );

    cleanup_topic(&adapter, &topic).await;
}

#[tokio::test]
async fn test_redrive_dlq() {
    let engine = setup_engine();
    let config = RabbitMQConfig {
        amqp_url: TEST_AMQP_URL.to_string(),
        max_attempts: 1,
        prefetch_count: 10,
        queue_mode: QueueMode::Standard,
    };
    let adapter = setup_adapter_with_config(Arc::clone(&engine), config).await;
    let topic = unique_topic();

    let received = Arc::new(Mutex::new(Vec::new()));
    let handler = Box::new(TestHandler::with_failures(Arc::clone(&received), 10));

    let function_path = format!("test.handler.{}", topic);
    engine.register_function(
        RegisterFunctionRequest {
            function_path: function_path.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        handler,
    );

    adapter
        .subscribe(&topic, "test-sub-1", &function_path, None, None)
        .await;
    sleep(Duration::from_millis(500)).await;

    adapter.emit(&topic, json!({"test": "redrive"})).await;
    sleep(Duration::from_millis(2000)).await;

    let dlq_count_before = adapter
        .dlq_count(&topic)
        .await
        .expect("Failed to get DLQ count");
    assert!(dlq_count_before > 0, "Should have messages in DLQ");

    let redriven = adapter
        .redrive_dlq(&topic)
        .await
        .expect("Failed to redrive DLQ");
    assert_eq!(
        redriven, dlq_count_before,
        "Should redrive all DLQ messages"
    );

    sleep(Duration::from_millis(2000)).await;

    let dlq_count_after = adapter
        .dlq_count(&topic)
        .await
        .expect("Failed to get DLQ count after redrive");

    assert!(
        dlq_count_after >= dlq_count_before,
        "Messages should be back in DLQ after failing again"
    );

    cleanup_topic(&adapter, &topic).await;
}

#[tokio::test]
async fn test_connection_failure() {
    let engine = setup_engine();
    let config = RabbitMQConfig {
        amqp_url: "amqp://invalid:invalid@localhost:9999".to_string(),
        max_attempts: 3,
        prefetch_count: 10,
        queue_mode: QueueMode::Standard,
    };

    let result = RabbitMQAdapter::new(config, engine).await;
    assert!(result.is_err(), "Should fail to connect with invalid URL");
}

#[tokio::test]
async fn test_invalid_config() {
    let engine = setup_engine();
    let config = RabbitMQConfig {
        amqp_url: "invalid-url".to_string(),
        max_attempts: 3,
        prefetch_count: 10,
        queue_mode: QueueMode::Standard,
    };

    let result = RabbitMQAdapter::new(config, engine).await;
    assert!(result.is_err(), "Should fail with invalid AMQP URL format");
}
