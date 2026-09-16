// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Process shutdown shared by every phase of foreground compose startup.

use tokio::sync::watch;

#[cfg(any(unix, windows))]
use crate::error::ComposeError;
use crate::error::Result;

/// A latched signal: once interrupted, every clone observes it immediately.
#[derive(Clone)]
pub(crate) struct ShutdownSignal {
    receiver: watch::Receiver<bool>,
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

        Ok(Self { receiver })
    }

    /// Adapts an existing latched cancellation source to lifecycle shutdown.
    pub(crate) fn from_receiver(receiver: watch::Receiver<bool>) -> Self {
        Self { receiver }
    }

    /// Returns a signal that latches when either input is requested.
    pub(crate) fn or(mut self, mut other: Self) -> Self {
        let requested = self.requested() || other.requested();
        let (sender, receiver) = watch::channel(requested);
        if !requested {
            tokio::spawn(async move {
                tokio::select! {
                    _ = self.wait() => {}
                    _ = other.wait() => {}
                }
                let _ = sender.send(true);
            });
        }
        Self { receiver }
    }

    pub(crate) fn requested(&self) -> bool {
        *self.receiver.borrow()
    }

    pub(crate) async fn wait(&mut self) {
        if self.requested() {
            return;
        }
        while self.receiver.changed().await.is_ok() {
            if self.requested() {
                return;
            }
        }
        // The sender lives until it publishes a shutdown request. A closed
        // channel here only happens while the runtime itself is going away.
        std::future::pending::<()>().await;
    }
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
