// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Give freed heap back to the operating system.
//!
//! glibc may retain freed pages inside allocator arenas after allocation bursts.
//! The engine service owns one periodic maintenance task, independently of
//! worker registration, observability configuration and worker reloads. This
//! reclaims eligible pages; it neither limits live allocations nor prevents OOM.

/// Resident set size of this process in bytes, read from `/proc/self/statm`.
/// `None` where that file does not exist.
pub fn resident_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * page_size())
}

#[cfg(unix)]
fn page_size() -> u64 {
    // SAFETY: sysconf has no preconditions and only reads a constant.
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if size > 0 { size as u64 } else { 4096 }
}

#[cfg(not(unix))]
fn page_size() -> u64 {
    4096
}

/// Return freed heap pages to the OS. Returns the RSS before and after the
/// trim, in bytes, on Linux with glibc; `None` on unsupported targets or when
/// RSS cannot be read. Other allocators may have different retention policies.
///
/// Cheap on a small heap. On a heap that just churned through gigabytes it
/// takes an arena lock at a time while it `madvise`s, so call it off the
/// async executor.
pub fn release_freed_memory() -> Option<(u64, u64)> {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        let before = resident_bytes()?;
        // SAFETY: malloc_trim only touches allocator bookkeeping; it is safe
        // to call from any thread at any time.
        unsafe {
            libc::malloc_trim(0);
        }
        let after = resident_bytes()?;
        Some((before, after))
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    {
        None
    }
}

/// One task per EngineBuilder::serve lifetime, not per worker. Dropping the
/// guard stops scheduling, including if serve is cancelled or returns early.
pub(crate) struct HeapMaintenance {
    shutdown: tokio::sync::watch::Sender<bool>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl HeapMaintenance {
    pub(crate) fn start() -> Self {
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        let task = Some(tokio::spawn(run_maintenance(
            rx,
            std::time::Duration::from_secs(60),
            std::sync::Arc::new(|| {
                if let Some((before, after)) = release_freed_memory() {
                    let released = before.saturating_sub(after);
                    if released >= 64 << 20 {
                        tracing::info!(
                            released_mb = released >> 20,
                            rss_mb = after >> 20,
                            "returned freed heap to the OS"
                        );
                    }
                }
            }),
        )));
        #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
        let task = {
            drop(rx);
            None
        };
        Self { shutdown, task }
    }

    pub(crate) async fn stop(mut self) {
        let _ = self.shutdown.send(true);
        if let Some(task) = self.task.take()
            && let Err(error) = task.await
        {
            tracing::warn!(%error, "heap maintenance task failed during shutdown");
        }
    }
}

impl Drop for HeapMaintenance {
    fn drop(&mut self) {
        // spawn_blocking cannot be aborted after starting. Let any current trim
        // finish; the task checks this flag before scheduling another one.
        let _ = self.shutdown.send(true);
    }
}

#[cfg(any(test, all(target_os = "linux", target_env = "gnu")))]
async fn run_maintenance(
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    period: std::time::Duration,
    sweep: std::sync::Arc<dyn Fn() + Send + Sync>,
) {
    let mut interval = tokio::time::interval(period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if *shutdown.borrow() {
            break;
        }
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { break; }
            }
            _ = interval.tick() => {
                if *shutdown.borrow() { break; }
                let sweep = sweep.clone();
                if let Err(error) = tokio::task::spawn_blocking(move || sweep()).await {
                    tracing::warn!(%error, "heap maintenance sweep failed");
                }
            }
        }
    }
}

#[cfg(test)]
mod maintenance_tests {
    use super::*;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    #[tokio::test]
    async fn shutdown_before_start_never_sweeps() {
        let (tx, rx) = tokio::sync::watch::channel(true);
        run_maintenance(
            rx,
            Duration::from_millis(1),
            Arc::new(|| panic!("sweep after shutdown")),
        )
        .await;
        drop(tx);
    }

    #[tokio::test]
    async fn closed_shutdown_channel_never_sweeps() {
        let (tx, rx) = tokio::sync::watch::channel(false);
        drop(tx);
        run_maintenance(
            rx,
            Duration::from_millis(1),
            Arc::new(|| panic!("closed lifecycle")),
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sweeps_repeat_without_any_observability_worker() {
        let (tx, rx) = tokio::sync::watch::channel(false);
        let (events, mut received) = tokio::sync::mpsc::unbounded_channel();
        let task = tokio::spawn(run_maintenance(
            rx,
            Duration::from_millis(10),
            Arc::new(move || {
                events.send(()).unwrap();
            }),
        ));
        for _ in 0..2 {
            tokio::time::timeout(Duration::from_secs(2), received.recv())
                .await
                .unwrap()
                .unwrap();
        }
        tx.send(true).unwrap();
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_waits_for_inflight_sweep_without_overlap() {
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let calls = Arc::new(AtomicUsize::new(0));
        let (started_tx, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = std::sync::Mutex::new(release_rx);
        let count = calls.clone();
        let task = tokio::spawn(run_maintenance(
            rx,
            Duration::from_millis(1),
            Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
                started_tx.send(()).unwrap();
                release_rx.lock().unwrap().recv().unwrap();
            }),
        ));
        tokio::time::timeout(Duration::from_secs(2), started_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let maintenance = HeapMaintenance {
            shutdown,
            task: Some(task),
        };
        let mut stopping = tokio::spawn(maintenance.stop());
        let early = tokio::time::timeout(Duration::from_millis(20), &mut stopping).await;
        let count_before_release = calls.load(Ordering::SeqCst);
        release_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), stopping)
            .await
            .unwrap()
            .unwrap();
        assert!(early.is_err(), "stop must wait for blocking work");
        assert_eq!(count_before_release, 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dropping_lifecycle_guard_stops_scheduling() {
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let observe = rx.clone();
        let task = tokio::spawn(run_maintenance(
            rx,
            Duration::from_secs(60),
            Arc::new(|| {}),
        ));
        let mut maintenance = HeapMaintenance {
            shutdown,
            task: Some(task),
        };
        let mut task = maintenance
            .task
            .take()
            .expect("maintenance task is present on Linux GNU");
        drop(maintenance);
        assert!(*observe.borrow());
        match tokio::time::timeout(Duration::from_secs(2), &mut task).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => panic!("maintenance task failed after drop: {error}"),
            Err(_) => {
                task.abort();
                let _ = task.await;
                panic!("maintenance task did not stop after guard drop");
            }
        }
    }
}
