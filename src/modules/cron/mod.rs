mod config;

use std::{
    collections::HashMap,
    pin::Pin,
    sync::{Arc, RwLock},
    time::Duration,
};

use async_trait::async_trait;
use colored::Colorize;
pub use config::CronModuleConfig;
use cron::Schedule;
use futures::Future;
use once_cell::sync::Lazy;
use redis::{Client, aio::ConnectionManager};
use serde_json::Value;
use tokio::{
    task::JoinHandle,
    time::{sleep, timeout},
};

use crate::{
    engine::{Engine, EngineTrait},
    modules::core_module::{AdapterFactory, ConfigurableModule, CoreModule},
    trigger::{Trigger, TriggerRegistrator},
};

/// Default timeout for Redis connection attempts
const REDIS_CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

/// Default lock TTL for distributed cron locking (in milliseconds)
const CRON_LOCK_TTL_MS: u64 = 30_000; // 30 seconds

/// Prefix for cron lock keys in Redis
const CRON_LOCK_PREFIX: &str = "cron_lock:";

/// Trait for cron scheduling operations
#[async_trait]
pub trait CronSchedulerAdapter: Send + Sync + 'static {
    /// Try to acquire a distributed lock for a cron job
    async fn try_acquire_lock(&self, job_id: &str) -> bool;

    /// Release the distributed lock for a cron job
    async fn release_lock(&self, job_id: &str);
}

/// Redis-based distributed lock for cron jobs
pub struct RedisCronLock {
    connection: Arc<tokio::sync::Mutex<ConnectionManager>>,
    instance_id: String,
}

impl RedisCronLock {
    pub async fn new(redis_url: &str) -> anyhow::Result<Self> {
        let client = Client::open(redis_url)?;

        let manager = timeout(REDIS_CONNECTION_TIMEOUT, client.get_connection_manager())
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "Redis connection timed out after {:?}. Please ensure Redis is running at: {}",
                    REDIS_CONNECTION_TIMEOUT,
                    redis_url
                )
            })?
            .map_err(|e| anyhow::anyhow!("Failed to connect to Redis at {}: {}", redis_url, e))?;

        // Generate a unique instance ID for this engine instance
        let instance_id = uuid::Uuid::new_v4().to_string();

        Ok(Self {
            connection: Arc::new(tokio::sync::Mutex::new(manager)),
            instance_id,
        })
    }
}

#[async_trait]
impl CronSchedulerAdapter for RedisCronLock {
    async fn try_acquire_lock(&self, job_id: &str) -> bool {
        let lock_key = format!("{}{}", CRON_LOCK_PREFIX, job_id);
        let mut conn = self.connection.lock().await;

        // Use SET with NX (only if not exists) and PX (expiry in milliseconds)
        let result: redis::RedisResult<Option<String>> = redis::cmd("SET")
            .arg(&lock_key)
            .arg(&self.instance_id)
            .arg("NX")
            .arg("PX")
            .arg(CRON_LOCK_TTL_MS)
            .query_async(&mut *conn)
            .await;

        match result {
            Ok(Some(_)) => {
                tracing::debug!(job_id = %job_id, instance_id = %self.instance_id, "Acquired cron lock");
                true
            }
            Ok(None) => {
                tracing::debug!(job_id = %job_id, "Failed to acquire cron lock - another instance holds it");
                false
            }
            Err(e) => {
                tracing::error!(job_id = %job_id, error = %e, "Error acquiring cron lock");
                false
            }
        }
    }

    async fn release_lock(&self, job_id: &str) {
        let lock_key = format!("{}{}", CRON_LOCK_PREFIX, job_id);
        let mut conn = self.connection.lock().await;

        // Only release if we own the lock (using Lua script for atomicity)
        let script = r#"
            if redis.call("get", KEYS[1]) == ARGV[1] then
                return redis.call("del", KEYS[1])
            else
                return 0
            end
        "#;

        let result: redis::RedisResult<i32> = redis::Script::new(script)
            .key(&lock_key)
            .arg(&self.instance_id)
            .invoke_async(&mut *conn)
            .await;

        match result {
            Ok(1) => {
                tracing::debug!(job_id = %job_id, "Released cron lock");
            }
            Ok(_) => {
                tracing::debug!(job_id = %job_id, "Lock was not held by this instance");
            }
            Err(e) => {
                tracing::error!(job_id = %job_id, error = %e, "Error releasing cron lock");
            }
        }
    }
}

struct CronJobInfo {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    schedule: Schedule,
    function_path: String,
    task_handle: JoinHandle<()>,
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

                    // Invoke the function
                    engine.invoke_function(&func_path, event_data);

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
            .start_cron_job(id.to_string(), schedule.clone(), function_path.to_string())
            .await;

        // Store the job info
        let mut jobs = self.jobs.write().await;
        jobs.insert(
            id.to_string(),
            CronJobInfo {
                id: id.to_string(),
                schedule,
                function_path: function_path.to_string(),
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

#[derive(Clone)]
pub struct CronCoreModule {
    adapter: Arc<CronAdapter>,
    engine: Arc<Engine>,
    _config: CronModuleConfig,
}

#[async_trait]
impl CoreModule for CronCoreModule {
    async fn create(
        engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn CoreModule>> {
        Self::create_with_adapters(engine, config).await
    }

    fn register_functions(&self, _engine: Arc<Engine>) {}

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing CronModule");

        use crate::trigger::TriggerType;

        let trigger_type = TriggerType {
            id: "cron".to_string(),
            _description: "Cron-based scheduled triggers".to_string(),
            registrator: Box::new(self.clone()),
            worker_id: None,
        };

        self.engine.register_trigger_type(trigger_type).await;

        tracing::info!("{} Cron trigger type initialized", "[READY]".green());
        Ok(())
    }
}

impl TriggerRegistrator for CronCoreModule {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let cron_expression = trigger
            .config
            .get("expression")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Box::pin(async move {
            if cron_expression.is_empty() {
                tracing::error!(
                    "Cron expression is not set for trigger {}",
                    trigger.id.purple()
                );
                return Err(anyhow::anyhow!("Cron expression is required"));
            }

            self.adapter
                .register(&trigger.id, &cron_expression, &trigger.function_path)
                .await
        })
    }

    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        Box::pin(async move {
            tracing::debug!(trigger_id = %trigger.id, "Unregistering cron trigger");
            self.adapter.unregister(&trigger.id).await
        })
    }
}

#[async_trait]
impl ConfigurableModule for CronCoreModule {
    type Config = CronModuleConfig;
    type Adapter = dyn CronSchedulerAdapter;
    const DEFAULT_ADAPTER_CLASS: &'static str = "modules::cron::RedisCronAdapter";

    fn build_registry() -> HashMap<String, AdapterFactory<Self::Adapter>> {
        let mut registry = HashMap::new();

        registry.insert(
            CronCoreModule::DEFAULT_ADAPTER_CLASS.to_string(),
            CronCoreModule::make_adapter_factory(
                |_engine: Arc<Engine>, config: Option<Value>| async move {
                    let redis_url = config
                        .as_ref()
                        .and_then(|c| c.get("redis_url"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("redis://localhost:6379")
                        .to_string();

                    Ok(Arc::new(RedisCronLock::new(&redis_url).await?)
                        as Arc<dyn CronSchedulerAdapter>)
                },
            ),
        );
        registry
    }

    async fn registry() -> &'static RwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<RwLock<HashMap<String, AdapterFactory<dyn CronSchedulerAdapter>>>> =
            Lazy::new(|| RwLock::new(CronCoreModule::build_registry()));
        &REGISTRY
    }

    fn build(engine: Arc<Engine>, config: Self::Config, adapter: Arc<Self::Adapter>) -> Self {
        let cron_adapter = CronAdapter::new(adapter, engine.clone());
        Self {
            engine,
            _config: config,
            adapter: Arc::new(cron_adapter),
        }
    }

    fn adapter_class_from_config(config: &Self::Config) -> Option<String> {
        config.adapter.as_ref().map(|a| a.class.clone())
    }

    fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
        config.adapter.as_ref().and_then(|a| a.config.clone())
    }
}
