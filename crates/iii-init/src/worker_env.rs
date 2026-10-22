// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Loads the worker environment the host sends over the `iii.env`
//! virtio-console port (see `iii_supervisor::env_channel`).
//!
//! Only iii-init's own plumbing (`III_WORKER_CMD`, `III_BLOCK_ROOT_*`, ...)
//! still arrives on the kernel command line. The worker's variables are read
//! here, exported into PID 1's environment, and inherited from there by the
//! worker, by supervisor restarts and by `iii worker exec` sessions.

use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use iii_supervisor::env_channel::{ENV_PORT_ENV, decode_payload, frame_len};

use crate::InitError;

/// How long to wait for the host's frame. The host writes it as soon as the
/// VM starts, so hitting this means the host side is broken.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Reads the env frame when the host announced one via `III_ENV_PORT` and
/// exports it. No-op when the variable is absent (older hosts, or a VM
/// started without worker env).
///
/// Fails closed: a worker started without the environment it was given
/// (credentials, engine URL) would misbehave in ways that are much harder
/// to diagnose than a boot error naming the port.
///
/// Must run while iii-init is still single-threaded (before
/// `supervisor::exec_worker` spawns its helper threads): it mutates the
/// process environment.
pub fn load_worker_env() -> Result<(), InitError> {
    let port_name = match std::env::var(ENV_PORT_ENV) {
        Ok(name) => name,
        Err(_) => return Ok(()),
    };

    let port_path =
        iii_supervisor::control::find_virtio_port_by_name(&port_name).ok_or_else(|| {
            InitError::WorkerEnv(format!(
                "{ENV_PORT_ENV}={port_name} but no such port in /sys/class/virtio-ports"
            ))
        })?;

    let pairs = read_env_frame(&port_path, READ_TIMEOUT)
        .map_err(|e| InitError::WorkerEnv(format!("reading {}: {e}", port_path.display())))?;

    // SAFETY: iii-init is single-threaded at this point. `main::run` calls
    // this before `exec_worker`, which is where the shell-dispatcher and
    // source-sync threads are spawned, so nothing can read the environment
    // concurrently.
    unsafe {
        for (key, value) in &pairs {
            std::env::set_var(key, value);
        }
        // Plumbing, not worker configuration.
        std::env::remove_var(ENV_PORT_ENV);
    }
    Ok(())
}

/// Reads one frame from the port, tolerating the transient EOF a
/// virtio-console port reports before its host backend is connected, and
/// giving up at `timeout`.
fn read_env_frame(port_path: &Path, timeout: Duration) -> io::Result<Vec<(String, String)>> {
    let mut port = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(port_path)?;
    let deadline = Instant::now() + timeout;

    let mut header = [0u8; 4];
    read_exact_patient(&mut port, &mut header, deadline)?;
    let mut payload = vec![0u8; frame_len(header)?];
    read_exact_patient(&mut port, &mut payload, deadline)?;
    decode_payload(&payload)
}

fn read_exact_patient(port: &mut File, buf: &mut [u8], deadline: Instant) -> io::Result<()> {
    let mut filled = 0;
    while filled < buf.len() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "no env frame from the host after {}s ({filled} of {} bytes)",
                    READ_TIMEOUT.as_secs(),
                    buf.len()
                ),
            ));
        }
        match port.read(&mut buf[filled..]) {
            Ok(n) if n > 0 => filled += n,
            // Host backend not connected yet: the port reads as EOF.
            Ok(_) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => wait_readable(port, remaining)?,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn wait_readable(port: &File, remaining: Duration) -> io::Result<()> {
    let mut pollfd = libc::pollfd {
        fd: port.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // Short slices so the deadline is re-checked even if poll keeps
    // reporting a hangup without data.
    let slice_ms = remaining.as_millis().clamp(1, 100) as libc::c_int;
    // SAFETY: one valid pollfd on the stack, count 1.
    let rc = unsafe { libc::poll(&mut pollfd, 1, slice_ms) };
    if rc < 0 {
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err);
        }
    }
    if rc > 0 && pollfd.revents & libc::POLLHUP != 0 && pollfd.revents & libc::POLLIN == 0 {
        std::thread::sleep(Duration::from_millis(25));
    }
    Ok(())
}
