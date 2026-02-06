// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

#![cfg(feature = "rabbitmq")]

use std::sync::Arc;

use super::publisher::Publisher;
use super::types::Job;

pub type Result<T> = std::result::Result<T, RetryError>;

#[derive(Debug)]
pub enum RetryError {
    Publisher(super::publisher::PublisherError),
}

impl std::fmt::Display for RetryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RetryError::Publisher(e) => write!(f, "Publisher error: {}", e),
        }
    }
}

impl std::error::Error for RetryError {}

impl From<super::publisher::PublisherError> for RetryError {
    fn from(err: super::publisher::PublisherError) -> Self {
        RetryError::Publisher(err)
    }
}

pub struct RetryHandler {
    publisher: Arc<Publisher>,
}

impl RetryHandler {
    pub fn new(publisher: Arc<Publisher>) -> Self {
        Self { publisher }
    }

    pub async fn handle_success(&self, topic: &str, job: &Job) -> Result<()> {
        tracing::debug!(
            job_id = %job.id,
            topic = %topic,
            "Job completed successfully"
        );
        Ok(())
    }

    pub async fn handle_failure(&self, topic: &str, job: &mut Job, error: &str) -> Result<()> {
        job.increment_attempts();

        if job.is_exhausted() {
            self.publisher.publish_to_dlq(topic, job, error).await?;

            tracing::warn!(
                job_id = %job.id,
                topic = %topic,
                attempts = job.attempts_made,
                "Job exhausted retries, moved to DLQ"
            );
        } else {
            self.publisher.requeue(topic, job).await?;

            tracing::debug!(
                job_id = %job.id,
                topic = %topic,
                attempts = job.attempts_made,
                "Job requeued for retry"
            );
        }

        Ok(())
    }
}
