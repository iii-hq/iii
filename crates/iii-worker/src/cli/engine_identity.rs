// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::collections::HashMap;

pub const ENGINE_RUN_ID_ENV: &str = "III_ENGINE_RUN_ID";

pub fn engine_run_id() -> Option<String> {
    std::env::var(ENGINE_RUN_ID_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn managed_engine_url(engine_url: impl Into<String>) -> String {
    let run_id = engine_run_id();
    managed_engine_url_with_run_id(engine_url.into(), run_id.as_deref())
}

fn managed_engine_url_with_run_id(mut engine_url: String, run_id: Option<&str>) -> String {
    let Some(run_id) = run_id else {
        return engine_url;
    };

    let separator = if engine_url.contains('?') { '&' } else { '?' };
    engine_url.push(separator);
    engine_url.push_str("engine_run_id=");
    engine_url.push_str(&run_id);
    engine_url
}

pub fn insert_engine_run_id(env: &mut HashMap<String, String>) {
    if let Some(run_id) = engine_run_id() {
        env.insert(ENGINE_RUN_ID_ENV.to_string(), run_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_run_identity_and_preserves_existing_query() {
        assert_eq!(
            managed_engine_url_with_run_id("ws://host:49134".into(), Some("run-1")),
            "ws://host:49134?engine_run_id=run-1"
        );
        assert_eq!(
            managed_engine_url_with_run_id("ws://host:49134?token=x".into(), Some("run-1")),
            "ws://host:49134?token=x&engine_run_id=run-1"
        );
        assert_eq!(
            managed_engine_url_with_run_id("ws://host:49134".into(), None),
            "ws://host:49134"
        );
    }
}
