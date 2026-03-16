// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

pub mod adapters;
mod config;
mod message;
#[allow(clippy::module_inception)]
mod queue;
pub mod registry;
mod subscriber_config;

use serde_json::Value;
use tokio::sync::mpsc;

pub use self::config::FunctionQueueConfig;
pub use self::message::QueueMessage;
pub use self::queue::QueueCoreModule;
pub use self::subscriber_config::SubscriberQueueConfig;

#[allow(clippy::too_many_arguments)]
#[async_trait::async_trait]
pub trait QueueAdapter: Send + Sync + 'static {
    async fn enqueue(
        &self,
        topic: &str,
        data: Value,
        traceparent: Option<String>,
        baggage: Option<String>,
    );
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
    async fn dlq_messages(&self, topic: &str, count: usize) -> anyhow::Result<Vec<Value>> {
        let _ = (topic, count);
        Ok(vec![])
    }

    // Function queue transport methods
    async fn publish_to_function_queue(
        &self,
        _queue_name: &str,
        _function_id: &str,
        _data: Value,
        _message_id: &str,
        _max_retries: u32,
        _backoff_ms: u64,
        _traceparent: Option<String>,
        _baggage: Option<String>,
    ) {
        unimplemented!("publish_to_function_queue not implemented for this adapter")
    }

    /// Set up transport topology for a function queue (exchanges, queues, bindings).
    /// Called once per queue during initialization.
    async fn setup_function_queue(
        &self,
        _queue_name: &str,
        _config: &FunctionQueueConfig,
    ) -> anyhow::Result<()> {
        Ok(()) // no-op by default (schemaless stores like builtin)
    }

    /// Start consuming from a function queue. Returns a receiver that yields QueueMessages.
    /// The adapter spawns an internal task that forwards messages into the channel.
    async fn consume_function_queue(
        &self,
        _queue_name: &str,
        _prefetch: u32,
    ) -> anyhow::Result<mpsc::Receiver<QueueMessage>> {
        unimplemented!("consume_function_queue not implemented for this adapter")
    }

    /// Acknowledge successful processing of a message.
    async fn ack_function_queue(&self, _queue_name: &str, _delivery_id: u64) -> anyhow::Result<()> {
        Ok(()) // no-op by default
    }

    /// Negative-acknowledge a message.
    /// When `attempt < max_retries`, the adapter should route to retry.
    /// When `attempt >= max_retries`, the adapter should route to DLQ.
    async fn nack_function_queue(
        &self,
        _queue_name: &str,
        _delivery_id: u64,
        _attempt: u32,
        _max_retries: u32,
    ) -> anyhow::Result<()> {
        Ok(()) // no-op by default
    }
}
