// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

/// Configurable builtin workers: enabled by default and have a default YAML config.
pub const BUILTIN_NAMES: [&str; 8] = [
    "iii-http",
    "iii-stream",
    "iii-state",
    "iii-queue",
    "iii-pubsub",
    "iii-cron",
    "iii-observability",
    "iii-sandbox",
];

/// Optional builtin workers: baked into the engine but disabled by default.
/// They have no auto-generated YAML config and must be configured manually.
pub const OPTIONAL_BUILTIN_NAMES: [&str; 2] = ["iii-exec", "iii-bridge"];

/// Internal builtin workers that are always present and never user-configured.
pub const MANDATORY_BUILTIN_NAMES: [&str; 4] = [
    "iii-worker-manager",
    "iii-telemetry",
    "iii-engine-functions",
    "iii-http-functions",
];

/// Returns true if the name is any engine builtin (configurable, optional, or mandatory).
pub fn is_any_builtin(name: &str) -> bool {
    BUILTIN_NAMES.contains(&name)
        || OPTIONAL_BUILTIN_NAMES.contains(&name)
        || MANDATORY_BUILTIN_NAMES.contains(&name)
}

/// Version used for built-in worker metadata when the caller did not request
/// an explicit registry version.
pub const ENGINE_BUILTIN_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Resolve the version to use for builtin worker telemetry and display.
pub fn resolve_builtin_version(requested: Option<&str>) -> &str {
    requested
        .filter(|version| !version.is_empty())
        .unwrap_or(ENGINE_BUILTIN_VERSION)
}

const HTTP_MANIFEST: &str = include_str!("../../../../engine/src/workers/rest_api/iii.worker.yaml");
const STREAM_MANIFEST: &str = include_str!("../../../../engine/src/workers/stream/iii.worker.yaml");
const STATE_MANIFEST: &str = include_str!("../../../../engine/src/workers/state/iii.worker.yaml");
const QUEUE_MANIFEST: &str = include_str!("../../../../engine/src/workers/queue/iii.worker.yaml");
const PUBSUB_MANIFEST: &str = include_str!("../../../../engine/src/workers/pubsub/iii.worker.yaml");
const CRON_MANIFEST: &str = include_str!("../../../../engine/src/workers/cron/iii.worker.yaml");
const OBSERVABILITY_MANIFEST: &str =
    include_str!("../../../../engine/src/workers/observability/iii.worker.yaml");

fn manifest_for_builtin(name: &str) -> Option<&'static str> {
    match name {
        "iii-http" => Some(HTTP_MANIFEST),
        "iii-stream" => Some(STREAM_MANIFEST),
        "iii-state" => Some(STATE_MANIFEST),
        "iii-queue" => Some(QUEUE_MANIFEST),
        "iii-pubsub" => Some(PUBSUB_MANIFEST),
        "iii-cron" => Some(CRON_MANIFEST),
        "iii-observability" => Some(OBSERVABILITY_MANIFEST),
        _ => None,
    }
}

fn extract_manifest_config_yaml(manifest: &str) -> Option<String> {
    let manifest: serde_yaml::Value = serde_yaml::from_str(manifest).ok()?;
    let config_key = serde_yaml::Value::String("config".into());
    let config = manifest.as_mapping()?.get(&config_key)?;
    let yaml = serde_yaml::to_string(config).ok()?;
    let yaml = yaml.strip_prefix("---\n").unwrap_or(&yaml).trim_end();

    if yaml.is_empty() || yaml == "null" {
        None
    } else {
        Some(yaml.to_string())
    }
}

// Flat SandboxConfig shape — parsed directly by the daemon's config
// loader. Renders in config.yaml as a clean `config:` block without a
// redundant `sandbox:` key nested under an entry already named
// `iii-sandbox`.
//
// `image_allowlist` ships with `python` and `node` because those cover
// ~95% of AI-agent use cases. `bash` and `alpine` are still catalog
// presets (see sandbox_daemon/catalog.rs::PRESETS) — users opt in by
// adding them to this list. An empty allowlist means nothing boots.
const SANDBOX_DEFAULT: &str = "\
auto_install: true
image_allowlist:
  - python
  - node
default_idle_timeout_secs: 300
max_concurrent_sandboxes: 32
default_cpus: 1
default_memory_mb: 512
";

/// Return the default YAML configuration for a builtin worker, or `None` if the
/// name is not a recognised builtin.
pub fn get_builtin_default(name: &str) -> Option<String> {
    if name == "iii-sandbox" {
        return Some(SANDBOX_DEFAULT.to_string());
    }

    manifest_for_builtin(name).and_then(extract_manifest_config_yaml)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::Value;

    #[test]
    fn all_builtins_return_some() {
        for name in &BUILTIN_NAMES {
            assert!(
                get_builtin_default(name).is_some(),
                "expected Some for builtin '{name}'"
            );
        }
    }

    #[test]
    fn unknown_name_returns_none() {
        assert!(get_builtin_default("iii-unknown").is_none());
        assert!(get_builtin_default("").is_none());
        assert!(get_builtin_default("http").is_none());
    }

    #[test]
    fn all_defaults_are_valid_yaml() {
        for name in &BUILTIN_NAMES {
            let yaml = get_builtin_default(name).unwrap();
            let result: Result<Value, _> = serde_yaml::from_str(&yaml);
            assert!(
                result.is_ok(),
                "invalid YAML for '{name}': {:?}",
                result.err()
            );
        }
    }

    #[test]
    fn resolve_builtin_version_uses_engine_version_by_default() {
        assert_eq!(resolve_builtin_version(None), ENGINE_BUILTIN_VERSION);
        assert_eq!(resolve_builtin_version(Some("")), ENGINE_BUILTIN_VERSION);
    }

    #[test]
    fn resolve_builtin_version_accepts_explicit_versions() {
        assert_eq!(resolve_builtin_version(Some("0.10.0")), "0.10.0");
        assert_eq!(
            resolve_builtin_version(Some("0.11.4-next.2")),
            "0.11.4-next.2"
        );
        assert_eq!(
            resolve_builtin_version(Some("custom-channel")),
            "custom-channel"
        );
    }

    #[test]
    fn observability_default_uses_engine_version_placeholder() {
        let yaml = get_builtin_default("iii-observability").unwrap();

        assert!(yaml.contains("service_version: ${SERVICE_VERSION:__III_ENGINE_VERSION__}"));
        assert!(!yaml.contains("service_version: 0.2.0"));
    }

    #[test]
    fn http_default_has_expected_fields() {
        let yaml = get_builtin_default("iii-http").unwrap();
        let val: Value = serde_yaml::from_str(&yaml).unwrap();
        let map = val.as_mapping().expect("expected mapping");

        assert_eq!(
            map[&Value::String("port".into())],
            Value::Number(3111.into())
        );
        assert_eq!(
            map[&Value::String("host".into())],
            Value::String("127.0.0.1".into())
        );
        assert_eq!(
            map[&Value::String("default_timeout".into())],
            Value::Number(30000.into())
        );
        assert_eq!(
            map[&Value::String("concurrency_request_limit".into())],
            Value::Number(1024.into())
        );

        let cors = map[&Value::String("cors".into())]
            .as_mapping()
            .expect("cors should be a mapping");
        assert!(cors.contains_key(&Value::String("allowed_origins".into())));
        assert!(cors.contains_key(&Value::String("allowed_methods".into())));
    }

    #[test]
    fn stream_default_uses_kv_adapter() {
        let yaml = get_builtin_default("iii-stream").unwrap();
        let val: Value = serde_yaml::from_str(&yaml).unwrap();
        let map = val.as_mapping().unwrap();

        assert_eq!(
            map[&Value::String("port".into())],
            Value::Number(3112.into())
        );

        let adapter = map[&Value::String("adapter".into())]
            .as_mapping()
            .expect("adapter should be a mapping");
        assert_eq!(
            adapter[&Value::String("name".into())],
            Value::String("kv".into())
        );

        let config = adapter[&Value::String("config".into())]
            .as_mapping()
            .expect("config should be a mapping");
        assert_eq!(
            config[&Value::String("store_method".into())],
            Value::String("file_based".into())
        );
    }

    #[test]
    fn state_default_uses_kv_adapter() {
        let yaml = get_builtin_default("iii-state").unwrap();
        let val: Value = serde_yaml::from_str(&yaml).unwrap();
        let map = val.as_mapping().unwrap();

        let adapter = map[&Value::String("adapter".into())]
            .as_mapping()
            .expect("adapter should be a mapping");
        assert_eq!(
            adapter[&Value::String("name".into())],
            Value::String("kv".into())
        );

        let config = adapter[&Value::String("config".into())]
            .as_mapping()
            .expect("config should be a mapping");
        assert_eq!(
            config[&Value::String("store_method".into())],
            Value::String("file_based".into())
        );
    }

    #[test]
    fn pubsub_default_uses_local_adapter() {
        let yaml = get_builtin_default("iii-pubsub").unwrap();
        let val: Value = serde_yaml::from_str(&yaml).unwrap();
        let map = val.as_mapping().unwrap();

        let adapter = map[&Value::String("adapter".into())]
            .as_mapping()
            .expect("adapter should be a mapping");
        assert_eq!(
            adapter[&Value::String("name".into())],
            Value::String("local".into())
        );
    }

    #[test]
    fn observability_default_uses_memory_exporter() {
        let yaml = get_builtin_default("iii-observability").unwrap();
        let val: Value = serde_yaml::from_str(&yaml).unwrap();
        let map = val.as_mapping().unwrap();

        assert_eq!(map[&Value::String("enabled".into())], Value::Bool(true));
        assert_eq!(
            map[&Value::String("exporter".into())],
            Value::String("memory".into())
        );
        assert_eq!(
            map[&Value::String("metrics_exporter".into())],
            Value::String("memory".into())
        );
        assert_eq!(
            map[&Value::String("logs_exporter".into())],
            Value::String("memory".into())
        );
    }

    #[test]
    fn sandbox_default_allowlist_is_python_and_node_only() {
        // The "just works" defaults ship with python + node only. bash
        // and alpine are still catalog presets but opt-in — users who
        // want them edit config.yaml by hand. Pinning the exact contents
        // here makes any future change to the default a conscious PR,
        // not a silent addition that changes the security surface.
        let yaml = get_builtin_default("iii-sandbox").unwrap();
        let val: Value = serde_yaml::from_str(&yaml).unwrap();
        let allowlist: Vec<&str> = val
            .as_mapping()
            .and_then(|m| m.get(&Value::String("image_allowlist".into())))
            .and_then(|v| v.as_sequence())
            .expect("image_allowlist must be a top-level sequence")
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(
            allowlist,
            vec!["python", "node"],
            "sandbox default allowlist changed — confirm this is intentional \
             and update engine/config.yaml + docs/api-reference/sandbox.mdx"
        );
    }

    #[test]
    fn sandbox_default_has_no_wrapping_sandbox_key() {
        // Guard against regressions to the wrapped shape. The flat shape
        // is what renders cleanly under `config:` in config.yaml; adding
        // a wrapping `sandbox:` key back would produce
        // `iii-sandbox.config.sandbox.image_allowlist` — double-nested
        // and ugly.
        let yaml = get_builtin_default("iii-sandbox").unwrap();
        let val: Value = serde_yaml::from_str(&yaml).unwrap();
        assert!(
            val.as_mapping()
                .map(|m| !m.contains_key(&Value::String("sandbox".into())))
                .unwrap_or(false),
            "iii-sandbox default must use flat shape (no wrapping `sandbox:` key)"
        );
    }

    #[test]
    fn builtin_names_matches_function() {
        for name in &BUILTIN_NAMES {
            assert!(
                get_builtin_default(name).is_some(),
                "BUILTIN_NAMES contains '{name}' but get_builtin_default returns None"
            );
        }

        let known: std::collections::HashSet<&str> = BUILTIN_NAMES.iter().copied().collect();
        for extra in &["iii-unknown", "iii-foo", "redis", ""] {
            assert!(
                !known.contains(extra),
                "unexpected name '{extra}' found in BUILTIN_NAMES"
            );
            assert!(
                get_builtin_default(extra).is_none(),
                "get_builtin_default should return None for '{extra}'"
            );
        }
    }
}
