// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Product analytics policy shared by the engine, tools, and launchers.
//! This does not control OpenTelemetry observability.

use std::path::Path;

/// Presence of any of these variables disables product analytics, even when
/// the explicit enabled setting is true. Preserve the existing CI contract.
pub const CI_ENV_VARS: &[&str] = &[
    "CI",
    "GITHUB_ACTIONS",
    "GITLAB_CI",
    "CIRCLECI",
    "JENKINS_URL",
    "TRAVIS",
    "BUILDKITE",
    "TF_BUILD",
    "CODEBUILD_BUILD_ID",
    "BITBUCKET_BUILD_NUMBER",
    "DRONE",
    "TEAMCITY_VERSION",
];

pub fn is_falsey(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "false" | "0" | "no" | "off"
    )
}

pub fn env_opt_out() -> bool {
    std::env::var("III_TELEMETRY_ENABLED").is_ok_and(|value| is_falsey(&value))
}

pub fn is_ci_environment() -> bool {
    CI_ENV_VARS.iter().any(|key| std::env::var(key).is_ok())
}

fn dev_marker_exists(home: &Path) -> bool {
    home.join(".iii").join("telemetry_dev_optout").exists()
}

pub fn is_dev_optout() -> bool {
    std::env::var("III_TELEMETRY_DEV").ok().as_deref() == Some("true")
        || dev_marker_exists(&dirs::home_dir().unwrap_or_else(std::env::temp_dir))
}

/// Re-evaluate at the send or spawn boundary, so all producers honor the same
/// setting and launchers can materialize it in a cleared/guest environment.
pub fn is_telemetry_disabled() -> bool {
    env_opt_out() || is_ci_environment() || is_dev_optout()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_explicit_off_values() {
        for value in ["false", "0", "no", "off", " FALSE ", "Off", "\tNO\n"] {
            assert!(is_falsey(value), "{value:?}");
        }
        for value in ["", " ", "true", "1", "yes", "on", "disabled"] {
            assert!(!is_falsey(value), "{value:?}");
        }
    }

    #[test]
    fn persisted_developer_opt_out_is_distinct_from_telemetry_identity() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join(".iii")).unwrap();
        std::fs::write(home.path().join(".iii/telemetry.yaml"), "version: 2\n").unwrap();
        assert!(!dev_marker_exists(home.path()));
        std::fs::write(home.path().join(".iii/telemetry_dev_optout"), "").unwrap();
        assert!(dev_marker_exists(home.path()));
    }
}
