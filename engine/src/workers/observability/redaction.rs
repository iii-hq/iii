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
//!   at any depth is replaced, and so is every credential inside a string
//!   value that is a URL: the password of its userinfo (the whole userinfo
//!   when it has no password, as a token used as the user name) and the value
//!   of each query parameter with a sensitive name, such as the signature of a
//!   presigned URL. The text is rewritten in place instead of parsed and
//!   re-serialized, so key order and formatting survive and a truncated
//!   payload is still redacted. A text that is not JSON is replaced as a whole
//!   (fail closed).
//!
//! The SDKs run the same URL rule; the shared vectors in
//! `sdk/fixtures/redaction` keep them in step.

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

/// Bit per lowercase letter that starts a [`SENSITIVE_FRAGMENTS`] entry, so
/// the search skips positions where no fragment can begin. Building it also
/// checks at compile time that every fragment starts with one.
const FRAGMENT_FIRST_LETTERS: u32 = {
    let mut mask = 0u32;
    let mut i = 0;
    while i < SENSITIVE_FRAGMENTS.len() {
        mask |= 1 << (SENSITIVE_FRAGMENTS[i].as_bytes()[0] - b'a');
        i += 1;
    }
    mask
};

/// Query parameters that carry a credential in a signed URL (S3 and R2
/// SigV2/SigV4, GCS, Azure SAS, CloudFront, the storage worker's local
/// provider), lowercased and matched in any case. [`is_sensitive_key`]
/// already catches most of them; the list is the one every SDK carries, since
/// their key rule is older and misses names such as `X-Amz-Signature`.
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

/// Longest normalized key kept on the stack; longer keys go to the heap.
const STACK_KEY_BYTES: usize = 64;

/// Whether the value under `key` must not leave the engine.
///
/// The key is lowercased and stripped of everything but ASCII letters and
/// digits, so `Set-Cookie`, `set_cookie` and `http.request.header.set-cookie`
/// match alike. It is sensitive when the result contains a
/// [`SENSITIVE_FRAGMENTS`] entry or ends in `token` (`idToken`, `csrf_token`,
/// `X-CSRF-Token`); counts such as `max_tokens` or `token_count` stay visible.
///
/// It runs for every attribute of every span and every key of every payload,
/// so the key is normalized on the stack rather than allocated.
pub(crate) fn is_sensitive_key(key: &str) -> bool {
    let mut buf = [0u8; STACK_KEY_BYTES];
    let mut len = 0;
    for byte in key.bytes() {
        if byte.is_ascii_alphanumeric() {
            if len == buf.len() {
                let normalized: Vec<u8> = key
                    .bytes()
                    .filter(u8::is_ascii_alphanumeric)
                    .map(|byte| byte.to_ascii_lowercase())
                    .collect();
                return is_sensitive_normalized(&normalized);
            }
            buf[len] = byte.to_ascii_lowercase();
            len += 1;
        }
    }
    is_sensitive_normalized(&buf[..len])
}

/// [`is_sensitive_key`] for a key already lowercased and stripped to ASCII
/// letters and digits.
fn is_sensitive_normalized(key: &[u8]) -> bool {
    key.ends_with(b"token")
        || (0..key.len()).any(|start| {
            let first = key[start];
            first.is_ascii_lowercase()
                && FRAGMENT_FIRST_LETTERS & (1 << (first - b'a')) != 0
                && SENSITIVE_FRAGMENTS
                    .iter()
                    .any(|fragment| key[start..].starts_with(fragment.as_bytes()))
        })
}

/// Whether the value of the query parameter `name` must not leave the engine:
/// its name is a sensitive key or one of [`SENSITIVE_URL_PARAMS`].
fn is_sensitive_url_param(name: &str) -> bool {
    is_sensitive_key(name)
        || SENSITIVE_URL_PARAMS
            .iter()
            .any(|param| name.eq_ignore_ascii_case(param))
}

/// Calls `found` with the byte range of each credential in `text` when it is
/// a URL, in order: the password of its userinfo, or the whole userinfo when
/// it has no `:` (a token used as the user name, as in a clone URL), and the
/// value of every query parameter whose name is sensitive
/// ([`is_sensitive_url_param`]). Empty values and values already [`REDACTED`]
/// are skipped, so a redacted URL yields nothing.
///
/// `text` is a URL when it starts with `scheme://` (a letter, then letters,
/// digits, `+`, `-` or `.`) or with `/`. The authority runs to the first `/`,
/// `?` or `#` and its userinfo to the last `@` in it; only a `scheme://` URL
/// has one. The query runs from the first `?` to the next `#`, in
/// `&`-separated `name=value` pairs. Every range starts and ends next to an
/// ASCII delimiter or at the end of `text`, so it falls on char boundaries.
///
/// It runs on every string value of every payload: anything that does not
/// start like a URL is turned away after a few bytes.
fn url_secrets(text: &str, mut found: impl FnMut(usize, usize)) {
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
                return;
            }
            Some(scheme_end + 3)
        }
        _ => return,
    };
    let mut secret = |start: usize, end: usize| {
        let value = &text[start..end];
        if !value.is_empty() && value != REDACTED {
            found(start, end);
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
    let Some(question) = text.find('?') else {
        return;
    };
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

/// Redacts one span, event or link attribute in place: the whole value when
/// the key is sensitive, the sensitive values inside it when it is the
/// captured payload.
pub(crate) fn redact_attribute(key: &str, value: &mut String) {
    if is_sensitive_key(key) {
        if value != REDACTED {
            // A fresh string: reusing the buffer would keep the allocation of
            // the value it replaces.
            *value = REDACTED.to_string();
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
    let Some(edits) = redactions(body) else {
        // The scanner lost the structure, so a sensitive key may hide in what
        // it could not read: drop the payload rather than guess.
        return Some(REDACTED_JSON.to_string());
    };
    if edits.is_empty() {
        return None;
    }
    // Exact size, so a short placeholder replacing a large value does not
    // leave the stored payload holding an allocation sized for the original.
    let removed: usize = edits.iter().map(|edit| edit.end - edit.start).sum();
    let added: usize = edits.iter().map(|edit| edit.with.len()).sum();
    let mut out = String::with_capacity(json.len() - removed + added);
    let mut copied = 0;
    for edit in &edits {
        out.push_str(&body[copied..edit.start]);
        out.push_str(&edit.with);
        copied = edit.end;
    }
    out.push_str(&body[copied..]);
    out.push_str(marker);
    Some(out)
}

/// A stretch of the payload text, by byte offsets, and what replaces it.
struct Edit {
    start: usize,
    end: usize,
    with: Cow<'static, str>,
}

impl Edit {
    fn new(start: usize, end: usize, with: impl Into<Cow<'static, str>>) -> Self {
        Self {
            start,
            end,
            with: with.into(),
        }
    }
}

/// The edits that redact `json`, in order, or `None` when the text is not
/// JSON: a non-string key, a literal that is not one, or a value followed by
/// anything but a delimiter. That last check keeps text from slipping past
/// unread, such as `abc` in `{"authorization":Bearer abc}`. A text that just
/// ends (a truncated payload) is fine: a value cut short runs to the end.
///
/// The value under a sensitive key becomes `"[REDACTED]"`; the credentials in
/// any other string value that is a URL become `[REDACTED]` inside it
/// ([`redact_url_value`]).
///
/// Every position the scanner stops at is an ASCII byte or the end of the
/// text, so each edit falls on char boundaries.
fn redactions(json: &str) -> Option<Vec<Edit>> {
    let bytes = json.as_bytes();
    let mut edits = Vec::new();
    // Open containers, innermost last: `b'{'` or `b'['`.
    let mut open: Vec<u8> = Vec::new();
    // At the start of an object or after a comma in one: a key comes next.
    let mut key_next = false;
    // A value just ended: only a comma, a closing bracket or the end of the
    // text may follow.
    let mut value_done = false;
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if matches!(byte, b' ' | b'\t' | b'\n' | b'\r') {
            i += 1;
            continue;
        }
        if value_done && !matches!(byte, b',' | b'}' | b']') {
            return None;
        }
        match byte {
            b',' => {
                // Outside any container a comma joins two values: `1,2`.
                let container = *open.last()?;
                key_next = container == b'{';
                value_done = false;
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
                value_done = true;
                i += 1;
            }
            b'"' if key_next => {
                key_next = false;
                let key_start = i;
                let key_end = string_end(bytes, i);
                i = skip_whitespace(bytes, key_end);
                if i == bytes.len() {
                    // Cut short in or after the key: no value to redact.
                    break;
                }
                if bytes[i] != b':' {
                    return None;
                }
                // Text follows, so the key is complete: if its escapes do not
                // decode it is not JSON, and the sensitive key may hide in them.
                let sensitive = is_sensitive_key(&key_text(&json[key_start..key_end])?);
                i = skip_whitespace(bytes, i + 1);
                if sensitive {
                    // Replaced whatever it is, so it need not be valid JSON.
                    let end = value_end(bytes, i);
                    if &json[i..end] != REDACTED_JSON {
                        edits.push(Edit::new(i, end, REDACTED_JSON));
                    }
                    i = end;
                    value_done = true;
                }
            }
            // An object key that is not a string (`{password: 1}`) is not JSON.
            _ if key_next => return None,
            b'"' => {
                let string = scan_string(bytes, i);
                redact_url_value(json, i, string, &mut edits);
                i = string.end;
                value_done = true;
            }
            // A colon only ever follows a key, consumed above.
            b':' => return None,
            _ => {
                let end = literal_end(bytes, i);
                if !is_json_literal(&bytes[i..end], end == bytes.len()) {
                    return None;
                }
                i = end;
                value_done = true;
            }
        }
    }
    Some(edits)
}

/// Queues the edits that redact the credentials in the string value whose
/// opening quote is at `start`, when it is a URL ([`url_secrets`]).
///
/// Text without escapes reads the same decoded, so each credential is
/// replaced where it stands. Text with escapes (`\u0026` for `&`, `\/`) is
/// decoded, and when it holds a credential the whole string is written back
/// re-encoded. If its escapes do not decode, as when a truncated payload was
/// cut inside one, the string is replaced as a whole (fail closed); escaped
/// strings without a `?` or `@` are not decoded, since a URL credential needs
/// one of them and no serializer escapes either. A string cut short stays
/// open, so the truncation marker after it still reads as before.
fn redact_url_value(json: &str, start: usize, string: ScannedString, edits: &mut Vec<Edit>) {
    let content_start = start + 1;
    let content_end = if string.closed {
        string.end - 1
    } else {
        string.end
    };
    let content = &json[content_start..content_end];
    if !string.escaped {
        url_secrets(content, |from, to| {
            edits.push(Edit::new(
                content_start + from,
                content_start + to,
                REDACTED,
            ));
        });
        return;
    }
    if !content.contains(['?', '@']) {
        return;
    }
    let quoted = &json[start..string.end];
    let decoded: Result<String, _> = if string.closed {
        serde_json::from_str(quoted)
    } else {
        serde_json::from_str(&format!("{quoted}\""))
    };
    let Ok(text) = decoded else {
        edits.push(Edit::new(start, string.end, REDACTED_JSON));
        return;
    };
    let mut redacted = String::new();
    let mut copied = 0;
    url_secrets(&text, |from, to| {
        redacted.push_str(&text[copied..from]);
        redacted.push_str(REDACTED);
        copied = to;
    });
    if redacted.is_empty() {
        return;
    }
    redacted.push_str(&text[copied..]);
    let Ok(mut encoded) = serde_json::to_string(&redacted) else {
        edits.push(Edit::new(start, string.end, REDACTED_JSON));
        return;
    };
    if !string.closed {
        encoded.pop();
    }
    edits.push(Edit::new(start, string.end, encoded));
}

/// Whether `token` is a JSON literal: `true`, `false`, `null` or a number,
/// plus the `NaN`, `Infinity` and `-Infinity` that Python's `json.dumps`
/// writes for non-finite floats. A token that runs to the end of the text may
/// have been cut short, so any prefix of one passes there.
fn is_json_literal(token: &[u8], at_end: bool) -> bool {
    const WORDS: [&[u8]; 6] = [
        b"true",
        b"false",
        b"null",
        b"NaN",
        b"Infinity",
        b"-Infinity",
    ];
    let word = WORDS.iter().any(|word| {
        if at_end {
            word.starts_with(token)
        } else {
            *word == token
        }
    });
    word || number_prefix(token).is_some_and(|whole| whole || at_end)
}

/// Reads `token` as a JSON number: `Some(true)` for a whole number,
/// `Some(false)` for a proper prefix of one (`-`, `1.`, `2e+`), `None` for
/// anything else.
fn number_prefix(token: &[u8]) -> Option<bool> {
    let digits = |i: &mut usize| {
        let start = *i;
        while token.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
        *i - start
    };
    let mut i = usize::from(token.first() == Some(&b'-'));
    match token.get(i) {
        None => return Some(false),
        Some(b'0') => i += 1,
        Some(b'1'..=b'9') => {
            digits(&mut i);
        }
        Some(_) => return None,
    }
    if token.get(i) == Some(&b'.') {
        i += 1;
        if digits(&mut i) == 0 {
            return (i == token.len()).then_some(false);
        }
    }
    if matches!(token.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(token.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if digits(&mut i) == 0 {
            return (i == token.len()).then_some(false);
        }
    }
    (i == token.len()).then_some(true)
}

/// The text of a complete key without its quotes, with escapes decoded when
/// it has any; `None` when they do not decode (`\q`, a lone surrogate).
fn key_text(quoted: &str) -> Option<Cow<'_, str>> {
    let inner = quoted.strip_prefix('"').unwrap_or(quoted);
    let inner = inner.strip_suffix('"').unwrap_or(inner);
    if inner.contains('\\') {
        return serde_json::from_str::<String>(quoted).ok().map(Cow::Owned);
    }
    Some(Cow::Borrowed(inner))
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

/// What [`scan_string`] learned about a string.
#[derive(Clone, Copy)]
struct ScannedString {
    /// Position past its closing quote, or the end of the text.
    end: usize,
    /// Whether it has its closing quote: false when the text ends first.
    closed: bool,
    /// Whether it holds an escape.
    escaped: bool,
}

/// Reads the string whose opening quote is at `start`.
fn scan_string(bytes: &[u8], start: usize) -> ScannedString {
    let mut escaped = false;
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                escaped = true;
                i += 2;
            }
            b'"' => {
                return ScannedString {
                    end: i + 1,
                    closed: true,
                    escaped,
                };
            }
            _ => i += 1,
        }
    }
    ScannedString {
        end: bytes.len(),
        closed: false,
        escaped,
    }
}

/// Position past the string whose opening quote is at `start`.
fn string_end(bytes: &[u8], start: usize) -> usize {
    scan_string(bytes, start).end
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
    fn fragments_are_normalized() {
        for fragment in SENSITIVE_FRAGMENTS {
            assert!(
                fragment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()),
                "{fragment} would never match a normalized key"
            );
        }
    }

    #[test]
    fn keys_longer_than_the_stack_buffer_are_checked_whole() {
        let padding = "x".repeat(STACK_KEY_BYTES);
        assert!(is_sensitive_key(&format!("{padding}_password")));
        assert!(is_sensitive_key(&format!("{padding}-Session-Token")));
        assert!(is_sensitive_key(&format!("Cookie_{padding}")));
        assert!(!is_sensitive_key(&format!("{padding}_max_tokens")));
        assert!(!is_sensitive_key(&padding));
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
            "",
            // What Python's json.dumps writes for non-finite floats.
            r#"{"x":NaN,"y":-Infinity,"z":[Infinity,-0.5e-3,0,1E+9]}"#,
            " { \"a\" : [ 1 , true ] } ",
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
        assert_eq!(
            redact_payload_json(r#"{"url":"https://exemplo.com/ação?token=ç🙂&q=José"}"#)
                .as_deref(),
            Some(r#"{"url":"https://exemplo.com/ação?token=[REDACTED]&q=José"}"#),
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
            // A key cut inside an escape has no value to redact.
            (r#"{"a":1,"\u00"#.to_string(), None),
            (cut(r#"{"\u0063ookie"#), None),
            // A literal cut short is still a literal.
            (cut(r#"{"ok":tr"#), None),
            (r#"{"n":-1.5e"#.to_string(), None),
            (
                cut(r#"{"cookie":tr"#),
                Some(cut(r#"{"cookie":"[REDACTED]""#)),
            ),
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
            "xxxx",
            // Text after a sensitive value would otherwise stay visible.
            r#"{"authorization":Bearer abc}"#,
            r#"{"cookie":"a" "b"}"#,
            // Not JSON, so nothing tells a sensitive key apart.
            "cookie=sid=abc",
            "cookie=sid=abc; token=xyz",
            r#"{"a":01}"#,
            "[1 2]",
            "1,2",
            r#"{"a":1}{"cookie":"x"}"#,
            // A complete key whose escapes do not decode.
            r#"{"\u0063ookie\q":"sid=abc"}"#,
            r#"{"a":{"\x":1}}"#,
        ] {
            assert_eq!(
                redact_payload_json(payload).as_deref(),
                Some(REDACTED_JSON),
                "{payload}"
            );
        }
    }

    #[test]
    fn redacted_values_do_not_keep_the_original_allocation() {
        let payload = format!(r#"{{"cookie":"{}","a":1}}"#, "s".repeat(64 * 1024));
        let redacted = redact_payload_json(&payload).unwrap();
        assert_eq!(redacted, r#"{"cookie":"[REDACTED]","a":1}"#);
        assert!(
            redacted.capacity() < 1024,
            "{} bytes kept",
            redacted.capacity()
        );

        let mut header = "s".repeat(64 * 1024);
        redact_attribute("cookie", &mut header);
        assert_eq!(header, REDACTED);
        assert!(header.capacity() < 1024, "{} bytes kept", header.capacity());
    }

    /// The vectors every SDK runs too (`sdk/fixtures/redaction`).
    #[test]
    fn redacts_url_credentials_from_the_shared_vectors() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../sdk/fixtures/redaction/url-credentials.json");
        let text = std::fs::read_to_string(&path).unwrap();
        let vectors: serde_json::Value = serde_json::from_str(&text).unwrap();
        let cases = vectors["cases"].as_array().unwrap();
        assert!(
            cases.len() > 10,
            "{} cases in {}",
            cases.len(),
            path.display()
        );
        let wrap = |value: &serde_json::Value| {
            serde_json::json!({ "headers": { "location": value }, "links": [value] }).to_string()
        };
        for case in cases {
            let note = case["note"].as_str().unwrap();
            let payload = wrap(&case["value"]);
            let expected = wrap(&case["expect"]);
            let redacted = redact_payload_json(&payload);
            assert_eq!(redacted.as_deref().unwrap_or(&payload), expected, "{note}");
            assert_eq!(
                redacted.is_none(),
                case["value"] == case["expect"],
                "{note}: None only when nothing changes"
            );
            assert_eq!(redact_payload_json(&expected), None, "{note}: idempotent");
        }
    }

    #[test]
    fn presigned_url_parameter_names() {
        for name in SENSITIVE_URL_PARAMS.iter().copied().chain([
            "X-Amz-Signature",
            "X-Goog-Credential",
            "Key-Pair-Id",
            "SIG",
        ]) {
            assert!(is_sensitive_url_param(name), "{name} must be redacted");
        }
        for name in [
            "X-Amz-Algorithm",
            "X-Amz-Date",
            "X-Amz-Expires",
            "X-Amz-SignedHeaders",
            "X-Amz-Content-Sha256",
            "X-Goog-SignedHeaders",
            "x-id",
            "Expires",
            "sv",
            "se",
            "sp",
            "sr",
            "page",
            "sort",
            "max_tokens",
            "",
        ] {
            assert!(!is_sensitive_url_param(name), "{name} must stay visible");
        }
    }

    #[test]
    fn redacts_url_credentials_at_any_depth_among_key_redactions() {
        let payload = r#"{"status_code":302,"headers":{"set-cookie":"sid=1","location":"/v1/objects/download?token=eyJ.a.b"},"body":{"items":[{"url":"https://u:pw@h.example/x?sig=s&page=2"},"https://h.example/?Signature=s"]},"authorization":"Bearer x","next":"https://h.example/?page=3"}"#;
        assert_eq!(
            redact_payload_json(payload).as_deref(),
            Some(
                r#"{"status_code":302,"headers":{"set-cookie":"[REDACTED]","location":"/v1/objects/download?token=[REDACTED]"},"body":{"items":[{"url":"https://u:[REDACTED]@h.example/x?sig=[REDACTED]&page=2"},"https://h.example/?Signature=[REDACTED]"]},"authorization":"[REDACTED]","next":"https://h.example/?page=3"}"#
            ),
        );
    }

    #[test]
    fn redacts_a_payload_that_is_a_url() {
        assert_eq!(
            redact_payload_json(r#""http://localhost:49200/v1/objects/download?token=eyJ.a.b""#)
                .as_deref(),
            Some(r#""http://localhost:49200/v1/objects/download?token=[REDACTED]""#),
        );
    }

    #[test]
    fn a_url_under_a_sensitive_key_is_replaced_whole() {
        assert_eq!(
            redact_payload_json(r#"{"token":"https://h.example/?sig=a&page=2"}"#).as_deref(),
            Some(r#"{"token":"[REDACTED]"}"#),
        );
    }

    #[test]
    fn leaves_ordinary_urls_and_url_keys_alone() {
        for payload in [
            r#"{"next":"https://registry.example.com/workers?page=2&sort=name"}"#,
            r#""/workers/acme?tab=versions""#,
            r#"{"https://h.example/?token=abc":1}"#,
            r#"{"note":"see https://h.example/?token=abc"}"#,
            r#"{"email":"ana@example.com","q":"a?b=c"}"#,
        ] {
            assert_eq!(redact_payload_json(payload), None, "{payload}");
        }
    }

    #[test]
    fn redacts_urls_in_payloads_cut_short() {
        let cases = [
            // Cut inside the signature: what is left of it goes.
            (
                cut(
                    r#"{"status_code":302,"headers":{"location":"https://h.example/k?X-Amz-Credential=AK%2F1&X-Amz-Date=1&X-Amz-Signature=9b1c6d"#,
                ),
                Some(cut(
                    r#"{"status_code":302,"headers":{"location":"https://h.example/k?X-Amz-Credential=[REDACTED]&X-Amz-Date=1&X-Amz-Signature=[REDACTED]"#,
                )),
            ),
            // Cut after the URL.
            (
                cut(
                    r#"{"location":"http://localhost:49200/v1/objects/download?token=eyJ.a.b","body":"xx"#,
                ),
                Some(cut(
                    r#"{"location":"http://localhost:49200/v1/objects/download?token=[REDACTED]","body":"xx"#,
                )),
            ),
            // Cut without a marker, inside a userinfo password that is complete.
            (
                r#"{"url":"https://deploy:hunter2@git.example.com/ac"#.to_string(),
                Some(r#"{"url":"https://deploy:[REDACTED]@git.example.com/ac"#.to_string()),
            ),
            // A token used as the user name, cut later.
            (
                cut(r#"{"remote":"https://ghp_x@github.com/acme/wor"#),
                Some(cut(r#"{"remote":"https://[REDACTED]@github.com/acme/wor"#)),
            ),
            // Cut before any value.
            (cut(r#"{"location":"https://h.example/k?X-Amz-Sig"#), None),
            (cut(r#"{"location":"https://h.example/k?token="#), None),
        ];
        for (payload, expected) in cases {
            let redacted = redact_payload_json(&payload);
            assert_eq!(redacted, expected, "{payload}");
            if let Some(redacted) = redacted {
                assert_eq!(
                    redact_payload_json(&redacted),
                    None,
                    "idempotent: {redacted}"
                );
            }
        }
    }

    #[test]
    fn redacts_escaped_urls() {
        let cases = [
            // `\u0026` for `&` and `\/` for `/`: the string is written back
            // decoded.
            (
                r#"{"location":"https:\/\/h.example\/k?X-Amz-Date=1\u0026X-Amz-Signature=abc\u0026x-id=GetObject","n":1}"#.to_string(),
                Some(r#"{"location":"https://h.example/k?X-Amz-Date=1&X-Amz-Signature=[REDACTED]&x-id=GetObject","n":1}"#.to_string()),
            ),
            // Nothing to redact: the escapes stay.
            (
                r#"{"location":"https:\/\/h.example\/k?page=2\u0026sort=name"}"#.to_string(),
                None,
            ),
            (r#"{"text":"line\none ?"}"#.to_string(), None),
            // Cut short after a whole escape: decoded, redacted, left open.
            (
                cut(r#"{"location":"https:\/\/h.example\/k?page=1\u0026token=ab"#),
                Some(cut(r#"{"location":"https://h.example/k?page=1&token=[REDACTED]"#)),
            ),
            // Cut inside an escape, or an escape that does not decode: the
            // string goes whole.
            (
                cut(r#"{"location":"https:\/\/h.example\/k?token=ab\u00"#),
                Some(cut(r#"{"location":"[REDACTED]""#)),
            ),
            (
                r#"{"location":"https://h.example/k?token=ab\q"}"#.to_string(),
                Some(r#"{"location":"[REDACTED]"}"#.to_string()),
            ),
        ];
        for (payload, expected) in cases {
            let redacted = redact_payload_json(&payload);
            assert_eq!(redacted, expected, "{payload}");
            if let Some(redacted) = redacted {
                assert_eq!(
                    redact_payload_json(&redacted),
                    None,
                    "idempotent: {redacted}"
                );
            }
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
