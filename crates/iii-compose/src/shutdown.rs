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
                tokio::select! {
                    _ = interrupted.recv() => {}
                    _ = terminated.recv() => {}
                }
                let _ = sender.send(true);
            });
        }

        #[cfg(windows)]
        {
            // Like the Unix streams, this installs the handler before it
            // returns, rather than waiting for an async `ctrl_c()` future to
            // receive its first poll.
            let mut interrupted = tokio::signal::windows::ctrl_c().map_err(signal_error)?;
            tokio::spawn(async move {
                let _ = interrupted.recv().await;
                let _ = sender.send(true);
            });
        }

        #[cfg(not(any(unix, windows)))]
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            let _ = sender.send(true);
        });

        Ok(Self { receiver })
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

#[cfg(any(unix, windows))]
fn signal_error(error: std::io::Error) -> ComposeError {
    ComposeError::SpawnFailed {
        container: "<daemon>".to_string(),
        message: format!("could not listen for shutdown signals: {error}"),
    }
}
