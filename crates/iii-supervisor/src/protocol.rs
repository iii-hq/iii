// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Control-channel wire protocol shared between `iii-worker` (host) and
//! `iii-supervisor` (guest). One JSON object per line, newline-terminated.
//! Line-delimited keeps partial reads survivable and makes the channel
//! trivially debuggable with `echo`/`cat` if anyone needs to poke at it
//! from a shell inside the VM.

use serde::{Deserialize, Serialize};

/// Virtio-console port name used to carry the supervisor control channel.
/// Set host-side via `ConsoleBuilder::port("iii.control", ...)` and
/// discovered guest-side by walking `/sys/class/virtio-ports/*/name`.
/// Single source of truth for both ends.
pub const CONTROL_PORT_NAME: &str = "iii.control";

/// Commands the host can send to the in-VM supervisor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Kill the current child process and respawn it with the same run_cmd.
    /// Used by the source watcher when host files change.
    Restart,
    /// Kill the child and exit the supervisor. Triggers VM poweroff.
    Shutdown,
    /// Liveness probe. Useful for the watcher to decide whether the fast
    /// path is available or to fall back to a full VM restart.
    Ping,
    /// Return current child status without disturbing it.
    Status,
}

/// Supervisor's reply to a [`Request`]. At most one response per request.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Response {
    /// Command applied successfully.
    Ok,
    /// Ping reply carrying the current child pid (0 if none).
    Alive { pid: u32 },
    /// Status reply.
    Status {
        /// PID of the running child process. `None` if no child is alive
        /// right now (e.g. during a restart window or after a child crash
        /// before respawn).
        pid: Option<u32>,
        /// Number of times the supervisor has cycled the child since boot.
        restarts: u32,
    },
    /// Something went wrong handling the request. `message` is a human-
    /// readable description; the host logs it and (usually) falls back to
    /// a full VM restart.
    Error { message: String },
}

/// Serialize a request as a single JSON line (no trailing newline —
/// callers add it when writing to the channel).
pub fn encode_request(req: &Request) -> String {
    serde_json::to_string(req).expect("Request is always serializable")
}

/// Serialize a response as a single JSON line.
pub fn encode_response(resp: &Response) -> String {
    serde_json::to_string(resp).expect("Response is always serializable")
}

/// Decode a JSON request line. Whitespace-tolerant (leading/trailing space,
/// CRLF from DOS-authored payloads). Returns the underlying serde error on
/// malformed input so callers can surface it in an `Error { message }`
/// response.
pub fn decode_request(line: &str) -> Result<Request, serde_json::Error> {
    serde_json::from_str(line.trim())
}

/// Decode a JSON response line.
pub fn decode_response(line: &str) -> Result<Response, serde_json::Error> {
    serde_json::from_str(line.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_restart_roundtrips() {
        let encoded = encode_request(&Request::Restart);
        assert_eq!(encoded, r#"{"op":"restart"}"#);
        assert_eq!(decode_request(&encoded).unwrap(), Request::Restart);
    }

    #[test]
    fn request_shutdown_roundtrips() {
        let encoded = encode_request(&Request::Shutdown);
        assert_eq!(decode_request(&encoded).unwrap(), Request::Shutdown);
    }

    #[test]
    fn request_ping_roundtrips() {
        let encoded = encode_request(&Request::Ping);
        assert_eq!(decode_request(&encoded).unwrap(), Request::Ping);
    }

    #[test]
    fn request_status_roundtrips() {
        let encoded = encode_request(&Request::Status);
        assert_eq!(decode_request(&encoded).unwrap(), Request::Status);
    }

    #[test]
    fn response_ok_roundtrips() {
        let encoded = encode_response(&Response::Ok);
        assert_eq!(decode_response(&encoded).unwrap(), Response::Ok);
    }

    #[test]
    fn response_alive_roundtrips() {
        let r = Response::Alive { pid: 42 };
        let encoded = encode_response(&r);
        assert_eq!(decode_response(&encoded).unwrap(), r);
    }

    #[test]
    fn response_status_roundtrips() {
        let r = Response::Status {
            pid: Some(1234),
            restarts: 7,
        };
        let encoded = encode_response(&r);
        assert_eq!(decode_response(&encoded).unwrap(), r);
    }

    #[test]
    fn response_status_no_pid_roundtrips() {
        let r = Response::Status {
            pid: None,
            restarts: 0,
        };
        let encoded = encode_response(&r);
        assert_eq!(decode_response(&encoded).unwrap(), r);
    }

    #[test]
    fn response_error_roundtrips() {
        let r = Response::Error {
            message: "child spawn failed: ENOENT".to_string(),
        };
        let encoded = encode_response(&r);
        assert_eq!(decode_response(&encoded).unwrap(), r);
    }

    #[test]
    fn decode_tolerates_trailing_whitespace() {
        assert_eq!(
            decode_request("{\"op\":\"ping\"}\n").unwrap(),
            Request::Ping
        );
        assert_eq!(
            decode_request("  {\"op\":\"ping\"}  \r\n").unwrap(),
            Request::Ping
        );
    }

    #[test]
    fn decode_rejects_unknown_op() {
        assert!(decode_request(r#"{"op":"nuke"}"#).is_err());
    }

    #[test]
    fn decode_rejects_malformed_json() {
        assert!(decode_request("not json").is_err());
    }
}
