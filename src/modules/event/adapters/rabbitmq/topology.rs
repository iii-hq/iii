// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

#![cfg(feature = "rabbitmq")]

use std::sync::Arc;

use lapin::{Channel, options::*, types::FieldTable};

use super::naming::RabbitNames;

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
}
