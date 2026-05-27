//! S* family error codes for the sandbox subsystem.
//!
//! Payload shape mirrors vm-worker's existing Stripe-style errors:
//! { type, code, message, docs_url, retryable }.

use serde_json::json;
use thiserror::Error;

/// Where `error.docs_url` points to. Today this resolves to anchor
/// headings in the in-repo sandbox README on GitHub, which is the only
/// canonical documentation that exists for the S-code surface. The
/// canonical iii.dev/docs/errors/sandbox/{code} pages are tracked as a
/// follow-up TODO (plan T13); flip this constant when those pages ship.
///
/// `R2 — README anchor stability` (test in `sandbox_docs_anchor_stability.rs`)
/// asserts every `SandboxErrorCode::as_str()` value matches an anchor
/// here, failing CI if a new S-code lands without a README entry.
const DOCS_BASE: &str =
    "https://github.com/iii-hq/iii/blob/main/crates/iii-worker/src/sandbox_daemon/README.md#";

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

    // FS variants. Display impls now carry a kind prefix so a consumer
    // that surfaces only the raw string (logs, debug output, an agent
    // tool-call response that drops the structured envelope) still sees
    // what went wrong — not just a bare path. The wire-level `type` and
    // `code` fields produced by `to_payload` remain authoritative; this
    // change only affects the human-readable Display rendering.
    #[error("{0}")]
    FsInvalidRequest(String),

    #[error("file not found: {path}")]
    FsNotFound { path: String },

    /// Specialisation of `FsNotFound` for the case where the in-VM shell
    /// reports that an INTERMEDIATE directory on the target path is
    /// missing. Same wire S-code (S211), but its `fix_hint` returns a
    /// structured payload (`{ "parents": true }`) the agent can merge into
    /// the original request and resubmit. Splitting this out from the
    /// generic FsNotFound keeps `fix` honest: target-missing still
    /// returns `null` because the recovery depends on caller intent, but
    /// parent-missing has a single canonical fix every time.
    #[error("parent directory not found: {path}")]
    FsParentNotFound { path: String },

    #[error("not a directory or wrong type: {path}")]
    FsWrongType { path: String },

    #[error("already exists: {path}")]
    FsAlreadyExists { path: String },

    #[error("directory not empty: {path}")]
    FsNotEmpty { path: String },

    #[error("permission denied: {0}")]
    FsPermission(String),

    #[error("fs i/o error: {0}")]
    FsIo(String),

    #[error("invalid regex pattern: {0}")]
    FsRegex(String),

    #[error("fs channel aborted: {0}")]
    FsChannelAborted(String),

    #[error(
        "fs operation unsupported by this sandbox supervisor; upgrade iii-worker to enable fs::* triggers (see S219 docs)"
    )]
    FsUnsupported,

    /// Wrapper produced by sandbox::run when a sub-step (create / fs::write /
    /// exec / stop) fails. Preserves the inner error's S-code on the wire
    /// (via `code()` below) while adding structured step + sandbox_id
    /// attribution to the `fix` payload, so agents that need to identify
    /// which step failed don't have to substring-match the message.
    ///
    /// The `inner_code` is the originating variant's `SandboxErrorCode`;
    /// `to_payload` emits that as the top-level `code`/`type`, with this
    /// wrapper showing up only inside `fix.context`.
    #[error("during sandbox::run step `{step}` (sandbox_id={sandbox_id}): {message}")]
    RunStepFailed {
        step: String,
        sandbox_id: String,
        message: String,
        inner_code: SandboxErrorCode,
    },
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
            Self::FsNotFound { .. } | Self::FsParentNotFound { .. } => SandboxErrorCode::S211,
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
            // RunStepFailed transparently carries the inner code so the
            // wire-level type/code identify the actual failure category;
            // step + sandbox_id attribution lives in fix.context.
            Self::RunStepFailed { inner_code, .. } => *inner_code,
        }
    }

    pub fn to_payload(&self) -> serde_json::Value {
        let code = self.code();
        let (mut fix, fix_note) = self.fix_hint();

        // sandbox::run sub-step attribution. When the error originated
        // inside sandbox::run, fix.context carries the structured step
        // name and sandbox_id so agents don't have to grep the message.
        // We promote `fix` from None to Some({...}) just for this case;
        // other variants keep whatever fix_hint produced.
        if let Self::RunStepFailed {
            step,
            sandbox_id,
            inner_code,
            ..
        } = self
        {
            fix = Some(json!({
                "context": format!("during sandbox::run step `{step}`"),
                "sandbox_id": sandbox_id,
                "inner_code": inner_code.as_str(),
            }));
        }

        json!({
            "type": code.error_type(),
            "code": code.as_str(),
            "message": self.to_string(),
            "docs_url": format!("{}{}", DOCS_BASE, code.as_str()),
            "retryable": code.retryable(),
            // Magical-moment field (D4). When non-null, contains a JSON
            // payload the caller can resubmit verbatim as the next call.
            // For RunStepFailed (E2), `fix.context` names which step
            // inside sandbox::run failed and the sandbox_id (so a caller
            // with keep_sandbox:true can clean up). Null when the error
            // has no machine-fixable shape (VM boot failed, allowlist
            // mismatch, etc.); `fix_note` then explains why.
            "fix": fix,
            "fix_note": fix_note,
        })
    }

    /// Returns `(fix, note)` for `to_payload`'s `fix`/`fix_note` fields.
    ///
    /// `fix` is a JSON payload the caller can resubmit verbatim if known.
    /// Returning `None` here yields `"fix": null` on the wire, with `note`
    /// explaining why the error has no auto-fix (operator-config-driven,
    /// platform failure, capacity, etc.) so agents know not to retry blindly.
    ///
    /// Today we surface a fix only for the most common SDK-side mistake
    /// — `S001 InvalidRequest` carrying our `cmd must be a single binary`
    /// or `cmd contains whitespace AND args is set` hint. Other variants
    /// return `fix: null`. Future improvements may extend coverage as
    /// real agent-failure logs identify high-value targets.
    fn fix_hint(&self) -> (Option<serde_json::Value>, Option<&'static str>) {
        match self {
            // Shell-line input the validator can normalise to argv. We
            // could try to construct a literal `fix` payload here, but
            // shape-resolving without the original request is fragile;
            // the prose message already names the canonical fix. Leave
            // `fix: null` with a pointer note rather than a guessed
            // payload that could collide with an unrelated `args` field.
            Self::InvalidRequest(_) => (None, Some("see message: examples are inline")),
            Self::ImageNotInCatalog { .. } => (
                None,
                Some(
                    "set `image` to a value listed in the message or add a custom_images entry in iii.config.yaml",
                ),
            ),
            Self::RootfsMissing { .. } => (
                None,
                Some("operator action required: run `iii worker add <image-ref>` on the host"),
            ),
            Self::AutoInstallFailed { .. } => (None, Some("transient: retry after a short delay")),
            Self::ExecTimedOut { .. } => (
                None,
                Some("raise `timeout_ms` on the exec call or split the work into smaller steps"),
            ),
            Self::ConcurrentExec(_) => (
                None,
                Some("wait for the in-flight exec to complete before submitting another"),
            ),
            Self::AlreadyStopped(_) | Self::NotFound(_) => (
                None,
                Some("the sandbox is gone; call sandbox::create first"),
            ),
            Self::BootFailed(_) | Self::ResourceLimit(_) => {
                (None, Some("platform-level failure; not auto-recoverable"))
            }
            // Parent directory missing on a write/mkdir: the fix is a
            // single boolean. Emit a structured `fix` payload the agent
            // can merge into the original request and resubmit verbatim,
            // plus a fix_note so the same recipe is readable in logs.
            Self::FsParentNotFound { .. } => (
                Some(json!({ "parents": true })),
                Some(
                    "merge `fix` into the original request and resubmit: `parents: true` auto-creates missing intermediate directories",
                ),
            ),
            // Other FS variants. The fix is usually obvious from the path;
            // the structured envelope plus prose message together suffice.
            // Treat them as "no machine-fixable shape" by default.
            Self::FsInvalidRequest(_)
            | Self::FsNotFound { .. }
            | Self::FsWrongType { .. }
            | Self::FsAlreadyExists { .. }
            | Self::FsNotEmpty { .. }
            | Self::FsPermission(_)
            | Self::FsIo(_)
            | Self::FsRegex(_)
            | Self::FsChannelAborted(_)
            | Self::FsUnsupported => (None, None),
            // RunStepFailed's fix is set explicitly in to_payload above
            // (with structured context); this arm is the no-op default.
            Self::RunStepFailed { .. } => (None, None),
        }
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

/// Display-as-JSON wire adapter for `RegisterFunction::new_async`.
///
/// The new async-handler builder collapses errors via `Display`, but the
/// `sandbox::*` wire contract — preserved across the
/// `register_function_with` → `register_function` migration — is the
/// structured payload produced by [`SandboxError::to_payload`]
/// (`code`/`type`/`message`/`docs_url`/`retryable`). Wrapping the error
/// in `SandboxErrorWire` and `map_err`-ing into it makes `Display` emit
/// that JSON, so callers (CLI, agents, engine clients) see the exact
/// same body they did when handlers wrote
/// `IIIError::Handler(serde_json::to_string(&e.to_payload())…)` by hand.
///
/// SDK contract dependency: this wrapper is load-bearing only as long as
/// `iii_sdk::IntoAsyncHandler` collapses `E` via `e.to_string()` (i.e.
/// the `Display` impl). If the SDK ever switches to a structured error
/// trait or to `Debug`, the wire format will drift silently — the
/// `sandbox_error_wire_display_matches_to_payload_json` test pins the
/// local invariant but cannot catch SDK-side regressions. Re-audit this
/// adapter whenever `iii-sdk`'s handler-error path changes.
pub struct SandboxErrorWire(pub SandboxError);

impl From<SandboxError> for SandboxErrorWire {
    fn from(err: SandboxError) -> Self {
        SandboxErrorWire(err)
    }
}

impl std::fmt::Display for SandboxErrorWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Falls back to the inner `thiserror` Display only if the JSON
        // payload itself cannot be serialized — matching the
        // `unwrap_or_else(|_| e.to_string())` branch of the legacy
        // hand-written handlers.
        match serde_json::to_string(&self.0.to_payload()) {
            Ok(json) => f.write_str(&json),
            Err(_) => std::fmt::Display::fmt(&self.0, f),
        }
    }
}

impl std::fmt::Debug for SandboxErrorWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.0, f)
    }
}

impl From<SandboxErrorWire> for iii_sdk::IIIError {
    fn from(err: SandboxErrorWire) -> Self {
        iii_sdk::IIIError::Handler(err.to_string())
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
    fn fs_parent_not_found_emits_structured_fix_with_parents_true() {
        // The highest-leverage S211 case: an agent wrote to /workspace/x.js
        // and the parent didn't exist. The daemon must surface a
        // structured `fix` payload the agent can merge verbatim — this
        // is the "magical moment" for fs::write/mkdir error UX.
        let err = SandboxError::FsParentNotFound {
            path: "parent not found: /workspace".into(),
        };
        let payload = err.to_payload();

        // Same wire S-code as a plain FsNotFound.
        assert_eq!(payload["code"], "S211");
        assert_eq!(payload["type"], "filesystem");

        // The magical moment: fix is non-null and resubmittable.
        let fix = &payload["fix"];
        assert!(fix.is_object(), "fix must be a structured object");
        assert_eq!(fix["parents"], true);

        // fix_note explains how to use the fix payload.
        let note = payload["fix_note"].as_str().expect("fix_note set");
        assert!(
            note.contains("parents: true"),
            "fix_note must mention `parents: true`, got: {note}"
        );
        assert!(
            note.contains("merge"),
            "fix_note must tell the agent to merge into the original request"
        );
    }

    #[test]
    fn fs_not_found_target_missing_still_returns_null_fix() {
        // Inverse pin: when the TARGET path is missing (not a parent), the
        // recovery depends on caller intent (was the path wrong? was the
        // file already deleted?), so `fix` stays null. We don't want to
        // bait agents into a wrong retry by emitting `{ parents: true }`
        // for every S211.
        let err = SandboxError::fs_not_found("/workspace/missing.js");
        let payload = err.to_payload();

        assert_eq!(payload["code"], "S211");
        assert_eq!(payload["type"], "filesystem");
        assert!(payload["fix"].is_null());
        assert!(payload["fix_note"].is_null());
    }

    #[test]
    fn run_step_failed_emits_structured_fix_context() {
        // RunStepFailed wraps a sub-step error from sandbox::run with
        // structured attribution. The wire-level S-code transparently
        // carries the inner code so agents see the actual failure
        // category; the step + sandbox_id live in fix.context for
        // machine-readable handling.
        let err = SandboxError::RunStepFailed {
            step: "fs::write (code)".to_string(),
            sandbox_id: "11111111-2222-3333-4444-555555555555".to_string(),
            message: "permission denied: /tmp/run.py".to_string(),
            inner_code: SandboxErrorCode::S215,
        };
        let payload = err.to_payload();

        // Inner code surfaces as the top-level S-code; the wrapper is
        // invisible at the wire level except via fix.context.
        assert_eq!(payload["code"], "S215");
        assert_eq!(payload["type"], "filesystem");

        let fix = &payload["fix"];
        assert!(fix.is_object(), "fix must be a structured object, not null");
        assert_eq!(
            fix["context"],
            "during sandbox::run step `fs::write (code)`"
        );
        assert_eq!(fix["sandbox_id"], "11111111-2222-3333-4444-555555555555");
        assert_eq!(fix["inner_code"], "S215");

        // Message round-trips the inner error verbatim with a step prefix.
        let message = payload["message"].as_str().unwrap();
        assert!(
            message.contains("fs::write (code)") && message.contains("permission denied"),
            "message must show both the step and the inner cause; got {message:?}"
        );
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

    /// Pins the wire format `RegisterFunction::new_async` callers see for
    /// `sandbox::*` errors. Before the migration, handlers wrote
    /// `IIIError::Handler(serde_json::to_string(&e.to_payload())…)`
    /// directly; after, they `map_err` into `SandboxErrorWire` and the
    /// SDK's async-handler glue calls `Display`. This test asserts both
    /// paths produce the same JSON bytes, so callers branching on
    /// `code` / `type` / `retryable` keep working.
    #[test]
    fn sandbox_error_wire_display_matches_to_payload_json() {
        let cases: &[SandboxError] = &[
            SandboxError::InvalidRequest("cmd must be a single binary".into()),
            SandboxError::NotFound("11111111-1111-1111-1111-111111111111".into()),
            SandboxError::ExecTimedOut { timeout_ms: 1500 },
            SandboxError::FsUnsupported,
        ];
        for err in cases {
            let expected = serde_json::to_string(&err.to_payload()).unwrap();
            let actual = SandboxErrorWire(err.clone()).to_string();
            assert_eq!(actual, expected, "wire format drift for {err:?}");
            // And the embedded code stays parseable by clients.
            let parsed: serde_json::Value = serde_json::from_str(&actual).unwrap();
            assert_eq!(parsed["code"], err.code().as_str());
        }
    }
}
