// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Control-channel loop.
//!
//! Reads newline-delimited JSON requests from a byte stream, dispatches
//! them to the shared [`child::State`], and writes a single JSON response
//! back. The stream is whatever the caller hands us: in production it's
//! the virtio-console port inside the VM (`/dev/vport0p1`); in tests it's
//! one end of a `socketpair` so we can drive it without booting a VM.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use crate::child::State;
use crate::protocol::{self, Request, Response};

/// Walk `/sys/class/virtio-ports/*/name` and return the `/dev/<dev>`
/// path for the entry whose name matches `target`. Returns `None` when
/// sysfs isn't mounted, no entries exist, or no name matches.
///
/// Virtio-console port numbering depends on controller count and whether
/// the implicit console is enabled — hardcoding `/dev/vport0p1` breaks
/// when libkrun wires the implicit console first. The sysfs name is
/// stable by construction (we pick it on the host), so lookup by name
/// is the right primitive.
///
/// Exposed as a public library function so both the standalone supervisor
/// binary and the merged `iii-init` path can locate the control port the
/// same way.
pub fn find_virtio_port_by_name(target: &str) -> Option<PathBuf> {
    let sysfs = std::path::Path::new("/sys/class/virtio-ports");
    let entries = std::fs::read_dir(sysfs).ok()?;
    for entry in entries.flatten() {
        let dev_name = entry.file_name();
        let dev_name_str = dev_name.to_string_lossy();
        let name_file = entry.path().join("name");
        if let Ok(contents) = std::fs::read_to_string(&name_file)
            && contents.trim() == target
        {
            return Some(PathBuf::from("/dev").join(dev_name_str.as_ref()));
        }
    }
    None
}

/// Dispatch a single request against the supervisor's state, returning
/// the response to send back. Pure function of `(state, req)` — no I/O.
pub fn dispatch(state: &State, req: &Request) -> (Response, bool) {
    match req {
        Request::Ping => (
            Response::Alive {
                pid: state.pid().unwrap_or(0),
            },
            false,
        ),
        Request::Status => (
            Response::Status {
                pid: state.pid(),
                restarts: state.restarts(),
            },
            false,
        ),
        Request::Restart => match state.kill_and_respawn() {
            Ok(_) => (Response::Ok, false),
            Err(e) => (
                Response::Error {
                    message: format!("restart failed: {e}"),
                },
                false,
            ),
        },
        Request::Shutdown => {
            let _ = state.kill_for_shutdown();
            // Second return value = `should_exit`. Caller exits the
            // loop after writing the response so the supervisor binary
            // returns from main with exit code 0, which triggers the
            // VM's poweroff path.
            (Response::Ok, true)
        }
    }
}

/// Run the control loop against a readable+writable byte stream. Blocks
/// until the stream closes, the supervisor receives `Shutdown`, or an
/// I/O error occurs.
///
/// `reader` and `writer` are intentionally separate so a caller can wrap
/// the same underlying fd twice (e.g. with `try_clone`) without needing
/// interior mutability on the stream itself.
pub fn serve<R: BufRead, W: Write>(state: State, reader: R, writer: W) -> anyhow::Result<()> {
    serve_with(state, reader, writer, |_req, _resp, _state| {})
}

/// Like [`serve`] but calls `on_dispatch(req, resp, state)` after every
/// successfully-parsed request/response cycle. Lets callers hook
/// post-dispatch side effects (e.g. `iii-init` syncs its `CHILD_PID`
/// atomic and worker cgroup after every `Restart`) without duplicating
/// the whole read-dispatch-write loop.
///
/// `on_dispatch` is NOT called for malformed requests — those short-
/// circuit with an `Error` response and don't reflect a real command.
/// Line-length capping is enforced here so a misbehaving writer can't
/// OOM the supervisor by streaming bytes without a newline.
pub fn serve_with<R, W, F>(
    state: State,
    mut reader: R,
    mut writer: W,
    mut on_dispatch: F,
) -> anyhow::Result<()>
where
    R: BufRead,
    W: Write,
    F: FnMut(&Request, &Response, &State),
{
    /// Max accepted line length. The protocol carries tiny JSON
    /// (`{"op":"restart"}` + similar). Anything beyond this is either
    /// malformed or an attacker flooding bytes without a newline to OOM
    /// the supervisor. 4 KiB leaves ample headroom for any legitimate
    /// request/response.
    const MAX_LINE: u64 = 4096;

    let mut line = String::new();
    loop {
        line.clear();
        // Take(MAX_LINE) on a mutable ref caps a single read_line call
        // without consuming the reader — Take<&mut R> implements BufRead
        // when R does, so read_line is still available.
        let n = {
            use std::io::Read;
            reader.by_ref().take(MAX_LINE).read_line(&mut line)?
        };
        if n == 0 {
            // EOF — host closed its end of the channel.
            tracing::info!("control channel closed, exiting loop");
            break;
        }
        // A full MAX_LINE read without a terminating newline means the
        // peer is either speaking the wrong protocol or actively
        // streaming junk. Reply with Error and drop the channel so
        // memory can't grow further.
        if n as u64 == MAX_LINE && !line.ends_with('\n') {
            let err = Response::Error {
                message: format!("request exceeds {MAX_LINE}-byte cap"),
            };
            writeln!(writer, "{}", protocol::encode_response(&err))?;
            writer.flush()?;
            break;
        }
        match protocol::decode_request(&line) {
            Ok(req) => {
                tracing::debug!(request = ?req, "dispatching");
                let (resp, should_exit) = dispatch(&state, &req);
                writeln!(writer, "{}", protocol::encode_response(&resp))?;
                writer.flush()?;
                on_dispatch(&req, &resp, &state);
                tracing::debug!(response = ?resp, "dispatched");
                if should_exit {
                    break;
                }
            }
            Err(e) => {
                let err = Response::Error {
                    message: format!("malformed request: {e}"),
                };
                writeln!(writer, "{}", protocol::encode_response(&err))?;
                writer.flush()?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::child::{Config, State};
    use std::io::{BufReader, Cursor};

    fn sleep_state() -> State {
        State::new(Config {
            run_cmd: "sleep 5".to_string(),
            workdir: "/tmp".to_string(),
        })
    }

    #[test]
    fn dispatch_ping_returns_alive_with_pid_zero_when_no_child() {
        let state = sleep_state();
        let (resp, exit) = dispatch(&state, &Request::Ping);
        assert!(!exit);
        assert!(matches!(resp, Response::Alive { pid: 0 }));
    }

    #[test]
    fn dispatch_ping_returns_alive_with_running_pid() {
        let state = sleep_state();
        let pid = state.spawn_initial().unwrap();
        let (resp, _) = dispatch(&state, &Request::Ping);
        assert_eq!(resp, Response::Alive { pid });
        state.kill_for_shutdown().unwrap();
    }

    #[test]
    fn dispatch_status_reflects_restarts() {
        let state = sleep_state();
        state.spawn_initial().unwrap();
        state.kill_and_respawn().unwrap();
        state.kill_and_respawn().unwrap();
        let (resp, _) = dispatch(&state, &Request::Status);
        match resp {
            Response::Status { restarts, pid } => {
                assert_eq!(restarts, 2);
                assert!(pid.is_some());
            }
            other => panic!("expected Status, got {:?}", other),
        }
        state.kill_for_shutdown().unwrap();
    }

    #[test]
    fn dispatch_restart_bumps_counter() {
        let state = sleep_state();
        state.spawn_initial().unwrap();
        let (resp, exit) = dispatch(&state, &Request::Restart);
        assert_eq!(resp, Response::Ok);
        assert!(!exit);
        assert_eq!(state.restarts(), 1);
        state.kill_for_shutdown().unwrap();
    }

    #[test]
    fn dispatch_shutdown_requests_exit() {
        let state = sleep_state();
        state.spawn_initial().unwrap();
        let (resp, exit) = dispatch(&state, &Request::Shutdown);
        assert_eq!(resp, Response::Ok);
        assert!(exit, "shutdown must request loop exit");
        assert_eq!(state.pid(), None);
    }

    #[test]
    fn serve_handles_ping_then_shutdown_and_exits() {
        // Two JSON lines: ping, shutdown. After shutdown, the loop
        // returns; the serve function returns Ok(()) and the caller
        // exits cleanly.
        let input = b"{\"op\":\"ping\"}\n{\"op\":\"shutdown\"}\n";
        let mut output = Vec::new();
        let state = sleep_state();
        state.spawn_initial().unwrap();

        serve(
            state.clone(),
            BufReader::new(Cursor::new(input)),
            &mut output,
        )
        .expect("serve");

        let lines: Vec<&str> = std::str::from_utf8(&output).unwrap().lines().collect();
        assert_eq!(lines.len(), 2, "expected one response per request");
        // First response is Alive { pid: N }.
        assert!(lines[0].starts_with(r#"{"result":"alive","pid":"#));
        // Second response is Ok (for shutdown).
        assert_eq!(lines[1], r#"{"result":"ok"}"#);
    }

    #[test]
    fn serve_replies_error_on_malformed_request_without_breaking() {
        let input = b"not json\n{\"op\":\"shutdown\"}\n";
        let mut output = Vec::new();
        let state = sleep_state();

        serve(state, BufReader::new(Cursor::new(input)), &mut output).expect("serve");

        let lines: Vec<&str> = std::str::from_utf8(&output).unwrap().lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with(r#"{"result":"error","message":"malformed request"#));
        assert_eq!(lines[1], r#"{"result":"ok"}"#);
    }

    #[test]
    fn serve_exits_cleanly_on_eof() {
        let input: &[u8] = b"";
        let mut output = Vec::new();
        let state = sleep_state();
        serve(state, BufReader::new(Cursor::new(input)), &mut output).expect("serve");
        assert!(output.is_empty());
    }
}
