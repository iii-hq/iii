// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use colored::Colorize;
use cron::Schedule;
use serde_json::json;
use tokio::{task::JoinHandle, time::sleep};

use crate::engine::{Engine, EngineTrait};

/// Trait for cron scheduling operations
#[async_trait]
pub trait CronSchedulerAdapter: Send + Sync + 'static {
    /// Try to acquire a distributed lock for a cron job
    async fn try_acquire_lock(&self, job_id: &str) -> bool;

    /// Release the distributed lock for a cron job
    async fn release_lock(&self, job_id: &str);
}

pub(crate) struct CronJobInfo {
    #[allow(dead_code)]
    pub id: String,
    #[allow(dead_code)]
    pub schedule: Schedule,
    pub function_path: String,
    #[allow(dead_code)]
    pub condition_function_path: Option<String>,
    pub task_handle: JoinHandle<()>,
}

pub struct CronAdapter {
    adapter: Arc<dyn CronSchedulerAdapter>,
    jobs: Arc<tokio::sync::RwLock<HashMap<String, CronJobInfo>>>,
    engine: Arc<Engine>,
}

impl CronAdapter {
    pub fn new(scheduler: Arc<dyn CronSchedulerAdapter>, engine: Arc<Engine>) -> Self {
        Self {
            adapter: scheduler,
            jobs: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            engine,
        }
    }

    /// Parse a cron expression string into a Schedule
    fn parse_cron_expression(expression: &str) -> anyhow::Result<Schedule> {
        expression
            .parse::<Schedule>()
            .map_err(|e| anyhow::anyhow!("Invalid cron expression '{}': {}", expression, e))
    }

    /// Start a cron job that will trigger at the specified schedule
    async fn start_cron_job(
        &self,
        id: String,
        schedule: Schedule,
        function_path: String,
        condition_function_path: Option<String>,
    ) -> JoinHandle<()> {
        let scheduler = Arc::clone(&self.adapter);
        let engine = Arc::clone(&self.engine);
        let job_id = id.clone();
        let func_path = function_path.clone();

        tokio::spawn(async move {
            tracing::debug!(job_id = %job_id, function_path = %func_path, "Starting cron job loop");

            loop {
                // Calculate time until next execution
                let now = chrono::Utc::now();
                let next: chrono::DateTime<chrono::Utc> = match schedule
                    .upcoming(chrono::Utc)
                    .next()
                {
                    Some(next) => next,
                    None => {
                        tracing::warn!(job_id = %job_id, "No upcoming schedule found for cron job");
                        break;
                    }
                };

                let duration_until_next = (next - now).to_std().unwrap_or(Duration::ZERO);

                tracing::debug!(
                    job_id = %job_id,
                    next_run = %next,
                    duration_secs = duration_until_next.as_secs(),
                    "Waiting for next cron execution"
                );

                // Wait until the next scheduled time
                sleep(duration_until_next).await;

                // Try to acquire the distributed lock
                if scheduler.try_acquire_lock(&job_id).await {
                    tracing::info!(
                        "{} Cron job {} → {}",
                        "[TRIGGERED]".green(),
                        job_id.purple(),
                        func_path.cyan()
                    );

                    // Create the cron event payload
                    let event_data = serde_json::json!({
                        "trigger": "cron",
                        "job_id": job_id,
                        "scheduled_time": next.to_rfc3339(),
                        "actual_time": chrono::Utc::now().to_rfc3339(),
                    });

                    if let Some(condition_function_path) = condition_function_path.as_ref() {
                        tracing::debug!(
                            condition_function_path = %condition_function_path,
                            "Checking trigger conditions"
                        );

                        match engine
                            .invoke_function(condition_function_path, event_data.clone())
                            .await
                        {
                            Ok(Some(result)) => {
                                if let Some(passed) = result.as_bool()
                                    && !passed
                                {
                                    tracing::debug!(
                                        function_path = %func_path,
                                        "Condition check failed, skipping handler"
                                    );
                                    scheduler.release_lock(&job_id).await;
                                    continue;
                                }
                            }
                            Ok(None) => {
                                tracing::warn!(
                                    condition_function_path = %condition_function_path,
                                    "Condition function returned no result"
                                );
                            }
                            Err(err) => {
                                tracing::error!(
                                    condition_function_path = %condition_function_path,
                                    error = ?err,
                                    "Error invoking condition function"
                                );
                                scheduler.release_lock(&job_id).await;
                                continue;
                            }
                        }
                    }

                    let http_triggers = engine.http_triggers.list_cron_triggers(&job_id);
                    for trigger in http_triggers {
                        let dispatcher = engine.webhook_dispatcher.clone();
                        let functions = engine.functions.clone();
                        let trigger_id = trigger.trigger_id.clone();
                        let function_path = trigger.function_path.clone();
                        let config = trigger.config.clone();
                        let payload = json!({
                            "trigger": {
                                "type": "cron",
                                "id": trigger_id,
                                "scheduled_at": next.to_rfc3339(),
                                "fired_at": chrono::Utc::now().to_rfc3339(),
                                "function_path": function_path,
                            },
                            "config": config,
                        });
                        tokio::spawn(async move {
                            let _ = dispatcher.deliver(&trigger, payload, &functions).await;
                        });
                    }

                    let _ = engine.invoke_function(&func_path, event_data).await;

                    // Release the lock
                    scheduler.release_lock(&job_id).await;
                } else {
                    tracing::debug!(
                        job_id = %job_id,
                        "Skipping cron execution - another instance is handling it"
                    );
                }
            }

            tracing::debug!(job_id = %job_id, "Cron job loop ended");
        })
    }

    /// Register a new cron trigger
    pub async fn register(
        &self,
        id: &str,
        cron_expression: &str,
        function_path: &str,
        condition_function_path: Option<String>,
    ) -> anyhow::Result<()> {
        // Check if already registered
        {
            let jobs = self.jobs.read().await;
            if jobs.contains_key(id) {
                return Err(anyhow::anyhow!("Cron job '{}' is already registered", id));
            }
        }

        // Parse the cron expression
        let schedule = Self::parse_cron_expression(cron_expression)?;

        tracing::info!(
            "{} Cron job {} ({}) → {}",
            "[REGISTERED]".green(),
            id.purple(),
            cron_expression.yellow(),
            function_path.cyan()
        );

        // Start the cron job
        let task_handle = self
            .start_cron_job(
                id.to_string(),
                schedule.clone(),
                function_path.to_string(),
                condition_function_path.clone(),
            )
            .await;

        // Store the job info
        let mut jobs = self.jobs.write().await;
        jobs.insert(
            id.to_string(),
            CronJobInfo {
                id: id.to_string(),
                schedule,
                function_path: function_path.to_string(),
                condition_function_path,
                task_handle,
            },
        );

        Ok(())
    }

    /// Unregister a cron trigger
    pub async fn unregister(&self, id: &str) -> anyhow::Result<()> {
        let mut jobs = self.jobs.write().await;

        if let Some(job_info) = jobs.remove(id) {
            tracing::info!(
                "{} Cron job {} → {}",
                "[UNREGISTERED]".yellow(),
                id.purple(),
                job_info.function_path.cyan()
            );

            // Abort the task
            job_info.task_handle.abort();

            // Release any held lock
            self.adapter.release_lock(id).await;

            Ok(())
        } else {
            Err(anyhow::anyhow!("Cron job '{}' not found", id))
        }
    }
}
