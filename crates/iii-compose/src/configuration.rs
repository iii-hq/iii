// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Configuration merge and delivery.
//!
//! Compose resolves defaults, the current configuration and overrides before spawn,
//! then injects the execution value into the configuration service in memory.
//! `III_CONFIG_NAME` identifies the entry workers read; only explicit saves
//! persist the active value. No execution snapshot file is created.

use sha2::{Digest, Sha256};

use crate::error::{ComposeError, Result};

/// Readable configuration identity, without sanitization, truncation, or a hash.
/// Ambiguous namespace/key boundaries require an explicit `config_name`.
pub(crate) fn default_config_name(namespace: &str, key: &str) -> Result<String> {
    let name = format!("{namespace}-{key}");
    if name.len() > 64
        || !name
            .bytes()
            .all(|ch| matches!(ch, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_'))
    {
        return Err(ComposeError::InvalidConfigName { name });
    }
    Ok(name)
}

/// Exact previous algorithm, used only to locate this container's legacy entry.
pub(crate) fn legacy_config_name(namespace: &str, key: &str) -> String {
    let mut digest = Sha256::new();
    digest.update((namespace.len() as u64).to_be_bytes());
    digest.update(namespace.as_bytes());
    digest.update(key.as_bytes());
    let suffix = hex::encode(&digest.finalize()[..8]);
    let prefix: String = namespace
        .chars()
        .chain(std::iter::once('-'))
        .chain(key.chars())
        .map(|ch| match ch {
            'a'..='z' | '0'..='9' | '-' | '_' => ch,
            _ => '-',
        })
        .take(47)
        .collect();
    format!("{prefix}-{suffix}")
}

/// Merges `config_override` onto a fetched base.
///
/// Maps merge key by key. Arrays and scalars replace wholesale — half-merged
/// lists are never what an operator means. An explicit `null` is a value that
/// replaces, not a delete operator. A mapping whose `name` the override
/// changes is replaced whole: the keys beside `name` belong to the variant it
/// picks (see [`picks_another_variant`]).
pub fn merge(base: serde_yaml::Value, override_value: serde_yaml::Value) -> serde_yaml::Value {
    match (base, override_value) {
        (serde_yaml::Value::Mapping(base), serde_yaml::Value::Mapping(overrides))
            if picks_another_variant(&base, &overrides) =>
        {
            serde_yaml::Value::Mapping(overrides)
        }
        (serde_yaml::Value::Mapping(mut base), serde_yaml::Value::Mapping(overrides)) => {
            for (key, value) in overrides {
                // Merged in place so an override never reshuffles the document:
                // the delivered value stays diffable against the base.
                match base.get_mut(&key) {
                    Some(slot) => {
                        let existing = std::mem::replace(slot, serde_yaml::Value::Null);
                        *slot = merge(existing, value);
                    }
                    None => {
                        base.insert(key, value);
                    }
                }
            }
            serde_yaml::Value::Mapping(base)
        }
        (_, override_value) => override_value,
    }
}

/// `{name: …, config: …}` is how a variant is chosen everywhere in iii: the
/// adapters of state, queue, cron, pubsub and the engine's own configuration
/// worker. The keys beside `name` belong to the variant it picks, so an
/// override that picks another one replaces the mapping whole. Merging would
/// carry the old variant's keys into the new one, and a closed schema then
/// rejects the result (iii-hq/iii#2138: `store_method` from `kv` leaking into
/// `redis`).
fn picks_another_variant(base: &serde_yaml::Mapping, overrides: &serde_yaml::Mapping) -> bool {
    let key = serde_yaml::Value::from("name");
    matches!(
        (base.get(&key), overrides.get(&key)),
        (Some(serde_yaml::Value::String(a)), Some(serde_yaml::Value::String(b))) if a != b
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_names_are_stable_and_readable() {
        assert_eq!(
            default_config_name("default", "harness").unwrap(),
            "default-harness"
        );
        assert_eq!(
            default_config_name("orders", "console").unwrap(),
            "orders-console"
        );
        assert_eq!(
            legacy_config_name("default", "harness"),
            "default-harness-a14f3656efb8d5ea"
        );
        assert_eq!(
            legacy_config_name("orders", "console"),
            "orders-console-946b336ce90783a6"
        );
    }

    #[test]
    fn readable_names_require_operator_chosen_disambiguation_across_namespaces() {
        assert_eq!(
            default_config_name("a-b", "c").unwrap(),
            default_config_name("a", "b-c").unwrap()
        );
        assert_ne!(
            legacy_config_name("a-b", "c"),
            legacy_config_name("a", "b-c")
        );
    }

    #[test]
    fn generated_names_reject_invalid_or_long_inputs_without_lossy_conversion() {
        assert_eq!(default_config_name(&"n".repeat(62), "x").unwrap().len(), 64);
        for (namespace, key) in [
            ("n".repeat(63), "x"),
            ("app".into(), "API"),
            ("app".into(), "api.v2"),
            ("app".into(), "日本語"),
        ] {
            let error = default_config_name(&namespace, key).unwrap_err();
            assert_eq!(error.code(), "INVALID_CONFIG_NAME");
            assert!(error.to_string().contains("config_name"));
        }
    }

    fn yaml(text: &str) -> serde_yaml::Value {
        serde_yaml::from_str(text).unwrap()
    }

    #[test]
    fn maps_merge_key_by_key() {
        let merged = merge(
            yaml("server:\n  port: 8080\n  host: a\n"),
            yaml("server:\n  port: 3000\n"),
        );
        assert_eq!(merged, yaml("server:\n  port: 3000\n  host: a\n"));
    }

    #[test]
    fn arrays_and_scalars_replace() {
        assert_eq!(
            merge(yaml("hosts: [a, b, c]\n"), yaml("hosts: [d]\n")),
            yaml("hosts: [d]\n")
        );
        assert_eq!(
            merge(yaml("level: info\n"), yaml("level: debug\n")),
            yaml("level: debug\n")
        );
    }

    #[test]
    fn explicit_null_is_a_value_not_a_delete() {
        let merged = merge(yaml("server:\n  tls: on\n"), yaml("server:\n  tls: null\n"));
        assert_eq!(merged, yaml("server:\n  tls: null\n"));
    }

    #[test]
    fn new_keys_are_added() {
        let merged = merge(yaml("a: 1\n"), yaml("b: 2\n"));
        assert_eq!(merged, yaml("a: 1\nb: 2\n"));
    }

    #[test]
    fn changing_a_variant_name_replaces_its_mapping() {
        // iii-hq/iii#2138: the kv default must not leak into the redis config.
        let merged = merge(
            yaml("adapter:\n  name: kv\n  config:\n    store_method: file_based\n"),
            yaml("adapter:\n  name: redis\n  config:\n    redis_url: redis://127.0.0.1:6379\n"),
        );
        assert_eq!(
            merged,
            yaml("adapter:\n  name: redis\n  config:\n    redis_url: redis://127.0.0.1:6379\n")
        );
    }

    #[test]
    fn keeping_the_variant_name_still_merges() {
        let merged = merge(
            yaml("adapter:\n  name: kv\n  config:\n    store_method: file_based\n"),
            yaml("adapter:\n  name: kv\n  config:\n    file_path: /data\n"),
        );
        assert_eq!(
            merged,
            yaml(
                "adapter:\n  name: kv\n  config:\n    store_method: file_based\n    file_path: /data\n"
            )
        );
    }

    #[test]
    fn a_name_alone_drops_the_old_variant_config() {
        let merged = merge(
            yaml("adapter:\n  name: kv\n  config:\n    store_method: file_based\n"),
            yaml("adapter:\n  name: redis\n"),
        );
        assert_eq!(merged, yaml("adapter:\n  name: redis\n"));
    }

    #[test]
    fn overriding_a_key_keeps_its_position() {
        let merged = merge(yaml("a: 1\nb: 2\nc: 3\n"), yaml("b: 20\n"));
        let keys: Vec<String> = match &merged {
            serde_yaml::Value::Mapping(map) => map
                .keys()
                .map(|key| key.as_str().unwrap().to_string())
                .collect(),
            other => panic!("expected a mapping, got {other:?}"),
        };

        assert_eq!(keys, vec!["a", "b", "c"]);
        assert_eq!(merged, yaml("a: 1\nb: 20\nc: 3\n"));
    }
}
