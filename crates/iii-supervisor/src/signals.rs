// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Outside-the-loop signal handling.
//!
//! If the supervisor receives SIGTERM/SIGINT/SIGHUP (VM shutting down
//! from the outside, e.g. libkrun tearing the VM down), we kill the
//! child cleanly and exit. The control loop is blocked on stdin-style
//! reads, so signal handling runs on a dedicated thread using
//! `sigwait`-style semantics via `signal_hook`-free nix primitives.

use std::thread;

use crate::child::State;

/// Install a background thread that waits on SIGTERM/SIGINT/SIGHUP and,
/// on delivery, kills the child and `exit(0)`s the supervisor process.
///
/// The rationale for `exit` rather than returning from `main`: the
/// control loop is blocked on a read from the virtio-console port, and
/// interrupting that portably across platforms is awkward. Exiting the
/// whole process is simpler, and the kernel/libkrun will observe the
/// exit and power the VM down. Any cleanup that matters (killing the
/// child) has already happened before we exit.
pub fn install(state: State) {
    thread::Builder::new()
        .name("iii-supervisor-signals".to_string())
        .spawn(move || {
            use nix::sys::signal::{SigSet, Signal};
            let mut set = SigSet::empty();
            set.add(Signal::SIGTERM);
            set.add(Signal::SIGINT);
            set.add(Signal::SIGHUP);
            if let Err(e) = set.thread_block() {
                tracing::error!(error = %e, "failed to block signals on handler thread");
                return;
            }
            match set.wait() {
                Ok(sig) => {
                    tracing::info!(signal = ?sig, "supervisor received signal, shutting down");
                    let _ = state.kill_for_shutdown();
                    std::process::exit(0);
                }
                Err(e) => {
                    tracing::error!(error = %e, "sigwait failed");
                }
            }
        })
        .expect("spawn signal thread");
}
