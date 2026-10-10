//! Payload redaction + truncation for invocation event capture.

use std::io::{self, Write};

use serde::{
    Serialize, Serializer,
    ser::{SerializeMap, SerializeSeq},
};
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

// Serialize the redacted view directly instead of building a second JSON tree.
struct Redacted<'a>(&'a Value);

impl Serialize for Redacted<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Object(map) => {
                let mut output = serializer.serialize_map(Some(map.len()))?;
                for (key, value) in map {
                    if is_sensitive_key(key) {
                        output.serialize_entry(key, REDACTED_PLACEHOLDER)?;
                    } else {
                        output.serialize_entry(key, &Redacted(value))?;
                    }
                }
                output.end()
            }
            Value::Array(items) => {
                let mut output = serializer.serialize_seq(Some(items.len()))?;
                for value in items {
                    output.serialize_element(&Redacted(value))?;
                }
                output.end()
            }
            value => value.serialize(serializer),
        }
    }
}

struct CappedWriter {
    bytes: Vec<u8>,
    cap: usize,
    truncated: bool,
}

impl CappedWriter {
    fn new(cap: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(cap.min(128)),
            cap,
            truncated: false,
        }
    }

    fn finish(mut self) -> (String, bool) {
        if self.truncated {
            let cut = self.cap.saturating_sub(TRUNCATION_MARKER.len());
            // A byte limit can split a UTF-8 character. Keep only its valid prefix.
            let cut = match std::str::from_utf8(&self.bytes[..cut]) {
                Ok(prefix) => prefix.len(),
                Err(error) => error.valid_up_to(),
            };
            self.bytes.truncate(cut);
            self.bytes.extend_from_slice(
                &TRUNCATION_MARKER.as_bytes()[..self.cap.min(TRUNCATION_MARKER.len())],
            );
        }
        (
            String::from_utf8(self.bytes).unwrap_or_else(|_| "null".into()),
            self.truncated,
        )
    }
}

impl Write for CappedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let count = buf.len().min(self.cap - self.bytes.len());
        if count == 0 {
            self.truncated = true;
            return Err(io::ErrorKind::WriteZero.into());
        }
        let required = self.bytes.len() + count;
        if required > self.bytes.capacity() {
            let capacity = self
                .bytes
                .capacity()
                .saturating_mul(2)
                .max(required)
                .min(self.cap);
            self.bytes.reserve_exact(capacity - self.bytes.len());
        }
        self.bytes.extend_from_slice(&buf[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Redact then serialize to JSON, optionally capped at `max_bytes`.
#[must_use]
pub fn redact_and_truncate(value: &Value, max_bytes: Option<usize>) -> (String, bool) {
    let Some(cap) = max_bytes else {
        return (
            serde_json::to_string(&Redacted(value)).unwrap_or_else(|_| "null".into()),
            false,
        );
    };

    let mut writer = CappedWriter::new(cap);
    if serde_json::to_writer(&mut writer, &Redacted(value)).is_err() && !writer.truncated {
        // Preserve the serialization fallback, including the caller's byte limit.
        writer.bytes.clear();
        let _ = writer.write_all(b"null");
    }
    writer.finish()
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
    fn truncation_respects_max_bytes_below_marker_length() {
        // When max_bytes < TRUNCATION_MARKER.len(), the truncated marker
        // itself must be capped, otherwise the output exceeds the cap.
        let input = json!({ "blob": "x".repeat(100) });
        for max in 1..TRUNCATION_MARKER.len() {
            let (out, truncated) = redact_and_truncate(&input, Some(max));
            assert!(truncated);
            assert!(out.len() <= max, "max={max} got len={}: {out:?}", out.len());
        }
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
