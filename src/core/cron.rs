use std::sync::{Arc, Mutex};

use tokio::{
    sync::RwLock,
    task::JoinHandle,
    time::{Duration, sleep},
};
use tonic::async_trait;

use crate::WorkerRegistry;

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
struct CronJob {
    id: String,
    task: String,
    time: String,
}

#[derive(Debug)]
enum CronError {
    InvalidTask,
    InvalidTimeFormat,
    SchedulingFailed,
    UnschedulingFailed,
    ExecutionFailed,
}

type CronJobs = Arc<RwLock<Vec<CronJob>>>;
type RunningJob = Arc<Mutex<Option<JoinHandle<()>>>>;

#[async_trait]
trait CronHandler: Send + Sync {
    fn registry(&self) -> WorkerRegistry {
        todo!()
    }

    fn tasks(&self) -> CronJobs {
        todo!()
    }

    fn running_jobs(&self) -> RunningJob {
        todo!()
    }

    fn is_running(&self) -> bool {
        let running = self.running_jobs();
        running
            .lock()
            .map(|guard| {
                guard
                    .as_ref()
                    .map(|handle| !handle.is_finished())
                    .unwrap_or(false)
            })
            .unwrap_or(false)
    }

    fn stop_running(&self) {
        if let Ok(mut guard) = self.running_jobs().lock() {
            if let Some(handle) = guard.take() {
                handle.abort();
                println!("Cron jobs stopped");
            }
        }
    }

    fn restart(self: Arc<Self>)
    where
        Self: Send + Sync + 'static,
    {
        self.stop_running();
        self.run_tasks();
    }

    fn set_running_jobs(&self, handle: JoinHandle<()>) {
        if let Ok(mut guard) = self.running_jobs().lock() {
            *guard = Some(handle);
        }
    }

    async fn schedule(&self, _cron_job: CronJob) -> Result<(), CronError> {
        todo!()
    }

    async fn add_schedule(self: Arc<Self>, cron_job: CronJob) -> Result<(), CronError>
    where
        Self: Send + Sync + 'static,
    {
        self.validates_time(&cron_job.time)?;
        self.validates_task(&cron_job.task).await?;
        self.schedule(cron_job).await?;
        self.restart();
        Ok(())
    }

    async fn unschedule(&self, _task: &str) -> Result<(), CronError> {
        todo!()
    }

    fn validates_time(&self, time: &str) -> Result<bool, CronError> {
        if time.is_empty() {
            return Err(CronError::InvalidTimeFormat);
        }
        Ok(true)
    }

    async fn validates_task(&self, task: &str) -> Result<bool, CronError> {
        if self.registry().read().await.get(task).is_some() {
            return Ok(true);
        }
        Err(CronError::InvalidTask)
    }

    fn run_tasks(self: Arc<Self>)
    where
        Self: Send + Sync + 'static,
    {
        if self.is_running() {
            println!("Cron jobs are already running");
            return;
        }

        let handler = Arc::clone(&self);
        let cron_jobs = self.tasks();
        let handle = tokio::spawn(async move {
            loop {
                let scheduled_jobs = {
                    let guard = cron_jobs.read().await;
                    guard.iter().cloned().collect::<Vec<_>>()
                };

                for job in scheduled_jobs {
                    match handler.validates_task(&job.task).await {
                        Ok(true) => {
                            if let Err(err) = handler.run(&job).await {
                                eprintln!("Error running task '{}': {:?}", job.task, err);
                            }
                        }
                        Ok(false) => continue,
                        Err(err) => {
                            eprintln!("Invalid task '{}': {:?}", job.task, err);
                            continue;
                        }
                    }
                }

                sleep(Duration::from_secs(1)).await;
            }
        });

        self.set_running_jobs(handle);
    }

    async fn run(&self, _cron_job: &CronJob) -> Result<(), CronError> {
        todo!()
    }
}

struct InMemoryCron {
    registry: WorkerRegistry,
    cron_jobs: CronJobs,
    running_job: RunningJob,
}

impl InMemoryCron {
    fn new(registry: WorkerRegistry) -> Self {
        Self {
            registry,
            cron_jobs: Arc::new(RwLock::new(Vec::new())),
            running_job: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl CronHandler for InMemoryCron {
    fn registry(&self) -> WorkerRegistry {
        self.registry.clone()
    }

    fn tasks(&self) -> CronJobs {
        Arc::clone(&self.cron_jobs)
    }

    fn running_jobs(&self) -> RunningJob {
        Arc::clone(&self.running_job)
    }

    async fn schedule(&self, cron_job: CronJob) -> Result<(), CronError> {
        let mut jobs = self.cron_jobs.write().await;
        if jobs.iter().any(|job| job.id == cron_job.id) {
            Err(CronError::SchedulingFailed)
        } else {
            jobs.push(cron_job);
            Ok(())
        }
    }

    async fn unschedule(&self, task: &str) -> Result<(), CronError> {
        let mut jobs = self.cron_jobs.write().await;
        let len_before = jobs.len();
        jobs.retain(|job| job.task != task);
        if jobs.len() == len_before {
            Err(CronError::UnschedulingFailed)
        } else {
            Ok(())
        }
    }

    async fn run(&self, cron_job: &CronJob) -> Result<(), CronError> {
        println!("Running task: {}", cron_job.task);
        Ok(())
    }
}
