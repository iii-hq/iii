// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use colored::Colorize;
use cron::Schedule;
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
    pub function_id: String,
    #[allow(dead_code)]
    pub condition_function_id: Option<String>,
    pub task_handle: JoinHandle<()>,
}

pub struct CronAdapter {
    adapter: Arc<dyn CronSchedulerAdapter>,
    jobs: Arc<tokio::sync::RwLock<HashMap<String, CronJobInfo>>>,
    engine: Arc<Engine>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
    shutdown_called: AtomicBool,
}

impl CronAdapter {
    pub fn new(scheduler: Arc<dyn CronSchedulerAdapter>, engine: Arc<Engine>) -> Self {
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        Self {
            adapter: scheduler,
            jobs: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            engine,
            shutdown_tx,
            shutdown_rx,
            shutdown_called: AtomicBool::new(false),
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
        function_id: String,
        condition_function_id: Option<String>,
    ) -> JoinHandle<()> {
        let scheduler = Arc::clone(&self.adapter);
        let engine = Arc::clone(&self.engine);
        let job_id = id.clone();
        let function_id = function_id.clone();
        let mut shutdown_rx = self.shutdown_rx.clone();

        tokio::spawn(async move {
            tracing::debug!(job_id = %job_id, function_id = %function_id, "Starting cron job loop");

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

                // Wait until the next scheduled time, or shutdown signal
                tokio::select! {
                    _ = sleep(duration_until_next) => {}
                    _ = shutdown_rx.changed() => {
                        tracing::info!(job_id = %job_id, "Cron job received shutdown signal");
                        break;
                    }
                }

                // Check shutdown flag after waking
                if *shutdown_rx.borrow() {
                    tracing::info!(job_id = %job_id, "Cron job shutting down");
                    break;
                }

                // Try to acquire the distributed lock
                if scheduler.try_acquire_lock(&job_id).await {
                    tracing::info!(
                        "{} Cron job {} → {}",
                        "[TRIGGERED]".green(),
                        job_id.purple(),
                        function_id.cyan()
                    );

                    // Create the cron event payload
                    let event_data = serde_json::json!({
                        "trigger": "cron",
                        "job_id": job_id,
                        "scheduled_time": next.to_rfc3339(),
                        "actual_time": chrono::Utc::now().to_rfc3339(),
                    });

                    if let Some(condition_function_id) = condition_function_id.as_ref() {
                        tracing::debug!(
                            condition_function_id = %condition_function_id,
                            "Checking trigger conditions"
                        );

                        match engine.call(condition_function_id, event_data.clone()).await {
                            Ok(Some(result)) => {
                                if let Some(passed) = result.as_bool()
                                    && !passed
                                {
                                    tracing::debug!(
                                        function_id = %function_id,
                                        "Condition check failed, skipping handler"
                                    );
                                    scheduler.release_lock(&job_id).await;
                                    continue;
                                }
                            }
                            Ok(None) => {
                                tracing::warn!(
                                    condition_function_id = %condition_function_id,
                                    "Condition function returned no result"
                                );
                            }
                            Err(err) => {
                                tracing::error!(
                                    condition_function_id = %condition_function_id,
                                    error = ?err,
                                    "Error invoking condition function"
                                );
                                scheduler.release_lock(&job_id).await;
                                continue;
                            }
                        }
                    }

                    let _ = engine.call(&function_id, event_data).await;
                    crate::modules::telemetry::collector::track_cron_execution();

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
        function_id: &str,
        condition_function_id: Option<String>,
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
            function_id.cyan()
        );

        // Start the cron job
        let task_handle = self
            .start_cron_job(
                id.to_string(),
                schedule.clone(),
                function_id.to_string(),
                condition_function_id.clone(),
            )
            .await;

        // Store the job info
        let mut jobs = self.jobs.write().await;
        jobs.insert(
            id.to_string(),
            CronJobInfo {
                id: id.to_string(),
                schedule,
                function_id: function_id.to_string(),
                condition_function_id,
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
                job_info.function_id.cyan()
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

    /// Shutdown all cron jobs by signaling them and aborting any that don't stop
    pub async fn shutdown(&self) {
        if self.shutdown_called.swap(true, Ordering::SeqCst) {
            return;
        }
        tracing::info!("Shutting down all cron jobs");
        let _ = self.shutdown_tx.send(true);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut jobs = self.jobs.write().await;
        for (id, job_info) in jobs.drain() {
            if !job_info.task_handle.is_finished() {
                tracing::debug!(job_id = %id, "Force-aborting cron job task");
                job_info.task_handle.abort();
            }
            self.adapter.release_lock(&id).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopSchedulerAdapter;

    #[async_trait]
    impl CronSchedulerAdapter for NoopSchedulerAdapter {
        async fn try_acquire_lock(&self, _job_id: &str) -> bool {
            false
        }

        async fn release_lock(&self, _job_id: &str) {}
    }

    fn test_engine() -> Arc<Engine> {
        Arc::new(Engine::new())
    }

    #[tokio::test]
    async fn shutdown_signal_stops_cron_jobs() {
        let engine = test_engine();
        let scheduler: Arc<dyn CronSchedulerAdapter> = Arc::new(NoopSchedulerAdapter);
        let adapter = CronAdapter::new(scheduler, engine);

        // Register a cron job with an hourly schedule (won't fire during the test)
        adapter
            .register("test-job-1", "0 0 * * * *", "test-function", None)
            .await
            .expect("Failed to register cron job");

        // Verify the job is running
        {
            let jobs = adapter.jobs.read().await;
            assert_eq!(jobs.len(), 1, "Expected one registered job");
            let job = jobs.get("test-job-1").unwrap();
            assert!(
                !job.task_handle.is_finished(),
                "Job task should still be running"
            );
        }

        // Shutdown and wait a bit for tasks to stop
        adapter.shutdown().await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        // After shutdown, the jobs map should be drained
        let jobs = adapter.jobs.read().await;
        assert!(jobs.is_empty(), "All jobs should be drained after shutdown");
    }

    /// Calling shutdown() twice must not panic or error.
    #[tokio::test]
    async fn shutdown_is_idempotent() {
        let engine = test_engine();
        let adapter = CronAdapter::new(Arc::new(NoopSchedulerAdapter), engine);

        adapter
            .register("job-1", "0 0 * * * *", "fn-1", None)
            .await
            .unwrap();

        adapter.shutdown().await;
        // Second call must not panic or deadlock
        adapter.shutdown().await;
    }

    /// Calling shutdown() directly (as destroy would) should abort all tasks
    /// even if no external shutdown signal was sent first.
    #[tokio::test]
    async fn shutdown_aborts_all_task_handles() {
        let engine = test_engine();
        let adapter = CronAdapter::new(Arc::new(NoopSchedulerAdapter), engine);

        // Register multiple cron jobs
        adapter
            .register("job-1", "0 0 * * * *", "fn-1", None)
            .await
            .unwrap();
        adapter
            .register("job-2", "0 30 * * * *", "fn-2", None)
            .await
            .unwrap();

        // Verify both jobs are running
        {
            let jobs = adapter.jobs.read().await;
            assert_eq!(jobs.len(), 2);
        }

        // Call shutdown directly (simulating destroy path)
        adapter.shutdown().await;

        // Verify all tasks are finished and map is drained
        let jobs = adapter.jobs.read().await;
        assert!(jobs.is_empty(), "All jobs should be drained after shutdown");
    }
}
