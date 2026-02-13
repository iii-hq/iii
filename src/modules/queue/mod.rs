// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

pub mod adapters;
mod config;
#[allow(clippy::module_inception)]
mod queue;
pub mod registry;
mod subscriber_config;

use serde_json::Value;

pub use self::queue::QueueCoreModule;
pub use self::subscriber_config::SubscriberQueueConfig;

#[async_trait::async_trait]
pub trait QueueAdapter: Send + Sync + 'static {
    async fn enqueue(&self, topic: &str, data: Value);
    async fn subscribe(
        &self,
        topic: &str,
        id: &str,
        function_id: &str,
        condition_function_id: Option<String>,
        queue_config: Option<SubscriberQueueConfig>,
    );
    async fn unsubscribe(&self, topic: &str, id: &str);
    async fn redrive_dlq(&self, topic: &str) -> anyhow::Result<u64>;
    async fn dlq_count(&self, topic: &str) -> anyhow::Result<u64>;

    async fn list_queues(&self) -> anyhow::Result<Vec<Value>> {
        Err(anyhow::anyhow!("list_queues not supported by this adapter"))
    }

    async fn queue_stats(&self, _topic: &str) -> anyhow::Result<Value> {
        Err(anyhow::anyhow!("queue_stats not supported by this adapter"))
    }

    async fn list_jobs(
        &self,
        _topic: &str,
        _state: &str,
        _offset: usize,
        _limit: usize,
    ) -> anyhow::Result<Vec<Value>> {
        Err(anyhow::anyhow!("list_jobs not supported by this adapter"))
    }

    async fn get_job(&self, _topic: &str, _job_id: &str) -> anyhow::Result<Option<Value>> {
        Err(anyhow::anyhow!("get_job not supported by this adapter"))
    }
}
