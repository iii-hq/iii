// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::Result;
use colored::Colorize;
use notify::{
    Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{CreateKind, DataChange, ModifyKind, RemoveKind, RenameMode},
};
use tokio::{
    process::{Child, Command},
    sync::{Mutex, mpsc},
    time::{Duration, timeout},
};

use crate::modules::shell::{config::ExecConfig, glob_exec::GlobExec};

#[derive(Debug, Clone)]
pub struct Exec {
    exec: Vec<String>,
    glob_exec: Option<GlobExec>,
    child: Arc<Mutex<Option<Child>>>,
    shutdown_tx: Arc<tokio::sync::watch::Sender<bool>>,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
    shutdown_called: Arc<AtomicBool>,
}

const MAX_WATCH_EVENTS: usize = 100;

impl Exec {
    pub fn new(config: ExecConfig) -> Self {
        tracing::info!("Creating Exec module with config: {:?}", config);

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        Self {
            glob_exec: config.watch.map(GlobExec::new),
            exec: config.exec,
            child: Arc::new(Mutex::new(None::<Child>)),
            shutdown_tx: Arc::new(shutdown_tx),
            shutdown_rx,
            shutdown_called: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Signal all loops in run()/run_pipeline() to stop, then kill the child process.
    pub async fn shutdown(&self) {
        if self.shutdown_called.swap(true, Ordering::SeqCst) {
            return; // Already called
        }
        tracing::info!("ExecModule received shutdown signal, stopping process");
        let _ = self.shutdown_tx.send(true);
        self.stop_process().await;
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let (tx, mut rx) = mpsc::channel::<Event>(MAX_WATCH_EVENTS);
        let mut watcher: RecommendedWatcher;

        if let Some(ref glob_exec) = self.glob_exec {
            tracing::info!("Creating watcher for glob exec: {:?}", glob_exec);

            watcher = Watcher::new(
                move |res| {
                    if let Ok(event) = res {
                        let _ = tx.blocking_send(event);
                    }
                },
                Config::default(),
            )?;

            for root in glob_exec.watch_roots() {
                watcher.watch(
                    Path::new(&root.path),
                    if root.recursive {
                        RecursiveMode::Recursive
                    } else {
                        RecursiveMode::NonRecursive
                    },
                )?;
            }
        }

        let cwd = std::env::current_dir().unwrap_or_default();

        // 🔥 start pipeline
        self.run_pipeline().await?;

        let mut shutdown_rx = self.shutdown_rx.clone();
        loop {
            tokio::select! {
                event = rx.recv() => {
                    match event {
                        Some(event) if self.should_restart(&event) => {
                            tracing::info!(
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

                            self.kill_process().await;
                            self.run_pipeline().await?;
                        }
                        Some(_) => continue,
                        None => break,
                    }
                }
                _ = shutdown_rx.changed() => {
                    tracing::info!("ExecModule file watcher shutting down");
                    break;
                }
            }
        }

        Ok(())
    }

    async fn run_pipeline(&self) -> Result<()> {
        let mut shutdown_rx = self.shutdown_rx.clone();

        let last_idx = self.exec.len() - 1;
        for (idx, cmd) in self.exec.iter().enumerate() {
            let spawned = self.spawn_single(cmd)?;
            *self.child.lock().await = Some(spawned);

            if idx < last_idx {
                // Take child out of the mutex so we don't hold the lock during wait.
                // This allows stop_process()/shutdown() to proceed if called concurrently.
                let mut child = self.child.lock().await.take().unwrap();

                // Wait for command to finish OR shutdown signal
                let status = tokio::select! {
                    status = child.wait() => status?,
                    _ = shutdown_rx.changed() => {
                        tracing::info!("Pipeline interrupted by shutdown signal");
                        // Put child back so stop_process() can kill it
                        *self.child.lock().await = Some(child);
                        return Ok(());
                    }
                };

                if !status.success() {
                    tracing::error!("Pipeline step failed, aborting pipeline");
                    break;
                }
            }
        }

        Ok(())
    }

    fn spawn_single(&self, command: &str) -> Result<Child> {
        tracing::info!("Starting process: {}", command.purple());

        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = Command::new("sh");
            c.arg("-c").arg(command);
            c.stdout(Stdio::inherit()).stderr(Stdio::inherit());

            // We need to detach from the current process
            // To coordinate process termination properly
            unsafe {
                c.pre_exec(|| {
                    nix::unistd::setsid()
                        .map_err(|e| std::io::Error::other(format!("setsid failed: {e}")))?;
                    Ok(())
                });
            }
            c
        };

        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(command);
            c.stdout(Stdio::inherit()).stderr(Stdio::inherit());

            // On Windows, create a new process group for easier termination of child process
            c.creation_flags(winapi::um::winbase::CREATE_NEW_PROCESS_GROUP);

            c
        };

        cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());

        Ok(cmd.spawn()?)
    }

    fn should_restart(&self, event: &Event) -> bool {
        let cwd = std::env::current_dir().unwrap_or_default();
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

        if let Some(ref glob_exec) = self.glob_exec {
            return event
                .paths
                .iter()
                .any(|path| glob_exec.should_trigger(path.strip_prefix(&cwd).unwrap_or(path)));
        }

        false
    }

    pub async fn stop_process(&self) {
        if let Some(mut child) = self.child.lock().await.take() {
            #[cfg(not(windows))]
            let pgid = child.id().map(|id| nix::unistd::Pid::from_raw(id as i32));

            #[cfg(not(windows))]
            if let Some(pgid) = pgid {
                // 1️⃣ Ask the whole process group politely
                let _ = nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGTERM);
            }

            #[cfg(windows)]
            {
                use winapi::{
                    shared::minwindef::{FALSE, TRUE},
                    um::{
                        consoleapi::SetConsoleCtrlHandler,
                        wincon::{
                            AttachConsole, CTRL_BREAK_EVENT, FreeConsole, GenerateConsoleCtrlEvent,
                        },
                    },
                };

                if let Some(pid) = child.id() {
                    unsafe {
                        if AttachConsole(pid) != 0 {
                            SetConsoleCtrlHandler(None, TRUE);
                            GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, 0);
                            SetConsoleCtrlHandler(None, FALSE);
                            FreeConsole();
                        }
                    }
                }
            }

            // 2️⃣ Wait a bit
            let exited = timeout(Duration::from_secs(3), child.wait()).await;

            if exited.is_err() {
                // 3️⃣ Force kill the entire process group
                tracing::warn!("Process did not exit gracefully, killing");
                #[cfg(not(windows))]
                if let Some(pgid) = pgid {
                    let _ = nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGKILL);
                }
                #[cfg(windows)]
                {
                    let _ = child.kill().await;
                }

                // Reap the child to avoid zombie processes
                let _ = child.wait().await;
            }
        }
    }

    async fn kill_process(&self) {
        if let Some(mut proc) = self.child.lock().await.take() {
            #[cfg(not(windows))]
            {
                if let Some(id) = proc.id() {
                    let pgid = nix::unistd::Pid::from_raw(id as i32);
                    if let Err(err) =
                        nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGKILL)
                    {
                        tracing::error!("Failed to kill process group: {:?}", err);
                    } else {
                        tracing::debug!("Process group killed");
                    }
                }
            }

            #[cfg(windows)]
            {
                if let Err(err) = proc.kill().await {
                    tracing::error!("Failed to kill process: {:?}", err);
                } else {
                    tracing::debug!("Process killed");
                }
            }

            // Reap the direct child to avoid zombies
            let _ = proc.wait().await;
        }
    }
}

#[cfg(test)]
#[cfg(not(windows))]
mod tests {
    use super::*;

    /// Spawns `sh -c "sleep 300 & sleep 300 & wait"` via Exec,
    /// calls stop_process(), and asserts all processes in the group are dead.
    #[tokio::test]
    async fn stop_process_kills_entire_process_group() {
        let exec = Exec::new(ExecConfig {
            watch: None,
            exec: vec!["sleep 300 & sleep 300 & wait".to_string()],
        });

        // Spawn the process (sh -c "sleep 300 & sleep 300 & wait")
        let child = exec.spawn_single(&exec.exec[0]).unwrap();
        let child_pid = child.id().unwrap() as i32;
        *exec.child.lock().await = Some(child);

        // Give children time to spawn
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Collect all PIDs in the process group (PGID = child_pid due to setsid)
        let pids_before: Vec<i32> = get_pids_in_group(child_pid);
        assert!(
            pids_before.len() >= 2,
            "expected at least 2 processes in group, got {:?}",
            pids_before
        );

        // Kill via stop_process
        exec.stop_process().await;

        // Give OS time to reap
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify all processes in the group are dead
        let pids_after: Vec<i32> = get_pids_in_group(child_pid);
        assert!(
            pids_after.is_empty(),
            "orphaned processes remain in group {}: {:?}",
            child_pid,
            pids_after
        );
    }

    /// Same test but for kill_process() (the file-change restart path).
    #[tokio::test]
    async fn kill_process_kills_entire_process_group() {
        let exec = Exec::new(ExecConfig {
            watch: None,
            exec: vec!["sleep 300 & sleep 300 & wait".to_string()],
        });

        let child = exec.spawn_single(&exec.exec[0]).unwrap();
        let child_pid = child.id().unwrap() as i32;
        *exec.child.lock().await = Some(child);

        tokio::time::sleep(Duration::from_millis(200)).await;

        let pids_before: Vec<i32> = get_pids_in_group(child_pid);
        assert!(
            pids_before.len() >= 2,
            "expected at least 2 processes in group, got {:?}",
            pids_before
        );

        exec.kill_process().await;

        tokio::time::sleep(Duration::from_millis(500)).await;

        let pids_after: Vec<i32> = get_pids_in_group(child_pid);
        assert!(
            pids_after.is_empty(),
            "orphaned processes remain in group {}: {:?}",
            child_pid,
            pids_after
        );
    }

    /// Verifies that stop_process() reaps the child after SIGKILL (no zombie left).
    /// Uses a process that traps SIGTERM so the graceful path always times out.
    #[tokio::test]
    async fn stop_process_reaps_child_after_sigkill() {
        let exec = Exec::new(ExecConfig {
            watch: None,
            exec: vec!["trap '' TERM; sleep 300".to_string()],
        });

        let child = exec.spawn_single(&exec.exec[0]).unwrap();
        let child_pid = child.id().unwrap() as i32;
        *exec.child.lock().await = Some(child);

        tokio::time::sleep(Duration::from_millis(200)).await;

        // stop_process sends SIGTERM, waits 3s, then SIGKILL
        exec.stop_process().await;

        // After stop_process returns, the child should be fully reaped (no zombie)
        tokio::time::sleep(Duration::from_millis(200)).await;

        let pids_after: Vec<i32> = get_pids_in_group(child_pid);
        assert!(
            pids_after.is_empty(),
            "zombie or orphaned processes remain in group {}: {:?}",
            child_pid,
            pids_after
        );
    }

    /// Calling shutdown() twice must not panic or error.
    #[tokio::test]
    async fn shutdown_is_idempotent() {
        let exec = Exec::new(ExecConfig {
            watch: None,
            exec: vec!["sleep 300".to_string()],
        });

        let child = exec.spawn_single(&exec.exec[0]).unwrap();
        *exec.child.lock().await = Some(child);

        tokio::time::sleep(Duration::from_millis(200)).await;

        // First shutdown
        exec.shutdown().await;
        // Second shutdown — must not panic
        exec.shutdown().await;
    }

    /// Returns PIDs of all alive processes whose PGID matches the given group id.
    fn get_pids_in_group(pgid: i32) -> Vec<i32> {
        let output = std::process::Command::new("ps")
            .args(["-eo", "pid,pgid"])
            .output()
            .expect("failed to run ps");
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout
            .lines()
            .skip(1) // header
            .filter_map(|line| {
                let mut cols = line.split_whitespace();
                let pid: i32 = cols.next()?.parse().ok()?;
                let group: i32 = cols.next()?.parse().ok()?;
                if group == pgid { Some(pid) } else { None }
            })
            .collect()
    }
}
