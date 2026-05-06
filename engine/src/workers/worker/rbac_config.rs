// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::collections::HashMap;

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::function::Function;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RbacConfig {
    #[serde(default)]
    pub auth_function_id: Option<String>,
    #[serde(default)]
    pub expose_functions: Vec<FunctionFilter>,
    #[serde(default)]
    pub on_trigger_registration_function_id: Option<String>,
    #[serde(default)]
    pub on_trigger_type_registration_function_id: Option<String>,
    #[serde(default)]
    pub on_function_registration_function_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WildcardPattern {
    raw: String,
}

impl WildcardPattern {
    pub fn new(pattern: &str) -> Self {
        Self {
            raw: pattern.to_string(),
        }
    }

    pub fn matches(&self, value: &str) -> bool {
        wildcard_match(&self.raw, value)
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    let parts: Vec<&str> = pattern.split('*').collect();

    if parts.len() == 1 {
        return pattern == value;
    }

    let mut pos = 0;

    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }

        if let Some(found) = value[pos..].find(part) {
            if i == 0 && found != 0 {
                return false;
            }
            pos += found + part.len();
        } else {
            return false;
        }
    }

    if let Some(last) = parts.last()
        && !last.is_empty()
        && !value.ends_with(last)
    {
        return false;
    }

    true
}

#[derive(Debug, Clone, Serialize)]
pub enum MetadataValue {
    Exact(Value),
    Wildcard(WildcardPattern),
}

impl MetadataValue {
    fn matches(&self, value: &Value) -> bool {
        match self {
            MetadataValue::Exact(expected) => value == expected,
            MetadataValue::Wildcard(pattern) => {
                if let Some(s) = value.as_str() {
                    pattern.matches(s)
                } else {
                    false
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub enum FunctionFilter {
    Match(WildcardPattern),
    Metadata(HashMap<String, MetadataValue>),
}

impl FunctionFilter {
    pub fn matches(&self, function_id: &str, metadata: Option<&Value>) -> bool {
        match self {
            FunctionFilter::Match(pattern) => pattern.matches(function_id),
            FunctionFilter::Metadata(expected) => {
                let Some(metadata) = metadata else {
                    return false;
                };
                let Some(obj) = metadata.as_object() else {
                    return false;
                };
                expected
                    .iter()
                    .all(|(key, matcher)| obj.get(key).is_some_and(|v| matcher.matches(v)))
            }
        }
    }
}

fn parse_match_pattern(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.starts_with("match(\"") && trimmed.ends_with("\")")
        || trimmed.starts_with("match('") && trimmed.ends_with("')")
    {
        Some(trimmed[7..trimmed.len() - 2].to_string())
    } else {
        None
    }
}

fn parse_metadata_value(value: Value) -> MetadataValue {
    if let Some(s) = value.as_str()
        && let Some(pattern) = parse_match_pattern(s)
    {
        return MetadataValue::Wildcard(WildcardPattern::new(&pattern));
    }
    MetadataValue::Exact(value)
}

impl<'de> Deserialize<'de> for FunctionFilter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct FilterVisitor;

        impl<'de> Visitor<'de> for FilterVisitor {
            type Value = FunctionFilter;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a match(\"pattern\") string or a map with 'metadata' key")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if let Some(pattern) = parse_match_pattern(v) {
                    Ok(FunctionFilter::Match(WildcardPattern::new(&pattern)))
                } else {
                    Err(de::Error::custom(format!(
                        "expected match(\"pattern\"), got: {}",
                        v
                    )))
                }
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut metadata_map: HashMap<String, MetadataValue> = HashMap::new();

                while let Some(key) = map.next_key::<String>()? {
                    if key == "metadata" {
                        let inner: HashMap<String, Value> = map.next_value()?;
                        for (k, v) in inner {
                            metadata_map.insert(k, parse_metadata_value(v));
                        }
                    } else {
                        let _: Value = map.next_value()?;
                    }
                }

                if metadata_map.is_empty() {
                    Err(de::Error::custom(
                        "expected a 'metadata' key with filter conditions",
                    ))
                } else {
                    Ok(FunctionFilter::Metadata(metadata_map))
                }
            }
        }

        deserializer.deserialize_any(FilterVisitor)
    }
}

// RBAC decision flow (post-fix):
//
//     InvokeFunction with session.config.rbac
//     │
//     ▼
//  1. function_id in forbidden_functions?   ── yes ──► DENY
//                       │ no
//                       ▼
//  2. function_id in allowed_functions?     ── yes ──► ALLOW
//                       │ no
//                       ▼
//  3. function_id in INFRASTRUCTURE_FUNCTIONS? ─ yes ─► ALLOW
//                       │ no
//                       ▼
//  4. any(expose_functions filter matches)? ── yes ──► ALLOW
//                       │ no
//                       ▼
//                     DENY (FORBIDDEN)
//
// Two independent rules define "infrastructure": the RBAC carve-out (this
// slice, specific IDs) and the middleware bypass (prefix match on
// `engine::*`, broader — see engine/src/engine/mod.rs). They diverge on
// purpose: the threat models differ, so do NOT unify them.
//
// INFRASTRUCTURE_FUNCTIONS is part of iii's public contract. Within a
// major version, it is additive-only: IDs are never removed from the
// carve-out except for a documented security fix, which MUST be called
// out in release notes and landed alongside a deprecation/migration
// note. Renames keep both old and new IDs in the slice through at least
// one major version. Security-driven removal is the only narrow
// exception to additive-only.
const INFRASTRUCTURE_FUNCTIONS: &[&str] = &[
    "engine::channels::create",
    "engine::workers::register",
    "engine::log::info",
    "engine::log::warn",
    "engine::log::error",
    "engine::log::debug",
    "engine::log::trace",
    "engine::baggage::get",
    "engine::baggage::set",
    "engine::baggage::get_all",
];

pub fn is_function_allowed(
    function_id: &str,
    config: Option<RbacConfig>,
    allowed_functions: &[String],
    forbidden_functions: &[String],
    function: Option<&Function>,
) -> bool {
    if forbidden_functions.iter().any(|f| f == function_id) {
        if INFRASTRUCTURE_FUNCTIONS.contains(&function_id) {
            tracing::warn!(
                function_id = %function_id,
                "auth function forbids infrastructure function '{}' — worker may behave unpredictably (connection setup, logging, or context propagation may be blocked)",
                function_id
            );
        }
        return false;
    }

    if allowed_functions.iter().any(|f| f == function_id) {
        return true;
    }

    if INFRASTRUCTURE_FUNCTIONS.contains(&function_id) {
        return true;
    }

    if let Some(config) = config {
        let metadata = function.and_then(|f| f.metadata.as_ref());

        config
            .expose_functions
            .iter()
            .any(|filter| filter.matches(function_id, metadata))
    } else {
        true
    }
}

impl<'de> Deserialize<'de> for WildcardPattern {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(WildcardPattern::new(&s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wildcard_exact_match() {
        let p = WildcardPattern::new("hello::world");
        assert!(p.matches("hello::world"));
        assert!(!p.matches("hello::worl"));
        assert!(!p.matches("hello::worldx"));
    }

    #[test]
    fn wildcard_prefix() {
        let p = WildcardPattern::new("engine::*");
        assert!(p.matches("engine::foo"));
        assert!(p.matches("engine::bar::baz"));
        assert!(!p.matches("other::foo"));
    }

    #[test]
    fn wildcard_suffix() {
        let p = WildcardPattern::new("*::public");
        assert!(p.matches("api::public"));
        assert!(p.matches("x::y::public"));
        assert!(!p.matches("api::private"));
    }

    #[test]
    fn wildcard_contains() {
        let p = WildcardPattern::new("*public*");
        assert!(p.matches("public"));
        assert!(p.matches("mypublicfn"));
        assert!(p.matches("public_api"));
        assert!(!p.matches("private"));
    }

    #[test]
    fn wildcard_star_matches_all() {
        let p = WildcardPattern::new("*");
        assert!(p.matches("anything"));
        assert!(p.matches(""));
    }

    #[test]
    fn wildcard_middle() {
        let p = WildcardPattern::new("api::*::read");
        assert!(p.matches("api::users::read"));
        assert!(p.matches("api::orders::read"));
        assert!(!p.matches("api::users::write"));
    }

    #[test]
    fn metadata_exact_match() {
        let mv = MetadataValue::Exact(json!(true));
        assert!(mv.matches(&json!(true)));
        assert!(!mv.matches(&json!(false)));
    }

    #[test]
    fn metadata_wildcard_match() {
        let mv = MetadataValue::Wildcard(WildcardPattern::new("*public*"));
        assert!(mv.matches(&json!("mypublic")));
        assert!(!mv.matches(&json!("private")));
        assert!(!mv.matches(&json!(42))); // non-string
    }

    #[test]
    fn filter_match_pattern() {
        let filter = FunctionFilter::Match(WildcardPattern::new("test::ew::*"));
        assert!(filter.matches("test::ew::echo", None));
        assert!(!filter.matches("test::other::echo", None));
    }

    #[test]
    fn filter_metadata() {
        let mut meta = HashMap::new();
        meta.insert("public".to_string(), MetadataValue::Exact(json!(true)));
        let filter = FunctionFilter::Metadata(meta);

        assert!(filter.matches("any", Some(&json!({"public": true}))));
        assert!(!filter.matches("any", Some(&json!({"public": false}))));
        assert!(!filter.matches("any", None));
    }

    #[test]
    fn parse_match_pattern_valid() {
        assert_eq!(
            parse_match_pattern("match(\"engine::*\")"),
            Some("engine::*".to_string())
        );
        assert_eq!(
            parse_match_pattern("match('engine::*')"),
            Some("engine::*".to_string())
        );
    }

    #[test]
    fn parse_match_pattern_invalid() {
        assert_eq!(parse_match_pattern("engine::*"), None);
        assert_eq!(parse_match_pattern("match(engine::*)"), None);
    }

    #[test]
    fn deserialize_config_yaml() {
        let yaml = r#"
            auth_function_id: my-project::auth
            expose_functions:
              - match("test::ew::*")
              - metadata:
                  public: true
        "#;
        let config: RbacConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            config.auth_function_id,
            Some("my-project::auth".to_string())
        );
        assert_eq!(config.expose_functions.len(), 2);
        assert!(config.on_trigger_registration_function_id.is_none());
        assert!(config.on_trigger_type_registration_function_id.is_none());
    }

    #[test]
    fn deserialize_config_with_trigger_hooks() {
        let yaml = r#"
            on_trigger_registration_function_id: my-project::on-trigger-reg
            on_trigger_type_registration_function_id: my-project::on-trigger-type-reg
            expose_functions: []
        "#;
        let config: RbacConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            config.on_trigger_registration_function_id,
            Some("my-project::on-trigger-reg".to_string())
        );
        assert_eq!(
            config.on_trigger_type_registration_function_id,
            Some("my-project::on-trigger-type-reg".to_string())
        );
    }

    #[test]
    fn access_resolution_forbidden_takes_precedence() {
        let config = RbacConfig {
            auth_function_id: None,
            expose_functions: vec![FunctionFilter::Match(WildcardPattern::new("*"))],
            on_trigger_registration_function_id: None,
            on_trigger_type_registration_function_id: None,
            on_function_registration_function_id: None,
        };
        let allowed = vec!["test::fn".to_string()];
        let forbidden = vec!["test::fn".to_string()];
        assert!(!is_function_allowed(
            "test::fn",
            Some(config),
            &allowed,
            &forbidden,
            None
        ));
    }

    #[test]
    fn access_resolution_allowed_overrides_expose() {
        let config = RbacConfig {
            auth_function_id: None,
            expose_functions: vec![],
            on_trigger_registration_function_id: None,
            on_trigger_type_registration_function_id: None,
            on_function_registration_function_id: None,
        };
        let allowed = vec!["test::fn".to_string()];
        assert!(is_function_allowed(
            "test::fn",
            Some(config),
            &allowed,
            &[],
            None
        ));
    }

    #[test]
    fn access_resolution_channel_create_always_allowed() {
        let config = RbacConfig {
            auth_function_id: None,
            expose_functions: vec![],
            on_trigger_registration_function_id: None,
            on_trigger_type_registration_function_id: None,
            on_function_registration_function_id: None,
        };
        assert!(is_function_allowed(
            "engine::channels::create",
            Some(config),
            &[],
            &[],
            None
        ));
    }

    #[test]
    fn access_resolution_deny_by_default() {
        let config = RbacConfig {
            auth_function_id: None,
            expose_functions: vec![FunctionFilter::Match(WildcardPattern::new("api::*"))],
            on_trigger_registration_function_id: None,
            on_trigger_type_registration_function_id: None,
            on_function_registration_function_id: None,
        };
        assert!(!is_function_allowed(
            "internal::fn",
            Some(config),
            &[],
            &[],
            None
        ));
    }

    fn rbac_config_with_expose(patterns: &[&str]) -> RbacConfig {
        RbacConfig {
            auth_function_id: None,
            expose_functions: patterns
                .iter()
                .map(|p| FunctionFilter::Match(WildcardPattern::new(p)))
                .collect(),
            on_trigger_registration_function_id: None,
            on_trigger_type_registration_function_id: None,
            on_function_registration_function_id: None,
        }
    }

    #[test]
    fn infrastructure_functions_always_allowed() {
        for id in INFRASTRUCTURE_FUNCTIONS {
            // Empty expose_functions
            let config = rbac_config_with_expose(&[]);
            assert!(
                is_function_allowed(id, Some(config), &[], &[], None),
                "expected {} to be allowed with empty expose_functions",
                id
            );

            // Unrelated expose pattern
            let config = rbac_config_with_expose(&["api::*"]);
            assert!(
                is_function_allowed(id, Some(config), &[], &[], None),
                "expected {} to be allowed when only api::* exposed",
                id
            );
        }
    }

    #[test]
    fn infrastructure_functions_respect_forbidden_list() {
        for id in INFRASTRUCTURE_FUNCTIONS {
            let config = rbac_config_with_expose(&[]);
            let forbidden = vec![id.to_string()];
            assert!(
                !is_function_allowed(id, Some(config), &[], &forbidden, None),
                "expected {} to be denied when present in forbidden_functions",
                id
            );
        }
    }

    #[test]
    fn infrastructure_functions_respect_allowed_list() {
        for id in INFRASTRUCTURE_FUNCTIONS {
            let config = rbac_config_with_expose(&[]);
            let allowed = vec![id.to_string()];
            assert!(
                is_function_allowed(id, Some(config), &allowed, &[], None),
                "expected {} to remain allowed when also in allowed_functions",
                id
            );
        }
    }

    #[test]
    fn discovery_functions_still_gated() {
        let discovery_ids = [
            "engine::functions::list",
            "engine::workers::list",
            "engine::triggers::list",
            "engine::trigger-types::list",
            "engine::traces::list",
            "engine::queue::list_topics",
            "engine::health::check",
            "engine::alerts::list",
        ];
        for id in discovery_ids {
            let config = rbac_config_with_expose(&[]);
            assert!(
                !is_function_allowed(id, Some(config), &[], &[], None),
                "expected discovery id {} to be denied with empty expose_functions",
                id
            );
        }
    }

    #[test]
    fn discovery_functions_allowed_via_expose() {
        let config = rbac_config_with_expose(&["engine::functions::*"]);
        assert!(is_function_allowed(
            "engine::functions::list",
            Some(config),
            &[],
            &[],
            None
        ));
    }

    /// BUG REPRODUCTION INVERTED: on this branch (with the carve-out), the
    /// reporter's config must ALLOW the infrastructure IDs that main DENIES.
    /// Run the SAME assertion block here but expect the opposite outcome.
    /// This test passing here is proof the fix works end-to-end.
    #[test]
    fn bug_repro_infra_calls_allowed_under_reporter_config_post_fix() {
        let config = RbacConfig {
            auth_function_id: None,
            expose_functions: vec![
                FunctionFilter::Match(WildcardPattern::new("api::*")),
                FunctionFilter::Match(WildcardPattern::new("session::*")),
                FunctionFilter::Match(WildcardPattern::new("stream::*")),
                FunctionFilter::Match(WildcardPattern::new("state::*")),
            ],
            on_trigger_registration_function_id: None,
            on_trigger_type_registration_function_id: None,
            on_function_registration_function_id: None,
        };

        for id in [
            "engine::log::info",
            "engine::workers::register",
            "engine::baggage::get",
            "engine::baggage::set",
        ] {
            assert!(
                is_function_allowed(id, Some(config.clone()), &[], &[], None),
                "FIX REGRESSION: expected {} to be ALLOWED on the fix branch with reporter's config",
                id
            );
        }
    }

    #[test]
    fn no_rbac_config_still_allows_everything() {
        for id in INFRASTRUCTURE_FUNCTIONS.iter().chain(
            [
                "engine::functions::list",
                "engine::workers::list",
                "api::anything",
                "internal::private",
            ]
            .iter(),
        ) {
            assert!(
                is_function_allowed(id, None, &[], &[], None),
                "expected {} to be allowed when config is None",
                id
            );
        }
    }
}
