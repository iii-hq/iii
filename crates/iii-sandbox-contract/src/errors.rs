//! S-code error contract for the sandbox subsystem.
//!
//! Wire shape: `ErrorBody { code: "invocation_failed", message: error_payload(..).to_string() }`.
//! Every code has one `<a id="Sxxx"></a>` anchor in `docs/next/creating-workers/sandboxes.mdx`,
//! which `DOCS_BASE` links to.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const DOCS_BASE: &str = "https://iii.dev/docs/creating-workers/sandboxes#";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
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
    S301,
    S302,
    S303,
    S304,
    S400,
    S501,
    S502,
    S503,
    S504,
    S505,
    S506,
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
            Self::S301 => "S301",
            Self::S302 => "S302",
            Self::S303 => "S303",
            Self::S304 => "S304",
            Self::S400 => "S400",
            Self::S501 => "S501",
            Self::S502 => "S502",
            Self::S503 => "S503",
            Self::S504 => "S504",
            Self::S505 => "S505",
            Self::S506 => "S506",
        }
    }

    pub fn error_type(&self) -> &'static str {
        match self {
            Self::S001
            | Self::S002
            | Self::S003
            | Self::S004
            | Self::S501
            | Self::S502
            | Self::S503
            | Self::S504
            | Self::S505 => "validation",
            Self::S100 | Self::S302 | Self::S303 | Self::S400 => "config",
            Self::S101 => "internal",
            Self::S102 | Self::S218 | Self::S301 | Self::S506 => "transient",
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
            Self::S300 | Self::S304 => "platform",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(self, Self::S102 | Self::S218 | Self::S301 | Self::S506)
    }
}

/// The flat `{type, code, message, docs_url, retryable, fix, fix_note}` payload SDK callers receive.
pub fn error_payload(
    code: SandboxErrorCode,
    message: &str,
    fix: Option<Value>,
    fix_note: Option<&str>,
) -> Value {
    json!({
        "type": code.error_type(),
        "code": code.as_str(),
        "message": message,
        "docs_url": format!("{DOCS_BASE}{}", code.as_str()),
        "retryable": code.retryable(),
        "fix": fix,
        "fix_note": fix_note,
    })
}

/// Parses an `ErrorBody.message` and returns its `code`; `None` if it is not an S-code payload.
/// Parses from the first `{`, since the SDK prefixes handler errors with `handler error: `.
pub fn code_of(message: &str) -> Option<SandboxErrorCode> {
    let v: Value = serde_json::from_str(&message[message.find('{')?..]).ok()?;
    SandboxErrorCode::deserialize(v.as_object()?.get("code")?).ok()
}
