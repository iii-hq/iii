// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{path::Path, process::Stdio, sync::Arc};

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
    children: Arc<Mutex<Vec<Child>>>,
    shutdown_tx: Arc<tokio::sync::watch::Sender<bool>>,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
}

const MAX_WATCH_EVENTS: usize = 100;

impl Exec {
    pub fn new(config: ExecConfig) -> Self {
        tracing::info!("Creating Exec module with config: {:?}", config);

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        Self {
            glob_exec: config.watch.map(GlobExec::new),
            exec: config.exec,
            children: Arc::new(Mutex::new(Vec::new())),
            shutdown_tx: Arc::new(shutdown_tx),
            shutdown_rx,
        }
    }

    /// Signal all loops in run()/run_pipeline() to stop, then kill the child process.
    pub async fn shutdown(&self) {
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
        let mut children = self.children.lock().await;
        for cmd in &self.exec {
            let spawned = self.spawn_single(cmd)?;
            children.push(spawned);
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
        let mut children: Vec<Child> = self.children.lock().await.drain(..).collect();

        if children.is_empty() {
            return;
        }

        // 1️⃣ Ask all process groups politely
        #[cfg(not(windows))]
        let pgids: Vec<Option<nix::unistd::Pid>> = children
            .iter()
            .map(|c| c.id().map(|id| nix::unistd::Pid::from_raw(id as i32)))
            .collect();

        #[cfg(not(windows))]
        for pgid in pgids.iter().flatten() {
            let _ = nix::sys::signal::killpg(*pgid, nix::sys::signal::Signal::SIGTERM);
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

            for child in &children {
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
        }

        // 2️⃣ Wait for all to exit (with timeout)
        let wait_all = async {
            for child in &mut children {
                let _ = child.wait().await;
            }
        };

        if timeout(Duration::from_secs(3), wait_all).await.is_err() {
            // 3️⃣ Force kill all remaining process groups
            tracing::warn!("Processes did not exit gracefully, killing");
            #[cfg(not(windows))]
            for pgid in pgids.iter().flatten() {
                let _ = nix::sys::signal::killpg(*pgid, nix::sys::signal::Signal::SIGKILL);
            }
            #[cfg(windows)]
            for child in &mut children {
                let _ = child.kill().await;
            }
        }
    }

    async fn kill_process(&self) {
        let children: Vec<Child> = self.children.lock().await.drain(..).collect();

        for mut proc in children {
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
        exec.children.lock().await.push(child);

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
        exec.children.lock().await.push(child);

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
