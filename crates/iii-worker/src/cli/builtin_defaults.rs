// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

/// All builtin worker names recognised by the CLI.
pub const BUILTIN_NAMES: [&str; 7] = [
    "iii-http",
    "iii-stream",
    "iii-state",
    "iii-queue",
    "iii-pubsub",
    "iii-cron",
    "iii-observability",
];

const HTTP_DEFAULT: &str = "\
port: 3111
host: 127.0.0.1
default_timeout: 30000
concurrency_request_limit: 1024
cors:
  allowed_origins:
    - '*'
  allowed_methods:
    - GET
    - POST
    - PUT
    - DELETE
    - OPTIONS
";

const STREAM_DEFAULT: &str = "\
port: 3112
host: 127.0.0.1
adapter:
  name: kv
  config:
    store_method: file_based
    file_path: ./data/stream_store
";

const STATE_DEFAULT: &str = "\
adapter:
  name: kv
  config:
    store_method: file_based
    file_path: ./data/state_store.db
";

const QUEUE_DEFAULT: &str = "\
adapter:
  name: kv
";

const PUBSUB_DEFAULT: &str = "\
adapter:
  name: local
";

const CRON_DEFAULT: &str = "\
adapter:
  name: kv
";

const OBSERVABILITY_DEFAULT: &str = "\
enabled: true
service_name: iii
service_version: 0.2.0
exporter: memory
memory_max_spans: 10000
metrics_enabled: true
metrics_exporter: memory
metrics_retention_seconds: 3600
metrics_max_count: 10000
logs_enabled: true
logs_exporter: memory
logs_max_count: 1000
logs_retention_seconds: 3600
logs_console_output: true
sampling_ratio: 1.0
";

/// Return the default YAML configuration for a builtin worker, or `None` if the
/// name is not a recognised builtin.
pub fn get_builtin_default(name: &str) -> Option<&'static str> {
    match name {
        "iii-http" => Some(HTTP_DEFAULT),
        "iii-stream" => Some(STREAM_DEFAULT),
        "iii-state" => Some(STATE_DEFAULT),
        "iii-queue" => Some(QUEUE_DEFAULT),
        "iii-pubsub" => Some(PUBSUB_DEFAULT),
        "iii-cron" => Some(CRON_DEFAULT),
        "iii-observability" => Some(OBSERVABILITY_DEFAULT),
        _ => None,
    }
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
            let result: Result<Value, _> = serde_yaml::from_str(yaml);
            assert!(
                result.is_ok(),
                "invalid YAML for '{name}': {:?}",
                result.err()
            );
        }
    }

    #[test]
    fn http_default_has_expected_fields() {
        let yaml = get_builtin_default("iii-http").unwrap();
        let val: Value = serde_yaml::from_str(yaml).unwrap();
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
        let val: Value = serde_yaml::from_str(yaml).unwrap();
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
        let val: Value = serde_yaml::from_str(yaml).unwrap();
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
        let val: Value = serde_yaml::from_str(yaml).unwrap();
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
        let val: Value = serde_yaml::from_str(yaml).unwrap();
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
    fn builtin_names_matches_function() {
        // Every name in BUILTIN_NAMES must be recognised by get_builtin_default.
        for name in &BUILTIN_NAMES {
            assert!(
                get_builtin_default(name).is_some(),
                "BUILTIN_NAMES contains '{name}' but get_builtin_default returns None"
            );
        }

        // Conversely, the function should not recognise anything outside the array.
        let known: std::collections::HashSet<&str> = BUILTIN_NAMES.iter().copied().collect();
        // Spot-check a few names that should NOT be in the set.
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
