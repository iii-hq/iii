use serde_json::{Value, json};
use std::sync::Mutex;
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

// Rust does not run destructors on `static` items at process exit, so
// `ContainerAsync::drop` never fires for the container held in `RABBITMQ`.
// Record the container id and force-remove it via the Docker CLI from an
// `atexit` hook so the rabbit container does not leak after the test binary
// exits. Ryuk is an unreliable safety net on Docker Desktop.
static CONTAINER_ID: Mutex<Option<String>> = Mutex::new(None);

extern "C" fn stop_rabbitmq_container() {
    let id = match CONTAINER_ID.lock() {
        Ok(mut guard) => guard.take(),
        Err(_) => return,
    };
    if let Some(id) = id {
        let _ = std::process::Command::new("docker")
            .args(["rm", "-f", &id])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

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

            *CONTAINER_ID
                .lock()
                .expect("rabbit container id mutex poisoned") = Some(container.id().to_string());
            // Safety: `stop_rabbitmq_container` is a plain `extern "C"` fn with no
            // captured state; libc::atexit is sound to call here.
            unsafe {
                libc::atexit(stop_rabbitmq_container);
            }

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
            "name": "rabbitmq",
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

/// Creates a RabbitMQ adapter queue config with a single priority queue named
/// `"{prefix}-priority"`: `concurrency: 1` (so priority ordering is observable),
/// `max_priority`, and `priority_field` reading the per-message priority from
/// `data`.
pub fn rabbitmq_priority_queue_config(
    amqp_url: &str,
    prefix: &str,
    max_priority: u8,
    priority_field: &str,
) -> Value {
    json!({
        "adapter": {
            "name": "rabbitmq",
            "config": {
                "amqp_url": amqp_url
            }
        },
        "queue_configs": {
            format!("{prefix}-priority"): {
                "type": "standard",
                "concurrency": 1,
                "max_retries": 3,
                "backoff_ms": 200,
                "poll_interval_ms": 100,
                "max_priority": max_priority,
                "priority_field": priority_field
            }
        }
    })
}

/// Creates a RabbitMQ adapter config whose **adapter-level** `priority_field` is
/// used to stamp the priority of messages published to topics (pub/sub fanout).
/// No function queues are declared — the subscriber declares its own priority
/// queue via its `queue_config.maxPriority`.
pub fn rabbitmq_priority_topic_config(amqp_url: &str, priority_field: &str) -> Value {
    json!({
        "adapter": {
            "name": "rabbitmq",
            "config": {
                "amqp_url": amqp_url,
                "priority_field": priority_field
            }
        }
    })
}

/// Creates a RabbitMQ adapter queue config with the given AMQP URL and prefix.
/// Defines two queues: "{prefix}-default" (standard) and "{prefix}-payment" (fifo).
pub fn rabbitmq_queue_config(amqp_url: &str, prefix: &str) -> Value {
    json!({
        "adapter": {
            "name": "rabbitmq",
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
