use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
    sync::{RwLock, Semaphore},
    task::JoinHandle,
    time::{Duration, interval},
};
use uuid::Uuid;

use super::{pubsub::BuiltInPubSubAdapter, queue_kv::QueueKvStore};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub queue: String,
    pub data: Value,
    pub attempts_made: u32,
    pub max_attempts: u32,
    pub backoff_delay_ms: u64,
    pub created_at: u64,
    #[serde(default)]
    pub process_at: Option<u64>,
}

impl Job {
    pub fn new(queue: &str, data: Value, max_attempts: u32, backoff_delay_ms: u64) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            queue: queue.to_string(),
            data,
            attempts_made: 0,
            max_attempts,
            backoff_delay_ms,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            process_at: None,
        }
    }

    pub fn increment_attempts(&mut self) {
        self.attempts_made += 1;
    }

    pub fn is_exhausted(&self) -> bool {
        self.attempts_made >= self.max_attempts
    }

    pub fn calculate_backoff(&self) -> u64 {
        self.backoff_delay_ms * 2_u64.pow(self.attempts_made.saturating_sub(1))
    }
}

#[derive(Debug, Clone)]
pub struct QueueConfig {
    pub max_attempts: u32,
    pub backoff_ms: u64,
    pub concurrency: u32,
    pub poll_interval_ms: u64,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            backoff_ms: 1000,
            concurrency: 10,
            poll_interval_ms: 100,
        }
    }
}

impl QueueConfig {
    pub fn from_value(config: Option<&Value>) -> Self {
        let mut cfg = Self::default();

        if let Some(config) = config {
            if let Some(max_attempts) = config.get("max_attempts").and_then(|v| v.as_u64()) {
                cfg.max_attempts = max_attempts as u32;
            }
            if let Some(backoff_ms) = config.get("backoff_ms").and_then(|v| v.as_u64()) {
                cfg.backoff_ms = backoff_ms;
            }
            if let Some(concurrency) = config.get("concurrency").and_then(|v| v.as_u64()) {
                cfg.concurrency = concurrency as u32;
            }
            if let Some(poll_interval) = config.get("poll_interval_ms").and_then(|v| v.as_u64()) {
                cfg.poll_interval_ms = poll_interval;
            }
        }

        cfg
    }
}

#[derive(Debug, Clone)]
pub struct SubscriptionConfig {
    pub concurrency: Option<u32>,
    pub max_attempts: Option<u32>,
    pub backoff_ms: Option<u64>,
}

impl SubscriptionConfig {
    pub fn effective_concurrency(&self, default: u32) -> u32 {
        self.concurrency.unwrap_or(default)
    }

    pub fn effective_max_attempts(&self, default: u32) -> u32 {
        self.max_attempts.unwrap_or(default)
    }

    pub fn effective_backoff_ms(&self, default: u64) -> u64 {
        self.backoff_ms.unwrap_or(default)
    }
}

pub struct SubscriptionHandle {
    pub id: String,
    pub queue: String,
}

#[async_trait]
pub trait JobHandler: Send + Sync {
    async fn handle(&self, job: &Job) -> Result<(), String>;
}

pub struct BuiltinQueue {
    kv_store: Arc<QueueKvStore>,
    pubsub: Arc<BuiltInPubSubAdapter>,
    config: QueueConfig,
    subscriptions: Arc<RwLock<HashMap<String, JoinHandle<()>>>>,
}

impl BuiltinQueue {
    pub fn new(
        kv_store: Arc<QueueKvStore>,
        pubsub: Arc<BuiltInPubSubAdapter>,
        config: QueueConfig,
    ) -> Self {
        Self {
            kv_store,
            pubsub,
            config,
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn rebuild_from_storage(&self) -> anyhow::Result<()> {
        let has_persisted_state = self.kv_store.has_queue_state("queue:").await;

        if !has_persisted_state {
            tracing::info!("No persisted queue state found, rebuilding from job records");

            let job_keys = self.kv_store.list_job_keys("queue:").await;

            let mut queue_jobs: HashMap<String, Vec<Job>> = HashMap::new();

            for key in job_keys {
                if key.contains(":jobs:")
                    && let Some(job_value) = self.kv_store.get_job(&key).await
                    && let Ok(job) = serde_json::from_value::<Job>(job_value)
                {
                    queue_jobs.entry(job.queue.clone()).or_default().push(job);
                }
            }

            for (queue_name, jobs) in queue_jobs {
                for job in jobs {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64;

                    if let Some(process_at) = job.process_at {
                        if process_at > now {
                            let delayed_key = self.delayed_key(&queue_name);
                            self.kv_store
                                .zadd(&delayed_key, process_at as i64, job.id.clone())
                                .await;
                            tracing::debug!(queue = %queue_name, job_id = %job.id, "Rebuilt job into delayed queue");
                        } else {
                            let waiting_key = self.waiting_key(&queue_name);
                            self.kv_store.lpush(&waiting_key, job.id.clone()).await;
                            tracing::debug!(queue = %queue_name, job_id = %job.id, "Rebuilt expired delayed job into waiting queue");
                        }
                    } else {
                        let waiting_key = self.waiting_key(&queue_name);
                        self.kv_store.lpush(&waiting_key, job.id.clone()).await;
                        tracing::debug!(queue = %queue_name, job_id = %job.id, "Rebuilt job into waiting queue");
                    }
                }
            }

            tracing::info!("Queue state rebuilt from storage");
        } else {
            tracing::info!("Queue state loaded from persisted lists/sorted_sets");

            let queue_names_set: std::collections::HashSet<String> = {
                let job_keys = self.kv_store.list_job_keys("queue:").await;
                job_keys
                    .iter()
                    .filter(|k| k.contains(":jobs:"))
                    .filter_map(|k| {
                        let parts: Vec<&str> = k.split(':').collect();
                        if parts.len() >= 2 {
                            Some(parts[1].to_string())
                        } else {
                            None
                        }
                    })
                    .collect()
            };

            for queue_name in queue_names_set {
                if let Err(e) = self.move_delayed_to_waiting(&queue_name).await {
                    tracing::error!(error = ?e, queue = %queue_name, "Failed to move expired delayed jobs to waiting");
                }
            }
        }

        Ok(())
    }

    fn job_key(&self, queue: &str, job_id: &str) -> String {
        format!("queue:{}:jobs:{}", queue, job_id)
    }

    fn waiting_key(&self, queue: &str) -> String {
        format!("queue:{}:waiting", queue)
    }

    fn active_key(&self, queue: &str) -> String {
        format!("queue:{}:active", queue)
    }

    fn delayed_key(&self, queue: &str) -> String {
        format!("queue:{}:delayed", queue)
    }

    fn dlq_key(&self, queue: &str) -> String {
        format!("queue:{}:dlq", queue)
    }

    pub async fn push(&self, queue: &str, data: Value) -> String {
        let job = Job::new(
            queue,
            data,
            self.config.max_attempts,
            self.config.backoff_ms,
        );
        let job_id = job.id.clone();

        let job_key = self.job_key(queue, &job.id);
        let job_json = serde_json::to_value(&job).expect("Failed to serialize job");
        self.kv_store.set_job(&job_key, job_json).await;

        let waiting_key = self.waiting_key(queue);
        self.kv_store.lpush(&waiting_key, job.id.clone()).await;

        self.pubsub.send_msg(serde_json::json!({
            "topic": format!("queue:job:{}", queue),
            "type": "available",
            "job_id": &job.id,
        }));

        tracing::debug!(queue = %queue, job_id = %job_id, "Job pushed to queue");

        job_id
    }

    pub async fn push_delayed(&self, queue: &str, data: Value, delay_ms: u64) -> String {
        let mut job = Job::new(
            queue,
            data,
            self.config.max_attempts,
            self.config.backoff_ms,
        );
        let job_id = job.id.clone();

        let process_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + delay_ms;

        job.process_at = Some(process_at);

        let job_key = self.job_key(queue, &job.id);
        let job_json = serde_json::to_value(&job).expect("Failed to serialize job");
        self.kv_store.set_job(&job_key, job_json).await;

        let delayed_key = self.delayed_key(queue);
        self.kv_store
            .zadd(&delayed_key, process_at as i64, job.id.clone())
            .await;

        tracing::debug!(queue = %queue, job_id = %job_id, delay_ms = %delay_ms, "Job scheduled with delay");

        job_id
    }

    async fn pop(&self, queue: &str) -> Option<Job> {
        let waiting_key = self.waiting_key(queue);
        let job_id = self.kv_store.rpop(&waiting_key).await?;

        let active_key = self.active_key(queue);
        self.kv_store.lpush(&active_key, job_id.clone()).await;

        let job_key = self.job_key(queue, &job_id);
        let job_value = self.kv_store.get_job(&job_key).await?;
        let job: Job = serde_json::from_value(job_value).ok()?;

        Some(job)
    }

    async fn ack(&self, queue: &str, job_id: &str) -> anyhow::Result<()> {
        let active_key = self.active_key(queue);
        self.kv_store.lrem(&active_key, 1, job_id).await;

        let job_key = self.job_key(queue, job_id);
        self.kv_store.delete_job(&job_key).await;

        tracing::debug!(queue = %queue, job_id = %job_id, "Job acknowledged");
        Ok(())
    }

    async fn nack(&self, queue: &str, job_id: &str, error: &str) -> anyhow::Result<()> {
        let active_key = self.active_key(queue);
        self.kv_store.lrem(&active_key, 1, job_id).await;

        let job_key = self.job_key(queue, job_id);
        let job_value = self.kv_store.get_job(&job_key).await;

        let Some(job_value) = job_value else {
            tracing::warn!(queue = %queue, job_id = %job_id, "Job not found for nack");
            return Ok(());
        };

        let mut job: Job = serde_json::from_value(job_value)?;
        job.increment_attempts();

        if job.is_exhausted() {
            let dlq_key = self.dlq_key(queue);
            let failed_data = serde_json::json!({
                "job": job,
                "error": error,
                "failed_at": SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
            });

            let failed_json = serde_json::to_string(&failed_data)?;
            self.kv_store.lpush(&dlq_key, failed_json).await;
            self.kv_store.delete_job(&job_key).await;

            tracing::warn!(queue = %queue, job_id = %job_id, attempts = job.attempts_made, "Job exhausted, moved to DLQ");
        } else {
            let delay = job.calculate_backoff();
            let delayed_key = self.delayed_key(queue);
            let process_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64
                + delay;

            job.process_at = Some(process_at);

            let job_json = serde_json::to_value(&job)?;
            self.kv_store.set_job(&job_key, job_json).await;
            self.kv_store
                .zadd(&delayed_key, process_at as i64, job_id.to_string())
                .await;

            tracing::debug!(queue = %queue, job_id = %job_id, attempts = job.attempts_made, delay_ms = delay, "Job scheduled for retry");
        }

        Ok(())
    }

    async fn move_delayed_to_waiting(&self, queue: &str) -> anyhow::Result<()> {
        let delayed_key = self.delayed_key(queue);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let ready_jobs = self.kv_store.zrangebyscore(&delayed_key, 0, now).await;

        if !ready_jobs.is_empty() {
            let waiting_key = self.waiting_key(queue);
            for job_id in &ready_jobs {
                self.kv_store.zrem(&delayed_key, job_id).await;
                self.kv_store.lpush(&waiting_key, job_id.clone()).await;
            }

            self.pubsub.send_msg(serde_json::json!({
                "topic": format!("queue:job:{}", queue),
                "type": "available",
            }));
        }

        Ok(())
    }

    pub async fn subscribe(
        &self,
        queue: &str,
        handler: Arc<dyn JobHandler>,
        config: Option<SubscriptionConfig>,
    ) -> SubscriptionHandle {
        let handle_id = Uuid::new_v4().to_string();

        let effective_concurrency = config
            .as_ref()
            .and_then(|c| c.concurrency)
            .unwrap_or(self.config.concurrency);

        let worker = Worker::new(
            Arc::new(self.clone()),
            queue.to_string(),
            handler,
            config,
            effective_concurrency,
        );

        let task_handle = tokio::spawn(async move {
            worker.run().await;
        });

        let mut subs = self.subscriptions.write().await;
        subs.insert(handle_id.clone(), task_handle);

        tracing::info!(queue = %queue, handle_id = %handle_id, "Subscribed to queue");

        SubscriptionHandle {
            id: handle_id,
            queue: queue.to_string(),
        }
    }

    pub async fn unsubscribe(&self, handle: SubscriptionHandle) {
        let mut subs = self.subscriptions.write().await;
        if let Some(task_handle) = subs.remove(&handle.id) {
            task_handle.abort();
            tracing::info!(queue = %handle.queue, handle_id = %handle.id, "Unsubscribed from queue");
        }
    }

    pub async fn dlq_count(&self, queue: &str) -> u64 {
        let dlq_key = self.dlq_key(queue);
        self.kv_store.llen(&dlq_key).await as u64
    }

    pub async fn dlq_redrive(&self, queue: &str) -> u64 {
        let dlq_key = self.dlq_key(queue);
        let mut count = 0;

        while let Some(item) = self.kv_store.rpop(&dlq_key).await {
            if let Ok(failed_data) = serde_json::from_str::<Value>(&item)
                && let Some(job_value) = failed_data.get("job")
                && let Ok(mut job) = serde_json::from_value::<Job>(job_value.clone())
            {
                job.attempts_made = 0;

                let job_key = self.job_key(queue, &job.id);
                let job_json = serde_json::to_value(&job).expect("Failed to serialize job");
                self.kv_store.set_job(&job_key, job_json).await;

                let waiting_key = self.waiting_key(queue);
                self.kv_store.lpush(&waiting_key, job.id).await;

                count += 1;
            }
        }

        if count > 0 {
            self.pubsub.send_msg(serde_json::json!({
                "topic": format!("queue:job:{}", queue),
                "type": "available",
            }));
        }

        tracing::info!(queue = %queue, count = %count, "DLQ jobs redriven");

        count
    }
}

impl Clone for BuiltinQueue {
    fn clone(&self) -> Self {
        Self {
            kv_store: Arc::clone(&self.kv_store),
            pubsub: Arc::clone(&self.pubsub),
            config: self.config.clone(),
            subscriptions: Arc::clone(&self.subscriptions),
        }
    }
}

struct Worker {
    queue_impl: Arc<BuiltinQueue>,
    queue_name: String,
    handler: Arc<dyn JobHandler>,
    semaphore: Arc<Semaphore>,
    poll_interval_ms: u64,
}

impl Worker {
    fn new(
        queue_impl: Arc<BuiltinQueue>,
        queue_name: String,
        handler: Arc<dyn JobHandler>,
        _subscription_config: Option<SubscriptionConfig>,
        concurrency: u32,
    ) -> Self {
        Self {
            queue_impl,
            queue_name,
            handler,
            semaphore: Arc::new(Semaphore::new(concurrency as usize)),
            poll_interval_ms: 100,
        }
    }

    async fn run(self) {
        let mut poll_interval = interval(Duration::from_millis(self.poll_interval_ms));

        loop {
            poll_interval.tick().await;

            if let Err(e) = self
                .queue_impl
                .move_delayed_to_waiting(&self.queue_name)
                .await
            {
                tracing::error!(error = ?e, queue = %self.queue_name, "Failed to move delayed jobs");
            }

            self.process_available_jobs().await;
        }
    }

    async fn process_available_jobs(&self) {
        loop {
            let job = match self.queue_impl.pop(&self.queue_name).await {
                Some(job) => job,
                None => break,
            };

            let job_id = job.id.clone();
            let permit = match self.semaphore.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    let waiting_key = self.queue_impl.waiting_key(&self.queue_name);
                    self.queue_impl.kv_store.lpush(&waiting_key, job.id).await;
                    let active_key = self.queue_impl.active_key(&self.queue_name);
                    self.queue_impl.kv_store.lrem(&active_key, 1, &job_id).await;
                    break;
                }
            };

            let queue_impl = Arc::clone(&self.queue_impl);
            let queue_name = self.queue_name.clone();
            let handler = Arc::clone(&self.handler);

            tokio::spawn(async move {
                let _permit = permit;

                match handler.handle(&job).await {
                    Ok(()) => {
                        if let Err(e) = queue_impl.ack(&queue_name, &job.id).await {
                            tracing::error!(error = ?e, job_id = %job.id, "Failed to ack job");
                        }
                    }
                    Err(error) => {
                        if let Err(e) = queue_impl.nack(&queue_name, &job.id, &error).await {
                            tracing::error!(error = ?e, job_id = %job.id, "Failed to nack job");
                        }
                    }
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::builtins::{kv::BuiltinKvStore, queue_kv::QueueKvStore};

    use super::*;

    #[allow(dead_code)]
    struct TestHandler {
        should_fail: bool,
    }

    fn make_queue_kv(config: Option<Value>) -> Arc<QueueKvStore> {
        let base_kv = Arc::new(BuiltinKvStore::new(config.clone()));
        Arc::new(QueueKvStore::new(base_kv, config))
    }

    #[async_trait]
    impl JobHandler for TestHandler {
        async fn handle(&self, _job: &Job) -> Result<(), String> {
            if self.should_fail {
                Err("Test error".to_string())
            } else {
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn test_push_and_pop() {
        let kv_store = make_queue_kv(None);
        let pubsub = Arc::new(BuiltInPubSubAdapter::new(None));
        let config = QueueConfig::default();
        let queue = BuiltinQueue::new(kv_store, pubsub, config);

        let data = serde_json::json!({"key": "value"});
        let job_id = queue.push("test_queue", data.clone()).await;
        assert!(!job_id.is_empty());

        let popped = queue.pop("test_queue").await;
        assert!(popped.is_some());
        let job = popped.unwrap();
        assert_eq!(job.id, job_id);
        assert_eq!(job.data, data);
        assert_eq!(job.queue, "test_queue");
    }

    #[tokio::test]
    async fn test_ack_removes_job() {
        let kv_store = make_queue_kv(None);
        let pubsub = Arc::new(BuiltInPubSubAdapter::new(None));
        let config = QueueConfig::default();
        let queue = BuiltinQueue::new(kv_store.clone(), pubsub, config);

        let data = serde_json::json!({"key": "value"});
        let job_id = queue.push("test_queue", data).await;

        let job = queue.pop("test_queue").await.unwrap();
        queue.ack("test_queue", &job.id).await.unwrap();

        let job_key = queue.job_key("test_queue", &job_id);
        let result = kv_store.get_job(&job_key).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_nack_with_retry() {
        let kv_store = make_queue_kv(None);
        let pubsub = Arc::new(BuiltInPubSubAdapter::new(None));
        let config = QueueConfig {
            max_attempts: 3,
            backoff_ms: 100,
            ..Default::default()
        };
        let queue = BuiltinQueue::new(kv_store.clone(), pubsub, config);

        let data = serde_json::json!({"key": "value"});
        queue.push("test_queue", data).await;

        let job = queue.pop("test_queue").await.unwrap();
        queue
            .nack("test_queue", &job.id, "Test error")
            .await
            .unwrap();

        let delayed_key = queue.delayed_key("test_queue");
        let delayed_jobs = kv_store.zrangebyscore(&delayed_key, 0, i64::MAX).await;
        assert_eq!(delayed_jobs.len(), 1);
    }

    #[tokio::test]
    async fn test_nack_exhausted_moves_to_dlq() {
        let kv_store = make_queue_kv(None);
        let pubsub = Arc::new(BuiltInPubSubAdapter::new(None));
        let config = QueueConfig {
            max_attempts: 1,
            backoff_ms: 100,
            ..Default::default()
        };
        let queue = BuiltinQueue::new(kv_store.clone(), pubsub, config);

        let data = serde_json::json!({"key": "value"});
        queue.push("test_queue", data).await;

        let job = queue.pop("test_queue").await.unwrap();
        queue
            .nack("test_queue", &job.id, "Test error")
            .await
            .unwrap();

        let dlq_count = queue.dlq_count("test_queue").await;
        assert_eq!(dlq_count, 1);
    }

    #[tokio::test]
    async fn test_push_delayed() {
        let kv_store = make_queue_kv(None);
        let pubsub = Arc::new(BuiltInPubSubAdapter::new(None));
        let config = QueueConfig::default();
        let queue = BuiltinQueue::new(kv_store.clone(), pubsub, config);

        let data = serde_json::json!({"key": "value"});
        let job_id = queue.push_delayed("test_queue", data, 5000).await;

        let popped = queue.pop("test_queue").await;
        assert!(popped.is_none());

        let delayed_key = queue.delayed_key("test_queue");
        let delayed_jobs = kv_store.zrangebyscore(&delayed_key, 0, i64::MAX).await;
        assert_eq!(delayed_jobs.len(), 1);
        assert_eq!(delayed_jobs[0], job_id);
    }

    #[tokio::test]
    async fn test_move_delayed_to_waiting() {
        let kv_store = make_queue_kv(None);
        let pubsub = Arc::new(BuiltInPubSubAdapter::new(None));
        let config = QueueConfig::default();
        let queue = BuiltinQueue::new(kv_store.clone(), pubsub, config);

        let data = serde_json::json!({"key": "value"});
        queue.push_delayed("test_queue", data, 1).await;

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        queue.move_delayed_to_waiting("test_queue").await.unwrap();

        let popped = queue.pop("test_queue").await;
        assert!(popped.is_some());
    }

    #[tokio::test]
    async fn test_dlq_redrive() {
        let kv_store = make_queue_kv(None);
        let pubsub = Arc::new(BuiltInPubSubAdapter::new(None));
        let config = QueueConfig {
            max_attempts: 1,
            backoff_ms: 100,
            ..Default::default()
        };
        let queue = BuiltinQueue::new(kv_store.clone(), pubsub, config);

        let data = serde_json::json!({"key": "value"});
        queue.push("test_queue", data).await;

        let job = queue.pop("test_queue").await.unwrap();
        queue
            .nack("test_queue", &job.id, "Test error")
            .await
            .unwrap();

        assert_eq!(queue.dlq_count("test_queue").await, 1);

        let redriven = queue.dlq_redrive("test_queue").await;
        assert_eq!(redriven, 1);
        assert_eq!(queue.dlq_count("test_queue").await, 0);

        let requeued_job = queue.pop("test_queue").await;
        assert!(requeued_job.is_some());
        assert_eq!(requeued_job.unwrap().attempts_made, 0);
    }

    #[tokio::test]
    async fn test_exponential_backoff_calculation() {
        let mut job = Job::new("test", serde_json::json!({}), 5, 1000);

        assert_eq!(job.calculate_backoff(), 1000);

        job.increment_attempts();
        assert_eq!(job.calculate_backoff(), 1000);

        job.increment_attempts();
        assert_eq!(job.calculate_backoff(), 2000);

        job.increment_attempts();
        assert_eq!(job.calculate_backoff(), 4000);
    }

    #[tokio::test]
    async fn test_subscription_config_overrides() {
        let config = SubscriptionConfig {
            concurrency: Some(20),
            max_attempts: Some(5),
            backoff_ms: Some(2000),
        };

        assert_eq!(config.effective_concurrency(10), 20);
        assert_eq!(config.effective_max_attempts(3), 5);
        assert_eq!(config.effective_backoff_ms(1000), 2000);
    }

    #[tokio::test]
    async fn test_rebuild_from_storage_waiting_jobs() {
        let kv_store = make_queue_kv(None);
        let pubsub = Arc::new(BuiltInPubSubAdapter::new(None));
        let config = QueueConfig::default();
        let queue = BuiltinQueue::new(kv_store.clone(), pubsub.clone(), config.clone());

        let data1 = serde_json::json!({"key": "value1"});
        let data2 = serde_json::json!({"key": "value2"});

        let job1_id = queue.push("test_queue", data1.clone()).await;
        let job2_id = queue.push("test_queue", data2.clone()).await;

        let new_queue = BuiltinQueue::new(kv_store.clone(), pubsub.clone(), config);
        new_queue.rebuild_from_storage().await.unwrap();

        let popped1 = new_queue.pop("test_queue").await;
        assert!(popped1.is_some());
        let job1 = popped1.unwrap();
        assert!(job1.id == job1_id || job1.id == job2_id);

        let popped2 = new_queue.pop("test_queue").await;
        assert!(popped2.is_some());
        let job2 = popped2.unwrap();
        assert!(job2.id == job1_id || job2.id == job2_id);
        assert_ne!(job1.id, job2.id);
    }

    #[tokio::test]
    async fn test_rebuild_from_storage_delayed_jobs() {
        let kv_store = make_queue_kv(None);
        let pubsub = Arc::new(BuiltInPubSubAdapter::new(None));
        let config = QueueConfig::default();
        let queue = BuiltinQueue::new(kv_store.clone(), pubsub.clone(), config.clone());

        let data = serde_json::json!({"key": "value"});
        let job_id = queue.push_delayed("test_queue", data, 5000).await;

        let new_queue = BuiltinQueue::new(kv_store.clone(), pubsub.clone(), config);
        new_queue.rebuild_from_storage().await.unwrap();

        let popped = new_queue.pop("test_queue").await;
        assert!(popped.is_none());

        let delayed_key = new_queue.delayed_key("test_queue");
        let delayed_jobs = kv_store.zrangebyscore(&delayed_key, 0, i64::MAX).await;
        assert_eq!(delayed_jobs.len(), 1);
        assert_eq!(delayed_jobs[0], job_id);
    }

    #[tokio::test]
    async fn test_rebuild_from_storage_with_retry() {
        let kv_store = make_queue_kv(None);
        let pubsub = Arc::new(BuiltInPubSubAdapter::new(None));
        let config = QueueConfig {
            max_attempts: 3,
            backoff_ms: 100,
            ..Default::default()
        };
        let queue = BuiltinQueue::new(kv_store.clone(), pubsub.clone(), config.clone());

        let data = serde_json::json!({"key": "value"});
        queue.push("test_queue", data).await;

        let job = queue.pop("test_queue").await.unwrap();
        queue
            .nack("test_queue", &job.id, "Test error")
            .await
            .unwrap();

        let new_queue = BuiltinQueue::new(kv_store.clone(), pubsub.clone(), config);
        new_queue.rebuild_from_storage().await.unwrap();

        let delayed_key = new_queue.delayed_key("test_queue");
        let delayed_jobs = kv_store.zrangebyscore(&delayed_key, 0, i64::MAX).await;
        assert_eq!(delayed_jobs.len(), 1);
    }

    #[tokio::test]
    async fn test_rebuild_from_storage_expired_delayed_to_waiting() {
        let dir = std::env::temp_dir().join(format!("kv_store_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let config_json = serde_json::json!({
            "store_method": "file_based",
            "file_path": dir.to_string_lossy(),
            "save_interval_ms": 5
        });

        let kv_store = make_queue_kv(Some(config_json.clone()));
        let pubsub = Arc::new(BuiltInPubSubAdapter::new(None));
        let config = QueueConfig {
            max_attempts: 3,
            backoff_ms: 1,
            ..Default::default()
        };
        let queue = BuiltinQueue::new(kv_store.clone(), pubsub.clone(), config.clone());

        let data = serde_json::json!({"key": "value"});
        queue.push("test_queue", data).await;

        let job = queue.pop("test_queue").await.unwrap();
        queue
            .nack("test_queue", &job.id, "Test error")
            .await
            .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        drop(queue);
        drop(kv_store);

        let new_kv_store = make_queue_kv(Some(config_json));
        let new_pubsub = Arc::new(BuiltInPubSubAdapter::new(None));
        let new_queue = BuiltinQueue::new(new_kv_store, new_pubsub, config);
        new_queue.rebuild_from_storage().await.unwrap();

        let popped = new_queue.pop("test_queue").await;
        assert!(popped.is_some());
        assert_eq!(popped.unwrap().id, job.id);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn test_queue_with_full_persistence_and_rebuild() {
        let dir = std::env::temp_dir().join(format!("kv_store_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let config_json = serde_json::json!({
            "store_method": "file_based",
            "file_path": dir.to_string_lossy(),
            "save_interval_ms": 5
        });

        let kv_store = make_queue_kv(Some(config_json.clone()));
        let pubsub = Arc::new(BuiltInPubSubAdapter::new(None));
        let queue_config = QueueConfig::default();
        let queue = BuiltinQueue::new(kv_store.clone(), pubsub.clone(), queue_config.clone());

        let data1 = serde_json::json!({"task": "task1"});
        let data2 = serde_json::json!({"task": "task2"});
        let data3 = serde_json::json!({"task": "task3"});

        let job1_id = queue.push("work_queue", data1).await;
        let job2_id = queue.push("work_queue", data2).await;
        let job3_id = queue.push_delayed("work_queue", data3, 5000).await;

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let lists_file = dir.join("_queue_lists.bin");
        let sorted_sets_file = dir.join("_queue_sorted_sets.bin");
        assert!(lists_file.exists(), "Lists should be persisted");
        assert!(sorted_sets_file.exists(), "Sorted sets should be persisted");

        drop(queue);
        drop(kv_store);

        let new_kv_store = make_queue_kv(Some(config_json));
        let new_pubsub = Arc::new(BuiltInPubSubAdapter::new(None));
        let new_queue = BuiltinQueue::new(new_kv_store.clone(), new_pubsub, queue_config);

        new_queue.rebuild_from_storage().await.unwrap();

        let popped1 = new_queue.pop("work_queue").await;
        assert!(popped1.is_some());
        let job1 = popped1.unwrap();
        assert!(job1.id == job1_id || job1.id == job2_id);

        let popped2 = new_queue.pop("work_queue").await;
        assert!(popped2.is_some());
        let job2 = popped2.unwrap();
        assert!(job2.id == job1_id || job2.id == job2_id);
        assert_ne!(job1.id, job2.id);

        let popped3 = new_queue.pop("work_queue").await;
        assert!(popped3.is_none(), "Delayed job should not be available yet");

        let delayed_key = new_queue.delayed_key("work_queue");
        let delayed_jobs = new_kv_store.zrangebyscore(&delayed_key, 0, i64::MAX).await;
        assert_eq!(delayed_jobs.len(), 1);
        assert_eq!(delayed_jobs[0], job3_id);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
