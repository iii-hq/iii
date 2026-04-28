// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! `FsRunner` trait + `IiiShellFsRunner` implementation.
//!
//! The trait is the seam used by unit tests: a `FakeRunner` implements it
//! without touching any real sockets. `IiiShellFsRunner` opens a fresh
//! `iii_shell_client::Session` per call and forwards to the supervisor.

use std::path::PathBuf;

use iii_shell_client::{Session, VmClientError};
use iii_shell_proto::{FsOp, FsReadMeta, FsResult};

use crate::sandbox_daemon::errors::SandboxError;

// ---------------------------------------------------------------------------
// Error mapping helpers
// ---------------------------------------------------------------------------

/// Map a `VmClientError` to the closest `SandboxError` variant, applying
/// the S210–S219 code table when the error is a typed `FsError`.
pub fn map_vm_error(e: VmClientError) -> SandboxError {
    match e {
        VmClientError::FsError { code, message } => match code.as_str() {
            "S210" => SandboxError::FsInvalidRequest(message),
            "S211" => SandboxError::fs_not_found(message),
            "S212" => SandboxError::fs_wrong_type(message),
            "S213" => SandboxError::fs_already_exists(message),
            "S214" => SandboxError::fs_not_empty(message),
            "S215" => SandboxError::FsPermission(message),
            "S216" => SandboxError::FsIo(message),
            "S217" => SandboxError::FsRegex(message),
            "S218" => SandboxError::FsChannelAborted(message),
            "S219" => SandboxError::FsUnsupported,
            other => SandboxError::FsIo(format!("fs error {other}: {message}")),
        },
        other => SandboxError::FsIo(format!("shell client error: {other}")),
    }
}

// ---------------------------------------------------------------------------
// FsRunner trait
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
pub trait FsRunner: Send + Sync + 'static {
    /// Run a one-shot filesystem op (anything except WriteStart/ReadStart).
    async fn fs_call(&self, shell_sock: PathBuf, op: FsOp) -> Result<FsResult, SandboxError>;

    /// Upload bytes from `reader` to `path` inside the guest.
    async fn fs_write_stream(
        &self,
        shell_sock: PathBuf,
        path: String,
        mode: String,
        parents: bool,
        reader: Box<dyn tokio::io::AsyncRead + Unpin + Send>,
    ) -> Result<FsResult, SandboxError>;

    /// Begin a streaming download; returns `(meta, AsyncRead)`.
    async fn fs_read_stream(
        &self,
        shell_sock: PathBuf,
        path: String,
    ) -> Result<(FsReadMeta, Box<dyn tokio::io::AsyncRead + Unpin + Send>), SandboxError>;
}

// ---------------------------------------------------------------------------
// Production implementation
// ---------------------------------------------------------------------------

pub struct IiiShellFsRunner;

#[async_trait::async_trait]
impl FsRunner for IiiShellFsRunner {
    async fn fs_call(&self, shell_sock: PathBuf, op: FsOp) -> Result<FsResult, SandboxError> {
        let session = Session::connect(&shell_sock).await.map_err(map_vm_error)?;
        session.fs_call(op).await.map_err(map_vm_error)
    }

    async fn fs_write_stream(
        &self,
        shell_sock: PathBuf,
        path: String,
        mode: String,
        parents: bool,
        reader: Box<dyn tokio::io::AsyncRead + Unpin + Send>,
    ) -> Result<FsResult, SandboxError> {
        let session = Session::connect(&shell_sock).await.map_err(map_vm_error)?;
        session
            .fs_write_stream(path, mode, parents, reader)
            .await
            .map_err(map_vm_error)
    }

    async fn fs_read_stream(
        &self,
        shell_sock: PathBuf,
        path: String,
    ) -> Result<(FsReadMeta, Box<dyn tokio::io::AsyncRead + Unpin + Send>), SandboxError> {
        let session = Session::connect(&shell_sock).await.map_err(map_vm_error)?;
        let (meta, stream_reader) = session.fs_read_stream(path).await.map_err(map_vm_error)?;
        Ok((meta, Box::new(stream_reader)))
    }
}

#[cfg(test)]
mod tests {
    //! Wire-ABI guard for the S210-S219 mapping.
    //!
    //! `IiiShellFsRunner` is the only place a wire S-code from the guest
    //! supervisor is translated back into a typed `SandboxError` variant.
    //! Every `sandbox::fs::*` trigger response — and therefore every SDK
    //! caller — depends on this mapping being exact. A silent rename
    //! here (e.g. someone refactors `FsNotFound { path }` to
    //! `FsNotFoundAt { path }`) would change the wire `code`/`type` that
    //! the SDK error JSON carries, breaking `if (err.code === 'S211')`
    //! branches in caller code with no compile error.
    //!
    //! These tests pin the round trip:
    //!     guest emits FsError { code, message }
    //!     -> map_vm_error
    //!     -> SandboxError::<variant>
    //!     -> err.to_payload() carries the same { code, type }
    //! for every code in the band, plus the catch-all paths.
    //!
    //! When this table changes, the SDK's documented error contract is
    //! changing too — that's the signal the test exists to surface.

    use super::*;

    fn fs_err(code: &'static str) -> VmClientError {
        VmClientError::FsError {
            code: code.into(),
            message: format!("simulated {code} from supervisor"),
        }
    }

    /// Each wire code maps to a specific SandboxError variant whose
    /// `to_payload()` round-trips back to the same `{ code, type }` pair.
    /// Adding a new S21x code? Add a row. Renaming a variant? This test
    /// will not compile until you update the row, which is the point.
    #[test]
    fn s21x_mapping_is_complete_and_round_trips() {
        let cases: &[(&str, &str, fn(&SandboxError) -> bool)] = &[
            ("S210", "filesystem", |e| {
                matches!(e, SandboxError::FsInvalidRequest(_))
            }),
            ("S211", "filesystem", |e| {
                matches!(e, SandboxError::FsNotFound { .. })
            }),
            ("S212", "filesystem", |e| {
                matches!(e, SandboxError::FsWrongType { .. })
            }),
            ("S213", "filesystem", |e| {
                matches!(e, SandboxError::FsAlreadyExists { .. })
            }),
            ("S214", "filesystem", |e| {
                matches!(e, SandboxError::FsNotEmpty { .. })
            }),
            ("S215", "filesystem", |e| {
                matches!(e, SandboxError::FsPermission(_))
            }),
            ("S216", "filesystem", |e| matches!(e, SandboxError::FsIo(_))),
            ("S217", "filesystem", |e| {
                matches!(e, SandboxError::FsRegex(_))
            }),
            ("S218", "transient", |e| {
                matches!(e, SandboxError::FsChannelAborted(_))
            }),
            ("S219", "filesystem", |e| {
                matches!(e, SandboxError::FsUnsupported)
            }),
        ];

        for (code, expected_type, variant_check) in cases {
            let mapped = map_vm_error(fs_err(code));
            assert!(
                variant_check(&mapped),
                "code {code} mapped to wrong variant: {mapped:?}",
            );
            let payload = mapped.to_payload();
            assert_eq!(
                payload["code"], *code,
                "code {code}: payload.code mismatch ({:?})",
                payload["code"]
            );
            assert_eq!(
                payload["type"], *expected_type,
                "code {code}: payload.type mismatch ({:?})",
                payload["type"]
            );
        }
    }

    /// `S218` is the only retryable code in the band — caller-side
    /// channel aborts can succeed on retry once the caller wires a
    /// fresh channel. Pin the retryable bit so a refactor of
    /// `SandboxErrorCode::retryable` doesn't silently shift it.
    #[test]
    fn s218_is_the_only_retryable_fs_code() {
        for code in &[
            "S210", "S211", "S212", "S213", "S214", "S215", "S216", "S217", "S219",
        ] {
            let payload = map_vm_error(fs_err(code)).to_payload();
            assert_eq!(
                payload["retryable"], false,
                "{code} should not be retryable"
            );
        }
        let payload = map_vm_error(fs_err("S218")).to_payload();
        assert_eq!(payload["retryable"], true, "S218 must be retryable");
    }

    /// Unknown S-codes (anything outside S210-S219) MUST surface as
    /// FsIo with the original code preserved in the message — never
    /// silently coerced to a typed variant. SDK callers branching on
    /// `code` will see `S216` (their generic IO bucket) plus the
    /// original wire string in the message for forensics.
    #[test]
    fn unknown_fs_code_falls_back_to_fs_io_with_code_in_message() {
        let mapped = map_vm_error(VmClientError::FsError {
            code: "S299".into(),
            message: "future unknown thing".into(),
        });
        match &mapped {
            SandboxError::FsIo(s) => {
                assert!(s.contains("S299"), "code preserved in message: {s}");
                assert!(s.contains("future unknown thing"), "msg preserved: {s}");
            }
            other => panic!("expected FsIo, got {other:?}"),
        }
        // And the wire payload uses S216 (FsIo's code) as the generic bucket.
        assert_eq!(mapped.to_payload()["code"], "S216");
    }

    /// Every non-`FsError` `VmClientError` variant — connection failures,
    /// protocol violations, write timeouts, EOF — funnels into FsIo with
    /// the `"shell client error: "` prefix. The prefix is what callers
    /// see in the message field of the trigger response, so it's part
    /// of the public surface; pin it.
    ///
    /// `VmClientError` is not `Clone`, so each iteration builds a fresh
    /// value via the constructor closure.
    #[test]
    fn non_fs_vm_errors_become_shell_client_fs_io() {
        // Each entry: (label, constructor). Label is for the panic
        // message; constructor builds a fresh `VmClientError` value.
        let cases: &[(&str, fn() -> VmClientError)] = &[
            ("SessionTerminated", || VmClientError::SessionTerminated),
            ("WorkerMissing", || {
                VmClientError::WorkerMissing("/tmp/missing.sock: ENOENT".into())
            }),
            ("Permission", || VmClientError::Permission("EACCES".into())),
            ("RelayDown", || {
                VmClientError::RelayDown("relay panic".into())
            }),
            ("AuthRejected", || {
                VmClientError::AuthRejected("uid mismatch".into())
            }),
            ("HandshakeTruncated", || VmClientError::HandshakeTruncated {
                got: 2,
            }),
            ("WriteBlocked", || VmClientError::WriteBlocked),
            ("ProtocolViolation", || {
                VmClientError::ProtocolViolation("bad frame".into())
            }),
            ("DispatcherError", || {
                VmClientError::DispatcherError("guest spawn failed".into())
            }),
        ];
        for (label, build) in cases {
            let mapped = map_vm_error(build());
            match &mapped {
                SandboxError::FsIo(s) => {
                    assert!(
                        s.starts_with("shell client error: "),
                        "missing prefix for {label}: {s}"
                    );
                }
                other => {
                    panic!("non-FsError VmClientError must map to FsIo, got {other:?} for {label}")
                }
            }
            assert_eq!(
                mapped.to_payload()["code"],
                "S216",
                "{label} should land on the FsIo S216 bucket"
            );
        }
    }
}
