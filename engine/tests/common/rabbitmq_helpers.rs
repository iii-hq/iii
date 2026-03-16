use serde_json::{Value, json};
use testcontainers::ContainerAsync;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::rabbitmq::RabbitMq;
use tokio::sync::OnceCell;

/// Holds a running RabbitMQ container and its AMQP + Management URLs.
/// The container lives for the lifetime of the static OnceCell (entire test process).
pub struct RabbitMqTestContext {
    pub amqp_url: String,
    pub mgmt_url: String,
    _container: ContainerAsync<RabbitMq>,
}

static RABBITMQ: OnceCell<RabbitMqTestContext> = OnceCell::const_new();

/// Returns a shared RabbitMQ test context. The container is started on first call
/// and reused for all subsequent calls within the same test binary.
/// Panics if Docker is not available (by design -- no silent skipping).
pub async fn get_rabbitmq() -> &'static RabbitMqTestContext {
    RABBITMQ
        .get_or_init(|| async {
            let container = RabbitMq::default()
                .start()
                .await
                .expect("Failed to start RabbitMQ container (is Docker running?)");
            let port = container
                .get_host_port_ipv4(5672)
                .await
                .expect("Failed to get RabbitMQ port");
            let mgmt_port = container
                .get_host_port_ipv4(15672)
                .await
                .expect("Failed to get RabbitMQ management port");
            let amqp_url = format!("amqp://guest:guest@127.0.0.1:{}", port);
            let mgmt_url = format!("http://127.0.0.1:{}", mgmt_port);
            RabbitMqTestContext {
                amqp_url,
                mgmt_url,
                _container: container,
            }
        })
        .await
}

/// Generates a short UUID prefix for queue name isolation between tests.
pub fn test_prefix() -> String {
    uuid::Uuid::new_v4().to_string()[..8].to_string()
}

/// Creates a RabbitMQ adapter queue config with a single queue named `"{prefix}-test"`
/// using the given `max_retries` and `backoff_ms`. Useful for focused failure/retry tests.
pub fn rabbitmq_queue_config_custom(
    amqp_url: &str,
    prefix: &str,
    max_retries: u32,
    backoff_ms: u64,
) -> Value {
    json!({
        "adapter": {
            "class": "modules::queue::RabbitMQAdapter",
            "config": {
                "amqp_url": amqp_url
            }
        },
        "queue_configs": {
            format!("{prefix}-test"): {
                "type": "standard",
                "concurrency": 1,
                "max_retries": max_retries,
                "backoff_ms": backoff_ms,
                "poll_interval_ms": 100
            }
        }
    })
}

/// Creates a RabbitMQ adapter queue config with the given AMQP URL and prefix.
/// Defines two queues: "{prefix}-default" (standard) and "{prefix}-payment" (fifo).
pub fn rabbitmq_queue_config(amqp_url: &str, prefix: &str) -> Value {
    json!({
        "adapter": {
            "class": "modules::queue::RabbitMQAdapter",
            "config": {
                "amqp_url": amqp_url
            }
        },
        "queue_configs": {
            format!("{prefix}-default"): {
                "type": "standard",
                "concurrency": 3,
                "max_retries": 2,
                "backoff_ms": 200,
                "poll_interval_ms": 100
            },
            format!("{prefix}-payment"): {
                "type": "fifo",
                "message_group_field": "transaction_id",
                "concurrency": 1,
                "max_retries": 2,
                "backoff_ms": 200,
                "poll_interval_ms": 100
            }
        }
    })
}
