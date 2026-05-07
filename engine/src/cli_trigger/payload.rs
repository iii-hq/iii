// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use anyhow::{Context, Result, bail};
use serde_json::Value;

/// Resolve a payload from CLI tokens and an optional `--json` value.
///
/// Resolution order:
/// 1. If neither is provided, return `null`.
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
            let value =
                serde_json::from_str(v).unwrap_or_else(|_| Value::String(v.to_string()));
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
        (None, None) => Ok(Value::Null),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn empty_returns_null() {
        assert_eq!(parse(&[], None).unwrap(), Value::Null);
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
}
