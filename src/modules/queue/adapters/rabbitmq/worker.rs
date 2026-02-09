// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

#![cfg(feature = "rabbitmq")]

use std::sync::Arc;

use futures::StreamExt;
use lapin::{Channel, message::Delivery, options::*};
use tokio::sync::Semaphore;

use crate::engine::{Engine, EngineTrait};

use super::consumer::JobParser;
use super::naming::RabbitNames;
use super::retry::RetryHandler;
use super::types::{Job, QueueMode};

pub struct Worker {
    channel: Arc<Channel>,
    retry_handler: Arc<RetryHandler>,
    engine: Arc<Engine>,
    semaphore: Option<Arc<Semaphore>>,
    queue_mode: QueueMode,
}

impl Worker {
    pub fn new(
        channel: Arc<Channel>,
        retry_handler: Arc<RetryHandler>,
        engine: Arc<Engine>,
        queue_mode: QueueMode,
        prefetch_count: u16,
    ) -> Self {
        let semaphore = match queue_mode {
            QueueMode::Fifo => None,
            QueueMode::Standard => Some(Arc::new(Semaphore::new(prefetch_count as usize))),
        };

        Self {
            channel,
            retry_handler,
            engine,
            semaphore,
            queue_mode,
        }
    }

    pub async fn run(
        self: Arc<Self>,
        topic: String,
        function_id: String,
        condition_function_id: Option<String>,
        consumer_tag: String,
    ) {
        let names = RabbitNames::new(&topic);

        let mut consumer = match self
            .channel
            .basic_consume(
                &names.queue(),
                &consumer_tag,
                BasicConsumeOptions::default(),
                lapin::types::FieldTable::default(),
            )
            .await
        {
            Ok(consumer) => consumer,
            Err(e) => {
                tracing::error!(
                    topic = %topic,
                    error = ?e,
                    "Failed to create consumer"
                );
                return;
            }
        };

        while let Some(delivery_result) = consumer.next().await {
            match delivery_result {
                Ok(delivery) => {
                    let worker = Arc::clone(&self);
                    let topic_clone = topic.clone();
                    let function_id_clone = function_id.clone();
                    let condition_function_id_clone = condition_function_id.clone();

                    match self.queue_mode {
                        QueueMode::Fifo => {
                            if let Err(e) = worker
                                .process_delivery(
                                    delivery,
                                    &topic_clone,
                                    &function_id_clone,
                                    condition_function_id_clone.as_deref(),
                                )
                                .await
                            {
                                tracing::error!(
                                    topic = %topic_clone,
                                    error = ?e,
                                    "Failed to process delivery"
                                );
                            }
                        }
                        QueueMode::Standard => {
                            let semaphore = self.semaphore.as_ref().map(Arc::clone);
                            tokio::spawn(async move {
                                let _permit = if let Some(ref sem) = semaphore {
                                    Some(sem.acquire().await.unwrap())
                                } else {
                                    None
                                };

                                if let Err(e) = worker
                                    .process_delivery(
                                        delivery,
                                        &topic_clone,
                                        &function_id_clone,
                                        condition_function_id_clone.as_deref(),
                                    )
                                    .await
                                {
                                    tracing::error!(
                                        topic = %topic_clone,
                                        error = ?e,
                                        "Failed to process delivery"
                                    );
                                }
                            });
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(
                        topic = %topic,
                        error = ?e,
                        "Error receiving delivery"
                    );
                }
            }
        }

        tracing::warn!(topic = %topic, "Consumer stream ended");
    }

    async fn process_delivery(
        &self,
        delivery: Delivery,
        topic: &str,
        function_id: &str,
        condition_function_id: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut job = JobParser::parse_from_delivery(&delivery)?;

        match self
            .process_job(&job, function_id, condition_function_id)
            .await
        {
            Ok(_) => {
                delivery
                    .ack(BasicAckOptions::default())
                    .await
                    .map_err(|e| format!("Failed to ack message: {}", e))?;

                self.retry_handler
                    .handle_success(topic, &job)
                    .await
                    .map_err(|e| format!("Failed to handle success: {}", e))?;
            }
            Err(e) => {
                delivery
                    .nack(BasicNackOptions {
                        requeue: false,
                        ..Default::default()
                    })
                    .await
                    .map_err(|e| format!("Failed to nack message: {}", e))?;

                self.retry_handler
                    .handle_failure(topic, &mut job, &format!("{:?}", e))
                    .await
                    .map_err(|e| format!("Failed to handle failure: {}", e))?;
            }
        }

        Ok(())
    }

    async fn process_job(
        &self,
        job: &Job,
        function_id: &str,
        condition_function_id: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let engine = Arc::clone(&self.engine);
        let data = job.data.clone();

        if let Some(condition_path) = condition_function_id {
            match engine.call(condition_path, data.clone()).await {
                Ok(Some(result)) => {
                    if let Some(passed) = result.as_bool()
                        && !passed
                    {
                        tracing::debug!(
                            function_id = %function_id,
                            "Condition check failed, skipping handler"
                        );
                        return Ok(());
                    }
                }
                Ok(None) => {
                    tracing::warn!(
                        condition_function_id = %condition_path,
                        "Condition function returned no result"
                    );
                }
                Err(err) => {
                    tracing::error!(
                        condition_function_id = %condition_path,
                        error = ?err,
                        "Error invoking condition function"
                    );
                    return Err(format!("Condition function error: {:?}", err).into());
                }
            }
        }

        match engine.call(function_id, data).await {
            Ok(_) => {
                tracing::debug!(job_id = %job.id, "Job processed successfully");
                Ok(())
            }
            Err(e) => {
                tracing::error!(job_id = %job.id, error = ?e, "Job processing failed");
                Err(format!("Job processing error: {:?}", e).into())
            }
        }
    }
}
