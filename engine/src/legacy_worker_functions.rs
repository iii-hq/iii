// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

const WORKERS_TO_COMPOSE_GUIDE: &str = "https://iii.dev/docs/upgrading/workers-to-compose";

/// Explain how to migrate a removed `worker::*` function.
///
/// This module is compiled into both the engine library and the `iii` binary.
/// Keeping the message here makes SDK invocation errors and dynamic CLI help
/// point at the same replacement without exposing a new public API.
pub(crate) fn migration_message(function_id: &str) -> Option<String> {
    let replacement = match function_id {
        "worker::add" => Some("compose::add"),
        "worker::remove" => Some("compose::remove"),
        "worker::update" => Some("compose::update"),
        "worker::start" => Some("compose::up"),
        // compose::stop stops the whole daemon, so compose::down is the safe
        // lifecycle replacement for stopping a project or one container.
        "worker::stop" => Some("compose::down"),
        // compose::list lists projects. compose::status is the replacement
        // that reports the containers belonging to one project.
        "worker::list" => Some("compose::status"),
        "worker::schema" => Some("compose::schema"),
        "worker::status" => Some("compose::status"),
        "worker::validate" => Some("compose::validate"),
        "worker::clear" | "worker::logs" => None,
        _ => return None,
    };

    let replacement = match replacement {
        Some(replacement) => format!(" Use {replacement} instead."),
        None => " There is no direct compose::* replacement.".to_string(),
    };

    Some(format!(
        "Function {function_id} was removed in iii 0.23.{replacement} Migration guide: \
         {WORKERS_TO_COMPOSE_GUIDE}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_removed_worker_function_has_a_migration_message() {
        for function_id in [
            "worker::add",
            "worker::remove",
            "worker::update",
            "worker::start",
            "worker::stop",
            "worker::list",
            "worker::clear",
            "worker::logs",
            "worker::schema",
            "worker::status",
            "worker::validate",
        ] {
            let message = migration_message(function_id).expect("removed function needs a hint");
            assert!(message.contains(function_id));
            assert!(message.contains(WORKERS_TO_COMPOSE_GUIDE));
        }
    }

    #[test]
    fn replacement_is_only_claimed_when_compose_has_one() {
        assert!(
            migration_message("worker::add")
                .unwrap()
                .contains("Use compose::add instead")
        );
        assert!(
            migration_message("worker::stop")
                .unwrap()
                .contains("Use compose::down instead")
        );
        assert!(
            migration_message("worker::logs")
                .unwrap()
                .contains("no direct compose::* replacement")
        );
        assert!(migration_message("worker::unknown").is_none());
        assert!(migration_message("orders::add").is_none());
    }
}
