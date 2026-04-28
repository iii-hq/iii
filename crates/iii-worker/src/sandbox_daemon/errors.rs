//! S* family error codes for the sandbox subsystem.
//!
//! Payload shape mirrors vm-worker's existing Stripe-style errors:
//! { type, code, message, docs_url, retryable }.

use serde_json::json;
use thiserror::Error;

const DOCS_BASE: &str = "https://iii.dev/docs/errors/sandbox/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxErrorCode {
    S001,
    S002,
    S003,
    S004,
    S100,
    S101,
    S102,
    S200,
    S210,
    S211,
    S212,
    S213,
    S214,
    S215,
    S216,
    S217,
    S218,
    S219,
    S300,
    S400,
}

impl SandboxErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::S001 => "S001",
            Self::S002 => "S002",
            Self::S003 => "S003",
            Self::S004 => "S004",
            Self::S100 => "S100",
            Self::S101 => "S101",
            Self::S102 => "S102",
            Self::S200 => "S200",
            Self::S210 => "S210",
            Self::S211 => "S211",
            Self::S212 => "S212",
            Self::S213 => "S213",
            Self::S214 => "S214",
            Self::S215 => "S215",
            Self::S216 => "S216",
            Self::S217 => "S217",
            Self::S218 => "S218",
            Self::S219 => "S219",
            Self::S300 => "S300",
            Self::S400 => "S400",
        }
    }

    pub fn error_type(&self) -> &'static str {
        match self {
            Self::S001 | Self::S002 | Self::S003 | Self::S004 => "validation",
            Self::S100 | Self::S400 => "config",
            Self::S101 => "internal",
            Self::S102 | Self::S218 => "transient",
            Self::S200 => "execution",
            Self::S210
            | Self::S211
            | Self::S212
            | Self::S213
            | Self::S214
            | Self::S215
            | Self::S216
            | Self::S217
            | Self::S219 => "filesystem",
            Self::S300 => "platform",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(self, Self::S102 | Self::S218)
    }
}

#[derive(Debug, Error, Clone)]
pub enum SandboxError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("sandbox not found: {0}")]
    NotFound(String),

    #[error("concurrent exec on sandbox {0}; await the previous exec before firing another")]
    ConcurrentExec(String),

    #[error("sandbox already stopped: {0}")]
    AlreadyStopped(String),

    #[error(
        "image '{image}' not in catalog; valid presets are 'python' and 'node', or add a custom image via worker config (see S100 docs)"
    )]
    ImageNotInCatalog { image: String },

    #[error(
        "rootfs missing on disk for image '{image}'. Run: iii worker add <image-ref> (see S101 docs)"
    )]
    RootfsMissing { image: String },

    #[error("auto-install failed for image '{image}': {reason}")]
    AutoInstallFailed { image: String, reason: String },

    #[error("exec timed out after {timeout_ms} ms")]
    ExecTimedOut { timeout_ms: u64 },

    /// S300 means the VM itself failed to boot (or its shell socket
    /// became unreachable mid-session). Per-exec spawn failures
    /// (`execve` ENOENT/ENOTDIR/EACCES on a healthy VM) must NOT use
    /// this variant — they surface as a normal `ExecResponse` with
    /// `exit_code: 127` (or `126`) per POSIX shell semantics. See
    /// `adapters.rs::classify_dispatcher_spawn_error`.
    #[error("VM boot failed: {0}")]
    BootFailed(String),

    #[error("resource limit exceeded: {0}")]
    ResourceLimit(String),

    // FS variants below carry the supervisor's verbatim message in their
    // inner `String` (or `path`) field — `IiiShellFsRunner::map_vm_error`
    // is the canonical constructor and never reformats. Display passes
    // the message through unchanged so it can't double-prefix the
    // supervisor's own framing. The S-code carries the typed category
    // on the wire (`to_payload`'s `code` + `type` fields).
    #[error("{0}")]
    FsInvalidRequest(String),

    #[error("{path}")]
    FsNotFound { path: String },

    #[error("{path}")]
    FsWrongType { path: String },

    #[error("{path}")]
    FsAlreadyExists { path: String },

    #[error("{path}")]
    FsNotEmpty { path: String },

    #[error("{0}")]
    FsPermission(String),

    #[error("{0}")]
    FsIo(String),

    #[error("{0}")]
    FsRegex(String),

    #[error("{0}")]
    FsChannelAborted(String),

    #[error(
        "fs operation unsupported by this sandbox supervisor; upgrade iii-worker to enable fs::* triggers (see S219 docs)"
    )]
    FsUnsupported,
}

impl SandboxError {
    // Code assignments are the wire ABI surfaced to SDK callers via the
    // flat `{type, code, message, docs_url, retryable}` payload they
    // receive from `iii.trigger()`. The `sdk_contract_mapping` test
    // pins this mapping; changing any arm below silently changes the
    // S-code every SDK user sees.
    pub fn code(&self) -> SandboxErrorCode {
        match self {
            Self::InvalidRequest(_) => SandboxErrorCode::S001,
            Self::NotFound(_) => SandboxErrorCode::S002,
            Self::ConcurrentExec(_) => SandboxErrorCode::S003,
            Self::AlreadyStopped(_) => SandboxErrorCode::S004,
            Self::ImageNotInCatalog { .. } => SandboxErrorCode::S100,
            Self::RootfsMissing { .. } => SandboxErrorCode::S101,
            Self::AutoInstallFailed { .. } => SandboxErrorCode::S102,
            Self::ExecTimedOut { .. } => SandboxErrorCode::S200,
            Self::FsInvalidRequest(_) => SandboxErrorCode::S210,
            Self::FsNotFound { .. } => SandboxErrorCode::S211,
            Self::FsWrongType { .. } => SandboxErrorCode::S212,
            Self::FsAlreadyExists { .. } => SandboxErrorCode::S213,
            Self::FsNotEmpty { .. } => SandboxErrorCode::S214,
            Self::FsPermission(_) => SandboxErrorCode::S215,
            Self::FsIo(_) => SandboxErrorCode::S216,
            Self::FsRegex(_) => SandboxErrorCode::S217,
            Self::FsChannelAborted(_) => SandboxErrorCode::S218,
            Self::FsUnsupported => SandboxErrorCode::S219,
            Self::BootFailed(_) => SandboxErrorCode::S300,
            Self::ResourceLimit(_) => SandboxErrorCode::S400,
        }
    }

    pub fn to_payload(&self) -> serde_json::Value {
        let code = self.code();
        json!({
            "type": code.error_type(),
            "code": code.as_str(),
            "message": self.to_string(),
            "docs_url": format!("{}{}", DOCS_BASE, code.as_str()),
            "retryable": code.retryable(),
        })
    }

    pub fn image_not_in_catalog(image: impl Into<String>) -> Self {
        Self::ImageNotInCatalog {
            image: image.into(),
        }
    }

    pub fn auto_install_failed(image: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::AutoInstallFailed {
            image: image.into(),
            reason: reason.into(),
        }
    }

    pub fn exec_timed_out(timeout_ms: u64) -> Self {
        Self::ExecTimedOut { timeout_ms }
    }

    /// Construct an `FsNotFound` error for `path`.
    pub fn fs_not_found(path: impl Into<String>) -> Self {
        Self::FsNotFound { path: path.into() }
    }

    /// Construct an `FsWrongType` error for `path`.
    pub fn fs_wrong_type(path: impl Into<String>) -> Self {
        Self::FsWrongType { path: path.into() }
    }

    /// Construct an `FsAlreadyExists` error for `path`.
    pub fn fs_already_exists(path: impl Into<String>) -> Self {
        Self::FsAlreadyExists { path: path.into() }
    }

    /// Construct an `FsNotEmpty` error for `path`.
    pub fn fs_not_empty(path: impl Into<String>) -> Self {
        Self::FsNotEmpty { path: path.into() }
    }

    /// Classify a `std::io::Error` into the closest S21x variant. Callers
    /// use this when bubbling `std::fs` / `tokio::fs` errors out of a
    /// supervisor handler so the wire-level S-code is stable.
    pub fn from_io(path: &str, err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::NotFound => Self::fs_not_found(path),
            std::io::ErrorKind::AlreadyExists => Self::fs_already_exists(path),
            std::io::ErrorKind::PermissionDenied => Self::FsPermission(format!("{path}: {err}")),
            _ => Self::FsIo(format!("{path}: {err}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s100_serializes_with_inline_fix() {
        let err = SandboxError::image_not_in_catalog("dangerous-image");
        let payload = err.to_payload();
        assert_eq!(payload["code"], "S100");
        assert_eq!(payload["type"], "config");
        assert!(
            payload["message"]
                .as_str()
                .unwrap()
                .contains("dangerous-image")
        );
        assert!(payload["message"].as_str().unwrap().contains("python"));
        assert_eq!(payload["retryable"], false);
    }

    #[test]
    fn s102_serializes_retryable_true() {
        let err = SandboxError::auto_install_failed("python", "network down");
        let payload = err.to_payload();
        assert_eq!(payload["code"], "S102");
        assert_eq!(payload["retryable"], true);
    }

    #[test]
    fn s200_timeout_code() {
        let err = SandboxError::exec_timed_out(30_000);
        let payload = err.to_payload();
        assert_eq!(payload["code"], "S200");
    }

    #[test]
    fn s400_resource_limit_is_config_type() {
        let err = SandboxError::ResourceLimit("cpu cap".into());
        let payload = err.to_payload();
        assert_eq!(payload["code"], "S400");
        assert_eq!(payload["type"], "config");
    }

    #[test]
    fn fs_codes_serialize_with_filesystem_type() {
        let err = SandboxError::FsNotFound {
            path: "/missing".into(),
        };
        let payload = err.to_payload();
        assert_eq!(payload["code"], "S211");
        assert_eq!(payload["type"], "filesystem");
        assert_eq!(payload["retryable"], false);
    }

    #[test]
    fn fs_channel_aborted_is_retryable() {
        let err = SandboxError::FsChannelAborted("closed early".into());
        let payload = err.to_payload();
        assert_eq!(payload["code"], "S218");
        assert_eq!(payload["retryable"], true);
    }

    #[test]
    fn fs_unsupported_surfaces_version_hint() {
        let err = SandboxError::FsUnsupported;
        let payload = err.to_payload();
        assert_eq!(payload["code"], "S219");
        assert!(payload["message"].as_str().unwrap().contains("supervisor"));
    }

    #[test]
    fn fs_contract_mapping() {
        let cases: &[(SandboxError, &str)] = &[
            (SandboxError::FsInvalidRequest("bad mode".into()), "S210"),
            (SandboxError::FsNotFound { path: "x".into() }, "S211"),
            (SandboxError::FsWrongType { path: "x".into() }, "S212"),
            (SandboxError::FsAlreadyExists { path: "x".into() }, "S213"),
            (SandboxError::FsNotEmpty { path: "x".into() }, "S214"),
            (SandboxError::FsPermission("x".into()), "S215"),
            (SandboxError::FsIo("x".into()), "S216"),
            (SandboxError::FsRegex("x".into()), "S217"),
            (SandboxError::FsChannelAborted("x".into()), "S218"),
            (SandboxError::FsUnsupported, "S219"),
        ];
        for (err, expected) in cases {
            assert_eq!(err.code().as_str(), *expected, "case: {err:?}");
        }
    }

    /// Wire ABI pin. SDKs receive the flat `to_payload()` shape via
    /// `iii.trigger()`; the S-codes below are the stable surface callers
    /// branch on. Changing any row silently renumbers the error every
    /// Node / Python / Rust caller sees.
    #[test]
    fn sdk_contract_mapping() {
        let cases: &[(SandboxError, &str)] = &[
            (SandboxError::InvalidRequest("x".into()), "S001"),
            (SandboxError::NotFound("x".into()), "S002"),
            (SandboxError::ConcurrentExec("x".into()), "S003"),
            (SandboxError::AlreadyStopped("x".into()), "S004"),
            (SandboxError::image_not_in_catalog("x"), "S100"),
            (SandboxError::RootfsMissing { image: "x".into() }, "S101"),
            (SandboxError::auto_install_failed("x", "y"), "S102"),
            (SandboxError::exec_timed_out(1), "S200"),
            (SandboxError::BootFailed("x".into()), "S300"),
            (SandboxError::ResourceLimit("x".into()), "S400"),
        ];
        for (err, expected) in cases {
            assert_eq!(
                err.code().as_str(),
                *expected,
                "variant {err:?} expected to serialize with code {expected}"
            );
        }
    }
}
