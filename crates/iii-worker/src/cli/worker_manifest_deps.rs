// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Parse and validate the optional `dependencies:` block of `iii.worker.yaml`.

use std::collections::BTreeMap;

/// Parse the `dependencies:` field from an already-loaded YAML document.
/// Returns an empty map when the field is absent. Returns `Err` when the
/// field exists but is malformed (not a mapping, empty keys, invalid
/// semver range, worker name with disallowed characters, or self-reference
/// when `self_name` is `Some`).
///
/// NOTE on prerelease ranges: this parser accepts any valid semver range,
/// including prereleases like `1.0.0-beta.1` or `^1.2.0-rc.0`. The default
/// iii registry resolver filters candidates to stable versions only
/// (`../registry/api/src/services/resolver.service.ts:222`), so declaring a
/// prerelease range will surface as a `version_not_found` error at
/// `/resolve` time even when the prerelease is actually published. This is
/// intentional — author declarations stay forward-compatible with any
/// registry that chooses to expose prereleases.
pub fn parse_dependencies(
    doc: &serde_yaml::Value,
    self_name: Option<&str>,
) -> Result<BTreeMap<String, String>, String> {
    let Some(field) = doc.get("dependencies") else {
        return Ok(BTreeMap::new());
    };

    if field.is_null() {
        return Ok(BTreeMap::new());
    }

    let mapping = field.as_mapping().ok_or_else(|| {
        "`dependencies` must be a mapping of worker-name -> semver range".to_string()
    })?;

    let mut out = BTreeMap::new();
    for (k, v) in mapping {
        let name = k
            .as_str()
            .ok_or_else(|| "`dependencies` keys must be strings".to_string())?
            .trim();
        let range = v
            .as_str()
            .ok_or_else(|| format!("`dependencies.{name}` must be a string semver range"))?
            .trim();

        if name.is_empty() {
            return Err("`dependencies` key cannot be empty".to_string());
        }
        if range.is_empty() {
            return Err(format!("`dependencies.{name}` range cannot be empty"));
        }
        super::registry::validate_worker_name(name)
            .map_err(|e| format!("invalid dependency key `{name}`: {e}"))?;
        semver::VersionReq::parse(range).map_err(|e| {
            format!("invalid semver range for dependency `{name}`: `{range}` ({e})")
        })?;
        if let Some(self_name) = self_name
            && name == self_name
        {
            return Err(format!(
                "dependency `{name}` refers to the worker itself; remove the self-reference"
            ));
        }
        if out.insert(name.to_string(), range.to_string()).is_some() {
            return Err(format!("duplicate dependency key `{name}`"));
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yaml(input: &str) -> serde_yaml::Value {
        serde_yaml::from_str(input).unwrap()
    }

    #[test]
    fn absent_field_is_empty_map() {
        let deps = parse_dependencies(&yaml("name: foo\n"), Some("foo")).unwrap();
        assert!(deps.is_empty());
    }

    #[test]
    fn parses_valid_dependencies() {
        let doc = yaml(
            "name: caller\ndependencies:\n  math-worker: \"^0.1.0\"\n  iii-http: \"~1.2.0\"\n",
        );
        let deps = parse_dependencies(&doc, Some("caller")).unwrap();
        assert_eq!(deps.len(), 2);
        assert_eq!(deps.get("math-worker").unwrap(), "^0.1.0");
        assert_eq!(deps.get("iii-http").unwrap(), "~1.2.0");
    }

    #[test]
    fn rejects_non_mapping() {
        let doc = yaml("name: caller\ndependencies: \"nope\"\n");
        let err = parse_dependencies(&doc, Some("caller")).unwrap_err();
        assert!(err.contains("must be a mapping"), "got: {err}");
    }

    #[test]
    fn rejects_empty_range() {
        let doc = yaml("name: caller\ndependencies:\n  math-worker: \"\"\n");
        let err = parse_dependencies(&doc, Some("caller")).unwrap_err();
        assert!(err.contains("range cannot be empty"), "got: {err}");
    }

    #[test]
    fn rejects_invalid_range() {
        let doc = yaml("name: caller\ndependencies:\n  math-worker: \"not-a-range\"\n");
        let err = parse_dependencies(&doc, Some("caller")).unwrap_err();
        assert!(err.contains("invalid semver range"), "got: {err}");
    }

    #[test]
    fn rejects_invalid_name() {
        let doc = yaml("name: caller\ndependencies:\n  \"../bad\": \"^1.0.0\"\n");
        let err = parse_dependencies(&doc, Some("caller")).unwrap_err();
        assert!(err.contains("invalid dependency key"), "got: {err}");
    }

    #[test]
    fn rejects_self_dependency() {
        let doc = yaml("name: caller\ndependencies:\n  caller: \"^0.1.0\"\n");
        let err = parse_dependencies(&doc, Some("caller")).unwrap_err();
        assert!(err.contains("refers to the worker itself"), "got: {err}");
    }

    #[test]
    fn null_dependencies_is_empty_map() {
        let doc = yaml("name: caller\ndependencies: null\n");
        let deps = parse_dependencies(&doc, Some("caller")).unwrap();
        assert!(deps.is_empty());
    }
}
