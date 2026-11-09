// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The `observers:` section of the engine config (protocol.md §3.6). Every
/// key is optional; an absent section is all defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ObserversConfig {
    /// Global kill switch for SDK in-process calls (`functionregistered
    /// { local_dispatch }`).
    pub local_dispatch: bool,
    /// Observers one connection may own.
    pub max_per_connection: usize,
    /// Observers one function may have.
    pub max_per_function: usize,
    /// Activations deeper than this many observe hops are dropped
    /// (`LOOP_GUARD`).
    pub max_observe_depth: u32,
    /// How long an observer outlives its observing worker's connection,
    /// waiting for a `Reattach`.
    pub observer_grace_ms: u64,
    /// How long the engine remembers the observers it disposed because their
    /// observing worker did not reattach within `observer_grace_ms`. A worker
    /// that reattaches later than the grace period but within this time has
    /// its replayed observers answered with their disposal; one that comes
    /// back after it replays them as new observers, as after an engine
    /// restart. `0` forgets them at once.
    pub disposed_retention_ms: u64,
    /// An added observer the function's host does not ack in time (its
    /// `onObserverAdded` hooks) fails with `OWNER_TIMEOUT`. The host runs one
    /// function's hooks one at a time, so the time counts from when it moved
    /// on to the observer: its announcement, or the host's answer for the
    /// observer before it.
    pub owner_ack_timeout_ms: u64,
    /// `max_in_flight` of an observer that sets none.
    pub default_max_in_flight: u32,
    /// Activations one observer may queue behind its in-flight limit before
    /// they are dropped (`OVERFLOW`).
    pub max_queue_per_observer: usize,
    /// How long an activation (and a call through an observer) may wait for its
    /// target's answer before its in-flight slot is freed and it counts as a
    /// `TIMEOUT` failure. The same 30 s the SDKs and the HTTP invoker default
    /// to for a call.
    pub activation_timeout_ms: u64,
}

impl Default for ObserversConfig {
    fn default() -> Self {
        Self {
            local_dispatch: true,
            max_per_connection: 1024,
            max_per_function: 4096,
            max_observe_depth: 8,
            observer_grace_ms: 30_000,
            disposed_retention_ms: 3_600_000,
            owner_ack_timeout_ms: 30_000,
            default_max_in_flight: 64,
            max_queue_per_observer: 1024,
            activation_timeout_ms: 30_000,
        }
    }
}

impl ObserversConfig {
    /// Refuses limits of 0, which would make every observer fail (or never
    /// fire) instead of meaning "no limit".
    pub fn validate(&self) -> Result<(), String> {
        for (key, value) in [
            ("max_per_connection", self.max_per_connection as u64),
            ("max_per_function", self.max_per_function as u64),
            ("max_observe_depth", u64::from(self.max_observe_depth)),
            (
                "default_max_in_flight",
                u64::from(self.default_max_in_flight),
            ),
            ("max_queue_per_observer", self.max_queue_per_observer as u64),
        ] {
            if value == 0 {
                return Err(format!("observers.{key} must be at least 1"));
            }
        }
        Ok(())
    }

    /// Timeouts of 0 are allowed but almost certainly a mistake: every
    /// observing worker disposed on disconnect, every add timed out, every
    /// activation failed.
    pub fn warnings(&self) -> Vec<String> {
        [
            ("observer_grace_ms", self.observer_grace_ms),
            ("owner_ack_timeout_ms", self.owner_ack_timeout_ms),
            ("activation_timeout_ms", self.activation_timeout_ms),
        ]
        .into_iter()
        .filter(|(_, value)| *value == 0)
        .map(|(key, _)| format!("observers.{key} is 0"))
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_section_and_an_empty_one_are_the_defaults() {
        let empty: ObserversConfig = serde_yaml::from_str("{}").unwrap();
        assert_eq!(empty, ObserversConfig::default());
        assert_eq!(
            ObserversConfig::default(),
            ObserversConfig {
                local_dispatch: true,
                max_per_connection: 1024,
                max_per_function: 4096,
                max_observe_depth: 8,
                observer_grace_ms: 30_000,
                disposed_retention_ms: 3_600_000,
                owner_ack_timeout_ms: 30_000,
                default_max_in_flight: 64,
                max_queue_per_observer: 1024,
                activation_timeout_ms: 30_000,
            }
        );
    }

    #[test]
    fn keys_override_one_by_one_and_unknown_keys_are_refused() {
        let config: ObserversConfig =
            serde_yaml::from_str("max_observe_depth: 3\nobserver_grace_ms: 250\n").unwrap();
        assert_eq!(config.max_observe_depth, 3);
        assert_eq!(config.observer_grace_ms, 250);
        assert_eq!(config.max_per_function, 4096);

        assert!(serde_yaml::from_str::<ObserversConfig>("max_observers: 3\n").is_err());
    }

    #[test]
    fn zero_limits_are_refused_and_zero_timeouts_warned_about() {
        assert!(ObserversConfig::default().validate().is_ok());
        assert!(ObserversConfig::default().warnings().is_empty());
        for yaml in [
            "max_per_connection: 0",
            "max_per_function: 0",
            "max_observe_depth: 0",
            "default_max_in_flight: 0",
            "max_queue_per_observer: 0",
        ] {
            let config: ObserversConfig = serde_yaml::from_str(yaml).unwrap();
            assert!(config.validate().is_err(), "{yaml}");
        }
        let config: ObserversConfig = serde_yaml::from_str("owner_ack_timeout_ms: 0").unwrap();
        assert!(config.validate().is_ok());
        assert_eq!(
            config.warnings(),
            vec!["observers.owner_ack_timeout_ms is 0"]
        );
    }
}
