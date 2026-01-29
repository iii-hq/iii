// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{
    collections::{HashMap, HashSet},
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

use crate::builtins::pubsub_lite::BuiltInPubSubLite;

use super::queue_kv::QueueKvStore;

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
    #[serde(default)]
    pub group_id: Option<String>,
}

impl Job {
    pub fn new(queue: &str, data: Value, max_attempts: u32, backoff_delay_ms: u64) -> Self {
        Self::new_with_group(queue, data, max_attempts, backoff_delay_ms, None)
    }

    pub fn new_with_group(
        queue: &str,
        data: Value,
        max_attempts: u32,
        backoff_delay_ms: u64,
        group_id: Option<String>,
    ) -> Self {
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
            group_id,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueueMode {
    #[default]
    Concurrent,
    Fifo,
}

#[derive(Debug, Clone)]
pub struct QueueConfig {
    pub max_attempts: u32,
    pub backoff_ms: u64,
    pub concurrency: u32,
    pub poll_interval_ms: u64,
    pub mode: QueueMode,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            backoff_ms: 1000,
            concurrency: 10,
            poll_interval_ms: 100,
            mode: QueueMode::Concurrent,
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
            if let Some(mode_str) = config.get("mode").and_then(|v| v.as_str()) {
                cfg.mode = match mode_str {
                    "fifo" => QueueMode::Fifo,
                    _ => QueueMode::Concurrent,
                };
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
    pub mode: Option<QueueMode>,
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

    pub fn effective_mode(&self, default: QueueMode) -> QueueMode {
        self.mode.unwrap_or(default)
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
    pubsub: Arc<BuiltInPubSubLite>,
    config: QueueConfig,
    subscriptions: Arc<RwLock<HashMap<String, JoinHandle<()>>>>,
}

impl BuiltinQueue {
    pub fn new(
        kv_store: Arc<QueueKvStore>,
        pubsub: Arc<BuiltInPubSubLite>,
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

    pub fn job_key(&self, queue: &str, job_id: &str) -> String {
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

    pub async fn push_fifo(&self, queue: &str, data: Value, group_id: &str) -> String {
        let job = Job::new_with_group(
            queue,
            data,
            self.config.max_attempts,
            self.config.backoff_ms,
            Some(group_id.to_string()),
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
            "group_id": group_id,
        }));

        tracing::debug!(queue = %queue, job_id = %job_id, group_id = %group_id, "FIFO job pushed to queue");

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

    async fn move_to_dlq(&self, queue: &str, job: &Job, error: &str) -> anyhow::Result<()> {
        let active_key = self.active_key(queue);
        self.kv_store.lrem(&active_key, 1, &job.id).await;

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

        let job_key = self.job_key(queue, &job.id);
        self.kv_store.delete_job(&job_key).await;

        tracing::warn!(queue = %queue, job_id = %job.id, attempts = job.attempts_made, "Job exhausted, moved to DLQ");

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
                // Use rpush to add to back (oldest position) so retried jobs
                // are processed before any jobs that were added after them.
                // This maintains FIFO order: rpop takes from back (oldest first).
                self.kv_store.rpush(&waiting_key, job_id.clone()).await;
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

        let effective_mode = config
            .as_ref()
            .and_then(|c| c.mode)
            .unwrap_or(self.config.mode);

        let task_handle = match effective_mode {
            QueueMode::Fifo => {
                let concurrency = config.as_ref().and_then(|c| c.concurrency).unwrap_or(1) as usize;

                if concurrency > 1 {
                    let worker = GroupedFifoWorker::new(
                        Arc::new(self.clone()),
                        queue.to_string(),
                        handler,
                        concurrency,
                    );
                    tokio::spawn(async move {
                        worker.run().await;
                    })
                } else {
                    let worker =
                        FifoWorker::new(Arc::new(self.clone()), queue.to_string(), handler);
                    tokio::spawn(async move {
                        worker.run().await;
                    })
                }
            }
            QueueMode::Concurrent => {
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
                tokio::spawn(async move {
                    worker.run().await;
                })
            }
        };

        let mut subs = self.subscriptions.write().await;
        subs.insert(handle_id.clone(), task_handle);

        tracing::info!(queue = %queue, handle_id = %handle_id, mode = ?effective_mode, "Subscribed to queue");

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

/// Shared helper for processing a FIFO job with inline retry.
/// This function handles the retry loop until the job succeeds or is exhausted.
/// Used by both FifoWorker (single-threaded FIFO) and GroupedFifoWorker (grouped FIFO).
async fn process_job_with_inline_retry(
    queue_impl: &BuiltinQueue,
    queue_name: &str,
    handler: &dyn JobHandler,
    mut job: Job,
    group_id_for_logging: Option<&str>,
) {
    loop {
        match handler.handle(&job).await {
            Ok(()) => {
                if let Err(e) = queue_impl.ack(queue_name, &job.id).await {
                    tracing::error!(error = ?e, job_id = %job.id, "Failed to ack job");
                }
                return;
            }
            Err(error) => {
                job.increment_attempts();

                if job.is_exhausted() {
                    if let Err(e) = queue_impl.move_to_dlq(queue_name, &job, &error).await {
                        tracing::error!(error = ?e, job_id = %job.id, "Failed to move job to DLQ");
                    }
                    return;
                }

                let delay = job.calculate_backoff();
                if let Some(gid) = group_id_for_logging {
                    tracing::debug!(
                        queue = %queue_name,
                        job_id = %job.id,
                        group_id = %gid,
                        attempts = job.attempts_made,
                        delay_ms = delay,
                        "FIFO job scheduled for inline retry"
                    );
                } else {
                    tracing::debug!(
                        queue = %queue_name,
                        job_id = %job.id,
                        attempts = job.attempts_made,
                        delay_ms = delay,
                        "FIFO job scheduled for inline retry"
                    );
                }
                tokio::time::sleep(Duration::from_millis(delay)).await;

                let job_key = queue_impl.job_key(queue_name, &job.id);
                if let Ok(job_json) = serde_json::to_value(&job) {
                    queue_impl.kv_store.set_job(&job_key, job_json).await;
                }
            }
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

struct GroupedFifoWorker {
    queue_impl: Arc<BuiltinQueue>,
    queue_name: String,
    handler: Arc<dyn JobHandler>,
    active_groups: Arc<RwLock<HashSet<String>>>,
    max_concurrent_groups: usize,
    poll_interval_ms: u64,
}

impl GroupedFifoWorker {
    fn new(
        queue_impl: Arc<BuiltinQueue>,
        queue_name: String,
        handler: Arc<dyn JobHandler>,
        max_concurrent_groups: usize,
    ) -> Self {
        Self {
            poll_interval_ms: queue_impl.config.poll_interval_ms,
            queue_impl,
            queue_name,
            handler,
            active_groups: Arc::new(RwLock::new(HashSet::new())),
            max_concurrent_groups,
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

            self.process_available_groups().await;
        }
    }

    async fn process_available_groups(&self) {
        loop {
            let active_count = self.active_groups.read().await.len();
            if active_count >= self.max_concurrent_groups {
                break;
            }

            // Pop a job
            let job = match self.queue_impl.pop(&self.queue_name).await {
                Some(job) => job,
                None => break,
            };

            let group_id = job
                .group_id
                .clone()
                .unwrap_or_else(|| "default".to_string());

            // Atomic check-and-insert to avoid TOCTOU race condition
            {
                let mut active = self.active_groups.write().await;
                if active.contains(&group_id) {
                    drop(active);
                    // Put job back - this group is busy
                    let waiting_key = self.queue_impl.waiting_key(&self.queue_name);
                    self.queue_impl
                        .kv_store
                        .lpush(&waiting_key, job.id.clone())
                        .await;
                    let active_key = self.queue_impl.active_key(&self.queue_name);
                    self.queue_impl.kv_store.lrem(&active_key, 1, &job.id).await;
                    continue;
                }
                active.insert(group_id.clone());
            }

            // Process job in background
            let queue_impl = Arc::clone(&self.queue_impl);
            let queue_name = self.queue_name.clone();
            let handler = Arc::clone(&self.handler);
            let active_groups = Arc::clone(&self.active_groups);

            tokio::spawn(async move {
                process_job_with_inline_retry(
                    &queue_impl,
                    &queue_name,
                    handler.as_ref(),
                    job,
                    Some(&group_id),
                )
                .await;

                active_groups.write().await.remove(&group_id);
            });
        }
    }
}

struct FifoWorker {
    queue_impl: Arc<BuiltinQueue>,
    queue_name: String,
    handler: Arc<dyn JobHandler>,
    poll_interval_ms: u64,
}

impl FifoWorker {
    fn new(
        queue_impl: Arc<BuiltinQueue>,
        queue_name: String,
        handler: Arc<dyn JobHandler>,
    ) -> Self {
        Self {
            poll_interval_ms: queue_impl.config.poll_interval_ms,
            queue_impl,
            queue_name,
            handler,
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

            // Process one job at a time, wait for completion (including retries)
            self.process_next_job().await;
        }
    }

    async fn process_next_job(&self) {
        let job = match self.queue_impl.pop(&self.queue_name).await {
            Some(job) => job,
            None => return,
        };

        process_job_with_inline_retry(
            &self.queue_impl,
            &self.queue_name,
            self.handler.as_ref(),
            job,
            None,
        )
        .await;
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
        let pubsub = Arc::new(BuiltInPubSubLite::new(None));
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
        let pubsub = Arc::new(BuiltInPubSubLite::new(None));
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
        let pubsub = Arc::new(BuiltInPubSubLite::new(None));
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
        let pubsub = Arc::new(BuiltInPubSubLite::new(None));
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
        let pubsub = Arc::new(BuiltInPubSubLite::new(None));
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
        let pubsub = Arc::new(BuiltInPubSubLite::new(None));
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
        let pubsub = Arc::new(BuiltInPubSubLite::new(None));
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
            mode: None,
        };

        assert_eq!(config.effective_concurrency(10), 20);
        assert_eq!(config.effective_max_attempts(3), 5);
        assert_eq!(config.effective_backoff_ms(1000), 2000);
    }

    #[tokio::test]
    async fn test_rebuild_from_storage_waiting_jobs() {
        let kv_store = make_queue_kv(None);
        let pubsub = Arc::new(BuiltInPubSubLite::new(None));
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
        let pubsub = Arc::new(BuiltInPubSubLite::new(None));
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
        let pubsub = Arc::new(BuiltInPubSubLite::new(None));
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
        let pubsub = Arc::new(BuiltInPubSubLite::new(None));
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
        let new_pubsub = Arc::new(BuiltInPubSubLite::new(None));
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
        let pubsub = Arc::new(BuiltInPubSubLite::new(None));
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
        let new_pubsub = Arc::new(BuiltInPubSubLite::new(None));
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

    #[tokio::test]
    async fn test_queue_config_fifo_mode() {
        let config_json = serde_json::json!({
            "mode": "fifo"
        });
        let config = QueueConfig::from_value(Some(&config_json));
        assert_eq!(config.mode, QueueMode::Fifo);
    }

    #[tokio::test]
    async fn test_queue_config_concurrent_mode_default() {
        let config = QueueConfig::default();
        assert_eq!(config.mode, QueueMode::Concurrent);
    }

    #[tokio::test]
    async fn test_queue_config_concurrent_mode_explicit() {
        let config_json = serde_json::json!({
            "mode": "concurrent"
        });
        let config = QueueConfig::from_value(Some(&config_json));
        assert_eq!(config.mode, QueueMode::Concurrent);
    }

    #[tokio::test]
    async fn test_subscription_config_fifo_mode() {
        let config = SubscriptionConfig {
            concurrency: None,
            max_attempts: None,
            backoff_ms: None,
            mode: Some(QueueMode::Fifo),
        };
        assert_eq!(
            config.effective_mode(QueueMode::Concurrent),
            QueueMode::Fifo
        );
    }

    #[tokio::test]
    async fn test_subscription_config_mode_defaults_to_global() {
        let config = SubscriptionConfig {
            concurrency: None,
            max_attempts: None,
            backoff_ms: None,
            mode: None,
        };
        assert_eq!(config.effective_mode(QueueMode::Fifo), QueueMode::Fifo);
        assert_eq!(
            config.effective_mode(QueueMode::Concurrent),
            QueueMode::Concurrent
        );
    }

    #[tokio::test]
    async fn test_job_with_group_id() {
        let job = Job::new_with_group(
            "test_queue",
            serde_json::json!({"key": "value"}),
            3,
            1000,
            Some("user-123".to_string()),
        );
        assert_eq!(job.group_id, Some("user-123".to_string()));
    }

    #[tokio::test]
    async fn test_job_without_group_id() {
        let job = Job::new("test_queue", serde_json::json!({}), 3, 1000);
        assert_eq!(job.group_id, None);
    }

    #[tokio::test]
    async fn test_push_fifo_with_group() {
        let kv_store = make_queue_kv(None);
        let pubsub = Arc::new(BuiltInPubSubLite::new(None));
        let config = QueueConfig::default();
        let queue = BuiltinQueue::new(kv_store.clone(), pubsub, config);

        let job_id = queue
            .push_fifo("test_queue", serde_json::json!({"order": 1}), "group-a")
            .await;

        let job_key = queue.job_key("test_queue", &job_id);
        let job_value = kv_store.get_job(&job_key).await.unwrap();
        let job: Job = serde_json::from_value(job_value).unwrap();

        assert_eq!(job.group_id, Some("group-a".to_string()));
    }

    #[tokio::test]
    async fn test_fifo_mode_parallel_groups() {
        let kv_store = make_queue_kv(None);
        let pubsub = Arc::new(BuiltInPubSubLite::new(None));
        let config = QueueConfig {
            mode: QueueMode::Fifo,
            ..Default::default()
        };
        let queue = Arc::new(BuiltinQueue::new(kv_store.clone(), pubsub, config));

        // Push jobs to different groups
        let group_a_1 = queue
            .push_fifo(
                "fifo_queue",
                serde_json::json!({"group": "a", "order": 1}),
                "group-a",
            )
            .await;
        let group_b_1 = queue
            .push_fifo(
                "fifo_queue",
                serde_json::json!({"group": "b", "order": 1}),
                "group-b",
            )
            .await;
        let group_a_2 = queue
            .push_fifo(
                "fifo_queue",
                serde_json::json!({"group": "a", "order": 2}),
                "group-a",
            )
            .await;
        let group_b_2 = queue
            .push_fifo(
                "fifo_queue",
                serde_json::json!({"group": "b", "order": 2}),
                "group-b",
            )
            .await;

        let processed_order = Arc::new(RwLock::new(Vec::<(String, String)>::new())); // (group, job_id)
        let processed_clone = Arc::clone(&processed_order);

        struct GroupOrderTracker {
            processed: Arc<RwLock<Vec<(String, String)>>>,
        }

        #[async_trait]
        impl JobHandler for GroupOrderTracker {
            async fn handle(&self, job: &Job) -> Result<(), String> {
                tokio::time::sleep(Duration::from_millis(20)).await;
                let group = job.group_id.clone().unwrap_or_default();
                self.processed.write().await.push((group, job.id.clone()));
                Ok(())
            }
        }

        let handler = Arc::new(GroupOrderTracker {
            processed: processed_clone,
        });

        let subscription_config = SubscriptionConfig {
            concurrency: Some(2), // Allow 2 groups to process in parallel
            max_attempts: None,
            backoff_ms: None,
            mode: Some(QueueMode::Fifo),
        };

        let handle = queue
            .subscribe("fifo_queue", handler, Some(subscription_config))
            .await;

        tokio::time::sleep(Duration::from_millis(300)).await;

        queue.unsubscribe(handle).await;

        let order = processed_order.read().await;

        // Verify within each group, jobs are in order
        let group_a: Vec<_> = order
            .iter()
            .filter(|(g, _)| g == "group-a")
            .map(|(_, id)| id.clone())
            .collect();
        let group_b: Vec<_> = order
            .iter()
            .filter(|(g, _)| g == "group-b")
            .map(|(_, id)| id.clone())
            .collect();

        assert_eq!(group_a.len(), 2);
        assert_eq!(group_a[0], group_a_1);
        assert_eq!(group_a[1], group_a_2);

        assert_eq!(group_b.len(), 2);
        assert_eq!(group_b[0], group_b_1);
        assert_eq!(group_b[1], group_b_2);
    }

    #[tokio::test]
    async fn test_fifo_mode_processes_in_order() {
        let kv_store = make_queue_kv(None);
        let pubsub = Arc::new(BuiltInPubSubLite::new(None));
        let config = QueueConfig {
            mode: QueueMode::Fifo,
            poll_interval_ms: 10,
            ..Default::default()
        };
        let queue = Arc::new(BuiltinQueue::new(kv_store.clone(), pubsub, config));

        // Push jobs
        let job1_id = queue
            .push("fifo_queue", serde_json::json!({"order": 1}))
            .await;
        let job2_id = queue
            .push("fifo_queue", serde_json::json!({"order": 2}))
            .await;
        let job3_id = queue
            .push("fifo_queue", serde_json::json!({"order": 3}))
            .await;

        // Track processing order
        let processed_order = Arc::new(RwLock::new(Vec::<String>::new()));
        let processed_clone = Arc::clone(&processed_order);

        struct OrderTracker {
            processed: Arc<RwLock<Vec<String>>>,
        }

        #[async_trait]
        impl JobHandler for OrderTracker {
            async fn handle(&self, job: &Job) -> Result<(), String> {
                // Variable delay based on job order to expose concurrent processing
                let order = job.data.get("order").and_then(|v| v.as_u64()).unwrap_or(0);
                // First job takes longest, so if concurrent, order would be 3,2,1
                let delay = match order {
                    1 => 50,
                    2 => 30,
                    3 => 10,
                    _ => 10,
                };
                tokio::time::sleep(Duration::from_millis(delay)).await;
                self.processed.write().await.push(job.id.clone());
                Ok(())
            }
        }

        let handler = Arc::new(OrderTracker {
            processed: processed_clone,
        });

        let subscription_config = SubscriptionConfig {
            concurrency: None,
            max_attempts: None,
            backoff_ms: None,
            mode: Some(QueueMode::Fifo),
        };

        let handle = queue
            .subscribe("fifo_queue", handler, Some(subscription_config))
            .await;

        // Wait for processing
        tokio::time::sleep(Duration::from_millis(300)).await;

        queue.unsubscribe(handle).await;

        let order = processed_order.read().await;
        assert_eq!(
            order.len(),
            3,
            "Expected 3 jobs to be processed, got {}",
            order.len()
        );
        assert_eq!(order[0], job1_id, "First job should be job1");
        assert_eq!(order[1], job2_id, "Second job should be job2");
        assert_eq!(order[2], job3_id, "Third job should be job3");
    }

    #[tokio::test]
    async fn test_fifo_mode_retry_blocks_queue() {
        let kv_store = make_queue_kv(None);
        let pubsub = Arc::new(BuiltInPubSubAdapter::new(None));
        let config = QueueConfig {
            mode: QueueMode::Fifo,
            max_attempts: 2,
            backoff_ms: 10, // Short backoff for test
            poll_interval_ms: 10,
            ..Default::default()
        };
        let queue = Arc::new(BuiltinQueue::new(kv_store.clone(), pubsub, config));

        // Push jobs
        let job1_id = queue
            .push("fifo_queue", serde_json::json!({"order": 1}))
            .await;
        let job2_id = queue
            .push("fifo_queue", serde_json::json!({"order": 2}))
            .await;

        let attempt_count = Arc::new(RwLock::new(0));
        let processed_ids = Arc::new(RwLock::new(Vec::<String>::new()));
        let attempt_clone = Arc::clone(&attempt_count);
        let processed_clone = Arc::clone(&processed_ids);

        struct RetryTracker {
            attempts: Arc<RwLock<i32>>,
            processed: Arc<RwLock<Vec<String>>>,
            fail_first_n: i32,
        }

        #[async_trait]
        impl JobHandler for RetryTracker {
            async fn handle(&self, job: &Job) -> Result<(), String> {
                let mut count = self.attempts.write().await;
                *count += 1;

                if *count <= self.fail_first_n {
                    return Err("Intentional failure".to_string());
                }

                self.processed.write().await.push(job.id.clone());
                Ok(())
            }
        }

        let handler = Arc::new(RetryTracker {
            attempts: attempt_clone,
            processed: processed_clone,
            fail_first_n: 1, // Fail job1 once, then succeed
        });

        let subscription_config = SubscriptionConfig {
            concurrency: None,
            max_attempts: None,
            backoff_ms: None,
            mode: Some(QueueMode::Fifo),
        };

        let handle = queue
            .subscribe("fifo_queue", handler, Some(subscription_config))
            .await;

        // Wait for retry and processing
        tokio::time::sleep(Duration::from_millis(500)).await;

        queue.unsubscribe(handle).await;

        // Both jobs should be processed, job1 first (after retry), then job2
        let processed = processed_ids.read().await;
        assert_eq!(processed.len(), 2);
        assert_eq!(processed[0], job1_id);
        assert_eq!(processed[1], job2_id);
    }

    #[tokio::test]
    async fn test_grouped_fifo_no_duplicate_group_processing() {
        let kv_store = make_queue_kv(None);
        let pubsub = Arc::new(BuiltInPubSubAdapter::new(None));
        let config = QueueConfig {
            mode: QueueMode::Fifo,
            poll_interval_ms: 1, // Fast polling to increase race likelihood
            ..Default::default()
        };
        let queue = Arc::new(BuiltinQueue::new(kv_store.clone(), pubsub, config));

        // Push many jobs to the same group
        for i in 0..20 {
            queue
                .push_fifo("race_queue", serde_json::json!({"order": i}), "same-group")
                .await;
        }

        let concurrent_count = Arc::new(RwLock::new(0_i32));
        let max_concurrent = Arc::new(RwLock::new(0_i32));
        let concurrent_clone = Arc::clone(&concurrent_count);
        let max_clone = Arc::clone(&max_concurrent);

        struct ConcurrencyTracker {
            concurrent: Arc<RwLock<i32>>,
            max_concurrent: Arc<RwLock<i32>>,
        }

        #[async_trait]
        impl JobHandler for ConcurrencyTracker {
            async fn handle(&self, _job: &Job) -> Result<(), String> {
                {
                    let mut count = self.concurrent.write().await;
                    *count += 1;
                    let mut max = self.max_concurrent.write().await;
                    if *count > *max {
                        *max = *count;
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
                {
                    let mut count = self.concurrent.write().await;
                    *count -= 1;
                }
                Ok(())
            }
        }

        let handler = Arc::new(ConcurrencyTracker {
            concurrent: concurrent_clone,
            max_concurrent: max_clone,
        });

        let subscription_config = SubscriptionConfig {
            concurrency: Some(5), // Allow multiple groups, but same group should serialize
            max_attempts: None,
            backoff_ms: None,
            mode: Some(QueueMode::Fifo),
        };

        let handle = queue
            .subscribe("race_queue", handler, Some(subscription_config))
            .await;

        tokio::time::sleep(Duration::from_millis(500)).await;
        queue.unsubscribe(handle).await;

        // For a single group, max concurrent should be 1
        let max = *max_concurrent.read().await;
        assert_eq!(
            max, 1,
            "Same group should never have concurrent processing, got {}",
            max
        );
    }
}
