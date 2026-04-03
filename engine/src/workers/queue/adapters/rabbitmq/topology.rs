// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

#![cfg(feature = "rabbitmq")]

use std::sync::Arc;

use lapin::{
    Channel,
    options::*,
    types::{AMQPValue, FieldTable},
};

use super::naming::{FnQueueNames, RabbitNames};

pub type Result<T> = std::result::Result<T, TopologyError>;

#[derive(Debug)]
pub enum TopologyError {
    Lapin(lapin::Error),
}

impl std::fmt::Display for TopologyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TopologyError::Lapin(e) => write!(f, "RabbitMQ error: {}", e),
        }
    }
}

impl std::error::Error for TopologyError {}

impl From<lapin::Error> for TopologyError {
    fn from(err: lapin::Error) -> Self {
        TopologyError::Lapin(err)
    }
}

pub struct TopologyManager {
    channel: Arc<Channel>,
}

impl TopologyManager {
    pub fn new(channel: Arc<Channel>) -> Self {
        Self { channel }
    }

    pub async fn setup_topic(&self, topic: &str) -> Result<()> {
        let names = RabbitNames::new(topic);

        self.setup_main_exchange_and_queue(&names).await?;
        self.setup_dlq(&names).await?;

        tracing::debug!(topic = %topic, "RabbitMQ topology setup complete");
        Ok(())
    }

    async fn setup_main_exchange_and_queue(&self, names: &RabbitNames) -> Result<()> {
        self.channel
            .exchange_declare(
                &names.exchange(),
                lapin::ExchangeKind::Topic,
                ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await?;

        self.channel
            .queue_declare(
                &names.queue(),
                QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await?;

        self.channel
            .queue_bind(
                &names.queue(),
                &names.exchange(),
                &names.topic,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        Ok(())
    }

    async fn setup_dlq(&self, names: &RabbitNames) -> Result<()> {
        self.channel
            .queue_declare(
                &names.dlq(),
                QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await?;

        Ok(())
    }

    pub async fn setup_function_queue(&self, queue_name: &str, backoff_ms: u64) -> Result<()> {
        let names = FnQueueNames::new(queue_name);

        // Main exchange + queue with DLX to retry
        self.channel
            .exchange_declare(
                &names.exchange(),
                lapin::ExchangeKind::Direct,
                ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await?;

        // DLX points to DLQ exchange: nack(requeue=false) sends exhausted
        // messages to DLQ automatically. Retry is handled explicitly by
        // the adapter (ack + publish to retry exchange).
        let mut main_queue_args = FieldTable::default();
        main_queue_args.insert(
            "x-dead-letter-exchange".into(),
            AMQPValue::LongString(names.dlq_exchange().into()),
        );

        self.channel
            .queue_declare(
                &names.queue(),
                QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                main_queue_args,
            )
            .await?;

        self.channel
            .queue_bind(
                &names.queue(),
                &names.exchange(),
                queue_name,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        // Retry exchange + queue (TTL -> back to main)
        self.channel
            .exchange_declare(
                &names.retry_exchange(),
                lapin::ExchangeKind::Direct,
                ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await?;

        let mut retry_queue_args = FieldTable::default();
        retry_queue_args.insert(
            "x-message-ttl".into(),
            AMQPValue::LongUInt(backoff_ms as u32),
        );
        retry_queue_args.insert(
            "x-dead-letter-exchange".into(),
            AMQPValue::LongString(names.exchange().into()),
        );
        retry_queue_args.insert(
            "x-dead-letter-routing-key".into(),
            AMQPValue::LongString(queue_name.into()),
        );

        self.channel
            .queue_declare(
                &names.retry_queue(),
                QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                retry_queue_args,
            )
            .await?;

        self.channel
            .queue_bind(
                &names.retry_queue(),
                &names.retry_exchange(),
                queue_name,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        // DLQ exchange + queue
        self.channel
            .exchange_declare(
                &names.dlq_exchange(),
                lapin::ExchangeKind::Direct,
                ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await?;

        self.channel
            .queue_declare(
                &names.dlq(),
                QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await?;

        self.channel
            .queue_bind(
                &names.dlq(),
                &names.dlq_exchange(),
                queue_name,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        tracing::debug!(queue = %queue_name, "Function queue RabbitMQ topology setup complete");
        Ok(())
    }
}
