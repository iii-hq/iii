use std::{path::Path, process::Stdio, sync::Arc};

use anyhow::Result;
use colored::Colorize;
use notify::{
    Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{CreateKind, DataChange, ModifyKind, RemoveKind, RenameMode},
};
use tokio::{
    process::Command,
    sync::{Mutex, mpsc},
};
use tracing::{debug, error, info};

use crate::modules::shell::config::ExecConfig;

#[derive(Debug, Clone)]
pub struct Exec {
    watch: Option<Vec<String>>,
    extensions: Vec<String>,
    exec: Vec<String>,
}

const MAX_WATCH_EVENTS: usize = 100;

impl Exec {
    pub fn new(config: ExecConfig) -> Self {
        let extensions = config
            .extensions
            .unwrap_or_default()
            .split(',')
            .map(|e| e.trim().trim_start_matches('.').to_string())
            .collect();

        Self {
            watch: config.watch,
            extensions,
            exec: config.exec,
        }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let (tx, mut rx) = mpsc::channel::<Event>(MAX_WATCH_EVENTS);

        let watch = self.watch.clone();
        let mut watcher: RecommendedWatcher;

        if let Some(watch) = watch {
            watcher = Watcher::new(
                move |res| {
                    if let Ok(event) = res {
                        let _ = tx.blocking_send(event);
                    }
                },
                Config::default(),
            )?;

            for path in &watch {
                watcher.watch(Path::new(path), RecursiveMode::Recursive)?;
            }

            info!(
                "Watching paths: {} extensions: {}",
                watch.join(", ").purple(),
                self.extensions.join(", ").purple()
            );
        }

        let child = Arc::new(Mutex::new(None::<tokio::process::Child>));
        let cwd = std::env::current_dir().unwrap_or_default();

        // 🔥 start pipeline
        self.run_pipeline(child.clone()).await?;

        while let Some(event) = rx.recv().await {
            if !self.should_restart(&event) {
                continue;
            }

            info!(
                "File change detected {} → restarting pipeline",
                event
                    .paths
                    .iter()
                    .map(|p| {
                        p.strip_prefix(&cwd)
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|_| p.to_string_lossy().to_string())
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
                    .purple()
            );

            self.kill_process(child.clone()).await;
            self.run_pipeline(child.clone()).await?;
        }

        Ok(())
    }

    async fn run_pipeline(&self, child: Arc<Mutex<Option<tokio::process::Child>>>) -> Result<()> {
        for cmd in &self.exec {
            debug!("Pipeline step: {}", cmd.purple());

            let spawned = self.spawn_single(cmd).await?;
            *child.lock().await = Some(spawned);

            // ⏳ wait for command to finish
            let status = child.lock().await.as_mut().unwrap().wait().await?;

            if !status.success() {
                error!("Pipeline step failed, aborting pipeline");
                break;
            }

            // clear child before next step
            *child.lock().await = None;
        }

        Ok(())
    }

    async fn spawn_single(&self, command: &str) -> Result<tokio::process::Child> {
        info!("Starting process: {}", command.purple());

        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = Command::new("sh");
            c.arg("-c").arg(command);

            unsafe {
                c.pre_exec(|| {
                    libc::setpgid(0, 0);
                    Ok(())
                });
            }

            c
        };

        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(command);
            c
        };

        cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());

        Ok(cmd.spawn()?)
    }

    fn should_restart(&self, event: &Event) -> bool {
        let is_valid_event = matches!(
            event.kind,
            EventKind::Create(CreateKind::File)
                | EventKind::Modify(ModifyKind::Data(DataChange::Content))
                | EventKind::Modify(ModifyKind::Name(RenameMode::Any))
                | EventKind::Remove(RemoveKind::File)
        );

        if !is_valid_event {
            return false;
        }

        event.paths.iter().any(|path| {
            path.extension()
                .and_then(|e| e.to_str())
                .map(|ext| self.extensions.contains(&ext.to_string()))
                .unwrap_or(false)
        })
    }

    async fn kill_process(&self, child: Arc<Mutex<Option<tokio::process::Child>>>) {
        if let Some(mut proc) = child.lock().await.take() {
            if let Err(err) = proc.kill().await {
                error!("Failed to kill process: {:?}", err);
            } else {
                debug!("Process killed");
            }
        }
    }
}
