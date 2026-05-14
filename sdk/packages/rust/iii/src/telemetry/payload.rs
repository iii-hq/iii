//! Payload redaction + truncation for invocation event capture.

use serde_json::Value;

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
        // `token` alone is too common a substring; require whole-key or suffix match.
        || lower == "token"
        || lower.ends_with("_token")
        || lower.ends_with("-token")
}

/// Recursively redact values of sensitive keys. Returns a new `Value`.
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

/// Redact then serialize to JSON, optionally capped at `max_bytes`.
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

    // Walk back to a char boundary so we don't emit half-codepoints.
    // (`floor_char_boundary` is unstable.)
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
        let s = "aéaéaéaé".repeat(2000);
        let input = json!({ "v": s });
        let (out, truncated) = redact_and_truncate(&input, Some(100));
        assert!(truncated);
        assert!(out.is_char_boundary(out.len()));
    }

    #[test]
    fn redaction_runs_before_truncation() {
        let input = json!({
            "api_key": "sk-must-not-leak",
            "blob": "x".repeat(8192),
        });
        let (out, _) = redact_and_truncate(&input, Some(4096));
        assert!(!out.contains("sk-must-not-leak"));
        assert!(out.contains("[REDACTED]"));
    }
}
