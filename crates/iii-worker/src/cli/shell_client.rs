// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Client for `iii worker exec` — runs one command inside a running
//! worker's microVM and returns its exit code.
//!
//! Two modes:
//!
//! - **Pipe mode** (default): host stdin is piped as `Stdin` frames,
//!   guest stdout/stderr come back as separate `Stdout`/`Stderr`
//!   frames, exit code propagates via `Exited`.
//! - **TTY mode** (`--tty` or `-t`): allocates a PTY in the guest,
//!   puts host stdin in raw mode, forwards every byte as-is, and
//!   relays `SIGWINCH` resizes via `Resize` frames. The child's
//!   stdout and stderr are merged onto the PTY master and come back
//!   as `Stdout` frames; the host paints them to its stdout without
//!   line-buffering so interactive shells behave correctly.
//!
//! Wire flow:
//!
//! 1. Connect to `~/.iii/managed/<worker>/shell.sock`.
//! 2. Read the 4-byte `id_offset` handshake the relay writes
//!    immediately post-accept.
//! 3. Generate a fresh `corr_id` inside the client's range and send
//!    a `ShellMessage::Request` frame. The relay routes it to the
//!    in-VM dispatcher, which spawns the child and begins streaming
//!    output.
//! 4. Two concurrent tokio tasks after Request: one pumps host stdin
//!    as `Stdin` frames (empty-bytes frame on EOF), the other
//!    consumes response frames and paints stdout/stderr locally.
//! 5. First `Exited { code }` (terminal frame) wins — we return its
//!    code. `Error { message }` from the guest returns code 1 and
//!    prints the message to stderr.

use std::io::IsTerminal;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use base64::Engine;
use iii_shell_client::read_frame_async;
use iii_supervisor::shell_protocol::{ShellMessage, encode_frame, flags::FLAG_TERMINAL};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::Mutex as TokioMutex;

use crate::cli::app::ExecArgs;

/// Shared writer alias. Both pipe and TTY mode wrap the socket write
/// half in an `Arc<Mutex<>>` so the stdin pump, SIGINT forwarder,
/// SIGWINCH forwarder, and timeout killer can all emit frames without
/// racing against each other at the framing boundary.
type SharedWriter = Arc<TokioMutex<tokio::net::unix::OwnedWriteHalf>>;

/// POSIX signal numbers we forward from the host. These are fixed by
/// the kernel ABI and identical across every Linux target we run on,
/// so hard-coding is safer than pulling them through `nix`/`libc`
/// constants that differ in type between Linux/macOS builds.
const SIG_INT: i32 = 2;
const SIG_KILL: i32 = 9;

/// Exit code used when `--timeout` expires. Matches coreutils
/// `timeout(1)` so shell scripts can distinguish our timeout from an
/// ordinary nonzero exit.
const EXIT_TIMEOUT: i32 = 124;

/// Exit code used when we bail on the second Ctrl-C in pipe mode.
/// Matches the shell convention `128 + SIGINT`.
const EXIT_INTERRUPTED: i32 = 130;

/// How much host stdin we ship per frame. Large enough that typical
/// command pipelines (`cat < bigfile`) don't round-trip a tiny frame
/// per syscall; small enough that latency-sensitive interactive uses
/// (TTY mode keystrokes) still feel responsive.
const STDIN_CHUNK: usize = 8 * 1024;

/// Verify the shell socket belongs to us before/after `connect()`.
/// Returns (dev, ino, mode) so the caller can compare the two stats.
/// Refuses non-sockets, non-euid owners, group/world-accessible modes.
#[cfg(unix)]
fn verify_shell_socket_ownership(sock: &Path) -> anyhow::Result<(u64, u64, u32)> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    // symlink_metadata so a planted symlink isn't followed.
    let meta = std::fs::symlink_metadata(sock).with_context(|| {
        format!(
            "shell socket {} not present — start the worker first",
            sock.display()
        )
    })?;
    if !meta.file_type().is_socket() {
        bail!(
            "refusing to connect to {}: not a Unix socket (type: {:?})",
            sock.display(),
            meta.file_type()
        );
    }
    let our_uid = unsafe { libc::geteuid() };
    if meta.uid() != our_uid {
        bail!(
            "refusing to connect to {}: socket is owned by uid {} (expected {})",
            sock.display(),
            meta.uid(),
            our_uid
        );
    }
    let mode = meta.mode() & 0o777;
    if mode & 0o077 != 0 {
        bail!(
            "refusing to connect to {}: mode {:o} is group/world-accessible \
             (expected 0o600 or stricter)",
            sock.display(),
            mode
        );
    }
    Ok((meta.dev(), meta.ino(), mode))
}

#[cfg(not(unix))]
fn verify_shell_socket_ownership(_sock: &Path) -> anyhow::Result<(u64, u64, u32)> {
    // Non-Unix hosts don't support this attack surface (no AF_UNIX).
    Ok((0, 0, 0))
}

/// Resolve the shell-channel socket path for a named worker with the
/// same validation `supervisor_ctl::control_socket_path` applies to
/// the control socket — no absolute paths, no `..`, no interior
/// slashes, `$HOME` must be set. Any of those would let a malformed
/// arg redirect the connect() to an arbitrary path.
pub fn shell_socket_path(worker_name: &str) -> anyhow::Result<PathBuf> {
    if worker_name.is_empty() {
        bail!("worker_name is empty");
    }
    if worker_name.contains('\0') {
        bail!("worker_name must not contain NUL bytes: {worker_name:?}");
    }
    let p = Path::new(worker_name);
    if p.is_absolute() {
        bail!("worker_name must not be absolute: {worker_name:?}");
    }
    let mut comps = p.components();
    match (comps.next(), comps.next()) {
        (Some(Component::Normal(_)), None) => {}
        _ => bail!("worker_name must be a single path segment: {worker_name:?}"),
    }
    let home = dirs::home_dir().ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(home
        .join(".iii/managed")
        .join(worker_name)
        .join("shell.sock"))
}

/// Decide whether to auto-enable TTY mode given explicit flags and
/// the terminal status of stdin/stdout. Pure function so the policy
/// is unit-testable without actually redirecting process handles.
///
/// Policy: auto-enable only when the user hasn't decided for us
/// (`-t` or `--no-tty`) AND both stdin+stdout are terminals. Matches
/// ssh's "auto-PTY when no command given" heuristic — conservative
/// enough that any redirect or pipe keeps clean byte-exact output.
fn should_auto_enable_tty(
    tty: bool,
    no_tty: bool,
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
) -> bool {
    !tty && !no_tty && stdin_is_terminal && stdout_is_terminal
}

/// Public entry point wired from `main.rs`. Returns a process exit
/// code so the caller can `std::process::exit(code)` directly.
///
/// Does **not** panic on guest-side failures: if the command can't be
/// found inside the VM, the guest sends `Error { message }` and we
/// surface that on stderr with a nonzero exit code.
pub async fn handle_managed_exec(args: ExecArgs) -> i32 {
    match run(args).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    }
}

async fn run(mut args: ExecArgs) -> anyhow::Result<i32> {
    if args.command.is_empty() {
        bail!(
            "no command specified; pass the program after `--`, \
             e.g. `iii worker exec <name> -- /bin/ls`"
        );
    }

    // SSH-style auto-detection: if the user didn't pass -t and didn't
    // force pipe mode with --no-tty, flip on TTY mode when both stdin
    // AND stdout are terminals. Requiring both prevents corrupting
    // piped/redirected output when only one end is interactive.
    if should_auto_enable_tty(
        args.tty,
        args.no_tty,
        std::io::stdin().is_terminal(),
        std::io::stdout().is_terminal(),
    ) {
        args.tty = true;
        tracing::debug!("shell_client: auto-enabled --tty (stdin+stdout both terminals)");
    }

    if args.tty && !std::io::stdin().is_terminal() {
        bail!(
            "--tty requires a terminal on stdin; \
             remove -t for piped input or run interactively"
        );
    }

    // Parse `--timeout` up front so a malformed value fails before we
    // even open the socket. The deadline is relative to "Request sent",
    // computed below, because request delivery itself can involve a
    // tiny amount of socket setup.
    let timeout_duration: Option<Duration> = match args.timeout.as_deref() {
        Some(raw) => Some(humantime::parse_duration(raw).with_context(|| {
            format!(
                "invalid --timeout value {raw:?} (expected humantime, \
                 e.g. `30s`, `5m`, `1500ms`)"
            )
        })?),
        None => None,
    };

    let sock = shell_socket_path(&args.name)?;
    let pre_fingerprint = verify_shell_socket_ownership(&sock)?;
    let mut stream = UnixStream::connect(&sock).await.with_context(|| {
        format!(
            "connect({}): is worker '{}' running? \
             start it with `iii worker start {}`.",
            sock.display(),
            args.name,
            args.name
        )
    })?;
    // Inode mismatch across connect = socket swapped mid-session.
    let post_fingerprint = verify_shell_socket_ownership(&sock)?;
    if pre_fingerprint != post_fingerprint {
        bail!(
            "refusing to talk to {}: socket inode changed between \
             pre-check and connect (swap attack?)",
            sock.display()
        );
    }

    // Handshake: relay sends 4 BE bytes = id_offset.
    let mut handshake = [0u8; 4];
    stream
        .read_exact(&mut handshake)
        .await
        .context("reading relay handshake (is the VM's shell dispatcher running?)")?;
    let id_offset = u32::from_be_bytes(handshake);
    let corr_id = id_offset + 1;

    // Build and send the Request. Split cmd + args the same way
    // microsandbox does: first element of `command` is the program,
    // the rest are its argv. In TTY mode, query the initial window
    // size from our stdin before any raw-mode switching so sttysize
    // defaults stay correct.
    let (rows, cols) = if args.tty { tty_size() } else { (24, 80) };
    let cmd = args.command[0].clone();
    let prog_args = args.command[1..].to_vec();
    let request = ShellMessage::Request {
        cmd,
        args: prog_args,
        env: args.env.clone(),
        cwd: args.workdir.clone(),
        tty: args.tty,
        rows,
        cols,
    };
    let frame = encode_frame(corr_id, 0, &request)?;
    stream
        .write_all(&frame)
        .await
        .context("sending Request frame")?;

    // Deadline starts ticking from "request out the door" — the guest
    // has seen the Request by now, and that's when meaningful work
    // begins. Wall-clock only; we read it on every response frame so
    // slow output doesn't let a session outlive its budget.
    let deadline = timeout_duration.map(|d| Instant::now() + d);

    let (reader, writer) = stream.into_split();
    // Both pipe and TTY modes share the writer across multiple tasks
    // (stdin pump, timeout killer, signal forwarders). Wrapping once
    // here lets every spawn site take a clone without thinking about
    // which mode is active.
    let writer: SharedWriter = Arc::new(TokioMutex::new(writer));

    // Enable raw mode once the request is out the door — before we
    // start pumping user keystrokes, and restored automatically when
    // the guard drops (including panic paths). Guarded only in TTY
    // mode; pipe mode leaves the host terminal alone.
    let _raw_guard = if args.tty {
        Some(RawModeGuard::install().context("enabling raw mode on host terminal")?)
    } else {
        None
    };

    // Spawn the stdin pump plus any mode-specific side channels. We
    // intentionally don't join them on the happy path: if the child
    // finishes while we're still reading host stdin, the pump's send
    // attempt fails, it errors out, and the task drops. The Exited
    // frame takes priority.
    //
    // Pipe mode also gets a SIGINT forwarder so `Ctrl-C` propagates to
    // the guest child. TTY mode doesn't need one: raw mode delivers
    // Ctrl-C as the literal 0x03 byte, which the guest PTY's line
    // discipline translates into SIGINT for the foreground process
    // group on its side.
    let stdin_task = if args.tty {
        tokio::spawn(pump_host_stdin_tty(writer.clone(), corr_id))
    } else {
        tokio::spawn(pump_host_stdin(writer.clone(), corr_id))
    };
    let winch_task = if args.tty {
        Some(tokio::spawn(pump_sigwinch(writer.clone(), corr_id)))
    } else {
        None
    };
    let sigint_task = if args.tty {
        None
    } else {
        Some(tokio::spawn(pump_sigint_pipe(writer.clone(), corr_id)))
    };

    // Main loop: read response frames, paint stdout/stderr, wait
    // for Exited. `Error { message }` terminates with code 1. The
    // `--timeout` deadline (if any) becomes an absolute cap on the
    // whole session: we race each `read_one_frame` against the
    // remaining budget, send SIGKILL to the guest session on
    // elapse, and return `EXIT_TIMEOUT`.
    let mut reader = reader;
    let mut stdout = tokio::io::stdout();
    let mut stderr = tokio::io::stderr();
    let mut timed_out = false;
    let exit_code: i32 = loop {
        let frame_opt = match deadline {
            Some(dl) => {
                let remaining = dl.saturating_duration_since(Instant::now());
                match tokio::time::timeout(remaining, read_frame_async(&mut reader)).await {
                    Ok(v) => v.map_err(|e| anyhow!("read frame: {e}"))?,
                    Err(_) => {
                        timed_out = true;
                        break EXIT_TIMEOUT;
                    }
                }
            }
            None => read_frame_async(&mut reader)
                .await
                .map_err(|e| anyhow!("read frame: {e}"))?,
        };
        let (got_corr, flags, msg) = match frame_opt {
            Some(v) => v,
            None => {
                bail!("relay closed without Exited response");
            }
        };
        // The relay pre-filters frames by this client's id range and
        // we only have one outstanding command, so every frame should
        // match our corr_id. If we ever see a stray one (bug in relay,
        // replayed state, future multiplexed client), drop it on the
        // floor — ignoring an unknown corr_id is strictly safer than
        // painting its Stdout onto ours or treating its Exited as our
        // exit code.
        if got_corr != corr_id {
            tracing::warn!(
                "shell_client: ignoring frame for corr_id={got_corr}, expected {corr_id}"
            );
            continue;
        }
        match handle_response_frame(msg, flags, &mut stdout, &mut stderr).await? {
            ResponseOutcome::Continue => {}
            ResponseOutcome::Exited(code) => break code,
        }
    };

    if timed_out {
        // Best-effort SIGKILL at the guest session: the child has
        // blown its budget and we're done waiting. If delivery fails
        // (socket torn down, relay gone) there's nothing to fall back
        // to — the dispatcher's own pid-1 reap and disconnect cleanup
        // will eventually catch the orphan.
        let kill_frame = match encode_frame(corr_id, 0, &ShellMessage::Signal { signal: SIG_KILL })
        {
            Ok(f) => f,
            Err(e) => {
                eprintln!("warning: failed to encode SIGKILL frame on timeout: {e}");
                Vec::new()
            }
        };
        if !kill_frame.is_empty() {
            let mut w = writer.lock().await;
            let _ = w.write_all(&kill_frame).await;
        }
        let label = args.timeout.as_deref().unwrap_or("?");
        eprintln!("error: timed out after {label}");
    }

    // Belt-and-suspenders flush for the timeout branch (which skips
    // the response-arm flushes).
    stdout.flush().await.ok();
    stderr.flush().await.ok();

    stdin_task.abort();
    let _ = stdin_task.await;
    if let Some(w) = winch_task {
        w.abort();
        let _ = w.await;
    }
    if let Some(s) = sigint_task {
        s.abort();
        let _ = s.await;
    }

    Ok(exit_code)
}

enum ResponseOutcome {
    Continue,
    Exited(i32),
}

/// Dispatch one decoded response frame. Extracted from `run()` so
/// unit tests can drive it against injectable stdout/stderr sinks
/// (the only way to regression-test the TTY-echo flush contract
/// without a live VM — see `tests::stdout_frame_flushes_after_write`).
async fn handle_response_frame<W1, W2>(
    msg: ShellMessage,
    flags: u8,
    stdout: &mut W1,
    stderr: &mut W2,
) -> anyhow::Result<ResponseOutcome>
where
    W1: tokio::io::AsyncWrite + Unpin,
    W2: tokio::io::AsyncWrite + Unpin,
{
    match msg {
        ShellMessage::Started { .. } => {
            tracing::debug!("session started");
            Ok(ResponseOutcome::Continue)
        }
        ShellMessage::Stdout { data_b64 } => {
            let bytes = decode_b64(&data_b64)?;
            stdout.write_all(&bytes).await.ok();
            // Flush per frame: line-buffered tokio::io::stdout() on a
            // TTY would otherwise hold single-byte raw-mode echoes
            // until the guest emits a newline — typing appears blind
            // until Enter.
            stdout.flush().await.ok();
            Ok(ResponseOutcome::Continue)
        }
        ShellMessage::Stderr { data_b64 } => {
            let bytes = decode_b64(&data_b64)?;
            stderr.write_all(&bytes).await.ok();
            stderr.flush().await.ok();
            Ok(ResponseOutcome::Continue)
        }
        ShellMessage::Error { message } => {
            eprintln!("error: {message}");
            if flags & FLAG_TERMINAL != 0 {
                Ok(ResponseOutcome::Exited(1))
            } else {
                Ok(ResponseOutcome::Continue)
            }
        }
        ShellMessage::Exited { code } => Ok(ResponseOutcome::Exited(code)),
        _ => Ok(ResponseOutcome::Continue),
    }
}

/// Forward host stdin to the guest. Sends chunks as `Stdin` frames
/// until EOF, then one zero-byte `Stdin` frame as the close signal.
///
/// When host stdin is a TTY (interactive `iii worker exec foo -- cat`),
/// we skip the pump entirely — the user presumably wants to let the
/// child's own empty stdin drive its behavior rather than wait for
/// the user to hit Ctrl-D. This matches Docker's `--no-tty`
/// convention.
///
/// **Invariant**: do NOT return on the happy path. The spawned task
/// owns `writer` (one half of the split client↔relay socket). If we
/// return, `writer` drops, the write side half-closes, and the relay
/// interprets that as "client done" — then it cancels its own writer
/// task in `tokio::select!` and the client sees "relay closed without
/// Exited response". The main task aborts this future after the
/// guest's terminal `Exited` frame arrives, which is the only way the
/// writer should go away.
async fn pump_host_stdin(writer: SharedWriter, corr_id: u32) -> anyhow::Result<()> {
    if std::io::stdin().is_terminal() {
        let eof = encode_frame(
            corr_id,
            0,
            &ShellMessage::Stdin {
                data_b64: String::new(),
            },
        )?;
        {
            let mut w = writer.lock().await;
            w.write_all(&eof).await?;
        }
        // Park forever holding `writer` so the write half stays open.
        // Main task calls `.abort()` on this future when Exited
        // arrives, which drops writer and cleanly ends the session.
        std::future::pending::<()>().await;
        return Ok(());
    }
    let mut stdin = tokio::io::stdin();
    let mut buf = vec![0u8; STDIN_CHUNK];
    loop {
        let n = stdin.read(&mut buf).await?;
        if n == 0 {
            let eof = encode_frame(
                corr_id,
                0,
                &ShellMessage::Stdin {
                    data_b64: String::new(),
                },
            )?;
            {
                let mut w = writer.lock().await;
                w.write_all(&eof).await?;
            }
            // After stdin EOF, park holding the writer. Same reason
            // as the TTY branch above: releasing `writer` here would
            // half-close the client→relay direction and make the
            // relay tear down the session before Exited arrives.
            std::future::pending::<()>().await;
            break;
        }
        let frame = encode_frame(
            corr_id,
            0,
            &ShellMessage::Stdin {
                data_b64: encode_b64(&buf[..n]),
            },
        )?;
        let mut w = writer.lock().await;
        w.write_all(&frame).await?;
    }
    Ok(())
}

/// Forward host SIGINT to the guest as `Signal { signal: SIGINT }` so
/// Ctrl-C in pipe mode actually interrupts the child running inside
/// the microVM. First Ctrl-C is polite (SIGINT), a second press
/// within the same session escalates to SIGKILL and exits the client
/// with [`EXIT_INTERRUPTED`] — the standard "I really mean it" pattern
/// borrowed from `kubectl exec` / `docker exec`.
///
/// Pipe mode only. In TTY mode the host terminal is in raw mode, so
/// Ctrl-C is delivered as a literal 0x03 byte and gets forwarded
/// through the stdin pump; the guest PTY's line discipline then
/// generates SIGINT on the child's foreground process group.
async fn pump_sigint_pipe(writer: SharedWriter, corr_id: u32) -> anyhow::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut sigint = match signal(SignalKind::interrupt()) {
        Ok(s) => s,
        Err(e) => {
            // Not fatal: without SIGINT forwarding the default handler
            // still ends the client process on Ctrl-C, just without
            // warning the guest first. Log and park.
            tracing::warn!("shell_client: could not install SIGINT handler: {e}");
            std::future::pending::<()>().await;
            return Ok(());
        }
    };

    let mut count: u32 = 0;
    loop {
        if sigint.recv().await.is_none() {
            break;
        }
        count += 1;
        let signal_num = if count == 1 { SIG_INT } else { SIG_KILL };
        let frame = match encode_frame(corr_id, 0, &ShellMessage::Signal { signal: signal_num }) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("shell_client: failed to encode signal frame: {e}");
                continue;
            }
        };
        {
            let mut w = writer.lock().await;
            if w.write_all(&frame).await.is_err() {
                break;
            }
        }
        if count >= 2 {
            // Second Ctrl-C: we've asked twice, the user is done
            // waiting. Exit immediately with the conventional
            // `128 + SIGINT` code. The guest's own reap loop will
            // clean up whenever the child actually dies.
            std::process::exit(EXIT_INTERRUPTED);
        }
    }
    Ok(())
}

fn encode_b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn decode_b64(s: &str) -> anyhow::Result<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| anyhow!("base64 decode: {e}"))
}

/// Query the host terminal's current row/col size. Falls back to
/// `(24, 80)` if stdin isn't a terminal or the ioctl fails — the
/// guest PTY will open with those defaults and a later SIGWINCH can
/// correct the size if the user resizes.
fn tty_size() -> (u16, u16) {
    use std::os::fd::AsRawFd;
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let fd = std::io::stdin().as_raw_fd();
    let ret = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) };
    if ret == 0 && ws.ws_row > 0 && ws.ws_col > 0 {
        (ws.ws_row, ws.ws_col)
    } else {
        (24, 80)
    }
}

/// Put the host terminal into raw mode for the lifetime of the
/// guard. Drop restores the original termios, which is critical for
/// panic and error paths — a shell left in raw mode is unusable
/// until `reset` is typed blind.
struct RawModeGuard {
    fd: std::os::fd::RawFd,
    original: nix::sys::termios::Termios,
}

impl RawModeGuard {
    fn install() -> anyhow::Result<Self> {
        use nix::sys::termios::{SetArg, cfmakeraw, tcgetattr, tcsetattr};
        use std::os::fd::AsRawFd;

        let stdin = std::io::stdin();
        let fd = stdin.as_raw_fd();
        // tcgetattr/tcsetattr take AsFd in nix 0.30; stdin has AsFd.
        let original =
            tcgetattr(&stdin).context("tcgetattr on host stdin (is it actually a terminal?)")?;
        let mut raw = original.clone();
        cfmakeraw(&mut raw);
        tcsetattr(&stdin, SetArg::TCSANOW, &raw).context("tcsetattr raw mode")?;
        Ok(Self { fd, original })
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        use nix::sys::termios::{SetArg, tcsetattr};
        use std::os::fd::BorrowedFd;
        let borrowed = unsafe { BorrowedFd::borrow_raw(self.fd) };
        // Best-effort: if restore fails the user is stuck, but
        // there's no recovery at Drop time. Print a hint so they
        // know what to type if their terminal is hosed.
        if tcsetattr(borrowed, SetArg::TCSANOW, &self.original).is_err() {
            eprintln!("warning: failed to restore terminal; type `reset` to recover");
        }
    }
}

/// TTY-mode stdin pump: no framing of line boundaries, no base64 of
/// chunked reads into shell metacharacters — just forward raw bytes
/// as the user types them. The guest PTY line discipline handles
/// Ctrl-C / Ctrl-D / echo.
///
/// Unlike `pump_host_stdin`, we do NOT send an empty Stdin frame on
/// EOF. In a TTY, "EOF" is the user pressing Ctrl-D, which the
/// terminal line discipline delivers as a regular byte that we
/// forward. Closing the master would hang up the session unexpectedly.
async fn pump_host_stdin_tty(writer: SharedWriter, corr_id: u32) -> anyhow::Result<()> {
    let mut stdin = tokio::io::stdin();
    let mut buf = vec![0u8; STDIN_CHUNK];
    loop {
        let n = stdin.read(&mut buf).await?;
        if n == 0 {
            // Host stdin EOF (e.g. parent closed our pipe) — park
            // rather than exit, same as pipe mode, to keep the write
            // half alive until the main task aborts us.
            std::future::pending::<()>().await;
            break;
        }
        let frame = encode_frame(
            corr_id,
            0,
            &ShellMessage::Stdin {
                data_b64: encode_b64(&buf[..n]),
            },
        )?;
        let mut w = writer.lock().await;
        w.write_all(&frame).await?;
    }
    Ok(())
}

/// Watch for SIGWINCH (terminal resize) and forward a `Resize` frame
/// each time the host terminal size changes. The guest applies it
/// with TIOCSWINSZ on the PTY master, which delivers SIGWINCH to the
/// child's foreground process group.
async fn pump_sigwinch(writer: SharedWriter, corr_id: u32) -> anyhow::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut winch = signal(SignalKind::window_change()).context("installing SIGWINCH handler")?;
    loop {
        if winch.recv().await.is_none() {
            break;
        }
        let (rows, cols) = tty_size();
        let frame = encode_frame(corr_id, 0, &ShellMessage::Resize { rows, cols })?;
        let mut w = writer.lock().await;
        if w.write_all(&frame).await.is_err() {
            // Socket gone; stop watching. Main task will detect the
            // EOF and bail on its own.
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_shell_socket_in_managed_dir() {
        if dirs::home_dir().is_none() {
            return;
        }
        let p = shell_socket_path("pdfkit").unwrap();
        assert!(p.ends_with(".iii/managed/pdfkit/shell.sock"));
    }

    #[test]
    fn rejects_empty_worker_name() {
        assert!(shell_socket_path("").is_err());
    }

    #[test]
    fn rejects_absolute_worker_name() {
        assert!(shell_socket_path("/etc/passwd").is_err());
    }

    #[test]
    fn rejects_traversal_worker_name() {
        assert!(shell_socket_path("../escape").is_err());
    }

    #[test]
    fn rejects_multi_segment_worker_name() {
        assert!(shell_socket_path("nested/path").is_err());
    }

    #[test]
    fn rejects_nul_byte_in_worker_name() {
        assert!(shell_socket_path("bad\0name").is_err());
    }

    #[test]
    fn auto_tty_on_when_both_streams_are_terminals() {
        assert!(should_auto_enable_tty(
            /* tty */ false, /* no_tty */ false, /* stdin  */ true,
            /* stdout */ true,
        ));
    }

    #[test]
    fn auto_tty_off_when_stdin_is_piped() {
        assert!(!should_auto_enable_tty(false, false, false, true));
    }

    #[test]
    fn auto_tty_off_when_stdout_is_redirected() {
        // Critical: the ssh-style "stdin-only TTY" case must NOT flip
        // on auto-TTY, because the output would go through the PTY
        // and corrupt any file redirection.
        assert!(!should_auto_enable_tty(false, false, true, false));
    }

    #[test]
    fn explicit_tty_is_never_overridden() {
        // `-t` already set: auto-detect is a no-op (caller already
        // decided). Function returns false because nothing to flip.
        assert!(!should_auto_enable_tty(true, false, true, true));
    }

    #[test]
    fn no_tty_flag_blocks_auto_detection() {
        assert!(!should_auto_enable_tty(false, true, true, true));
    }

    /// AsyncWrite substitute that records Write/Flush events
    /// separately so tests can assert the flush happened, not just
    /// the write.
    #[derive(Default)]
    struct RecordingWriter {
        events: std::sync::Arc<std::sync::Mutex<Vec<WriteEvent>>>,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum WriteEvent {
        Write(Vec<u8>),
        Flush,
    }

    impl RecordingWriter {
        fn new() -> Self {
            Self {
                events: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            }
        }
    }

    impl tokio::io::AsyncWrite for RecordingWriter {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            self.events
                .lock()
                .unwrap()
                .push(WriteEvent::Write(buf.to_vec()));
            std::task::Poll::Ready(Ok(buf.len()))
        }
        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            self.events.lock().unwrap().push(WriteEvent::Flush);
            std::task::Poll::Ready(Ok(()))
        }
        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
    }

    /// Regression for the TTY-echo bug: each Stdout frame must be
    /// followed by flush(). Without it, single-byte raw-mode echoes
    /// stay in the line-buffered tokio stdout until the guest emits
    /// a newline — user types blind until Enter.
    #[tokio::test]
    async fn stdout_frame_flushes_after_write() {
        let mut stdout = RecordingWriter::new();
        let mut stderr = RecordingWriter::new();
        let stdout_events = stdout.events.clone();

        let msg = ShellMessage::Stdout {
            data_b64: encode_b64(b"x"),
        };

        let outcome = handle_response_frame(msg, 0, &mut stdout, &mut stderr)
            .await
            .expect("Stdout dispatch must succeed");
        assert!(matches!(outcome, ResponseOutcome::Continue));

        let events = stdout_events.lock().unwrap().clone();
        assert_eq!(
            events,
            vec![WriteEvent::Write(b"x".to_vec()), WriteEvent::Flush],
            "Stdout frame must be followed by a Flush so raw-mode \
             TTY echo appears without waiting for a newline; got {events:?}"
        );
    }

    #[tokio::test]
    async fn stderr_frame_flushes_after_write() {
        let mut stdout = RecordingWriter::new();
        let mut stderr = RecordingWriter::new();
        let stderr_events = stderr.events.clone();

        let msg = ShellMessage::Stderr {
            data_b64: encode_b64(b"e"),
        };

        let outcome = handle_response_frame(msg, 0, &mut stdout, &mut stderr)
            .await
            .expect("Stderr dispatch must succeed");
        assert!(matches!(outcome, ResponseOutcome::Continue));

        let events = stderr_events.lock().unwrap().clone();
        assert_eq!(
            events,
            vec![WriteEvent::Write(b"e".to_vec()), WriteEvent::Flush],
            "Stderr frame must be followed by a Flush; got {events:?}"
        );
    }

    #[tokio::test]
    async fn exited_frame_returns_exit_code() {
        let mut stdout = RecordingWriter::new();
        let mut stderr = RecordingWriter::new();
        let outcome = handle_response_frame(
            ShellMessage::Exited { code: 42 },
            FLAG_TERMINAL,
            &mut stdout,
            &mut stderr,
        )
        .await
        .unwrap();
        match outcome {
            ResponseOutcome::Exited(code) => assert_eq!(code, 42),
            other => panic!("expected Exited(42), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn non_terminal_error_continues_loop() {
        let mut stdout = RecordingWriter::new();
        let mut stderr = RecordingWriter::new();
        let outcome = handle_response_frame(
            ShellMessage::Error {
                message: "soft".into(),
            },
            0,
            &mut stdout,
            &mut stderr,
        )
        .await
        .unwrap();
        assert!(matches!(outcome, ResponseOutcome::Continue));
    }

    #[tokio::test]
    async fn terminal_error_exits_with_code_1() {
        let mut stdout = RecordingWriter::new();
        let mut stderr = RecordingWriter::new();
        let outcome = handle_response_frame(
            ShellMessage::Error {
                message: "hard".into(),
            },
            FLAG_TERMINAL,
            &mut stdout,
            &mut stderr,
        )
        .await
        .unwrap();
        match outcome {
            ResponseOutcome::Exited(code) => assert_eq!(code, 1),
            other => panic!("expected Exited(1), got {other:?}"),
        }
    }

    // Test-only Debug impl — keeps the derive out of production.
    impl std::fmt::Debug for ResponseOutcome {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                ResponseOutcome::Continue => write!(f, "Continue"),
                ResponseOutcome::Exited(code) => write!(f, "Exited({code})"),
            }
        }
    }
}
