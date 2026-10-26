// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Loads the worker environment the host sends over the `iii.env`
//! virtio-console port (see `iii_supervisor::env_channel`).
//!
//! The frame carries the worker's variables and the worker command
//! (`III_WORKER_CMD`). The rest of iii-init's plumbing (`III_BLOCK_ROOT_*`,
//! `III_WORKER_WORKDIR`, ...) still arrives on the kernel command line. The
//! frame is exported into PID 1's environment, and the worker, supervisor
//! restarts and `iii worker exec` sessions inherit it from there.

use std::fs::OpenOptions;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use iii_supervisor::env_channel::{ENV_PORT_ENV, decode_payload, frame_len};

use crate::InitError;

/// How long to wait for the port to appear and for the host's frame. The
/// host writes the frame as soon as the VM starts, so hitting this means
/// the host side is broken.
const TIMEOUT: Duration = Duration::from_secs(30);

/// Pause between retries while the port is missing or reads as EOF.
const RETRY: Duration = Duration::from_millis(25);

/// Reads the env frame when the host announced one via `III_ENV_PORT` and
/// exports it. No-op when the variable is absent (older hosts).
///
/// Fails closed: a worker started without the environment and command it
/// was given would fail later, and much less clearly, than a boot error
/// naming the port.
///
/// Needs `/sys` and `/dev` (mounted by `mount::mount_filesystems`), and must
/// run while iii-init is still single-threaded (before
/// `supervisor::exec_worker` spawns its helper threads), because it mutates
/// the process environment. `main::run` calls it right after
/// `mount_filesystems`, so the worker env is visible to every later boot
/// step, as it was when the env rode the kernel command line.
pub fn load_worker_env() -> Result<(), InitError> {
    let port_name = match std::env::var(ENV_PORT_ENV) {
        Ok(name) => name,
        Err(_) => return Ok(()),
    };
    let started = Instant::now();
    let deadline = started + TIMEOUT;

    let port_path = wait_for_port(&port_name, deadline).ok_or_else(|| {
        InitError::WorkerEnv(format!(
            "{ENV_PORT_ENV}={port_name} but no such port appeared in /sys/class/virtio-ports \
             within {}s",
            TIMEOUT.as_secs()
        ))
    })?;

    let pairs = read_env_frame(&port_path, started, deadline)
        .map_err(|e| InitError::WorkerEnv(format!("reading {}: {e}", port_path.display())))?;

    // SAFETY: iii-init is single-threaded at this point. `main::run` calls
    // this before `exec_worker`, which is where the shell-dispatcher and
    // source-sync threads are spawned, so nothing can read the environment
    // concurrently. `decode_payload` has already rejected the inputs that
    // make `set_var` panic (empty name, '=' in the name, NUL).
    unsafe {
        for (key, value) in &pairs {
            std::env::set_var(key, value);
        }
        // Plumbing, not worker configuration.
        std::env::remove_var(ENV_PORT_ENV);
    }
    Ok(())
}

/// Port add/name events are asynchronous virtio control traffic, handled by
/// a kernel workqueue; a slow host can still be announcing the port when
/// iii-init gets here.
fn wait_for_port(name: &str, deadline: Instant) -> Option<PathBuf> {
    loop {
        if let Some(path) = iii_supervisor::control::find_virtio_port_by_name(name) {
            return Some(path);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(RETRY);
    }
}

fn read_env_frame(
    port_path: &Path,
    started: Instant,
    deadline: Instant,
) -> io::Result<Vec<(String, String)>> {
    let mut port = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(port_path)?;
    read_frame_patient(&mut port, started, deadline)
}

/// Reads one frame from a non-blocking reader, tolerating the transient EOF
/// a virtio-console port reports before its host backend is connected, and
/// giving up at `deadline`. `started` is when the whole load began (port
/// lookup included), for the timeout message.
fn read_frame_patient<R: Read + AsRawFd>(
    port: &mut R,
    started: Instant,
    deadline: Instant,
) -> io::Result<Vec<(String, String)>> {
    let mut header = [0u8; 4];
    read_exact_patient(port, &mut header, started, deadline)?;
    let mut payload = vec![0u8; frame_len(header)?];
    read_exact_patient(port, &mut payload, started, deadline)?;
    decode_payload(&payload)
}

fn read_exact_patient<R: Read + AsRawFd>(
    port: &mut R,
    buf: &mut [u8],
    started: Instant,
    deadline: Instant,
) -> io::Result<()> {
    let mut filled = 0;
    while filled < buf.len() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "no complete env frame from the host after {:.1}s ({filled} of {} bytes)",
                    started.elapsed().as_secs_f32(),
                    buf.len()
                ),
            ));
        }
        match port.read(&mut buf[filled..]) {
            Ok(n) if n > 0 => filled += n,
            // Host backend not connected yet: the port reads as EOF.
            Ok(_) => std::thread::sleep(RETRY),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                wait_readable(port.as_raw_fd(), remaining)?
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn wait_readable(fd: RawFd, remaining: Duration) -> io::Result<()> {
    let mut pollfd = libc::pollfd {
        fd,
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
        std::thread::sleep(RETRY);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use iii_supervisor::env_channel::encode_frame;
    use std::collections::VecDeque;
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    fn env() -> Vec<(String, String)> {
        vec![
            ("GREETING".to_string(), "Ol\u{e1} \u{1f496}".to_string()),
            ("PEM".to_string(), "a\nb".to_string()),
            (
                "III_WORKER_CMD".to_string(),
                "exec bash /opt/iii/dev-run.sh".to_string(),
            ),
        ]
    }

    fn read_within<R: Read + AsRawFd>(port: &mut R, ms: u64) -> io::Result<Vec<(String, String)>> {
        let now = Instant::now();
        read_frame_patient(port, now, now + Duration::from_millis(ms))
    }

    /// Scripted reads: `Ok(0)` entries model the transient EOF of a port
    /// whose host backend is not connected yet.
    struct Scripted(VecDeque<io::Result<Vec<u8>>>);

    impl Read for Scripted {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match self.0.pop_front() {
                None => Ok(0),
                Some(Err(e)) => Err(e),
                Some(Ok(mut chunk)) => {
                    let n = chunk.len().min(buf.len());
                    buf[..n].copy_from_slice(&chunk[..n]);
                    if n < chunk.len() {
                        self.0.push_front(Ok(chunk.split_off(n)));
                    }
                    Ok(n)
                }
            }
        }
    }

    impl AsRawFd for Scripted {
        fn as_raw_fd(&self) -> RawFd {
            -1 // poll ignores negative fds
        }
    }

    #[test]
    fn frame_split_across_reads_on_a_nonblocking_socket() {
        let (mut host, mut guest) = UnixStream::pair().unwrap();
        guest.set_nonblocking(true).unwrap();
        let frame = encode_frame(&env()).unwrap();
        let writer = std::thread::spawn(move || {
            for chunk in [&frame[..2], &frame[2..7], &frame[7..]] {
                host.write_all(chunk).unwrap();
                std::thread::sleep(Duration::from_millis(20));
            }
            host
        });
        assert_eq!(read_within(&mut guest, 5_000).unwrap(), env());
        drop(writer.join().unwrap());
    }

    #[test]
    fn frame_larger_than_the_socket_buffer_arrives_whole() {
        let (mut host, mut guest) = UnixStream::pair().unwrap();
        guest.set_nonblocking(true).unwrap();
        // Over a Linux socketpair's ~180 KiB, so the writer really blocks;
        // each variable stays under the 128 KiB execve limit.
        let big: Vec<(String, String)> = (0..3)
            .map(|i| (format!("BIG{i}"), "x".repeat(100 * 1024)))
            .collect();
        let frame = encode_frame(&big).unwrap();
        let writer = std::thread::spawn(move || host.write_all(&frame).map(|_| host));
        assert_eq!(read_within(&mut guest, 5_000).unwrap(), big);
        drop(writer.join().unwrap().unwrap());
    }

    #[test]
    fn transient_eof_before_the_host_connects_is_retried() {
        let frame = encode_frame(&env()).unwrap();
        let mut port = Scripted(VecDeque::from(vec![
            Ok(vec![]),
            Err(io::ErrorKind::Interrupted.into()),
            Ok(vec![]),
            Ok(frame[..3].to_vec()),
            Ok(vec![]),
            Ok(frame[3..].to_vec()),
        ]));
        assert_eq!(read_within(&mut port, 5_000).unwrap(), env());
    }

    #[test]
    fn incomplete_frame_times_out_without_echoing_the_payload() {
        let (mut host, mut guest) = UnixStream::pair().unwrap();
        guest.set_nonblocking(true).unwrap();
        let frame = encode_frame(&[("API_KEY".to_string(), "sk-secret".to_string())]).unwrap();
        host.write_all(&frame[..frame.len() - 3]).unwrap();
        let err = read_within(&mut guest, 300).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        let message = err.to_string();
        assert!(message.contains("bytes"), "{message}");
        assert!(!message.contains("sk-secret"), "{message}");
        drop(host);
    }

    #[test]
    fn closed_host_end_times_out_instead_of_spinning_forever() {
        let mut port = Scripted(VecDeque::new()); // EOF on every read
        let err = read_within(&mut port, 150).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn oversized_header_is_rejected() {
        let header = (u32::MAX).to_be_bytes().to_vec();
        let mut port = Scripted(VecDeque::from(vec![Ok(header)]));
        let err = read_within(&mut port, 1_000).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn missing_port_gives_up_at_the_deadline() {
        assert!(
            wait_for_port(
                "iii.env-test-does-not-exist",
                Instant::now() + Duration::from_millis(60)
            )
            .is_none()
        );
    }
}
