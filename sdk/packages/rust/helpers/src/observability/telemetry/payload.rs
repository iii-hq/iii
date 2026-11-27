//! Payload redaction + truncation for invocation event capture.

use std::borrow::Cow;
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

/// Query parameters that carry a credential in a signed URL (S3 and R2
/// SigV2/SigV4, GCS, Azure SAS, CloudFront, the storage worker's local
/// provider), lowercased. The engine and every SDK carry the same list.
const SENSITIVE_URL_PARAMS: &[&str] = &[
    "token",
    "sig",
    "signature",
    "policy",
    "key-pair-id",
    "awsaccesskeyid",
    "x-amz-signature",
    "x-amz-credential",
    "x-amz-security-token",
    "x-goog-signature",
    "x-goog-credential",
];

fn is_sensitive_url_param(name: &str) -> bool {
    is_sensitive_key(name)
        || SENSITIVE_URL_PARAMS
            .iter()
            .any(|param| name.eq_ignore_ascii_case(param))
}

/// Replaces the credentials in `text` with the placeholder when it is a URL:
/// one that starts with `scheme://` or `/`. The password of its userinfo goes,
/// or the whole userinfo when it has no `:` (a token used as the user name);
/// the authority runs to the first `/`, `?` or `#`, the userinfo to the last
/// `@` in it. So does the value of every query parameter whose name is
/// sensitive (the query runs from the first `?` to the next `#`). Everything
/// else is kept as it is; empty and already redacted values are skipped.
fn redact_url(text: &str) -> Cow<'_, str> {
    let bytes = text.as_bytes();
    let authority_start = match bytes.first() {
        Some(b'/') => None,
        Some(first) if first.is_ascii_alphabetic() => {
            let scheme_end = bytes
                .iter()
                .position(|byte| {
                    !(byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
                })
                .unwrap_or(bytes.len());
            if !bytes[scheme_end..].starts_with(b"://") {
                return Cow::Borrowed(text);
            }
            Some(scheme_end + 3)
        }
        _ => return Cow::Borrowed(text),
    };
    let mut secrets = Vec::new();
    let mut secret = |start: usize, end: usize| {
        let value = &text[start..end];
        if !value.is_empty() && value != REDACTED_PLACEHOLDER {
            secrets.push((start, end));
        }
    };
    if let Some(start) = authority_start {
        let end = bytes[start..]
            .iter()
            .position(|byte| matches!(byte, b'/' | b'?' | b'#'))
            .map_or(bytes.len(), |offset| start + offset);
        if let Some(at) = bytes[start..end].iter().rposition(|&byte| byte == b'@') {
            match bytes[start..start + at]
                .iter()
                .position(|&byte| byte == b':')
            {
                Some(colon) => secret(start + colon + 1, start + at),
                None => secret(start, start + at),
            }
        }
    }
    if let Some(question) = text.find('?') {
        let query_end = text[question + 1..]
            .find('#')
            .map_or(text.len(), |offset| question + 1 + offset);
        let mut param_start = question + 1;
        for param in text[question + 1..query_end].split('&') {
            if let Some((name, _)) = param.split_once('=')
                && is_sensitive_url_param(name)
            {
                secret(param_start + name.len() + 1, param_start + param.len());
            }
            param_start += param.len() + 1;
        }
    }
    if secrets.is_empty() {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    for (start, end) in secrets {
        out.push_str(&text[copied..start]);
        out.push_str(REDACTED_PLACEHOLDER);
        copied = end;
    }
    out.push_str(&text[copied..]);
    Cow::Owned(out)
}

/// Recursively redact values of sensitive keys, and the credentials in string
/// values that are URLs: the password of the userinfo (the whole userinfo when
/// it has no password) and the values of query parameters with a sensitive
/// name, such as the signature of a presigned URL.
/// Returns a new `Value`.
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
        Value::String(text) => Value::String(redact_url(text).into_owned()),
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
            Value::String(text) => serializer.serialize_str(&redact_url(text)),
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
    fn redacts_url_credentials_in_string_values() {
        let input = json!({
            "status_code": 302,
            "headers": {
                "location": "https://acct.r2.cloudflarestorage.com/bucket/key?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=AK%2F20261126&X-Amz-Signature=abc",
            },
            "body": { "upload": { "url": "http://localhost:49200/v1/objects/upload?token=eyJ.a.b" } },
            "next": "https://registry.example.com/workers?page=2&sort=name",
        });
        let expected = json!({
            "status_code": 302,
            "headers": {
                "location": "https://acct.r2.cloudflarestorage.com/bucket/key?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=[REDACTED]&X-Amz-Signature=[REDACTED]",
            },
            "body": { "upload": { "url": "http://localhost:49200/v1/objects/upload?token=[REDACTED]" } },
            "next": "https://registry.example.com/workers?page=2&sort=name",
        });
        assert_eq!(redact(&input), expected);
        // The serializer used by redact_and_truncate applies the same rule.
        let (out, _) = redact_and_truncate(&input, None);
        assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), expected);
        assert_eq!(redact(&expected), expected);
    }

    #[test]
    fn url_rule_reads_only_urls() {
        assert_eq!(
            redact_url("/v1/objects/download?token=abc"),
            "/v1/objects/download?token=[REDACTED]"
        );
        assert_eq!(
            redact_url("https://u:pw@h.example/"),
            "https://u:[REDACTED]@h.example/"
        );
        assert_eq!(
            redact_url("https://ghp_x@github.com/o/r.git"),
            "https://[REDACTED]@github.com/o/r.git"
        );
        for text in [
            "see https://h.example/?token=abc",
            "ana@example.com",
            "a?token=b",
            "https://h.example/?page=2",
            "",
        ] {
            assert!(matches!(redact_url(text), Cow::Borrowed(_)), "{text}");
        }
        let input =
            json!({ "https://h.example/?token=abc": 1, "token": "https://h.example/?sig=a" });
        assert_eq!(
            redact(&input),
            json!({ "https://h.example/?token=abc": 1, "token": "[REDACTED]" })
        );
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
