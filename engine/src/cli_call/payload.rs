// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use anyhow::{Context, Result, bail};
use serde_json::Value;

fn decode_value(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_string()))
}

/// The variable that holds default payloads by function id, for a parent process to hand a call
/// fields it must not put in the arguments.
pub const DEFAULTS_ENV: &str = "III_CALL_DEFAULTS";

/// The value of [`DEFAULTS_ENV`] from what `std::env::var` read: `None` when it is not set. A value
/// that is not UTF-8 fails the call instead of reading as unset, which would make the call
/// without its defaults; the error quotes nothing of it.
pub fn defaults_from_env(read: Result<String, std::env::VarError>) -> Result<Option<String>> {
    match read {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => bail!("{DEFAULTS_ENV} is not valid UTF-8"),
    }
}

/// Puts the entry of `function_path` in `defaults`, the value of [`DEFAULTS_ENV`], under
/// `payload`: its keys are the base, and the keys `--json` and `key=value` set override them.
///
/// `defaults` is a JSON object keyed by function id, each entry an object. Only the entry of the
/// function being called is read, so another call from the same process receives nothing. A
/// blank value is unset. A value or an entry that is not an object fails the call, and so does a
/// payload that is not an object while there is an entry to merge. No error quotes the value: it
/// may hold a token.
pub fn apply_defaults(
    function_path: &str,
    defaults: Option<&str>,
    payload: Value,
) -> Result<Value> {
    let Some(raw) = defaults.filter(|raw| !raw.trim().is_empty()) else {
        return Ok(payload);
    };
    let all: Value = serde_json::from_str(raw)
        .map_err(|error| anyhow::anyhow!("{DEFAULTS_ENV} is not valid JSON: {error}"))?;
    let Value::Object(mut all) = all else {
        bail!("{DEFAULTS_ENV} must be a JSON object keyed by function id");
    };
    let Some(entry) = all.remove(function_path) else {
        return Ok(payload);
    };
    let Value::Object(mut merged) = entry else {
        bail!("the {function_path} entry of {DEFAULTS_ENV} must be a JSON object");
    };
    let Value::Object(overrides) = payload else {
        bail!("--json must be an object when {DEFAULTS_ENV} has defaults for {function_path}");
    };
    merged.extend(overrides);
    Ok(Value::Object(merged))
}

/// Resolve a payload from CLI tokens and an optional `--json` value.
///
/// Resolution order:
/// 1. If neither is provided, return an empty object `{}`. This matches the
///    old `--payload='{}'` default and lets engine functions whose input is
///    a struct (with all-optional fields) deserialize from a bare
///    `iii call <fn-path>` invocation.
/// 2. If only `--json` is provided, parse it and return as-is (any JSON value).
/// 3. If only kv pairs are provided, build an object from them.
/// 4. If both are provided: `--json` must be an object. kv pairs override
///    individual keys (shallow merge). Returns the merged object.
///
/// Values in kv pairs are JSON-decoded when possible (`count=10` → 10),
/// otherwise treated as strings.
pub fn parse(kv_tokens: &[String], json_override: Option<&str>) -> Result<Value> {
    let kv_obj = if kv_tokens.is_empty() {
        None
    } else {
        let mut obj = serde_json::Map::new();
        for token in kv_tokens {
            let (k, v) = token
                .split_once('=')
                .with_context(|| format!("expected key=value, got `{}`", token))?;
            if k.is_empty() {
                bail!("payload key must not be empty (got `{}`)", token);
            }
            let value = decode_value(v);
            obj.insert(k.to_string(), value);
        }
        Some(obj)
    };

    let json_value = match json_override {
        Some(raw) => Some(
            serde_json::from_str::<Value>(raw)
                .with_context(|| format!("--json: invalid JSON `{}`", raw))?,
        ),
        None => None,
    };

    match (json_value, kv_obj) {
        (None, None) => Ok(Value::Object(serde_json::Map::new())),
        (None, Some(obj)) => Ok(Value::Object(obj)),
        (Some(v), None) => Ok(v),
        (Some(Value::Object(mut base)), Some(overrides)) => {
            for (k, v) in overrides {
                base.insert(k, v);
            }
            Ok(Value::Object(base))
        }
        (Some(_), Some(_)) => {
            bail!("--json must be an object when combined with kv pairs")
        }
    }
}

/// Resolve a payload while collecting repeated singular arguments into one
/// plural array field.
///
/// This is the CLI adapter for functions whose JSON contract takes a list but
/// whose shell syntax repeats a readable singular key. Other keys keep the
/// normal [`parse`] rules. Collected key-value arguments override either the
/// singular compatibility field or the plural field from `--json`.
pub fn parse_collecting(
    kv_tokens: &[String],
    json_override: Option<&str>,
    singular: &str,
    plural: &str,
) -> Result<Value> {
    let mut remaining = Vec::with_capacity(kv_tokens.len());
    let mut collected = Vec::new();

    for token in kv_tokens {
        match token.split_once('=') {
            Some((key, value)) if key == singular => collected.push(decode_value(value)),
            _ => remaining.push(token.clone()),
        }
    }

    let mut payload = parse(&remaining, json_override)?;
    if collected.is_empty() {
        return Ok(payload);
    }

    let object = payload
        .as_object_mut()
        .context("--json must be an object when combined with kv pairs")?;
    object.remove(singular);
    object.insert(plural.to_string(), Value::Array(collected));
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_base_of_their_function_only() {
        let defaults = r#"{"sandbox::start": {"auth": "t", "worker": "old", "env": {"K": "v"}}}"#;
        let payload = parse(&["worker=api".to_string()], None).unwrap();
        assert_eq!(
            apply_defaults("sandbox::start", Some(defaults), payload.clone()).unwrap(),
            serde_json::json!({"auth": "t", "worker": "api", "env": {"K": "v"}})
        );
        assert_eq!(
            apply_defaults("state::get", Some(defaults), payload.clone()).unwrap(),
            payload,
            "another function receives nothing"
        );
        for unset in [None, Some(""), Some("  ")] {
            assert_eq!(
                apply_defaults("sandbox::start", unset, payload.clone()).unwrap(),
                payload
            );
        }
    }

    #[test]
    fn defaults_that_are_not_utf8_fail_instead_of_reading_as_unset() {
        assert_eq!(
            defaults_from_env(Ok("{}".to_string())).unwrap(),
            Some("{}".to_string())
        );
        assert_eq!(
            defaults_from_env(Err(std::env::VarError::NotPresent)).unwrap(),
            None
        );
        let error = defaults_from_env(Err(std::env::VarError::NotUnicode(
            std::ffi::OsString::from("secret-token"),
        )))
        .unwrap_err()
        .to_string();
        assert_eq!(error, "III_CALL_DEFAULTS is not valid UTF-8");
    }

    #[test]
    fn defaults_that_are_not_objects_fail_without_quoting_the_value() {
        let payload = serde_json::json!({});
        for (defaults, expected) in [
            ("not json secret-token", "is not valid JSON"),
            (
                "[\"secret-token\"]",
                "must be a JSON object keyed by function id",
            ),
            (
                r#"{"f": "secret-token"}"#,
                "the f entry of III_CALL_DEFAULTS must be a JSON object",
            ),
        ] {
            let error = apply_defaults("f", Some(defaults), payload.clone())
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "{error}");
            assert!(!error.contains("secret-token"), "{error}");
        }
        let error = apply_defaults("f", Some(r#"{"f": {}}"#), serde_json::json!([1]))
            .unwrap_err()
            .to_string();
        assert!(error.contains("--json must be an object"), "{error}");
    }
    use serde_json::json;

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn empty_returns_empty_object() {
        assert_eq!(parse(&[], None).unwrap(), json!({}));
    }

    #[test]
    fn kv_pairs_become_object() {
        let got = parse(&[s("a=10"), s("b=20")], None).unwrap();
        assert_eq!(got, json!({"a": 10, "b": 20}));
    }

    #[test]
    fn kv_string_falls_back_when_not_json() {
        let got = parse(&[s("name=alice"), s("age=30")], None).unwrap();
        assert_eq!(got, json!({"name": "alice", "age": 30}));
    }

    #[test]
    fn json_only_returned_verbatim() {
        let got = parse(&[], Some(r#"{"a": 1, "b": "two"}"#)).unwrap();
        assert_eq!(got, json!({"a": 1, "b": "two"}));
    }

    #[test]
    fn json_only_array_returned_verbatim() {
        let got = parse(&[], Some(r#"[1, 2, 3]"#)).unwrap();
        assert_eq!(got, json!([1, 2, 3]));
    }

    #[test]
    fn json_object_with_kv_overrides_keys() {
        let got = parse(&[s("a=99")], Some(r#"{"a":1,"b":2}"#)).unwrap();
        assert_eq!(got, json!({"a": 99, "b": 2}));
    }

    #[test]
    fn json_object_with_kv_adds_new_keys() {
        let got = parse(&[s("c=3")], Some(r#"{"a":1,"b":2}"#)).unwrap();
        assert_eq!(got, json!({"a": 1, "b": 2, "c": 3}));
    }

    #[test]
    fn json_array_with_kv_errors() {
        let err = parse(&[s("a=1")], Some("[1,2,3]")).unwrap_err().to_string();
        assert!(err.contains("must be an object"), "got: {}", err);
    }

    #[test]
    fn invalid_json_errors_with_clear_message() {
        let err = parse(&[], Some("not-json")).unwrap_err().to_string();
        assert!(err.contains("--json: invalid JSON"), "got: {}", err);
    }

    #[test]
    fn kv_missing_equals_errors() {
        let err = parse(&[s("just-a-word")], None).unwrap_err().to_string();
        assert!(err.contains("expected key=value"), "got: {}", err);
    }

    #[test]
    fn kv_empty_key_errors() {
        let err = parse(&[s("=value")], None).unwrap_err().to_string();
        assert!(err.contains("must not be empty"), "got: {}", err);
    }

    #[test]
    fn collecting_repeated_singular_values_builds_a_plural_array() {
        let got = parse_collecting(
            &[
                s("file=worker-compose.yaml"),
                s("worker=database"),
                s("worker=web"),
            ],
            None,
            "worker",
            "workers",
        )
        .unwrap();

        assert_eq!(
            got,
            json!({
                "file": "worker-compose.yaml",
                "workers": ["database", "web"],
            })
        );
    }

    #[test]
    fn collecting_one_singular_value_still_builds_an_array() {
        let got = parse_collecting(&[s("worker=database")], None, "worker", "workers").unwrap();
        assert_eq!(got, json!({"workers": ["database"]}));
    }

    #[test]
    fn collected_values_override_json_worker_fields() {
        let got = parse_collecting(
            &[s("worker=web")],
            Some(r#"{"worker":"old","workers":["old"]}"#),
            "worker",
            "workers",
        )
        .unwrap();

        assert_eq!(got, json!({"workers": ["web"]}));
    }
}
