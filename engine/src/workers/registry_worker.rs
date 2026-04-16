// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Spawns non-built-in workers via `iii-worker start`.
//! All registry resolution, binary download, and OCI management is handled
//! by `iii-worker` itself — the engine only manages the child process lifecycle.

use std::{path::PathBuf, sync::Arc};

use serde_json::Value;
use tokio::sync::Mutex;

use crate::{engine::Engine, workers::traits::Worker};

// =============================================================================
// iii-worker binary resolution
// =============================================================================

/// Resolve the `iii-worker` binary. Checks ~/.local/bin/ and system PATH.
pub fn resolve_iii_worker_binary() -> Option<PathBuf> {
    let exe_name = if cfg!(target_os = "windows") {
        "iii-worker.exe"
    } else {
        "iii-worker"
    };

    // Check ~/.local/bin/ (standard managed binary location)
    let managed_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".local")
        .join("bin")
        .join(exe_name);
    if managed_path.exists() {
        return Some(managed_path);
    }

    // Check system PATH
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(exe_name))
            .find(|p| p.exists())
    })
}

// =============================================================================
// ExternalWorkerProcess
// =============================================================================

/// A non-built-in worker process spawned via `iii-worker start`.
/// Handles both binary and OCI workers — iii-worker determines the type
/// and auto-installs from the registry if needed.
pub struct ExternalWorkerProcess {
    pub name: String,
    pub child: Arc<Mutex<Option<tokio::process::Child>>>,
    /// When the process was spawned. `is_alive` grants a grace window from
    /// this instant to cover the gap between `iii-worker start` exiting and
    /// the detached VM writing its pidfile.
    pub spawned_at: std::time::Instant,
}

/// How long after spawn we trust "still booting, no pidfile yet" as alive.
///
/// iii-worker start returns immediately after forking the detached VM boot
/// process. The VM then provisions rootfs, installs deps, and writes
/// `~/.iii/managed/{name}/vm.pid` only once libkrun is up. On a warm cache
/// this is sub-second; a cold first-boot with dep install can take tens of
/// seconds. 30s is conservative enough to avoid false-negative "dead" reads
/// during boot without masking a genuine crash for long.
const SPAWN_GRACE: std::time::Duration = std::time::Duration::from_secs(30);

impl ExternalWorkerProcess {
    /// Spawns `iii-worker start <name> --port <port>` as a detached child.
    ///
    /// `port` is the engine's configured `iii-worker-manager` port; the CLI
    /// uses it to build the `III_ENGINE_URL` env var handed to the spawned
    /// VM-based worker so it connects back to the right place. When the
    /// engine runs on the default port this is equivalent to the pre-fix
    /// behavior; when it runs on a non-default port (e.g. SDK integration
    /// tests with multiple `iii-worker-manager` entries), the spawned worker
    /// no longer silently connects to the wrong port.
    pub async fn spawn(name: &str, port: u16) -> Result<Self, String> {
        let worker_binary = resolve_iii_worker_binary()
            .ok_or_else(|| {
                "iii-worker binary not found. Install with `iii update worker` or place in ~/.local/bin/".to_string()
            })?;

        let logs_dir = dirs::home_dir()
            .unwrap_or_default()
            .join(".iii/logs")
            .join(name);
        std::fs::create_dir_all(&logs_dir)
            .map_err(|e| format!("Failed to create logs dir: {}", e))?;

        let stdout_file = std::fs::File::create(logs_dir.join("stdout.log"))
            .map_err(|e| format!("Failed to create stdout log: {}", e))?;
        let stderr_file = std::fs::File::create(logs_dir.join("stderr.log"))
            .map_err(|e| format!("Failed to create stderr log: {}", e))?;

        let port_str = port.to_string();
        let mut cmd = tokio::process::Command::new(&worker_binary);
        cmd.args(["start", name, "--port", &port_str])
            .stdout(stdout_file)
            .stderr(stderr_file);

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn iii-worker for '{}': {}", name, e))?;

        tracing::info!(
            worker = %name,
            pid = ?child.id(),
            port = port,
            "Worker starting via iii-worker (logs: `iii worker logs {}`)", name
        );

        Ok(Self {
            name: name.to_string(),
            child: Arc::new(Mutex::new(Some(child))),
            spawned_at: std::time::Instant::now(),
        })
    }

    /// Probes whether the detached worker process is still alive.
    ///
    /// The real VM PID lives in `~/.iii/managed/{name}/vm.pid` — the tokio
    /// `Child` handle is stale because `iii-worker start` exits immediately
    /// after spawning the detached boot process.
    ///
    /// Returns:
    /// - `true` if a pidfile exists and the PID responds to signal 0
    /// - `true` if we're still inside the post-spawn grace window (VM might
    ///   just be finishing boot and writing its pidfile)
    /// - `false` otherwise (crashed, force-stopped, or never booted)
    pub fn is_alive(&self) -> bool {
        let pidfile = dirs::home_dir()
            .unwrap_or_default()
            .join(".iii/managed")
            .join(&self.name)
            .join("vm.pid");

        if let Ok(s) = std::fs::read_to_string(&pidfile)
            && let Ok(pid) = s.trim().parse::<u32>()
        {
            #[cfg(unix)]
            {
                use nix::sys::signal::kill;
                use nix::unistd::Pid;
                if kill(Pid::from_raw(pid as i32), None).is_ok() {
                    return true;
                }
            }
            #[cfg(not(unix))]
            {
                let _ = pid;
                return true;
            }
        }

        // No pidfile (or stale/dead pid). Grant a grace window from spawn
        // so we don't misread a still-booting worker as dead.
        self.spawned_at.elapsed() < SPAWN_GRACE
    }

    pub async fn stop(&self) {
        // iii-worker start spawns the actual worker as a detached process and
        // exits immediately, so the child handle here is already gone.
        // Use `iii-worker stop <name>` which reads the PID file and kills the
        // actual worker process.
        if let Some(binary) = resolve_iii_worker_binary() {
            let result = tokio::process::Command::new(&binary)
                .args(["stop", &self.name])
                .output()
                .await;
            match result {
                Ok(output) if output.status.success() => {
                    tracing::info!(worker = %self.name, "Worker stopped via iii-worker");
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    tracing::warn!(
                        worker = %self.name,
                        stderr = %stderr.trim(),
                        "iii-worker stop returned non-zero"
                    );
                }
                Err(e) => {
                    tracing::warn!(worker = %self.name, error = %e, "Failed to run iii-worker stop");
                }
            }
        } else {
            tracing::warn!(worker = %self.name, "Cannot stop worker: iii-worker binary not found");
        }

        // Clean up the child handle if it's still around
        let _ = self.child.lock().await.take();
    }
}

// =============================================================================
// ExternalWorkerWrapper (Worker trait impl)
// =============================================================================

/// Worker trait wrapper for external workers (binary or OCI via iii-worker).
pub struct ExternalWorkerWrapper {
    process: ExternalWorkerProcess,
    display_name: &'static str,
}

impl ExternalWorkerWrapper {
    pub fn new(process: ExternalWorkerProcess) -> Self {
        let display_name = Box::leak(format!("ExternalWorker({})", &process.name).into_boxed_str());
        Self {
            process,
            display_name,
        }
    }
}

#[async_trait::async_trait]
impl Worker for ExternalWorkerWrapper {
    fn name(&self) -> &'static str {
        self.display_name
    }

    async fn create(_engine: Arc<Engine>, _config: Option<Value>) -> anyhow::Result<Box<dyn Worker>>
    where
        Self: Sized,
    {
        Err(anyhow::anyhow!(
            "ExternalWorkerWrapper::create should not be called directly"
        ))
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn start_background_tasks(
        &self,
        _shutdown_rx: tokio::sync::watch::Receiver<bool>,
        _shutdown_tx: tokio::sync::watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        // Shutdown is handled by destroy() which calls `iii-worker stop`.
        // No background task needed here since iii-worker start exits
        // immediately and the actual worker runs as a detached process.
        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        self.process.stop().await;
        Ok(())
    }

    async fn is_alive(&self) -> bool {
        self.process.is_alive()
    }

    fn register_functions(&self, _engine: Arc<Engine>) {
        // External workers register their own functions via the bridge protocol
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Compile-time assertion: ExternalWorkerProcess must be Send + Sync
    const _: () = {
        fn assert_send_sync<T: Send + Sync>() {}
        fn check() {
            assert_send_sync::<ExternalWorkerProcess>();
        }
        let _ = check;
    };

    #[test]
    fn external_worker_wrapper_name_format() {
        let process = ExternalWorkerProcess {
            name: "test-worker".to_string(),
            child: Arc::new(Mutex::new(None)),
            spawned_at: std::time::Instant::now(),
        };
        let wrapper = ExternalWorkerWrapper::new(process);
        assert_eq!(wrapper.name(), "ExternalWorker(test-worker)");
    }

    #[tokio::test]
    async fn external_worker_wrapper_create_returns_error() {
        let engine = Arc::new(crate::engine::Engine::new());
        let result = ExternalWorkerWrapper::create(engine, None).await;
        assert!(result.is_err());
        let err_msg = result.err().unwrap().to_string();
        assert!(err_msg.contains("should not be called directly"));
    }

    #[tokio::test]
    async fn external_worker_wrapper_initialize_succeeds() {
        let process = ExternalWorkerProcess {
            name: "init-test".to_string(),
            child: Arc::new(Mutex::new(None)),
            spawned_at: std::time::Instant::now(),
        };
        let wrapper = ExternalWorkerWrapper::new(process);
        assert!(wrapper.initialize().await.is_ok());
    }

    #[tokio::test]
    async fn external_worker_wrapper_destroy_succeeds_with_no_child() {
        let process = ExternalWorkerProcess {
            name: "destroy-test".to_string(),
            child: Arc::new(Mutex::new(None)),
            spawned_at: std::time::Instant::now(),
        };
        let wrapper = ExternalWorkerWrapper::new(process);
        assert!(wrapper.destroy().await.is_ok());
    }
}
