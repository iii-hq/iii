// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

const WORKERS_TO_COMPOSE_GUIDE: &str = "https://iii.dev/docs/upgrading/workers-to-compose";

/// Kept in sync with `crate::deprecation::STREAM_MIGRATION_GUIDE` (a lib test
/// asserts it); duplicated because this module is also compiled into the
/// `iii` binary, which does not include the deprecation module.
const STREAM_MIGRATION_GUIDE: &str = "https://iii.dev/docs/upgrading/migrate-from-streams";

/// Function ids registered by the deprecated iii-stream engine worker.
const STREAM_FUNCTIONS: &[&str] = &[
    "stream::set",
    "stream::get",
    "stream::delete",
    "stream::list",
    "stream::list_groups",
    "stream::list_all",
    "stream::send",
    "stream::update",
];

/// Explain why a deprecated `stream::*` function is missing (iii-stream is not
/// running), how to enable it, and where to migrate.
fn stream_unavailable_message(function_id: &str) -> Option<String> {
    if !STREAM_FUNCTIONS.contains(&function_id) {
        return None;
    }
    Some(format!(
        "Function {function_id} is not available: the iii-stream worker is not running. To keep \
         using it, enable iii-stream in the engine config.yaml (`workers:` entry `- name: \
         iii-stream`) or under `engine.workers.iii-stream` in worker-compose.yaml. \
         {function_id} is deprecated (iii-stream) and will be removed in a future release \
         (version TBD). Behavior is unchanged for now. Migration guide: \
         {STREAM_MIGRATION_GUIDE}"
    ))
}

/// Explain how to migrate a removed `worker::*` function, or how to enable or
/// migrate away from a deprecated `stream::*` function that is not running.
///
/// This module is compiled into both the engine library and the `iii` binary.
/// Keeping the message here makes SDK invocation errors and dynamic CLI help
/// point at the same replacement without exposing a new public API.
pub(crate) fn migration_message(function_id: &str) -> Option<String> {
    if let Some(message) = stream_unavailable_message(function_id) {
        return Some(message);
    }

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

    #[test]
    fn missing_stream_functions_explain_iii_stream_and_link_the_guide() {
        for function_id in STREAM_FUNCTIONS {
            let message = migration_message(function_id).expect("stream function needs a hint");
            assert!(message.contains(function_id), "{message}");
            assert!(
                message.contains("the iii-stream worker is not running"),
                "{message}"
            );
            assert!(message.contains("engine.workers.iii-stream"), "{message}");
            assert!(message.contains("- name: iii-stream"), "{message}");
            assert!(
                message.contains(&format!(
                    "{function_id} is deprecated (iii-stream) and will be removed in a future \
                     release (version TBD). Behavior is unchanged for now. Migration guide: \
                     {STREAM_MIGRATION_GUIDE}"
                )),
                "{message}"
            );
            assert!(!message.contains("compose::add"), "{message}");
        }
        // Custom per-stream functions and unrelated ids are not rewritten.
        assert!(migration_message("stream::set(orders)").is_none());
        assert!(migration_message("stream::unknown").is_none());
    }
}
