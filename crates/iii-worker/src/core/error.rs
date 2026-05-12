// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

use std::io;
use std::path::PathBuf;

use serde_json::{Value, json};
use thiserror::Error;

/// Each variant maps to a stable W-code surfaced over the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerOpErrorKind {
    InvalidName,                   // W100
    InvalidSource,                 // W101
    LocalPathNotAllowedViaTrigger, // W102
    MissingTarget,                 // W103
    ConsentRequired,               // W104
    NotFound,                      // W110
    AlreadyExists,                 // W111
    NotInstalled,                  // W112
    NotRunning,                    // W113
    AlreadyRunning,                // W114
    LockBusy,                      // W120
    LockIo,                        // W121
    ConfigIo,                      // W130
    ConfigParse,                   // W131
    Registry,                      // W140
    OciPull,                       // W141
    Download,                      // W142
    LockfileMismatch,              // W150
    Spawn,                         // W160
    StartTimeout,                  // W161
    StopTimeout,                   // W162
    Cancelled,                     // W170
    Internal,                      // W900
}

impl WorkerOpErrorKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidName => "W100",
            Self::InvalidSource => "W101",
            Self::LocalPathNotAllowedViaTrigger => "W102",
            Self::MissingTarget => "W103",
            Self::ConsentRequired => "W104",
            Self::NotFound => "W110",
            Self::AlreadyExists => "W111",
            Self::NotInstalled => "W112",
            Self::NotRunning => "W113",
            Self::AlreadyRunning => "W114",
            Self::LockBusy => "W120",
            Self::LockIo => "W121",
            Self::ConfigIo => "W130",
            Self::ConfigParse => "W131",
            Self::Registry => "W140",
            Self::OciPull => "W141",
            Self::Download => "W142",
            Self::LockfileMismatch => "W150",
            Self::Spawn => "W160",
            Self::StartTimeout => "W161",
            Self::StopTimeout => "W162",
            Self::Cancelled => "W170",
            Self::Internal => "W900",
        }
    }
}

#[derive(Debug, Error)]
pub enum WorkerOpError {
    #[error("invalid worker name {name:?}: {reason}")]
    InvalidName { name: String, reason: String },

    #[error("invalid worker source {input:?}: {reason}")]
    InvalidSource { input: String, reason: String },

    #[error("local path {path:?} is not allowed via the worker::* trigger surface")]
    LocalPathNotAllowedViaTrigger { path: String },

    #[error("missing target for {op:?}: {reason}")]
    MissingTarget { op: String, reason: String },

    #[error("{op:?} requires confirmation: pass yes:true")]
    ConsentRequired { op: String },

    #[error("worker {name:?} not found")]
    NotFound { name: String },

    #[error("worker {name:?} already exists")]
    AlreadyExists { name: String },

    #[error("worker {name:?} is not installed")]
    NotInstalled { name: String },

    #[error("worker {name:?} is not running")]
    NotRunning { name: String },

    #[error("worker {name:?} is already running (pid {pid})")]
    AlreadyRunning { name: String, pid: u32 },

    #[error("project lock busy{}", match holder_pid { Some(p) => format!(" (held by pid {})", p), None => String::new() })]
    LockBusy { holder_pid: Option<u32> },

    #[error("lock I/O failed at {path:?}: {source}")]
    LockIo {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("config I/O failed at {path:?}: {source}")]
    ConfigIo {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("config parse failed at {path:?}: {message}")]
    ConfigParse { path: PathBuf, message: String },

    #[error("registry error: {message}")]
    Registry { message: String },

    #[error("OCI pull failed for {reference:?}: {message}")]
    OciPull { reference: String, message: String },

    #[error("download from {url:?} failed: {source}")]
    Download {
        url: String,
        #[source]
        source: io::Error,
    },

    #[error("lockfile mismatch for {worker:?}: expected {expected}, found {found}")]
    LockfileMismatch {
        worker: String,
        expected: String,
        found: String,
    },

    #[error("failed to spawn worker {worker:?}: {source}")]
    Spawn {
        worker: String,
        #[source]
        source: io::Error,
    },

    #[error("worker {worker:?} did not start within {waited_secs}s")]
    StartTimeout { worker: String, waited_secs: u64 },

    #[error("worker {worker:?} did not stop within {waited_secs}s")]
    StopTimeout { worker: String, waited_secs: u64 },

    #[error("operation cancelled")]
    Cancelled,

    #[error("internal: {message}")]
    Internal { message: String },
}

impl WorkerOpError {
    pub fn kind(&self) -> WorkerOpErrorKind {
        use WorkerOpErrorKind as K;
        match self {
            Self::InvalidName { .. } => K::InvalidName,
            Self::InvalidSource { .. } => K::InvalidSource,
            Self::LocalPathNotAllowedViaTrigger { .. } => K::LocalPathNotAllowedViaTrigger,
            Self::MissingTarget { .. } => K::MissingTarget,
            Self::ConsentRequired { .. } => K::ConsentRequired,
            Self::NotFound { .. } => K::NotFound,
            Self::AlreadyExists { .. } => K::AlreadyExists,
            Self::NotInstalled { .. } => K::NotInstalled,
            Self::NotRunning { .. } => K::NotRunning,
            Self::AlreadyRunning { .. } => K::AlreadyRunning,
            Self::LockBusy { .. } => K::LockBusy,
            Self::LockIo { .. } => K::LockIo,
            Self::ConfigIo { .. } => K::ConfigIo,
            Self::ConfigParse { .. } => K::ConfigParse,
            Self::Registry { .. } => K::Registry,
            Self::OciPull { .. } => K::OciPull,
            Self::Download { .. } => K::Download,
            Self::LockfileMismatch { .. } => K::LockfileMismatch,
            Self::Spawn { .. } => K::Spawn,
            Self::StartTimeout { .. } => K::StartTimeout,
            Self::StopTimeout { .. } => K::StopTimeout,
            Self::Cancelled => K::Cancelled,
            Self::Internal { .. } => K::Internal,
        }
    }

    /// Wire envelope: `{ type, code: "Wxxx", message, details }`. Per-variant
    /// `details` holds the structured fields callers can switch on.
    pub fn to_payload(&self) -> Value {
        let details = match self {
            Self::InvalidName { name, reason } => {
                json!({ "name": name, "reason": reason })
            }
            Self::InvalidSource { input, reason } => {
                json!({ "input": input, "reason": reason })
            }
            Self::LocalPathNotAllowedViaTrigger { path } => json!({ "path": path }),
            Self::MissingTarget { op, reason } => json!({ "op": op, "reason": reason }),
            Self::ConsentRequired { op } => json!({ "op": op }),
            Self::NotFound { name }
            | Self::AlreadyExists { name }
            | Self::NotInstalled { name }
            | Self::NotRunning { name } => json!({ "name": name }),
            Self::AlreadyRunning { name, pid } => json!({ "name": name, "pid": pid }),
            Self::LockBusy { holder_pid } => json!({ "holder_pid": holder_pid }),
            Self::LockIo { path, .. } => json!({ "path": path.display().to_string() }),
            Self::ConfigIo { path, .. } => json!({ "path": path.display().to_string() }),
            Self::ConfigParse { path, message } => {
                json!({ "path": path.display().to_string(), "message": message })
            }
            Self::Registry { message } => json!({ "message": message }),
            Self::OciPull { reference, message } => {
                json!({ "reference": reference, "message": message })
            }
            Self::Download { url, .. } => json!({ "url": url }),
            Self::LockfileMismatch {
                worker,
                expected,
                found,
            } => {
                json!({ "worker": worker, "expected": expected, "found": found })
            }
            Self::Spawn { worker, .. } => json!({ "worker": worker }),
            Self::StartTimeout {
                worker,
                waited_secs,
            }
            | Self::StopTimeout {
                worker,
                waited_secs,
            } => {
                json!({ "worker": worker, "waited_secs": waited_secs })
            }
            Self::Cancelled => json!({}),
            Self::Internal { message } => json!({ "message": message }),
        };
        json!({
            "type": "WorkerOpError",
            "code": self.kind().code(),
            "message": self.to_string(),
            "details": details,
        })
    }

    pub fn invalid_name(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidName {
            name: name.into(),
            reason: reason.into(),
        }
    }

    pub fn local_path_not_allowed_via_trigger(path: impl Into<String>) -> Self {
        Self::LocalPathNotAllowedViaTrigger { path: path.into() }
    }

    pub fn not_found(name: impl Into<String>) -> Self {
        Self::NotFound { name: name.into() }
    }

    pub fn cancelled() -> Self {
        Self::Cancelled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn invalid_name_to_payload_has_w100_code() {
        let err = WorkerOpError::invalid_name("BAD NAME!", "contains spaces");
        let payload = err.to_payload();
        assert_eq!(payload["type"], "WorkerOpError");
        assert_eq!(payload["code"], "W100");
        assert!(payload["message"].as_str().unwrap().contains("BAD NAME!"),);
        assert_eq!(payload["details"]["name"], "BAD NAME!");
        assert_eq!(payload["details"]["reason"], "contains spaces");
    }

    #[test]
    fn local_path_not_allowed_via_trigger_is_w102() {
        let err = WorkerOpError::local_path_not_allowed_via_trigger("./my-worker");
        assert_eq!(err.to_payload()["code"], "W102");
    }

    #[test]
    fn missing_target_is_w103() {
        let err = WorkerOpError::MissingTarget {
            op: "remove".into(),
            reason: "names is empty; pass non-empty names or all:true".into(),
        };
        let payload = err.to_payload();
        assert_eq!(payload["code"], "W103");
        assert_eq!(payload["details"]["op"], "remove");
        assert!(
            payload["details"]["reason"]
                .as_str()
                .unwrap()
                .contains("names is empty")
        );
        assert!(
            !payload["message"]
                .as_str()
                .unwrap()
                .contains("invalid worker source"),
            "MissingTarget must not reuse InvalidSource's 'invalid worker source' stem"
        );
    }

    #[test]
    fn consent_required_is_w104() {
        let err = WorkerOpError::ConsentRequired { op: "stop".into() };
        let payload = err.to_payload();
        assert_eq!(payload["code"], "W104");
        assert_eq!(payload["details"]["op"], "stop");
        assert!(
            !payload["message"]
                .as_str()
                .unwrap()
                .contains("invalid worker source"),
            "ConsentRequired must not reuse InvalidSource's 'invalid worker source' stem"
        );
        assert!(payload["message"].as_str().unwrap().contains("yes:true"));
    }

    #[test]
    fn payload_round_trips_through_serde_json() {
        let err = WorkerOpError::not_found("pdfkit");
        let v = err.to_payload();
        let parsed: serde_json::Value = serde_json::from_str(&v.to_string()).unwrap();
        assert_eq!(parsed, v);
    }

    #[test]
    fn cancelled_carries_no_details() {
        let err = WorkerOpError::cancelled();
        let payload = err.to_payload();
        assert_eq!(payload["code"], "W170");
        assert_eq!(payload["details"], json!({}));
    }

    #[test]
    fn invalid_source_payload_uses_input_key_to_match_struct_field() {
        let err = WorkerOpError::InvalidSource {
            input: "ghcr.io/x@:bad".into(),
            reason: "missing tag".into(),
        };
        let payload = err.to_payload();
        assert_eq!(payload["code"], "W101");
        assert_eq!(payload["details"]["input"], "ghcr.io/x@:bad");
        assert_eq!(payload["details"]["reason"], "missing tag");
        assert!(
            payload["details"].get("name").is_none(),
            "InvalidSource details must use 'input' key (matches struct field), not 'name' (which would collide with InvalidName semantics)"
        );
    }
}
