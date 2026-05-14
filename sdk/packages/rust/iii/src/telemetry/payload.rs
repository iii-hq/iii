//! Payload redaction + truncation for invocation event capture.
//!
//! When payload capture is enabled (the default; gate with
//! `III_DISABLE_TRACE_PAYLOADS=1`), the iii-sdk dispatcher emits
//! `iii.invocation.input` and `iii.invocation.output` span events carrying
//! the JSON payload of every worker invocation. Two safety nets must run
//! before those payloads leave the process:
//!
//! 1. **Redaction.** Keys whose name matches well-known credential patterns
//!    (`api_key`, `token`, `password`, `secret`, `credential`, `authorization`)
//!    are replaced with `"[REDACTED]"`. The match is case-insensitive and
//!    walks the value recursively, so nested objects and arrays inside
//!    arrays are covered. Without this, exposing the events tab in a shared
//!    console would leak provider API keys, OAuth tokens, and similar.
//!
//! 2. **Optional truncation.** Conversation histories, tool-call payloads,
//!    and LLM responses can reach megabytes. By default we emit the full
//!    payload — readability beats ring-buffer pressure for the common
//!    "what did the LLM see?" question. Set `III_TRACE_PAYLOAD_MAX_BYTES=N`
//!    to cap each serialized payload at N bytes (truncated output gets a
//!    `..."[TRUNCATED]"` suffix); set `0` or leave unset for unlimited.

use serde_json::Value;

/// Read the `III_TRACE_PAYLOAD_MAX_BYTES` env var.
///
/// Returns `Some(n)` for a positive integer; `None` for unset, `"0"`,
/// `"unlimited"`, or anything that fails to parse. `None` means do not
/// truncate.
#[must_use]
pub fn resolve_max_bytes_from_env() -> Option<usize> {
    let raw = std::env::var("III_TRACE_PAYLOAD_MAX_BYTES").ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("unlimited") {
        return None;
    }
    match trimmed.parse::<usize>() {
        Ok(0) => None,
        Ok(n) => Some(n),
        Err(_) => None,
    }
}

pub const REDACTED_PLACEHOLDER: &str = "[REDACTED]";
const TRUNCATION_MARKER: &str = "...\"[TRUNCATED]\"";

/// Returns true if `key` should have its value redacted. Case-insensitive
/// substring match against the well-known credential key fragments.
fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    [
        "api_key",
        "apikey",
        "api-key",
        "password",
        "secret",
        "credential",
        "authorization",
        "auth_token",
        "access_token",
        "refresh_token",
        "bearer",
        "private_key",
        "client_secret",
    ]
    .iter()
    .any(|fragment| lower.contains(fragment))
        // `token` alone is too common; gate to whole-token or suffix match.
        || lower == "token"
        || lower.ends_with("_token")
        || lower.ends_with("-token")
}

/// Recursively walks a `serde_json::Value`, replacing values of sensitive
/// keys with `[REDACTED]`. Returns a new `Value`; does not mutate input.
#[must_use]
pub fn redact(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                if is_sensitive_key(k) {
                    out.insert(k.clone(), Value::String(REDACTED_PLACEHOLDER.into()));
                } else {
                    out.insert(k.clone(), redact(v));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(redact).collect()),
        _ => value.clone(),
    }
}

/// Redact sensitive keys then serialize to JSON, optionally capped at
/// `max_bytes`.
///
/// Pass `None` (or unset `III_TRACE_PAYLOAD_MAX_BYTES`) to emit the full
/// payload — the new default.
///
/// Returns `(json_string, truncated)`. When `truncated == true`, the
/// returned string ends with the truncation marker so consumers can tell
/// the payload was clipped. Truncation operates on UTF-8 byte boundaries
/// to avoid emitting malformed multi-byte sequences.
#[must_use]
pub fn redact_and_truncate(value: &Value, max_bytes: Option<usize>) -> (String, bool) {
    let redacted = redact(value);
    let serialized = serde_json::to_string(&redacted).unwrap_or_else(|_| "null".into());

    let Some(cap) = max_bytes else {
        return (serialized, false);
    };

    if serialized.len() <= cap {
        return (serialized, false);
    }

    // Find the largest UTF-8 char boundary <= cap so we don't split a
    // multi-byte codepoint. `floor_char_boundary` is unstable, so do it by
    // hand: walk back at most 3 bytes (max UTF-8 sequence is 4 bytes).
    let mut cut = cap.saturating_sub(TRUNCATION_MARKER.len());
    while cut > 0 && !serialized.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut truncated = serialized[..cut].to_string();
    truncated.push_str(TRUNCATION_MARKER);
    (truncated, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_top_level_sensitive_keys() {
        let input = json!({
            "api_key": "sk-abc123",
            "model": "claude-3-5",
        });
        let out = redact(&input);
        assert_eq!(out["api_key"], json!("[REDACTED]"));
        assert_eq!(out["model"], json!("claude-3-5"));
    }

    #[test]
    fn redacts_nested_sensitive_keys() {
        let input = json!({
            "headers": {
                "Authorization": "Bearer xyz",
                "Content-Type": "application/json"
            },
            "config": { "secret": "hush" }
        });
        let out = redact(&input);
        assert_eq!(out["headers"]["Authorization"], json!("[REDACTED]"));
        assert_eq!(out["headers"]["Content-Type"], json!("application/json"));
        assert_eq!(out["config"]["secret"], json!("[REDACTED]"));
    }

    #[test]
    fn redacts_inside_arrays() {
        // Parent key `accounts` is not sensitive → recurse into the
        // array; each item's `access_token` inside should be redacted
        // individually while the username stays visible.
        let input = json!({
            "accounts": [
                { "access_token": "a", "user": "alice" },
                { "access_token": "b", "user": "bob" }
            ]
        });
        let out = redact(&input);
        assert_eq!(out["accounts"][0]["access_token"], json!("[REDACTED]"));
        assert_eq!(out["accounts"][0]["user"], json!("alice"));
        assert_eq!(out["accounts"][1]["access_token"], json!("[REDACTED]"));
        assert_eq!(out["accounts"][1]["user"], json!("bob"));
    }

    #[test]
    fn sensitive_parent_key_redacts_entire_subtree() {
        // When the parent key itself is sensitive (e.g. `credentials`,
        // `password`, `secret`), the whole value is replaced — we don't
        // try to be clever about partial redaction inside it. This is
        // the safer default: leaking partial credentials is still
        // leaking credentials.
        let input = json!({
            "credentials": [
                { "username": "alice", "token": "a" },
            ]
        });
        let out = redact(&input);
        assert_eq!(out["credentials"], json!("[REDACTED]"));
    }

    #[test]
    fn case_insensitive_match() {
        let input = json!({
            "API_KEY": "x",
            "PassWord": "y",
            "client_SECRET": "z",
        });
        let out = redact(&input);
        assert_eq!(out["API_KEY"], json!("[REDACTED]"));
        assert_eq!(out["PassWord"], json!("[REDACTED]"));
        assert_eq!(out["client_SECRET"], json!("[REDACTED]"));
    }

    #[test]
    fn token_alone_matched_but_not_substring() {
        // bare "token" key → redact; "notification" (contains "tion") → keep.
        let input = json!({
            "token": "tok-1",
            "id_token": "tok-2",
            "notification": "ping",
            "function_id": "do_thing",
        });
        let out = redact(&input);
        assert_eq!(out["token"], json!("[REDACTED]"));
        assert_eq!(out["id_token"], json!("[REDACTED]"));
        assert_eq!(out["notification"], json!("ping"));
        assert_eq!(out["function_id"], json!("do_thing"));
    }

    #[test]
    fn no_truncation_when_under_limit() {
        let input = json!({ "model": "claude-3-5" });
        let (out, truncated) = redact_and_truncate(&input, Some(4096));
        assert!(!truncated);
        assert!(!out.ends_with(TRUNCATION_MARKER));
    }

    #[test]
    fn truncates_when_over_limit() {
        let big_string = "x".repeat(8192);
        let input = json!({ "blob": big_string });
        let (out, truncated) = redact_and_truncate(&input, Some(4096));
        assert!(truncated);
        assert!(out.ends_with(TRUNCATION_MARKER));
        assert!(out.len() <= 4096);
    }

    #[test]
    fn never_truncates_when_max_is_none() {
        let big_string = "x".repeat(1_000_000);
        let input = json!({ "blob": big_string });
        let (out, truncated) = redact_and_truncate(&input, None);
        assert!(!truncated);
        assert!(!out.ends_with(TRUNCATION_MARKER));
        assert!(out.len() > 1_000_000);
    }

    #[test]
    fn truncation_preserves_utf8_boundaries() {
        // Build a payload that places a multi-byte char near the cut.
        // Use é (U+00E9, 2 bytes in UTF-8) every other position.
        let s = "aéaéaéaé".repeat(2000);
        let input = json!({ "v": s });
        let (out, truncated) = redact_and_truncate(&input, Some(100));
        assert!(truncated);
        // The truncated string must still be valid UTF-8 — if we cut mid
        // codepoint, `out.chars().count()` would panic via the slice. We
        // already produced a String, so just confirm it round-trips.
        assert!(out.is_char_boundary(out.len()));
    }

    #[test]
    fn redaction_runs_before_truncation() {
        // Even if the truncation cap is small, sensitive keys near the top
        // of the payload must be redacted before serialization.
        let input = json!({
            "api_key": "sk-must-not-leak",
            "blob": "x".repeat(8192),
        });
        let (out, _) = redact_and_truncate(&input, Some(4096));
        assert!(!out.contains("sk-must-not-leak"));
        assert!(out.contains("[REDACTED]"));
    }
}
