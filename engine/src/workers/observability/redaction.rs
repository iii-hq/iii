// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Credential redaction for span data, applied by the engine to every span it
//! stores or exports.
//!
//! Worker SDKs redact captured invocation payloads (`iii.payload.json`) before
//! they export, but each SDK carries its own copy of the rule and a worker on
//! an older SDK keeps an older rule. Every span crosses the engine (worker
//! spans arrive over the telemetry connection and are stored and forwarded to
//! the collector from there), so the engine applies the current rule to all
//! of them.
//!
//! Both passes are idempotent:
//! - an attribute whose key is sensitive has its whole value replaced;
//! - in the JSON text of `iii.payload.json`, the value of every sensitive key
//!   at any depth is replaced. The text is rewritten in place instead of
//!   parsed and re-serialized, so key order and formatting survive and a
//!   truncated payload is still redacted. A text whose structure the scanner
//!   cannot follow is replaced as a whole (fail closed).

use std::borrow::Cow;

/// What a redacted value becomes; the SDKs use the same placeholder.
pub(crate) const REDACTED: &str = "[REDACTED]";

/// [`REDACTED`] as a JSON string, the form it takes inside a payload.
const REDACTED_JSON: &str = "\"[REDACTED]\"";

/// Attribute the SDKs record a captured invocation payload under.
pub(crate) const PAYLOAD_ATTRIBUTE_KEY: &str = "iii.payload.json";

/// Marker every SDK appends to a payload it cut at `III_TRACE_PAYLOAD_MAX_BYTES`.
const SDK_TRUNCATION_MARKER: &str = "...\"[TRUNCATED]\"";

/// A key is sensitive when its normalized form contains one of these.
const SENSITIVE_FRAGMENTS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "credential",
    "authorization",
    "apikey",
    "privatekey",
    "bearer",
    "cookie",
    "signature",
    "accesstoken",
    "refreshtoken",
    "authtoken",
];

/// Whether the value under `key` must not leave the engine.
///
/// The key is lowercased and stripped of everything but ASCII letters and
/// digits, so `Set-Cookie`, `set_cookie` and `http.request.header.set-cookie`
/// match alike. It is sensitive when the result contains a
/// [`SENSITIVE_FRAGMENTS`] entry or ends in `token` (`idToken`, `csrf_token`,
/// `X-CSRF-Token`); counts such as `max_tokens` or `token_count` stay visible.
pub(crate) fn is_sensitive_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    normalized.ends_with("token")
        || SENSITIVE_FRAGMENTS
            .iter()
            .any(|fragment| normalized.contains(fragment))
}

/// Redacts one span, event or link attribute in place: the whole value when
/// the key is sensitive, the sensitive values inside it when it is the
/// captured payload.
pub(crate) fn redact_attribute(key: &str, value: &mut String) {
    if is_sensitive_key(key) {
        if value != REDACTED {
            REDACTED.clone_into(value);
        }
    } else if key == PAYLOAD_ATTRIBUTE_KEY
        && let Some(redacted) = redact_payload_json(value)
    {
        *value = redacted;
    }
}

/// Redacts a captured payload; `None` when nothing in it needs redacting.
pub(crate) fn redact_payload_json(json: &str) -> Option<String> {
    let (body, marker) = match json.strip_suffix(SDK_TRUNCATION_MARKER) {
        Some(body) => (body, SDK_TRUNCATION_MARKER),
        None => (json, ""),
    };
    let Some(ranges) = sensitive_values(body) else {
        // The scanner lost the structure, so a sensitive key may hide in what
        // it could not read: drop the payload rather than guess.
        return Some(REDACTED_JSON.to_string());
    };
    if ranges.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(json.len());
    let mut copied = 0;
    for (start, end) in ranges {
        out.push_str(&body[copied..start]);
        out.push_str(REDACTED_JSON);
        copied = end;
    }
    out.push_str(&body[copied..]);
    out.push_str(marker);
    Some(out)
}

/// Byte ranges of the values under sensitive keys in `json`, in order, or
/// `None` when the text stops looking like JSON. A text that just ends (a
/// truncated payload) is fine: a value cut short runs to the end.
///
/// Every position the scanner stops at is an ASCII byte or the end of the
/// text, so each range falls on char boundaries.
fn sensitive_values(json: &str) -> Option<Vec<(usize, usize)>> {
    let bytes = json.as_bytes();
    let mut ranges = Vec::new();
    // Open containers, innermost last: `b'{'` or `b'['`.
    let mut open: Vec<u8> = Vec::new();
    // At the start of an object or after a comma in one: a key comes next.
    let mut key_next = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b',' => {
                key_next = open.last() == Some(&b'{');
                i += 1;
            }
            b'{' => {
                open.push(b'{');
                key_next = true;
                i += 1;
            }
            b'[' => {
                open.push(b'[');
                key_next = false;
                i += 1;
            }
            close @ (b'}' | b']') => {
                let opener = if close == b'}' { b'{' } else { b'[' };
                if open.pop() != Some(opener) {
                    return None;
                }
                key_next = false;
                i += 1;
            }
            b'"' if key_next => {
                key_next = false;
                let key_end = string_end(bytes, i);
                let sensitive = is_sensitive_key(&key_text(&json[i..key_end]));
                i = skip_whitespace(bytes, key_end);
                if i == bytes.len() {
                    break;
                }
                if bytes[i] != b':' {
                    return None;
                }
                i = skip_whitespace(bytes, i + 1);
                if sensitive {
                    let end = value_end(bytes, i);
                    if &json[i..end] != REDACTED_JSON {
                        ranges.push((i, end));
                    }
                    i = end;
                }
            }
            // An object key that is not a string (`{password: 1}`) is not JSON.
            _ if key_next => return None,
            b'"' => i = string_end(bytes, i),
            // A colon only ever follows a key, consumed above.
            b':' => return None,
            _ => i = literal_end(bytes, i),
        }
    }
    Some(ranges)
}

/// The key's text without its quotes, with escapes decoded when it has any.
fn key_text(quoted: &str) -> Cow<'_, str> {
    let inner = quoted.strip_prefix('"').unwrap_or(quoted);
    let inner = inner.strip_suffix('"').unwrap_or(inner);
    if inner.contains('\\')
        && let Ok(decoded) = serde_json::from_str::<String>(quoted)
    {
        return Cow::Owned(decoded);
    }
    Cow::Borrowed(inner)
}

/// End of the value starting at `start`: past its closing quote or bracket,
/// at the delimiter after a literal, or the end of the text when cut short.
fn value_end(bytes: &[u8], start: usize) -> usize {
    match bytes.get(start) {
        // No value: `{"cookie":}`, or the text ends right after the colon.
        None | Some(b',' | b':' | b'}' | b']') => start,
        Some(b'"') => string_end(bytes, start),
        Some(b'{' | b'[') => {
            let mut depth = 0usize;
            let mut i = start;
            while i < bytes.len() {
                match bytes[i] {
                    b'"' => {
                        i = string_end(bytes, i);
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return i + 1;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            bytes.len()
        }
        Some(_) => literal_end(bytes, start),
    }
}

/// Position past the string whose opening quote is at `start`.
fn string_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// Position of the delimiter that ends the literal (number, `true`, ...) at
/// `start`.
fn literal_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len()
        && !matches!(
            bytes[i],
            b',' | b':' | b'{' | b'}' | b'[' | b']' | b'"' | b' ' | b'\t' | b'\n' | b'\r'
        )
    {
        i += 1;
    }
    i
}

fn skip_whitespace(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cut(text: &str) -> String {
        format!("{text}{SDK_TRUNCATION_MARKER}")
    }

    #[test]
    fn keys_from_issue_2563_are_sensitive() {
        for key in [
            "cookie",
            "Cookie",
            "set-cookie",
            "Set-Cookie",
            "http.request.header.cookie",
            "idToken",
            "customToken",
            "resetToken",
            "x-hub-signature-256",
            "X-Signature",
        ] {
            assert!(is_sensitive_key(key), "{key} must be redacted");
        }
    }

    #[test]
    fn keys_the_sdks_redact_stay_sensitive() {
        for key in [
            "password",
            "api_key",
            "apiKey",
            "x-api-key",
            "API-KEY",
            "secret",
            "client_secret",
            "credentials",
            "authorization",
            "Proxy-Authorization",
            "http.request.header.authorization",
            "auth_token",
            "access_token",
            "accessToken",
            "refresh_token",
            "refresh_token_expires_at",
            "bearer",
            "private_key",
            "token",
            "csrf_token",
            "X-CSRF-Token",
        ] {
            assert!(is_sensitive_key(key), "{key} must be redacted");
        }
    }

    #[test]
    fn token_counts_and_ordinary_keys_stay_visible() {
        for key in [
            "max_tokens",
            "input_tokens",
            "outputTokens",
            "gen_ai.usage.input_tokens",
            "token_count",
            "tokenizer",
            "name",
            "session_id",
            "function_id",
            "key",
            PAYLOAD_ATTRIBUTE_KEY,
        ] {
            assert!(!is_sensitive_key(key), "{key} must stay visible");
        }
    }

    #[test]
    fn redacts_sensitive_values_at_any_depth() {
        let payload = r#"{"headers":{"cookie":"sid=abc","set-cookie":["a=1","b=2"],"content-type":"application/json"},"body":{"user":{"idToken":"eyJhbGciOi","name":"Ana"}}}"#;
        assert_eq!(
            redact_payload_json(payload).as_deref(),
            Some(
                r#"{"headers":{"cookie":"[REDACTED]","set-cookie":"[REDACTED]","content-type":"application/json"},"body":{"user":{"idToken":"[REDACTED]","name":"Ana"}}}"#
            ),
        );
    }

    #[test]
    fn replaces_structured_values_whole_and_keeps_formatting() {
        let payload = "{ \"b\" : 1,\n  \"credentials\" : { \"user\": \"a\", \"pw\": [1, {\"x\": \"}\"}] },\n  \"items\": [ {\"token\": null}, {\"apiKey\": 42, \"note\": \"a\\\"b\"} ] }";
        assert_eq!(
            redact_payload_json(payload).as_deref(),
            Some(
                "{ \"b\" : 1,\n  \"credentials\" : \"[REDACTED]\",\n  \"items\": [ {\"token\": \"[REDACTED]\"}, {\"apiKey\": \"[REDACTED]\", \"note\": \"a\\\"b\"} ] }"
            ),
        );
    }

    #[test]
    fn leaves_payloads_without_sensitive_keys_alone() {
        for payload in [
            r#"{"a":1,"nested":{"max_tokens":5,"list":[true,null,"cookie"]}}"#,
            r#""a string""#,
            "42",
            "null",
            "[]",
            "{}",
            "xxxx",
            "",
        ] {
            assert_eq!(redact_payload_json(payload), None, "{payload}");
        }
    }

    #[test]
    fn redaction_is_idempotent() {
        let once = redact_payload_json(r#"{"cookie":"a","b":{"secret":[1]}}"#).unwrap();
        assert_eq!(
            once,
            r#"{"cookie":"[REDACTED]","b":{"secret":"[REDACTED]"}}"#
        );
        assert_eq!(redact_payload_json(&once), None);
    }

    #[test]
    fn decodes_escaped_keys() {
        assert_eq!(
            redact_payload_json(r#"{"\u0063ookie":"sid=abc"}"#).as_deref(),
            Some(r#"{"\u0063ookie":"[REDACTED]"}"#),
        );
    }

    #[test]
    fn keeps_multibyte_text_intact() {
        assert_eq!(
            redact_payload_json(r#"{"nome":"José ç 🙂","cookie":"ação"}"#).as_deref(),
            Some(r#"{"nome":"José ç 🙂","cookie":"[REDACTED]"}"#),
        );
    }

    #[test]
    fn redacts_payloads_cut_short() {
        let cases = [
            // Cut by an SDK, which appends its marker.
            (
                cut(r#"{"user":"ana","cookie":"sid=abc"#),
                Some(cut(r#"{"user":"ana","cookie":"[REDACTED]""#)),
            ),
            (
                cut(r#"{"credentials":{"user":"a","pw"#),
                Some(cut(r#"{"credentials":"[REDACTED]""#)),
            ),
            (cut(r#"{"a":1,"#), None),
            // Cut without a marker.
            (
                r#"{"cookie":"sid=ab"#.to_string(),
                Some(r#"{"cookie":"[REDACTED]""#.to_string()),
            ),
            (
                r#"{"cookie":"#.to_string(),
                Some(r#"{"cookie":"[REDACTED]""#.to_string()),
            ),
            (r#"{"cook"#.to_string(), None),
        ];
        for (payload, expected) in cases {
            assert_eq!(redact_payload_json(&payload), expected, "{payload}");
        }
    }

    #[test]
    fn drops_payloads_it_cannot_follow() {
        for payload in [
            "{password: \"hunter2\"}",
            r#"{"a":1]"#,
            r#"{"a":1}}"#,
            "key: value",
        ] {
            assert_eq!(
                redact_payload_json(payload).as_deref(),
                Some(REDACTED_JSON),
                "{payload}"
            );
        }
    }

    #[test]
    fn redact_attribute_reads_only_the_payload_as_json() {
        let mut header = "sid=abc".to_string();
        redact_attribute("http.request.header.cookie", &mut header);
        assert_eq!(header, REDACTED);

        let mut payload = r#"{"resetToken":"t"}"#.to_string();
        redact_attribute(PAYLOAD_ATTRIBUTE_KEY, &mut payload);
        assert_eq!(payload, r#"{"resetToken":"[REDACTED]"}"#);

        let mut note = r#"{"resetToken":"t"}"#.to_string();
        redact_attribute("note", &mut note);
        assert_eq!(note, r#"{"resetToken":"t"}"#);
    }
}
