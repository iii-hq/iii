// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Process shutdown shared by every phase of foreground compose startup.

use std::{
    future::Future,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::watch;

use crate::error::ComposeError;
use crate::error::Result;

/// A latched signal: once interrupted, every clone observes it immediately.
#[derive(Clone)]
pub(crate) struct ShutdownSignal {
    receivers: Vec<watch::Receiver<bool>>,
}

/// One cancellation source owned by the daemon, never a second OS handler.
#[derive(Clone)]
pub(crate) struct ShutdownController {
    sender: watch::Sender<bool>,
    parent: Option<ShutdownSignal>,
}

impl Default for ShutdownController {
    fn default() -> Self {
        Self {
            sender: watch::channel(false).0,
            parent: None,
        }
    }
}

impl ShutdownController {
    pub(crate) fn with_parent(parent: ShutdownSignal) -> Self {
        Self {
            parent: Some(parent),
            ..Self::default()
        }
    }

    pub(crate) fn signal(&self) -> ShutdownSignal {
        let signal = ShutdownSignal::from_receiver(self.sender.subscribe());
        match &self.parent {
            Some(parent) => signal.or(parent.clone()),
            None => signal,
        }
    }

    pub(crate) fn cancel(&self) {
        self.sender.send_replace(true);
    }
}

impl ShutdownSignal {
    /// Installs the OS signal handlers before compose starts any child.
    pub(crate) fn install() -> Result<Self> {
        let (sender, receiver) = watch::channel(false);

        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};

            // `signal` registers with Tokio before returning. A signal cannot
            // therefore take the default terminate path in the gap before the
            // listener task gets its first poll.
            let mut interrupted = signal(SignalKind::interrupt()).map_err(signal_error)?;
            let mut terminated = signal(SignalKind::terminate()).map_err(signal_error)?;
            tokio::spawn(async move {
                loop {
                    let exit_code = tokio::select! {
                        Some(()) = interrupted.recv() => 130,
                        Some(()) = terminated.recv() => 143,
                    };
                    request_shutdown(&sender, exit_code);
                }
            });
        }

        #[cfg(windows)]
        {
            // Like the Unix streams, this installs the handler before it
            // returns, rather than waiting for an async `ctrl_c()` future to
            // receive its first poll.
            let mut interrupted = tokio::signal::windows::ctrl_c().map_err(signal_error)?;
            tokio::spawn(async move {
                while interrupted.recv().await.is_some() {
                    request_shutdown(&sender, 130);
                }
            });
        }

        #[cfg(not(any(unix, windows)))]
        tokio::spawn(async move {
            while tokio::signal::ctrl_c().await.is_ok() {
                request_shutdown(&sender, 130);
            }
        });

        Ok(Self::from_receiver(receiver))
    }

    /// Adapts an existing latched cancellation source to lifecycle shutdown.
    pub(crate) fn from_receiver(receiver: watch::Receiver<bool>) -> Self {
        Self {
            receivers: vec![receiver],
        }
    }

    /// Combines sources without spawning a forwarding task. Already-requested
    /// cancellation is visible synchronously, including before the first poll.
    pub(crate) fn or(mut self, other: Self) -> Self {
        self.receivers.extend(other.receivers);
        self
    }

    pub(crate) fn requested(&self) -> bool {
        self.receivers.iter().any(|receiver| *receiver.borrow())
    }

    pub(crate) async fn wait(&mut self) {
        let waits = self.receivers.iter_mut().map(|receiver| {
            Box::pin(async move {
                loop {
                    if *receiver.borrow_and_update() {
                        return;
                    }
                    if receiver.changed().await.is_err() {
                        std::future::pending::<()>().await;
                    }
                }
            })
        });
        futures::future::select_all(waits).await;
    }

    /// Only for cancellation-safe work: preparation and waiting for locks.
    /// Lifecycle futures must observe the signal themselves and reap children.
    pub(crate) async fn run<T>(&self, work: impl Future<Output = T>) -> Option<T> {
        let mut signal = self.clone();
        tokio::select! {
            biased;
            _ = signal.wait() => None,
            result = work => Some(result),
        }
    }
}

/// Cancellation for synchronous disk work moved off the async executor.
#[derive(Clone, Default)]
pub(crate) struct BlockingCancellation(Arc<AtomicBool>);

impl BlockingCancellation {
    #[cfg(test)]
    pub(crate) fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub(crate) fn check(&self, path: &Path) -> Result<()> {
        if self.0.load(Ordering::Acquire) {
            return Err(ComposeError::Io {
                path: path.to_path_buf(),
                source: std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "preparation cancelled",
                ),
            });
        }
        Ok(())
    }
}

/// Dropping the future requests cooperative cancellation of its blocking job.
/// The job owns any artifact lock and temporary-directory guard until it exits.
/// Aborting a JoinHandle alone would not stop a running spawn_blocking closure.
pub(crate) async fn blocking<T: Send + 'static>(
    path: std::path::PathBuf,
    work: impl FnOnce(BlockingCancellation) -> Result<T> + Send + 'static,
) -> Result<T> {
    struct CancelOnDrop(BlockingCancellation);
    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            self.0.0.store(true, Ordering::Release);
        }
    }
    let guard = CancelOnDrop(BlockingCancellation::default());
    let active = BlockingJob::new();
    let cancel = guard.0.clone();
    tokio::task::spawn_blocking(move || {
        let _active = active;
        work(cancel)
    })
    .await
    .map_err(|error| ComposeError::Io {
        path,
        source: std::io::Error::other(error),
    })?
}

// The CLI exits explicitly rather than dropping its Tokio runtime. Wait for
// cancelled disk jobs to release artifact locks and staging directories first.
struct BlockingJob;
fn blocking_jobs() -> &'static watch::Sender<usize> {
    static JOBS: std::sync::OnceLock<watch::Sender<usize>> = std::sync::OnceLock::new();
    JOBS.get_or_init(|| watch::channel(0).0)
}
impl BlockingJob {
    fn new() -> Self {
        blocking_jobs().send_modify(|count| *count += 1);
        Self
    }
}
impl Drop for BlockingJob {
    fn drop(&mut self) {
        blocking_jobs().send_modify(|count| *count -= 1);
    }
}
pub(crate) async fn drain_blocking_jobs() {
    let mut jobs = blocking_jobs().subscribe();
    let _ = jobs.wait_for(|count| *count == 0).await;
}

// Tokio keeps its handler installed even after a signal stream is dropped.
// Keep listening through teardown so another Ctrl+C remains an escape hatch,
// including when no ShutdownSignal receivers remain alive.
fn request_shutdown(sender: &watch::Sender<bool>, exit_code: i32) {
    if sender.send_replace(true) {
        std::process::exit(exit_code);
    }
}

#[cfg(any(unix, windows))]
fn signal_error(error: std::io::Error) -> ComposeError {
    ComposeError::SpawnFailed {
        container: "<daemon>".to_string(),
        message: format!("could not listen for shutdown signals: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn controller_latches_without_receivers_and_combines_synchronously() {
        let controller = ShutdownController::default();
        controller.cancel();
        let other = ShutdownController::default();
        let mut signal = other.signal().or(controller.signal());
        assert!(signal.requested());
        assert!(signal.run(std::future::ready(42)).await.is_none());
        tokio::time::timeout(std::time::Duration::from_secs(1), signal.wait())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn dropping_blocking_work_requests_cancellation_and_releases_owned_resources() {
        let (started, ready) = tokio::sync::oneshot::channel();
        let (resume, gate) = std::sync::mpsc::channel();
        let (finished, done) = tokio::sync::oneshot::channel();
        let mut job = Box::pin(blocking("blocking-test".into(), move |cancel| {
            started.send(()).unwrap();
            gate.recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            let cancelled = cancel.check(Path::new("blocking-test")).is_err();
            let _ = finished.send(cancelled);
            Ok(())
        }));
        tokio::select! {
            result = &mut job => panic!("blocking job exited before cancellation: {result:?}"),
            result = ready => result.unwrap(),
        }
        drop(job);
        resume.send(()).unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), done)
                .await
                .unwrap()
                .unwrap()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn second_interrupt_exits_during_shutdown() {
        assert_second_signal_exits(nix::sys::signal::Signal::SIGINT).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn interrupt_after_sigterm_exits_during_shutdown() {
        assert_second_signal_exits(nix::sys::signal::Signal::SIGTERM).await;
    }

    #[cfg(unix)]
    async fn assert_second_signal_exits(first: nix::sys::signal::Signal) {
        use nix::{
            sys::signal::{Signal, kill},
            unistd::Pid,
        };
        use std::{process::Stdio, time::Duration};
        use tokio::io::{AsyncBufReadExt, BufReader};

        // Never install process-wide handlers or send signals in the test runner.
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "shutdown::tests::signal_fixture",
                "--ignored",
                "--nocapture",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = Pid::from_raw(child.id().unwrap() as i32);
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(line) = lines.next_line().await.unwrap() {
                if line == "signals-ready" {
                    return;
                }
            }
            panic!("fixture exited before installing its signal handlers");
        })
        .await
        .expect("fixture should install its handlers");

        kill(pid, first).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(line) = lines.next_line().await.unwrap() {
                if line == "shutdown-requested" {
                    return;
                }
            }
            panic!("the first signal must request graceful shutdown, not exit");
        })
        .await
        .expect("the first signal should latch shutdown");

        kill(pid, Signal::SIGINT).unwrap();
        let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .expect("the second Ctrl+C must exit even during stalled teardown")
            .unwrap();
        assert_eq!(status.code(), Some(130));
    }

    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "subprocess fixture for process-wide signal handling"]
    async fn signal_fixture() {
        use std::io::Write;

        let mut shutdown = ShutdownSignal::install().unwrap();
        println!("signals-ready");
        std::io::stdout().flush().unwrap();
        shutdown.wait().await;
        // The escape hatch must outlive the last receiver during teardown.
        drop(shutdown);
        println!("shutdown-requested");
        std::io::stdout().flush().unwrap();
        std::future::pending::<()>().await;
    }

    #[tokio::test]
    async fn combined_signal_latches_when_second_source_is_requested() {
        let (_first_sender, first) = watch::channel(false);
        let (second_sender, second) = watch::channel(false);
        let mut combined =
            ShutdownSignal::from_receiver(first).or(ShutdownSignal::from_receiver(second));

        second_sender.send(true).expect("second receiver is alive");
        tokio::time::timeout(std::time::Duration::from_secs(1), combined.wait())
            .await
            .expect("combined signal should be requested");

        assert!(combined.requested());
    }

    #[tokio::test]
    async fn combined_signal_preserves_a_request_made_before_combining() {
        let (first_sender, first) = watch::channel(false);
        let (_second_sender, second) = watch::channel(false);
        first_sender.send(true).expect("first receiver is alive");
        let mut combined =
            ShutdownSignal::from_receiver(first).or(ShutdownSignal::from_receiver(second));

        tokio::time::timeout(std::time::Duration::from_secs(1), combined.wait())
            .await
            .expect("combined signal should already be requested");

        assert!(combined.requested());
    }
}
