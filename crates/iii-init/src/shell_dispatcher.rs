// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Guest-side dispatcher for the `iii.exec` virtio-console shell channel.
//!
//! Runs on a dedicated thread alongside the existing supervisor control
//! thread. Reads multiplexed frames from the port (one stream, many
//! concurrent sessions distinguished by `corr_id`), spawns child
//! processes in pipe mode or TTY mode, and streams their output back
//! on the same port. TTY mode allocates a pseudo-terminal via
//! `openpty(3)` and points the child's stdin/stdout/stderr at the
//! slave so `isatty(3)` says yes — needed for interactive shells,
//! line editors, and any program that toggles buffering based on
//! terminal detection.
//!
//! Threading model:
//!
//! - One reader thread (the `run` call) owns the port read half and
//!   dispatches incoming frames.
//! - Per-session stdout/stderr reader threads stream child output
//!   back through the shared writer.
//! - Per-session wait thread observes child exit and sends `Exited`
//!   with the terminal flag.
//! - All writes funnel through `Arc<Mutex<File>>` so frames don't
//!   interleave on the wire.
//!
//! Signals, stdin EOF, and cleanup are all routed by `corr_id` via the
//! shared `SessionRegistry`; session entries are removed after the
//! wait thread has emitted the terminal `Exited` frame.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;

use iii_supervisor::shell_protocol::{self as sp, ShellMessage, flags::FLAG_TERMINAL};

/// One live exec session tracked by the dispatcher.
///
/// The only state we keep per session is what inbound-frame handling
/// needs: the pid (to deliver signals), a pidfd for PID-reuse-safe
/// signal delivery, and the open stdin handle (to forward input bytes,
/// or drop to signal EOF). Stdout/stderr reader threads and the waiter
/// thread own their own `Child` pieces and finish independently — the
/// waiter removes the entry from the registry after emitting the
/// terminal frame.
struct SessionHandle {
    pid: u32,
    /// Pidfd for the child, opened right after spawn. Used by the
    /// Signal handler so `pidfd_send_signal(2)` delivers to the exact
    /// process we spawned even if PID 1's reap loop has already
    /// waitpid'd and the kernel has recycled the PID to another
    /// process. `None` on kernels < 5.3 where `pidfd_open(2)` is not
    /// available — Signal delivery then falls back to `libc::kill`
    /// with the documented TOCTOU caveat. Dropped with the session
    /// entry, which closes the fd.
    pidfd: Option<OwnedFd>,
    /// Pipe-mode stdin. `None` after the host has sent an empty Stdin
    /// frame (EOF) or after the child has exited. Writes are silently
    /// ignored in either case so a misbehaving peer can't panic the
    /// dispatcher.
    ///
    /// Wrapped in `Arc<Mutex<_>>` so the `Stdin` handler can clone the
    /// handle under the session-map lock, drop that lock, and then
    /// call `write_all` outside it. Without the wrap, a child with a
    /// full stdin pipe would block `write_all` while the session-map
    /// lock is held, starving every waiter thread and every other
    /// session's spawn/cleanup path. (The reader loop itself is still
    /// serialized on stdin writes — a full isolation would need a
    /// per-session writer thread; tracked as a follow-up.)
    stdin: Option<Arc<Mutex<ChildStdin>>>,
    /// TTY-mode master fd (wrapped as a `File` so it implements
    /// `Write`). `Some` iff the session was spawned with `tty: true`.
    /// Used for both stdin-forwarding and `TIOCSWINSZ` on Resize.
    /// A session is exactly one of pipe or TTY — never both.
    master: Option<Arc<Mutex<File>>>,
}

type SessionRegistry = Arc<Mutex<HashMap<u32, SessionHandle>>>;

/// Per-inbound-frame write target selected under the session-map
/// lock. The actual write happens after the lock is released so a
/// backpressured child can't stall the dispatcher's inbound path.
enum WriteTarget {
    Tty(Arc<Mutex<File>>),
    Pipe(Arc<Mutex<ChildStdin>>),
}

/// Writer handle shared by every thread that emits frames. A single
/// dedicated writer thread drains this channel onto the virtio-console
/// port, so no caller ever blocks across a virtio write. If the guest
/// side of the port wedges (host relay gone, kernel ring full), the
/// channel saturates at [`WRITER_CHANNEL_CAPACITY`] and further
/// `try_send`s drop frames with a log line — session threads keep
/// running instead of freezing under a shared mutex.
type Writer = SyncSender<Vec<u8>>;

/// Bound on queued frames between session threads and the writer
/// thread. Roughly 1024 × 8 KiB worst case ≈ 8 MiB in flight. Large
/// enough that a short virtio stall never drops frames; small enough
/// that a permanent stall (e.g. host relay dead) can't balloon guest
/// memory. Chosen empirically, not tuned.
const WRITER_CHANNEL_CAPACITY: usize = 1024;

/// How many bytes of child output we ship per frame. Chosen to balance
/// overhead (frame header + JSON envelope + base64 expansion) against
/// latency — 8 KiB lets `echo` + `cat` feel snappy without allocating
/// megabyte buffers for a wedged `tail -f`.
const IO_CHUNK_SIZE: usize = 8 * 1024;

/// Open a pidfd for `pid` via the raw `pidfd_open(2)` syscall.
///
/// Returns `None` when the kernel doesn't support the syscall
/// (EINVAL / ENOSYS on pre-5.3) or the process is already gone
/// (ESRCH — PID 1's reap loop raced us). Signal delivery falls back
/// to `libc::kill` when the pidfd is absent, with the documented
/// PID-reuse caveat.
///
/// The returned `OwnedFd` closes the kernel fd on drop, so storing
/// it on `SessionHandle` keeps the fd alive for the session lifetime
/// and releases it when the waiter thread's `remove_session_if_owned`
/// removes the entry.
fn pidfd_open(pid: u32) -> Option<OwnedFd> {
    let ret = unsafe {
        libc::syscall(
            libc::SYS_pidfd_open,
            pid as libc::pid_t,
            0u32 as libc::c_uint,
        )
    };
    if ret < 0 {
        None
    } else {
        Some(unsafe { OwnedFd::from_raw_fd(ret as std::os::fd::RawFd) })
    }
}

/// Deliver `signal` via `pidfd_send_signal(2)`. Error return is
/// swallowed — ESRCH (child gone) and EINVAL (invalid signal) are
/// both benign in normal operation; Signal is best-effort.
fn pidfd_send_signal(pidfd: &OwnedFd, signal: i32) {
    unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            pidfd.as_raw_fd(),
            signal as libc::c_int,
            std::ptr::null::<libc::siginfo_t>(),
            0u32 as libc::c_uint,
        );
    }
}

#[cfg(test)]
thread_local! {
    /// Test hook: when set, `clone_master` returns Err(EMFILE) instead
    /// of actually dup'ing. Used to exercise the post-spawn cleanup
    /// path in `spawn_tty_session` that SIGKILLs the child and
    /// unregisters it from `child_exits`. Thread-local so parallel
    /// test execution doesn't cross-contaminate.
    static FORCE_MASTER_CLONE_FAIL: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

/// Thin wrapper over `File::try_clone` so tests can inject an EMFILE
/// failure without actually exhausting fds.
fn clone_master(master: &File) -> std::io::Result<File> {
    #[cfg(test)]
    {
        if FORCE_MASTER_CLONE_FAIL.with(|c| c.get()) {
            return Err(std::io::Error::from_raw_os_error(libc::EMFILE));
        }
    }
    master.try_clone()
}

/// Open `port_path` and run the dispatcher loop until the host closes
/// the channel. Blocks the calling thread — the caller is expected to
/// be a dedicated thread spawned from `iii-init`.
///
/// Returns `Ok(())` on clean EOF, `Err` if the port itself fails
/// (rare — a wedged session produces a per-session `Error` frame and
/// does not bring the dispatcher down).
pub fn run(port_path: &Path) -> anyhow::Result<()> {
    let writer_file = OpenOptions::new().read(true).write(true).open(port_path)?;
    let reader_file = writer_file.try_clone()?;
    let writer = spawn_writer_thread(writer_file);
    let sessions: SessionRegistry = Arc::new(Mutex::new(HashMap::new()));

    let mut reader = BufReader::new(reader_file);
    loop {
        match sp::read_frame_blocking(&mut reader) {
            Ok(Some((corr_id, _flags, msg))) => {
                handle_frame(corr_id, msg, &sessions, &writer);
            }
            Ok(None) => break,
            Err(e) => {
                // A frame-level error (bad length, truncated body) is
                // usually fatal for this channel — the peer is out of
                // sync. Log and drop the reader; sessions already
                // running keep writing to the port, which the host
                // relay will also bail on once it sees the same
                // framing trouble.
                eprintln!("iii-init: shell dispatcher read error: {e}");
                break;
            }
        }
    }
    Ok(())
}

/// Spawn the single writer thread that owns the virtio-console port's
/// write half. All session threads push frames into its channel; the
/// thread drains them serially, so no other thread ever blocks on
/// virtio I/O. Mirrors the host relay's `vm_writer_task` design.
///
/// The returned [`SyncSender`] is the sole handle callers use to emit
/// frames; dropping all clones closes the channel and the writer
/// thread exits cleanly. If `write_all` fails (port closed, host
/// relay torn down), the thread logs and exits — further session
/// sends silently no-op as the channel becomes disconnected.
fn spawn_writer_thread(mut file: File) -> Writer {
    let (tx, rx) = sync_channel::<Vec<u8>>(WRITER_CHANNEL_CAPACITY);
    thread::Builder::new()
        .name("iii-init-shell-writer".to_string())
        .spawn(move || {
            while let Ok(frame) = rx.recv() {
                if let Err(e) = file.write_all(&frame) {
                    eprintln!("iii-init: shell dispatcher writer failed: {e}");
                    break;
                }
            }
        })
        .expect("spawn writer thread");
    tx
}

fn handle_frame(corr_id: u32, msg: ShellMessage, sessions: &SessionRegistry, writer: &Writer) {
    match msg {
        ShellMessage::Request {
            cmd,
            args,
            env,
            cwd,
            tty,
            rows,
            cols,
        } => {
            let spawn_result = if tty {
                spawn_tty_session(corr_id, cmd, args, env, cwd, rows, cols, sessions, writer)
            } else {
                spawn_pipe_session(corr_id, cmd, args, env, cwd, sessions, writer)
            };
            if let Err(e) = spawn_result {
                send_frame(
                    writer,
                    corr_id,
                    FLAG_TERMINAL,
                    &ShellMessage::Error {
                        message: format!("spawn: {e}"),
                    },
                );
            }
        }
        ShellMessage::Stdin { data_b64 } => {
            let decoded = match decode_b64(&data_b64) {
                Ok(d) => d,
                Err(_) => return, // drop malformed; nothing useful to do
            };
            // Lock scope tight: extract either the master Arc (TTY
            // mode), take the stdin Arc (EOF), or clone the stdin Arc
            // (data). Never hold the map lock across the actual write
            // — a child with a backpressured stdin pipe could
            // otherwise wedge every waiter thread and every other
            // session's spawn/cleanup path under this single lock.
            let write_target = {
                let mut map = sessions.lock().expect("session map mutex poisoned");
                match map.get_mut(&corr_id) {
                    None => None,
                    Some(session) => {
                        if let Some(master) = session.master.clone() {
                            Some(WriteTarget::Tty(master))
                        } else if decoded.is_empty() {
                            // Pipe-mode EOF — drop the session's
                            // stdin Arc so the last strong reference
                            // goes away (once any concurrent write
                            // completes) and the child sees EOF on
                            // its read.
                            session.stdin.take();
                            None
                        } else {
                            session.stdin.as_ref().map(|s| WriteTarget::Pipe(s.clone()))
                        }
                    }
                }
            };
            match write_target {
                Some(WriteTarget::Tty(master)) => {
                    // TTY mode: write to master. Empty frame is a
                    // no-op — in a TTY, EOF is a line-discipline
                    // convention (Ctrl-D = 0x04) sent as a byte, not
                    // a side-channel. Closing the master would hang
                    // up the child's controlling TTY and deliver
                    // SIGHUP, which is not what an empty Stdin means.
                    if !decoded.is_empty() {
                        let mut file = master.lock().expect("master mutex poisoned");
                        let _ = file.write_all(&decoded);
                    }
                }
                Some(WriteTarget::Pipe(stdin)) => {
                    // Best-effort: if the child has closed stdin we
                    // silently drop the bytes. A broken stdin is not
                    // worth a wire-level error frame; the child will
                    // exit on its own and the Exited frame carries
                    // the status.
                    let mut guard = stdin.lock().expect("stdin mutex poisoned");
                    let _ = guard.write_all(&decoded);
                }
                None => {}
            }
        }
        ShellMessage::Signal { signal } => {
            if signal <= 0 {
                return;
            }
            // Hold the lock across the syscall: both pidfd_send_signal
            // and libc::kill are non-blocking, and keeping the lock
            // prevents a racing waiter from dropping the pidfd between
            // our lookup and the send. The lock is not held across any
            // blocking I/O here, so other frame types don't starve.
            let map = sessions.lock().expect("session map mutex poisoned");
            if let Some(session) = map.get(&corr_id) {
                if let Some(ref pidfd) = session.pidfd {
                    // pidfd_send_signal is immune to PID reuse — the
                    // kernel routes by the original process even if
                    // its PID has been recycled.
                    pidfd_send_signal(pidfd, signal);
                } else {
                    // Pre-5.3 fallback: subject to the documented
                    // PID-reuse race window between PID 1's reap and
                    // our kill. On current kernels this branch is
                    // unreachable because pidfd_open succeeds at
                    // spawn time.
                    unsafe {
                        libc::kill(session.pid as libc::pid_t, signal as libc::c_int);
                    }
                }
            }
        }
        ShellMessage::Resize { rows, cols } => {
            let master = {
                let map = sessions.lock().expect("session map mutex poisoned");
                map.get(&corr_id).and_then(|s| s.master.clone())
            };
            if let Some(master) = master {
                let ws = libc::winsize {
                    ws_row: rows,
                    ws_col: cols,
                    ws_xpixel: 0,
                    ws_ypixel: 0,
                };
                let guard = master.lock().expect("master mutex poisoned");
                // Best-effort: if TIOCSWINSZ fails (unlikely on a
                // healthy PTY master) the child keeps its previous
                // size; nothing else we can do without tearing down
                // the session.
                unsafe {
                    libc::ioctl(guard.as_raw_fd(), libc::TIOCSWINSZ, &ws);
                }
            }
        }
        // Host-only variants received from the host are ignored; only
        // Request/Stdin/Signal/Resize make sense as inbound. Anything
        // else is a peer protocol error — dropping is less noisy
        // than responding with Error and keeps the channel alive.
        _ => {}
    }
}

#[allow(clippy::zombie_processes)]
fn spawn_pipe_session(
    corr_id: u32,
    cmd: String,
    args: Vec<String>,
    env: Vec<String>,
    cwd: Option<String>,
    sessions: &SessionRegistry,
    writer: &Writer,
) -> anyhow::Result<()> {
    let mut command = Command::new(&cmd);
    command
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd.as_deref() {
        command.current_dir(dir);
    }
    for kv in &env {
        if let Some((k, v)) = kv.split_once('=') {
            command.env(k, v);
        }
    }
    // Race-free: the `child_exits` registry lock is held across
    // `spawn`, so PID 1's reap loop cannot observe and discard the
    // child's exit before we register as its owner. The inner
    // closure owns the spawn error only long enough to thread it
    // back out through the `Option` return.
    let mut spawn_err: Option<std::io::Error> = None;
    let mut maybe_child: Option<std::process::Child> = None;
    let reg = crate::child_exits::register_spawn(|| match command.spawn() {
        Ok(c) => {
            let pid = c.id();
            maybe_child = Some(c);
            Some(pid)
        }
        Err(e) => {
            spawn_err = Some(e);
            None
        }
    });
    let (pid, exit_rx) = match reg {
        Some(v) => v,
        None => {
            let e = spawn_err
                .unwrap_or_else(|| std::io::Error::other("spawn returned None without error"));
            return Err(e.into());
        }
    };
    let mut child = maybe_child.expect("register_spawn returned pid without child");
    let stdout = child.stdout.take().expect("stdout requested");
    let stderr = child.stderr.take().expect("stderr requested");
    let stdin = child.stdin.take().map(|s| Arc::new(Mutex::new(s)));
    // Open a pidfd as early as possible after spawn so Signal
    // handlers route by the kernel's pidfd instead of the raw pid.
    // If it fails (pre-5.3 kernel, or ESRCH because PID 1 already
    // reaped), we store None and Signal falls back to libc::kill.
    let pidfd = pidfd_open(pid);

    // Register before announcing so a racing Stdin frame finds the
    // entry.
    sessions.lock().expect("session map mutex poisoned").insert(
        corr_id,
        SessionHandle {
            pid,
            pidfd,
            stdin,
            master: None,
        },
    );

    send_frame(writer, corr_id, 0, &ShellMessage::Started { pid });

    // Stream stdout / stderr in dedicated threads so neither can
    // starve the other. Each thread emits Stdout/Stderr frames until
    // its pipe closes (child exit or explicit fd close). We keep the
    // `JoinHandle`s so the waiter thread can block on them before
    // emitting the terminal `Exited` frame — otherwise a fast-exiting
    // child can get its Exited frame out before the reader threads
    // have drained remaining output, and the host relay will drop
    // those late Stdout/Stderr frames (FLAG_TERMINAL on Exited
    // removes the route).
    let stdout_handle = {
        let writer = writer.clone();
        thread::Builder::new()
            .name(format!("iii-exec-{corr_id}-out"))
            .spawn(move || stream_fd(writer, corr_id, stdout, OutputKind::Stdout))
            .ok()
    };
    let stderr_handle = {
        let writer = writer.clone();
        thread::Builder::new()
            .name(format!("iii-exec-{corr_id}-err"))
            .spawn(move || stream_fd(writer, corr_id, stderr, OutputKind::Stderr))
            .ok()
    };

    // Waiter thread: the PID-1 reap loop (in `supervisor.rs`) will
    // observe the child's exit and forward the code via `exit_rx`.
    // We deliberately do NOT call `child.wait()` — PID 1 is the only
    // reaper; calling wait here would race with it and hang. Instead
    // we receive the exit code from the shared `child_exits`
    // registry and drop the `Child` handle after: by then the kernel
    // has already reaped the process, and `Drop for Child` is a
    // no-op on an already-dead child.
    //
    // Before emitting the terminal `Exited` frame we join the
    // stdout/stderr reader threads so the host sees every byte of
    // child output before the route is torn down.
    {
        let writer_owned = writer.clone();
        let sessions_owned = sessions.clone();
        let spawn_result = thread::Builder::new()
            .name(format!("iii-exec-{corr_id}-wait"))
            .spawn(move || {
                let code = exit_rx.recv().unwrap_or(-1);
                drop(child);
                if let Some(h) = stdout_handle {
                    let _ = h.join();
                }
                if let Some(h) = stderr_handle {
                    let _ = h.join();
                }
                send_frame(
                    &writer_owned,
                    corr_id,
                    FLAG_TERMINAL,
                    &ShellMessage::Exited { code },
                );
                // pid-match before removing: if the host side has
                // already reassigned this corr_id to a new session
                // (e.g. after a client disconnect + reclaim), the
                // entry here belongs to someone else. Clobbering it
                // would orphan the new session in the registry and
                // cause Signal/Resize/Stdin frames for it to drop.
                remove_session_if_owned(&sessions_owned, corr_id, pid);
            });
        if let Err(e) = spawn_result {
            // No waiter means no Exited frame, no map cleanup, and
            // a running child nobody is watching. Tear it all down
            // synchronously rather than leaving the host to time out.
            abort_session_on_waiter_spawn_failure(writer, sessions, corr_id, pid, &e.to_string());
            return Ok(());
        }
    }

    Ok(())
}

/// Spawn a child attached to a freshly-allocated PTY. The slave side
/// becomes the child's stdin/stdout/stderr and its controlling
/// terminal; the master side stays on the dispatcher for stream-out
/// and Resize/Stdin plumbing.
///
/// Unlike the pipe path, TTY sessions have a single reader thread
/// (the PTY merges stdout and stderr onto the master) and emit all
/// output as `ShellMessage::Stdout`. If the user cares about separate
/// streams they should use pipe mode.
///
/// clippy::zombie_processes is disabled because PID 1 is the reaper,
/// not us. The waiter thread receives the exit code via the shared
/// `child_exits` registry and drops the `Child` handle after — by
/// then the kernel has reaped. See the comment on the waiter thread
/// in `spawn_pipe_session` for the full rationale.
#[allow(clippy::too_many_arguments, clippy::zombie_processes)]
fn spawn_tty_session(
    corr_id: u32,
    cmd: String,
    args: Vec<String>,
    env: Vec<String>,
    cwd: Option<String>,
    rows: u16,
    cols: u16,
    sessions: &SessionRegistry,
    writer: &Writer,
) -> anyhow::Result<()> {
    use nix::pty::openpty;

    let pty = openpty(None, None)?;
    let master_owned = pty.master;
    let slave_owned = pty.slave;

    // Apply the initial window size on the master before spawning —
    // this becomes the child's TTY size via the slave it inherits.
    // TIOCSWINSZ on the master is the canonical way to resize a PTY
    // session and triggers SIGWINCH delivery to the foreground group.
    let ws = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let ret = unsafe { libc::ioctl(master_owned.as_raw_fd(), libc::TIOCSWINSZ, &ws) };
    if ret < 0 {
        return Err(std::io::Error::last_os_error().into());
    }

    // The child inherits the slave three times via Stdio::from, which
    // takes ownership. Dup so we hand out three independent fds and
    // keep one last copy for the pre_exec controlling-tty dance.
    let slave_stdin = slave_owned.try_clone()?;
    let slave_stdout = slave_owned.try_clone()?;
    let slave_stderr = slave_owned;

    let mut command = Command::new(&cmd);
    command
        .args(&args)
        .stdin(Stdio::from(slave_stdin))
        .stdout(Stdio::from(slave_stdout))
        .stderr(Stdio::from(slave_stderr));
    if let Some(dir) = cwd.as_deref() {
        command.current_dir(dir);
    }
    for kv in &env {
        if let Some((k, v)) = kv.split_once('=') {
            command.env(k, v);
        }
    }
    // pre_exec runs in the child between fork and the spawn's final
    // syscall. Only async-signal-safe syscalls allowed — no
    // allocations, no Rust collections, no locks.
    //
    // Step 1: setsid() — detach from the parent's session so the
    //   child becomes a session leader and can acquire its own
    //   controlling terminal.
    // Step 2: TIOCSCTTY on fd 0 — which is already the slave thanks
    //   to Stdio::from above — makes that slave the session's
    //   controlling TTY. From here `isatty(0)` says yes and job
    //   control works (SIGINT on Ctrl-C, etc).
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    // Race-free spawn (same pattern as pipe mode — see the long note
    // on `register_spawn` there).
    let mut spawn_err: Option<std::io::Error> = None;
    let mut maybe_child: Option<std::process::Child> = None;
    let reg = crate::child_exits::register_spawn(|| match command.spawn() {
        Ok(c) => {
            let pid = c.id();
            maybe_child = Some(c);
            Some(pid)
        }
        Err(e) => {
            spawn_err = Some(e);
            None
        }
    });
    let (pid, exit_rx) = match reg {
        Some(v) => v,
        None => {
            let e = spawn_err
                .unwrap_or_else(|| std::io::Error::other("spawn returned None without error"));
            return Err(e.into());
        }
    };
    let child = maybe_child.expect("register_spawn returned pid without child");

    // Master end: wrap as File so we get blocking Read/Write. Dup
    // once for the reader thread; the original goes into the session
    // map (Arc<Mutex<File>>) for inbound Stdin + Resize.
    //
    // try_clone can fail (EMFILE under FD pressure). At this point the
    // child is already spawned and registered with `child_exits`, so a
    // bare `?` would orphan: PID 1 would dispatch the exit code to an
    // `exit_rx` nobody's receiving on and the registry entry would
    // leak.
    //
    // Cleanup order matters: SIGKILL FIRST (while the registry still
    // claims this pid, so PID 1 routes the exit code into the sender
    // we're about to drop, rather than invoking its default orphan
    // reap — which in the tiny window between unregister and kill
    // could let the kernel recycle the pid to a different process and
    // deliver our SIGKILL to the wrong target). THEN unregister,
    // which drops the sender — any buffered code is discarded.
    // `Child::drop` is a no-op on Unix (no waitpid), so no extra step
    // is needed for the handle; it falls out of scope at return.
    let master_file: File = unsafe { File::from_raw_fd(master_owned.into_raw_fd()) };
    let master_reader = match clone_master(&master_file) {
        Ok(f) => f,
        Err(e) => {
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGKILL);
            }
            crate::child_exits::unregister(pid);
            let _ = child;
            return Err(e.into());
        }
    };
    let master = Arc::new(Mutex::new(master_file));
    // Pidfd captured after the master-clone cleanup path so a
    // try_clone failure doesn't leak the fd — if we get past this
    // point the session is live and the pidfd lives until teardown.
    let pidfd = pidfd_open(pid);

    sessions.lock().expect("session map mutex poisoned").insert(
        corr_id,
        SessionHandle {
            pid,
            pidfd,
            stdin: None,
            master: Some(master.clone()),
        },
    );

    send_frame(writer, corr_id, 0, &ShellMessage::Started { pid });

    // One reader thread: PTY merges stdout and stderr onto the
    // master, so we emit everything as Stdout frames. The reader
    // exits when the master sees EIO (child exited + slave closed).
    let output_handle = {
        let writer = writer.clone();
        thread::Builder::new()
            .name(format!("iii-exec-{corr_id}-tty"))
            .spawn(move || stream_fd(writer, corr_id, master_reader, OutputKind::Stdout))
            .ok()
    };

    // Waiter thread: same contract as pipe mode — join the reader so
    // no output frames race past the terminal Exited frame.
    {
        let writer_owned = writer.clone();
        let sessions_owned = sessions.clone();
        let spawn_result = thread::Builder::new()
            .name(format!("iii-exec-{corr_id}-wait"))
            .spawn(move || {
                let code = exit_rx.recv().unwrap_or(-1);
                drop(child);
                if let Some(h) = output_handle {
                    let _ = h.join();
                }
                send_frame(
                    &writer_owned,
                    corr_id,
                    FLAG_TERMINAL,
                    &ShellMessage::Exited { code },
                );
                // See spawn_pipe_session for the ownership-check
                // rationale.
                remove_session_if_owned(&sessions_owned, corr_id, pid);
            });
        if let Err(e) = spawn_result {
            // See spawn_pipe_session for the abort rationale.
            abort_session_on_waiter_spawn_failure(writer, sessions, corr_id, pid, &e.to_string());
            return Ok(());
        }
    }

    Ok(())
}

/// Remove `corr_id` from the session registry iff the entry still
/// belongs to `expected_pid`. Protects against clobbering a freshly
/// installed session that reused the same correlation id after a
/// host-side reclaim.
fn remove_session_if_owned(sessions: &SessionRegistry, corr_id: u32, expected_pid: u32) {
    let mut map = sessions.lock().expect("session map mutex poisoned");
    if let Some(entry) = map.get(&corr_id)
        && entry.pid == expected_pid
    {
        map.remove(&corr_id);
    }
}

/// Cleanup for the rare case where spawning the per-session waiter
/// thread fails (EAGAIN on RLIMIT_NPROC, OOM on thread stack alloc).
/// The child is already running but nothing will observe its exit,
/// so we SIGKILL it, unregister the pending exit_rx so PID 1's reap
/// drops the code cleanly, and emit terminal `Error` + `Exited(-1)`
/// frames so the host doesn't wait forever. The session entry is
/// removed last, matching the normal waiter-thread teardown order.
fn abort_session_on_waiter_spawn_failure(
    writer: &Writer,
    sessions: &SessionRegistry,
    corr_id: u32,
    pid: u32,
    reason: &str,
) {
    // Best-effort SIGKILL; ignore errors (child may already be dead).
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
    crate::child_exits::unregister(pid);
    send_frame(
        writer,
        corr_id,
        0,
        &ShellMessage::Error {
            message: format!("waiter thread spawn failed: {reason}"),
        },
    );
    send_frame(
        writer,
        corr_id,
        FLAG_TERMINAL,
        &ShellMessage::Exited { code: -1 },
    );
    remove_session_if_owned(sessions, corr_id, pid);
}

enum OutputKind {
    Stdout,
    Stderr,
}

fn stream_fd<R: Read>(writer: Writer, corr_id: u32, mut src: R, kind: OutputKind) {
    let mut buf = vec![0u8; IO_CHUNK_SIZE];
    loop {
        match src.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let encoded = encode_b64(&buf[..n]);
                let msg = match kind {
                    OutputKind::Stdout => ShellMessage::Stdout { data_b64: encoded },
                    OutputKind::Stderr => ShellMessage::Stderr { data_b64: encoded },
                };
                send_frame(&writer, corr_id, 0, &msg);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

fn send_frame(writer: &Writer, corr_id: u32, flags: u8, msg: &ShellMessage) {
    let frame = match sp::encode_frame(corr_id, flags, msg) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("iii-init: shell dispatcher encode failed: {e}");
            return;
        }
    };
    // Terminal frames carry the child's final status — dropping one
    // leaves the host waiting forever. A fatal `Error` is similarly
    // load-bearing: it's the only non-terminal signal that the session
    // failed. Block until the writer drains for these, best-effort
    // `try_send` for the rest.
    //
    // Data frames (Stdout/Stderr) still use `try_send` so a wedged
    // virtio port or a gone host relay can't deadlock session threads.
    // Losing a byte of output is preferable to losing the terminal
    // signal, which is what this split buys us.
    let is_terminal = flags & FLAG_TERMINAL != 0;
    let is_error = matches!(msg, ShellMessage::Error { .. });
    if is_terminal || is_error {
        if let Err(e) = writer.send(frame) {
            // `send` only returns Err when the receiver is dropped,
            // i.e. the writer thread exited. Nothing we can do.
            let _ = e;
        }
    } else if let Err(e) = writer.try_send(frame) {
        use std::sync::mpsc::TrySendError;
        match e {
            TrySendError::Full(_) => {
                eprintln!("iii-init: shell writer channel full, dropping frame corr_id={corr_id}");
            }
            TrySendError::Disconnected(_) => {
                // Writer thread exited — further writes are pointless.
                // Stay quiet; the first failure already logged.
            }
        }
    }
}

fn encode_b64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn decode_b64(s: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;
    use std::sync::OnceLock;
    use std::time::Duration;

    /// Process-wide reap thread for tests.
    ///
    /// Production runs PID 1's `waitpid(-1)` loop in `supervisor.rs`, and
    /// that loop calls `child_exits::dispatch_exit` to unblock session
    /// waiter threads. The unit tests here bypass `supervisor::run_*`
    /// and call `handle_frame` directly, so without this helper every
    /// child becomes a zombie, every waiter thread blocks on
    /// `exit_rx.recv()`, the terminal `Exited` frame never fires, and
    /// the host's read times out with `WouldBlock`.
    ///
    /// Started lazily via `OnceLock`: the first test to spawn children
    /// kicks off a single detached reaper thread that lives until the
    /// test process exits. Repeated calls are no-ops.
    #[cfg(target_os = "linux")]
    fn ensure_test_reaper() {
        static STARTED: OnceLock<()> = OnceLock::new();
        STARTED.get_or_init(|| {
            use nix::errno::Errno;
            use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
            use nix::unistd::Pid;
            thread::Builder::new()
                .name("shell-dispatcher-test-reaper".to_string())
                .spawn(|| {
                    loop {
                        match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
                            Ok(WaitStatus::Exited(pid, code)) => {
                                crate::child_exits::dispatch_exit(pid.as_raw(), code);
                            }
                            Ok(WaitStatus::Signaled(pid, sig, _)) => {
                                crate::child_exits::dispatch_exit(pid.as_raw(), 128 + sig as i32);
                            }
                            Ok(WaitStatus::StillAlive) | Err(Errno::ECHILD) => {
                                thread::sleep(Duration::from_millis(10));
                            }
                            // Stopped/Continued/PtraceEvent/PtraceSyscall —
                            // not terminal, leave the child alone.
                            Ok(_) => {}
                            // EINTR or other transient errno; brief pause
                            // and retry so we don't busy-loop on errors.
                            Err(_) => thread::sleep(Duration::from_millis(10)),
                        }
                    }
                })
                .expect("spawn test reaper");
        });
    }

    /// Drive the dispatcher over an in-process socketpair and assert
    /// the happy path for a pipe-mode `echo` invocation.
    ///
    /// Rationale: we can't attach the socketpair to a `File` handle
    /// easily (the `run` entry point opens the path itself), so we
    /// duplicate just enough of the dispatch glue here to prove the
    /// message-handling logic is correct. Integration coverage for
    /// the full virtio-console path lives in iii-worker's integration
    /// tests.
    fn run_over_socketpair() {
        ensure_test_reaper();
        let (host, guest) = UnixStream::pair().expect("socketpair");
        host.set_read_timeout(Some(Duration::from_secs(5))).unwrap();

        // Guest thread: replicate the dispatcher's core loop against
        // the socketpair ends directly, bypassing File::open.
        let guest_clone = guest.try_clone().unwrap();
        thread::spawn(move || {
            let guest_file = unsafe {
                use std::os::unix::io::{FromRawFd, IntoRawFd};
                File::from_raw_fd(guest.into_raw_fd())
            };
            let writer = spawn_writer_thread(guest_file);
            let sessions: SessionRegistry = Arc::new(Mutex::new(HashMap::new()));
            let mut reader = BufReader::new(guest_clone);
            while let Ok(Some((corr_id, _f, msg))) = sp::read_frame_blocking(&mut reader) {
                handle_frame(corr_id, msg, &sessions, &writer);
            }
        });

        // Host: send a Request for `echo hi`, drive stdin EOF, collect
        // Started + Stdout + Exited.
        let req = ShellMessage::Request {
            cmd: "/bin/sh".into(),
            args: vec!["-c".into(), "echo hi".into()],
            env: vec![],
            cwd: None,
            tty: false,
            rows: 24,
            cols: 80,
        };
        let mut host_writer = host.try_clone().unwrap();
        sp::write_frame_blocking(&mut host_writer, 1, 0, &req).unwrap();
        sp::write_frame_blocking(
            &mut host_writer,
            1,
            0,
            &ShellMessage::Stdin {
                data_b64: String::new(),
            },
        )
        .unwrap();

        let mut host_reader = BufReader::new(host);
        let mut saw_started = false;
        let mut got_stdout = Vec::new();
        let mut exited_code: Option<i32> = None;
        while exited_code.is_none() {
            let (corr_id, flags, msg) = sp::read_frame_blocking(&mut host_reader)
                .expect("read frame")
                .expect("no EOF before Exited");
            assert_eq!(corr_id, 1);
            match msg {
                ShellMessage::Started { .. } => saw_started = true,
                ShellMessage::Stdout { data_b64 } => {
                    got_stdout.extend(decode_b64(&data_b64).unwrap());
                }
                ShellMessage::Stderr { .. } => {}
                ShellMessage::Exited { code } => {
                    assert!(flags & FLAG_TERMINAL != 0, "Exited missing terminal flag");
                    exited_code = Some(code);
                }
                other => panic!("unexpected message: {other:?}"),
            }
        }
        assert!(saw_started);
        assert_eq!(exited_code, Some(0));
        assert_eq!(String::from_utf8_lossy(&got_stdout), "hi\n");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn echo_pipe_session_happy_path() {
        // Some CI environments don't have /bin/sh; guard with a
        // pre-check so the test skips cleanly rather than flakes.
        if !std::path::Path::new("/bin/sh").exists() {
            return;
        }
        run_over_socketpair();
    }

    /// TTY mode happy path: `/bin/sh -c 'echo hi'` should succeed
    /// and produce at least "hi" somewhere in its output. The PTY
    /// line discipline may transform line endings (\n -> \r\n) and
    /// shells can emit prompts or other chatter, so we assert a
    /// containment rather than exact equality.
    #[test]
    #[cfg(target_os = "linux")]
    fn tty_session_happy_path() {
        if !std::path::Path::new("/bin/sh").exists() {
            return;
        }
        ensure_test_reaper();
        let (host, guest) = UnixStream::pair().expect("socketpair");
        host.set_read_timeout(Some(Duration::from_secs(5))).unwrap();

        let guest_clone = guest.try_clone().unwrap();
        thread::spawn(move || {
            let guest_file = unsafe {
                use std::os::unix::io::{FromRawFd, IntoRawFd};
                File::from_raw_fd(guest.into_raw_fd())
            };
            let writer = spawn_writer_thread(guest_file);
            let sessions: SessionRegistry = Arc::new(Mutex::new(HashMap::new()));
            let mut reader = BufReader::new(guest_clone);
            while let Ok(Some((corr_id, _f, msg))) = sp::read_frame_blocking(&mut reader) {
                handle_frame(corr_id, msg, &sessions, &writer);
            }
        });

        let req = ShellMessage::Request {
            cmd: "/bin/sh".into(),
            args: vec!["-c".into(), "echo hi".into()],
            env: vec![],
            cwd: None,
            tty: true,
            rows: 24,
            cols: 80,
        };
        let mut host_writer = host.try_clone().unwrap();
        sp::write_frame_blocking(&mut host_writer, 7, 0, &req).unwrap();

        let mut host_reader = BufReader::new(host);
        let mut saw_started = false;
        let mut output = Vec::new();
        let mut exited_code: Option<i32> = None;
        while exited_code.is_none() {
            let (corr_id, flags, msg) = sp::read_frame_blocking(&mut host_reader)
                .expect("read frame")
                .expect("no EOF before Exited");
            assert_eq!(corr_id, 7);
            match msg {
                ShellMessage::Started { .. } => saw_started = true,
                ShellMessage::Stdout { data_b64 } => {
                    output.extend(decode_b64(&data_b64).unwrap());
                }
                ShellMessage::Exited { code } => {
                    assert!(flags & FLAG_TERMINAL != 0);
                    exited_code = Some(code);
                }
                other => panic!("unexpected message: {other:?}"),
            }
        }
        assert!(saw_started);
        assert_eq!(exited_code, Some(0));
        let output_str = String::from_utf8_lossy(&output);
        assert!(
            output_str.contains("hi"),
            "expected 'hi' in PTY output, got: {output_str:?}"
        );
    }

    #[test]
    fn b64_roundtrip() {
        let raw = b"hello, world\n\0\xff";
        let encoded = encode_b64(raw);
        let decoded = decode_b64(&encoded).unwrap();
        assert_eq!(decoded, raw);
    }

    // ----- Integration tests per plan spec -------------------------
    //
    // The plan (~/.claude/plans/use-graphify-to-query-parallel-naur.md,
    // Verification §Integration tests) lists six scenarios we need to
    // cover end-to-end. We drive them here against an in-process
    // socketpair rather than a live libkrun VM: the dispatcher is the
    // only component whose behavior is specific to this branch, and
    // all six plan cases exercise dispatcher semantics (spawn, I/O
    // routing, signal delivery, concurrency, exit-code encoding).
    // Full VM-level coverage belongs behind the `integration-vm`
    // feature flag in `iii-worker` and is tracked as follow-up.

    /// Boot the dispatcher glue over a fresh socketpair. Returns a
    /// blocking host-side reader and a writable host-side sender the
    /// test can use to drive requests. The guest loop lives in a
    /// detached thread for the lifetime of the socketpair.
    #[cfg(target_os = "linux")]
    fn start_dispatcher_over_socketpair() -> (UnixStream, UnixStream) {
        ensure_test_reaper();
        let (host, guest) = UnixStream::pair().expect("socketpair");
        host.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();

        let guest_clone = guest.try_clone().unwrap();
        thread::spawn(move || {
            let guest_file = unsafe {
                use std::os::unix::io::{FromRawFd, IntoRawFd};
                File::from_raw_fd(guest.into_raw_fd())
            };
            let writer = spawn_writer_thread(guest_file);
            let sessions: SessionRegistry = Arc::new(Mutex::new(HashMap::new()));
            let mut reader = BufReader::new(guest_clone);
            while let Ok(Some((corr_id, _f, msg))) = sp::read_frame_blocking(&mut reader) {
                handle_frame(corr_id, msg, &sessions, &writer);
            }
        });

        let host_write = host.try_clone().unwrap();
        (host, host_write)
    }

    /// Drive a single pipe-mode session to completion. Collects
    /// stdout, stderr, and the exit code; returns a tuple so the
    /// caller can assert. Panics on timeout or protocol error so
    /// test failures point at the actual stuck frame.
    #[cfg(target_os = "linux")]
    fn drive_pipe_session(
        host_read: UnixStream,
        host_write: &mut UnixStream,
        corr_id: u32,
        req: ShellMessage,
        stdin_bytes: Option<&[u8]>,
    ) -> (Vec<u8>, Vec<u8>, i32) {
        sp::write_frame_blocking(host_write, corr_id, 0, &req).unwrap();
        if let Some(bytes) = stdin_bytes {
            sp::write_frame_blocking(
                host_write,
                corr_id,
                0,
                &ShellMessage::Stdin {
                    data_b64: encode_b64(bytes),
                },
            )
            .unwrap();
        }
        // Send stdin EOF so programs like `cat` exit.
        sp::write_frame_blocking(
            host_write,
            corr_id,
            0,
            &ShellMessage::Stdin {
                data_b64: String::new(),
            },
        )
        .unwrap();

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code: Option<i32> = None;
        let mut host_reader = BufReader::new(host_read);
        while exit_code.is_none() {
            let (got_id, flags, msg) = sp::read_frame_blocking(&mut host_reader)
                .expect("read frame")
                .expect("EOF before Exited");
            assert_eq!(got_id, corr_id, "frame for wrong corr_id");
            match msg {
                ShellMessage::Started { .. } => {}
                ShellMessage::Stdout { data_b64 } => {
                    stdout.extend(decode_b64(&data_b64).unwrap());
                }
                ShellMessage::Stderr { data_b64 } => {
                    stderr.extend(decode_b64(&data_b64).unwrap());
                }
                ShellMessage::Error { message } => {
                    panic!("guest returned Error: {message}");
                }
                ShellMessage::Exited { code } => {
                    assert!(flags & FLAG_TERMINAL != 0, "Exited without terminal flag");
                    exit_code = Some(code);
                }
                other => panic!("unexpected inbound: {other:?}"),
            }
        }
        (stdout, stderr, exit_code.unwrap())
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn plan_exit_code_propagates() {
        if !std::path::Path::new("/bin/sh").exists() {
            return;
        }
        let (host_read, mut host_write) = start_dispatcher_over_socketpair();
        let (_out, _err, code) = drive_pipe_session(
            host_read,
            &mut host_write,
            1,
            ShellMessage::Request {
                cmd: "/bin/sh".into(),
                args: vec!["-c".into(), "exit 42".into()],
                env: vec![],
                cwd: None,
                tty: false,
                rows: 24,
                cols: 80,
            },
            None,
        );
        assert_eq!(code, 42, "shell `exit 42` must propagate");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn plan_stderr_is_separated_from_stdout() {
        if !std::path::Path::new("/bin/sh").exists() {
            return;
        }
        let (host_read, mut host_write) = start_dispatcher_over_socketpair();
        let (out, err, code) = drive_pipe_session(
            host_read,
            &mut host_write,
            2,
            ShellMessage::Request {
                cmd: "/bin/sh".into(),
                args: vec!["-c".into(), "echo err >&2".into()],
                env: vec![],
                cwd: None,
                tty: false,
                rows: 24,
                cols: 80,
            },
            None,
        );
        assert_eq!(code, 0);
        assert!(out.is_empty(), "stdout should be empty, got {out:?}");
        assert_eq!(String::from_utf8_lossy(&err), "err\n");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn plan_stdin_pipe_round_trip() {
        if !std::path::Path::new("/bin/cat").exists() {
            return;
        }
        let (host_read, mut host_write) = start_dispatcher_over_socketpair();
        let payload = b"roundtrip payload\n";
        let (out, _err, code) = drive_pipe_session(
            host_read,
            &mut host_write,
            3,
            ShellMessage::Request {
                cmd: "/bin/cat".into(),
                args: vec![],
                env: vec![],
                cwd: None,
                tty: false,
                rows: 24,
                cols: 80,
            },
            Some(payload),
        );
        assert_eq!(code, 0);
        assert_eq!(out, payload, "cat must echo stdin back to stdout");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn plan_four_concurrent_sessions_complete_independently() {
        // Four parallel `sleep 0.1` runs across four distinct corr_ids
        // on one dispatcher. Each must emit Started + Exited without
        // cross-talk between ids. Uses 0.1s so the wall-clock test
        // stays snappy but overlap is still real.
        if !std::path::Path::new("/bin/sleep").exists() {
            return;
        }
        ensure_test_reaper();
        let (host, _) = UnixStream::pair().expect("socketpair");
        let (_writer_unused, _) = {
            // Reuse the integration helper but we need one shared
            // guest + one shared host for multiplexing. Rebuild
            // manually here.
            let (h, g) = UnixStream::pair().expect("socketpair");
            h.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let g_clone = g.try_clone().unwrap();
            thread::spawn(move || {
                let f = unsafe {
                    use std::os::unix::io::{FromRawFd, IntoRawFd};
                    File::from_raw_fd(g.into_raw_fd())
                };
                let w = spawn_writer_thread(f);
                let sessions: SessionRegistry = Arc::new(Mutex::new(HashMap::new()));
                let mut r = BufReader::new(g_clone);
                while let Ok(Some((id, _f, msg))) = sp::read_frame_blocking(&mut r) {
                    handle_frame(id, msg, &sessions, &w);
                }
            });
            (h, 0u8)
        };
        // Kill the first unused pair; keep the second alive.
        drop(host);
        let (host, g) = UnixStream::pair().expect("socketpair");
        host.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let g_clone = g.try_clone().unwrap();
        thread::spawn(move || {
            let f = unsafe {
                use std::os::unix::io::{FromRawFd, IntoRawFd};
                File::from_raw_fd(g.into_raw_fd())
            };
            let w = spawn_writer_thread(f);
            let sessions: SessionRegistry = Arc::new(Mutex::new(HashMap::new()));
            let mut r = BufReader::new(g_clone);
            while let Ok(Some((id, _f, msg))) = sp::read_frame_blocking(&mut r) {
                handle_frame(id, msg, &sessions, &w);
            }
        });

        let mut host_writer = host.try_clone().unwrap();
        for id in 1..=4u32 {
            sp::write_frame_blocking(
                &mut host_writer,
                id,
                0,
                &ShellMessage::Request {
                    cmd: "/bin/sleep".into(),
                    args: vec!["0.1".into()],
                    env: vec![],
                    cwd: None,
                    tty: false,
                    rows: 24,
                    cols: 80,
                },
            )
            .unwrap();
        }

        let mut reader = BufReader::new(host);
        let mut exited: std::collections::HashSet<u32> = std::collections::HashSet::new();
        while exited.len() < 4 {
            let (id, flags, msg) = sp::read_frame_blocking(&mut reader)
                .expect("read frame")
                .expect("EOF before all Exited");
            match msg {
                ShellMessage::Exited { code } => {
                    assert_eq!(code, 0, "sleep should exit 0 for corr_id={id}");
                    assert!(flags & FLAG_TERMINAL != 0);
                    exited.insert(id);
                }
                ShellMessage::Started { .. }
                | ShellMessage::Stdout { .. }
                | ShellMessage::Stderr { .. } => {}
                other => panic!("unexpected: {other:?}"),
            }
        }
        assert_eq!(exited, [1u32, 2, 3, 4].iter().copied().collect());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn plan_sigkill_signal_terminates_child() {
        // Send `Signal { signal: 9 }` at a sleeping child. Exit code
        // follows the shell convention `128 + signal`.
        if !std::path::Path::new("/bin/sleep").exists() {
            return;
        }
        let (host_read, mut host_write) = start_dispatcher_over_socketpair();
        let corr_id = 42u32;
        sp::write_frame_blocking(
            &mut host_write,
            corr_id,
            0,
            &ShellMessage::Request {
                cmd: "/bin/sleep".into(),
                args: vec!["30".into()],
                env: vec![],
                cwd: None,
                tty: false,
                rows: 24,
                cols: 80,
            },
        )
        .unwrap();

        // Wait for Started so the child PID exists before signaling.
        let mut reader = BufReader::new(host_read);
        loop {
            let (id, _f, msg) = sp::read_frame_blocking(&mut reader)
                .expect("read")
                .expect("EOF");
            assert_eq!(id, corr_id);
            if matches!(msg, ShellMessage::Started { .. }) {
                break;
            }
        }

        sp::write_frame_blocking(
            &mut host_write,
            corr_id,
            0,
            &ShellMessage::Signal { signal: 9 },
        )
        .unwrap();

        loop {
            let (id, flags, msg) = sp::read_frame_blocking(&mut reader)
                .expect("read")
                .expect("EOF before Exited");
            assert_eq!(id, corr_id);
            if let ShellMessage::Exited { code } = msg {
                assert!(flags & FLAG_TERMINAL != 0);
                assert_eq!(code, 128 + 9, "SIGKILL → 128 + 9 = 137, got {code}");
                break;
            }
        }
    }

    /// P1 regression for the master-clone failure cleanup path.
    ///
    /// Prior risk: if `File::try_clone` on the PTY master fails
    /// (EMFILE under fd pressure) AFTER the child has been spawned
    /// and registered with `child_exits`, a bare `?` return would
    /// orphan the child — PID 1's reap would dispatch the exit to an
    /// `exit_rx` nobody's receiving on, and the child_exits registry
    /// slot would leak.
    ///
    /// Current behavior: SIGKILL the pid, `child_exits::unregister`,
    /// drop the child handle, return Err so handle_frame emits a
    /// terminal `Error` frame.
    ///
    /// Test contract: toggle the injected-failure hook, drive a TTY
    /// Request directly through `handle_frame`, assert:
    ///   - a single `Error` frame with `FLAG_TERMINAL` is emitted,
    ///   - no `Started` frame precedes it,
    ///   - the sessions map has no entry for the corr_id (spawn
    ///     returned before session registration).
    #[test]
    #[cfg(target_os = "linux")]
    fn tty_session_cleans_up_on_try_clone_failure() {
        if !std::path::Path::new("/bin/true").exists() {
            return;
        }
        // RAII restore so a panic doesn't leak the hook into
        // sibling tests (thread-locals are per-thread, but belt +
        // suspenders in case the test harness reuses the thread).
        struct Restore;
        impl Drop for Restore {
            fn drop(&mut self) {
                FORCE_MASTER_CLONE_FAIL.with(|c| c.set(false));
            }
        }
        FORCE_MASTER_CLONE_FAIL.with(|c| c.set(true));
        let _restore = Restore;

        let (tx, rx) = sync_channel::<Vec<u8>>(16);
        let sessions: SessionRegistry = Arc::new(Mutex::new(HashMap::new()));

        handle_frame(
            99,
            ShellMessage::Request {
                cmd: "/bin/true".into(),
                args: vec![],
                env: vec![],
                cwd: None,
                tty: true,
                rows: 24,
                cols: 80,
            },
            &sessions,
            &tx,
        );

        // First frame must be Error with FLAG_TERMINAL.
        let frame = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("expected an Error frame on the writer channel");
        // Frame layout: [4 B len][4 B corr_id][1 B flags][payload].
        let (got_corr, got_flags, got_msg) =
            sp::decode_frame_body(&frame[4..]).expect("decode Error frame");
        assert_eq!(got_corr, 99);
        assert!(
            got_flags & FLAG_TERMINAL != 0,
            "Error must carry FLAG_TERMINAL so host-side route is torn down"
        );
        match got_msg {
            ShellMessage::Error { message } => {
                assert!(
                    message.contains("spawn:"),
                    "Error message should start with 'spawn:', got {message:?}"
                );
            }
            other => panic!("expected Error frame, got {other:?}"),
        }

        // No session entry should have been inserted — spawn_tty_session
        // bails before the `sessions.insert` line.
        assert!(
            !sessions.lock().unwrap().contains_key(&99),
            "sessions map must not contain corr_id after spawn failure"
        );

        // No further frames. Error terminated the session; a Started
        // frame leaking here would mean the clean-up path ran after
        // announcing the session, which is exactly the bug we're
        // guarding against.
        assert!(
            rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "no trailing frames after Error"
        );
    }
}
