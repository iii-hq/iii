// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

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
         {function_id} is deprecated (iii-stream) and will be removed in an upcoming release. \
         Behavior is unchanged for now. Migration guide: \
         {STREAM_MIGRATION_GUIDE}"
    ))
}

/// Explain how to enable or migrate away from a deprecated `stream::*`
/// function that is not running.
///
/// This module is compiled into both the engine library and the `iii` binary.
/// Keeping the message here makes SDK invocation errors and dynamic CLI help
/// point at the same replacement without exposing a new public API.
pub(crate) fn migration_message(function_id: &str) -> Option<String> {
    stream_unavailable_message(function_id)
}

#[cfg(test)]
mod tests {
    use super::*;

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
                    "{function_id} is deprecated (iii-stream) and will be removed in an upcoming \
                     release. Behavior is unchanged for now. Migration guide: \
                     {STREAM_MIGRATION_GUIDE}"
                )),
                "{message}"
            );
            assert!(!message.contains("compose::add"), "{message}");
        }
        // Custom per-stream functions and unrelated ids are not rewritten.
        assert!(migration_message("stream::set(orders)").is_none());
        assert!(migration_message("stream::unknown").is_none());
        assert!(migration_message("worker::start").is_none());
    }
}
