// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
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
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
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
/// needs: the pid (to deliver signals) and the open stdin handle (to
/// forward input bytes, or drop to signal EOF). Stdout/stderr reader
/// threads and the waiter thread own their own `Child` pieces and
/// finish independently — the waiter removes the entry from the
/// registry after emitting the terminal frame.
struct SessionHandle {
    pid: u32,
    /// Pipe-mode stdin, shared so writers can drop the session-map
    /// lock before calling `write_all`. `None` after EOF or exit.
    stdin: Option<Arc<Mutex<ChildStdin>>>,
    /// TTY-mode master fd. Exactly one of `stdin` / `master` is set.
    master: Option<Arc<Mutex<File>>>,
    /// Set by the waiter after `exit_rx.recv()` returns. Once true
    /// the pid has been reaped and the kernel may recycle the pgid,
    /// so `kill(-pid, sig)` could land on an unrelated process group.
    terminated: bool,
}

type SessionRegistry = Arc<Mutex<HashMap<u32, SessionHandle>>>;

/// Registry for in-flight `WriteStart` streaming sessions.
///
/// Key: `corr_id`. Value: a sender that the dispatcher uses to forward
/// `FsChunk` / `FsEnd` frames to the write handler thread. The write
/// thread removes its own entry from the registry when it finishes
/// (success or error) so subsequent frames for that id fall through to
/// the catch-all `_ => {}` arm silently.
type FsWriteRegistry = Arc<Mutex<HashMap<u32, std::sync::mpsc::SyncSender<ShellMessage>>>>;

/// Writer handle shared by every thread that emits frames. A single
/// dedicated writer thread drains this channel onto the virtio-console
/// port, so no caller ever blocks across a virtio write. If the guest
/// side of the port wedges (host relay gone, kernel ring full), the
/// channel saturates at [`WRITER_CHANNEL_CAPACITY`] and further
/// `try_send`s drop frames with a log line — session threads keep
/// running instead of freezing under a shared mutex.
pub(crate) type Writer = SyncSender<Vec<u8>>;

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
    // Registry for in-flight WriteStart sessions. Keyed by corr_id;
    // value is a sender that forwards FsChunk/FsEnd to the write thread.
    let fs_writes: FsWriteRegistry = Arc::new(Mutex::new(HashMap::new()));

    let mut reader = BufReader::new(reader_file);
    loop {
        match sp::read_frame_blocking(&mut reader) {
            Ok(Some((corr_id, _flags, msg))) => {
                handle_frame(corr_id, msg, &sessions, &writer, &fs_writes);
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

fn handle_frame(
    corr_id: u32,
    msg: ShellMessage,
    sessions: &SessionRegistry,
    writer: &Writer,
    fs_writes: &FsWriteRegistry,
) {
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
                Err(_) => return,
            };
            // Snapshot under the map lock, then write without it.
            // Otherwise a full child-stdin pipe buffer stalls every
            // other session's Signal/Resize/Stdin dispatch.
            enum Target {
                Tty(Arc<Mutex<File>>),
                Pipe(Arc<Mutex<ChildStdin>>),
                PipeEof,
                None,
            }
            let target = {
                let mut map = sessions.lock().expect("session map mutex poisoned");
                match map.get_mut(&corr_id) {
                    Some(session) if session.terminated => Target::None,
                    Some(session) => {
                        if let Some(master) = session.master.clone() {
                            Target::Tty(master)
                        } else if decoded.is_empty() {
                            session.stdin.take();
                            Target::PipeEof
                        } else if let Some(stdin) = session.stdin.clone() {
                            Target::Pipe(stdin)
                        } else {
                            Target::None
                        }
                    }
                    None => Target::None,
                }
            };
            match target {
                Target::Tty(master) => {
                    // Empty frame is a no-op — in a TTY, EOF is the
                    // Ctrl-D byte, not a close. Closing the master
                    // would SIGHUP the child.
                    if !decoded.is_empty() {
                        let mut file = master.lock().expect("master mutex poisoned");
                        let _ = file.write_all(&decoded);
                    }
                }
                Target::Pipe(stdin_mu) => {
                    let mut stdin = stdin_mu.lock().expect("stdin mutex poisoned");
                    let _ = stdin.write_all(&decoded);
                }
                Target::PipeEof | Target::None => {}
            }
        }
        ShellMessage::Signal { signal } => {
            // Refuse delivery if terminated: PID 1 has reaped and the
            // pgid may be recycled, so `kill(-pid, sig)` could hit an
            // unrelated process group.
            let pid = {
                let map = sessions.lock().expect("session map mutex poisoned");
                map.get(&corr_id).filter(|s| !s.terminated).map(|s| s.pid)
            };
            if let Some(pid) = pid
                && signal > 0
            {
                // kill(2) is async-signal-safe and never blocks. We
                // don't care about the result — ESRCH means the
                // child already died, and the waiter thread will
                // have emitted Exited or is about to.
                //
                // Signal the whole process group (-pid) so pipelines
                // and other multi-process children inside `sh -c`
                // receive the signal too. Both spawn paths make the
                // child a pgroup leader (pipe mode via setpgid(0,0)
                // in pre_exec, TTY mode via setsid()), so pgid == pid.
                unsafe {
                    libc::kill(-(pid as libc::pid_t), signal as libc::c_int);
                }
            }

            // SIGKILL on a corr_id is also the relay's "host
            // disconnected, abandon any in-flight session" signal
            // (see shell_relay.rs::client_session cleanup, which
            // fans out Signal{9} for every owned corr_id when the
            // host disconnects). For exec sessions the libc::kill
            // above terminates the child; for in-flight FS write
            // sessions we drop the chunk_tx so handle_write_start's
            // recv() returns Disconnected, unlinks the temp file,
            // and returns S218 promptly. Without this, the temp
            // leaks for up to WRITE_IDLE_TIMEOUT (30s) before
            // streaming.rs's safety valve fires — too late for any
            // caller-side test that asserts cleanup at trigger
            // timeout.
            if signal == libc::SIGKILL {
                let _ = fs_writes
                    .lock()
                    .expect("fs_writes mutex poisoned")
                    .remove(&corr_id);
            }
        }
        ShellMessage::Resize { rows, cols } => {
            let master = {
                let map = sessions.lock().expect("session map mutex poisoned");
                map.get(&corr_id)
                    .filter(|s| !s.terminated)
                    .and_then(|s| s.master.clone())
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
        // ── Filesystem ops ────────────────────────────────────────────────
        ShellMessage::FsRequest(op) => {
            use iii_shell_proto::FsOp;
            use iii_shell_proto::flags::FLAG_TERMINAL;

            match op {
                // ── WriteStart: streaming upload ─────────────────────────
                FsOp::WriteStart {
                    path,
                    mode,
                    parents,
                } => {
                    // Create a sync channel for this write session and
                    // register the sender so FsChunk/FsEnd frames can
                    // be forwarded to the write thread.
                    let (chunk_tx, chunk_rx) = std::sync::mpsc::sync_channel::<ShellMessage>(64);
                    fs_writes
                        .lock()
                        .expect("fs_writes mutex poisoned")
                        .insert(corr_id, chunk_tx);

                    let writer = writer.clone();
                    let fs_writes = fs_writes.clone();
                    thread::Builder::new()
                        .name(format!("iii-fs-write-{corr_id}"))
                        .spawn(move || {
                            let result = crate::fs_handler::streaming::handle_write_start(
                                path, mode, parents, chunk_rx,
                            );
                            // Always remove the registry entry when done.
                            fs_writes
                                .lock()
                                .expect("fs_writes mutex poisoned")
                                .remove(&corr_id);
                            match result {
                                Ok(fs_result) => {
                                    send_frame(
                                        &writer,
                                        corr_id,
                                        FLAG_TERMINAL,
                                        &ShellMessage::FsResponse(fs_result),
                                    );
                                }
                                Err(e) => {
                                    send_frame(
                                        &writer,
                                        corr_id,
                                        FLAG_TERMINAL,
                                        &ShellMessage::FsError {
                                            code: e.code.to_string(),
                                            message: e.message,
                                        },
                                    );
                                }
                            }
                        })
                        .ok();
                }

                // ── ReadStart: streaming download ─────────────────────────
                FsOp::ReadStart { path } => {
                    let writer = writer.clone();
                    thread::Builder::new()
                        .name(format!("iii-fs-read-{corr_id}"))
                        .spawn(move || {
                            let result = crate::fs_handler::streaming::handle_read_start(
                                path, &writer, corr_id,
                            );
                            // handle_read_start only returns Err for
                            // pre-FsMeta failures; emit the terminal
                            // FsError here.
                            if let Err(e) = result {
                                send_frame(
                                    &writer,
                                    corr_id,
                                    FLAG_TERMINAL,
                                    &ShellMessage::FsError {
                                        code: e.code.to_string(),
                                        message: e.message,
                                    },
                                );
                            }
                        })
                        .ok();
                }

                // ── One-shot ops ──────────────────────────────────────────
                one_shot => {
                    let writer = writer.clone();
                    thread::Builder::new()
                        .name(format!("iii-fs-op-{corr_id}"))
                        .spawn(move || {
                            let result = dispatch_one_shot(one_shot);
                            match result {
                                Ok(fs_result) => {
                                    send_frame(
                                        &writer,
                                        corr_id,
                                        FLAG_TERMINAL,
                                        &ShellMessage::FsResponse(fs_result),
                                    );
                                }
                                Err(e) => {
                                    send_frame(
                                        &writer,
                                        corr_id,
                                        FLAG_TERMINAL,
                                        &ShellMessage::FsError {
                                            code: e.code.to_string(),
                                            message: e.message,
                                        },
                                    );
                                }
                            }
                        })
                        .ok();
                }
            }
        }

        // ── FsChunk / FsEnd: forward to active write session ──────────────
        ShellMessage::FsChunk { .. } | ShellMessage::FsEnd => {
            let sender = fs_writes
                .lock()
                .expect("fs_writes mutex poisoned")
                .get(&corr_id)
                .cloned();
            if let Some(tx) = sender {
                // try_send avoids blocking the dispatcher loop. Stdin
                // can tolerate a dropped frame; an upload cannot —
                // dropping a chunk while a later FsEnd still arrives
                // would let the writer thread set ended_cleanly=true
                // and rename a truncated temp file into place
                // (streaming.rs:147-188), giving the caller a
                // success response for a corrupt file. Abort the
                // session instead: drop the registry entry so the
                // writer thread observes Disconnected on its next
                // recv_timeout, unlinks the temp file, and emits a
                // terminal FsError(S218). Subsequent FsChunk/FsEnd
                // frames for this corr_id find no sender and are
                // silently ignored — correct, the session is over.
                if let Err(e) = tx.try_send(msg) {
                    use std::sync::mpsc::TrySendError;
                    match e {
                        TrySendError::Full(_) => {
                            eprintln!(
                                "iii-init: fs write channel full, \
                                 aborting upload for corr_id={corr_id}"
                            );
                            fs_writes
                                .lock()
                                .expect("fs_writes mutex poisoned")
                                .remove(&corr_id);
                        }
                        TrySendError::Disconnected(_) => {
                            // Write thread already exited — ignore.
                        }
                    }
                }
            }
            // If no entry exists the write session already finished or
            // was never started — silently drop.
        }

        // Host-only variants received from the host are ignored; only
        // Request/Stdin/Signal/Resize/FsRequest/FsChunk/FsEnd make sense
        // as inbound. Anything else is a peer protocol error — dropping
        // is less noisy than responding with Error and keeps the channel
        // alive.
        _ => {}
    }
}

/// Dispatch a one-shot `FsOp` variant to the corresponding handler.
/// `WriteStart` and `ReadStart` are handled by the streaming path and
/// must never reach this function.
fn dispatch_one_shot(op: iii_shell_proto::FsOp) -> crate::fs_handler::FsCallResult {
    use iii_shell_proto::FsOp;
    match op {
        FsOp::Ls { path } => crate::fs_handler::ops::ls(path),
        FsOp::Stat { path } => crate::fs_handler::ops::stat(path),
        FsOp::Mkdir {
            path,
            mode,
            parents,
        } => crate::fs_handler::ops::mkdir(path, mode, parents),
        FsOp::Rm { path, recursive } => crate::fs_handler::ops::rm(path, recursive),
        FsOp::Chmod {
            path,
            mode,
            uid,
            gid,
            recursive,
        } => crate::fs_handler::ops::chmod(path, mode, uid, gid, recursive),
        FsOp::Mv {
            src,
            dst,
            overwrite,
        } => crate::fs_handler::ops::mv(src, dst, overwrite),
        FsOp::Grep {
            path,
            pattern,
            recursive,
            ignore_case,
            include_glob,
            exclude_glob,
            max_matches,
            max_line_bytes,
        } => crate::fs_handler::ops::grep(
            path,
            pattern,
            recursive,
            ignore_case,
            include_glob,
            exclude_glob,
            max_matches,
            max_line_bytes,
        ),
        FsOp::Sed {
            files,
            path,
            recursive,
            include_glob,
            exclude_glob,
            pattern,
            replacement,
            regex,
            first_only,
            ignore_case,
        } => crate::fs_handler::ops::sed(
            files,
            path,
            recursive,
            include_glob,
            exclude_glob,
            pattern,
            replacement,
            regex,
            first_only,
            ignore_case,
        ),
        // Streaming ops are dispatched before this function is called.
        FsOp::WriteStart { .. } | FsOp::ReadStart { .. } => Err(crate::fs_handler::FsError::new(
            "S219",
            "streaming ops must not reach dispatch_one_shot",
        )),
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
    // Put the child in its own process group so `Signal` frames can
    // reach grandchildren. Without this, `/bin/sh -c "a | b"` leaves
    // `a` and `b` in the dispatcher's pgroup, and `kill(pid, SIGINT)`
    // hits only the shell, while Ctrl-C from an exec client would
    // exit the session with pipeline stages still alive in the VM.
    // TTY mode already gets the same effect via `setsid()`.
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
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

    // Register before announcing so a racing Stdin frame finds the
    // entry.
    sessions.lock().expect("session map mutex poisoned").insert(
        corr_id,
        SessionHandle {
            pid,
            stdin,
            master: None,
            terminated: false,
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
        let writer = writer.clone();
        let sessions = sessions.clone();
        thread::Builder::new()
            .name(format!("iii-exec-{corr_id}-wait"))
            .spawn(move || {
                let code = exit_rx.recv().unwrap_or(-1);
                // Mark terminated BEFORE joining readers — reader
                // joins can block while guest drains output, and the
                // kernel can recycle the reaped pgid in that window.
                mark_terminated(&sessions, corr_id, pid);
                drop(child);
                if let Some(h) = stdout_handle {
                    let _ = h.join();
                }
                if let Some(h) = stderr_handle {
                    let _ = h.join();
                }
                send_frame(
                    &writer,
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
                remove_session_if_owned(&sessions, corr_id, pid);
            })
            .ok();
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
    let master_reader = match master_file.try_clone() {
        Ok(f) => f,
        Err(e) => {
            // Kill the whole session (setsid() made pid the pgroup
            // leader, so -pid targets every descendant spawned before
            // the TTY cleanup raced in).
            unsafe {
                libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
            }
            crate::child_exits::unregister(pid);
            let _ = child;
            return Err(e.into());
        }
    };
    let master = Arc::new(Mutex::new(master_file));

    sessions.lock().expect("session map mutex poisoned").insert(
        corr_id,
        SessionHandle {
            pid,
            stdin: None,
            master: Some(master.clone()),
            terminated: false,
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
        let writer = writer.clone();
        let sessions = sessions.clone();
        thread::Builder::new()
            .name(format!("iii-exec-{corr_id}-wait"))
            .spawn(move || {
                let code = exit_rx.recv().unwrap_or(-1);
                mark_terminated(&sessions, corr_id, pid);
                drop(child);
                if let Some(h) = output_handle {
                    let _ = h.join();
                }
                send_frame(
                    &writer,
                    corr_id,
                    FLAG_TERMINAL,
                    &ShellMessage::Exited { code },
                );
                // See spawn_pipe_session for the ownership-check
                // rationale.
                remove_session_if_owned(&sessions, corr_id, pid);
            })
            .ok();
    }

    Ok(())
}

/// Mark a session terminated iff the entry still belongs to
/// `expected_pid`. Pid check guards against a host-side reclaim that
/// assigned the same `corr_id` to a new session.
fn mark_terminated(sessions: &SessionRegistry, corr_id: u32, expected_pid: u32) {
    let mut map = sessions.lock().expect("session map mutex poisoned");
    if let Some(entry) = map.get_mut(&corr_id)
        && entry.pid == expected_pid
    {
        entry.terminated = true;
    }
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

pub(crate) fn send_frame(writer: &Writer, corr_id: u32, flags: u8, msg: &ShellMessage) {
    let frame = match sp::encode_frame(corr_id, flags, msg) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("iii-init: shell dispatcher encode failed: {e}");
            return;
        }
    };
    // `try_send` so a wedged writer (virtio ring full, host relay gone)
    // never blocks the caller. Dropping a frame is preferable to
    // deadlocking every session thread on a shared writer mutex —
    // which is the bug this writer-thread design replaces.
    if let Err(e) = writer.try_send(frame) {
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

    /// Skip with a visible log line instead of a silent `return` so
    /// a distroless CI image can't report green with zero assertions.
    fn skip_if_missing(binary: &str) -> bool {
        if !std::path::Path::new(binary).exists() {
            eprintln!("[skip] shell_dispatcher test: {binary} not present");
            return true;
        }
        false
    }

    /// Serialize spawning tests: the shared OnceLock reaper calls
    /// `waitpid(-1, WNOHANG)` process-wide, so parallel tests race on
    /// `child_exits::dispatch_exit`.
    fn serial_spawn_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

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
            let fs_writes: FsWriteRegistry = Arc::new(Mutex::new(HashMap::new()));
            let mut reader = BufReader::new(guest_clone);
            while let Ok(Some((corr_id, _f, msg))) = sp::read_frame_blocking(&mut reader) {
                handle_frame(corr_id, msg, &sessions, &writer, &fs_writes);
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
        if skip_if_missing("/bin/sh") {
            return;
        }
        let _g = serial_spawn_guard();
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
        if skip_if_missing("/bin/sh") {
            return;
        }
        let _g = serial_spawn_guard();
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
            let fs_writes: FsWriteRegistry = Arc::new(Mutex::new(HashMap::new()));
            let mut reader = BufReader::new(guest_clone);
            while let Ok(Some((corr_id, _f, msg))) = sp::read_frame_blocking(&mut reader) {
                handle_frame(corr_id, msg, &sessions, &writer, &fs_writes);
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
            let fs_writes: FsWriteRegistry = Arc::new(Mutex::new(HashMap::new()));
            let mut reader = BufReader::new(guest_clone);
            while let Ok(Some((corr_id, _f, msg))) = sp::read_frame_blocking(&mut reader) {
                handle_frame(corr_id, msg, &sessions, &writer, &fs_writes);
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
        if skip_if_missing("/bin/sh") {
            return;
        }
        let _g = serial_spawn_guard();
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
        if skip_if_missing("/bin/sh") {
            return;
        }
        let _g = serial_spawn_guard();
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
        if skip_if_missing("/bin/cat") {
            return;
        }
        let _g = serial_spawn_guard();
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
        // Four parallel `sleep 0.1` across four distinct corr_ids on
        // one dispatcher. Each must emit Started + Exited without
        // cross-talk between ids.
        if skip_if_missing("/bin/sleep") {
            return;
        }
        let _g = serial_spawn_guard();
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
                let fs_writes: FsWriteRegistry = Arc::new(Mutex::new(HashMap::new()));
                let mut r = BufReader::new(g_clone);
                while let Ok(Some((id, _f, msg))) = sp::read_frame_blocking(&mut r) {
                    handle_frame(id, msg, &sessions, &w, &fs_writes);
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
            let fs_writes: FsWriteRegistry = Arc::new(Mutex::new(HashMap::new()));
            let mut r = BufReader::new(g_clone);
            while let Ok(Some((id, _f, msg))) = sp::read_frame_blocking(&mut r) {
                handle_frame(id, msg, &sessions, &w, &fs_writes);
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
        if skip_if_missing("/bin/sleep") {
            return;
        }
        let _g = serial_spawn_guard();
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
}
