// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Whether this process runs on a CI runner.
//!
//! Compose tells the registry, so a CI install is counted as one, and the
//! engine's telemetry stays off. One list serves both: the engine re-exports
//! it from `iii::workers::telemetry::environment`.

/// Environment variables that mark a CI runner. Public so a test that needs to
/// look like a developer machine can clear the whole set instead of keeping its
/// own copy, which would drift and pass locally while failing on a runner.
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

/// True when any of [`CI_ENV_VARS`] is set, to any value.
pub fn is_ci_environment() -> bool {
    CI_ENV_VARS
        .iter()
        .any(|var| std::env::var_os(var).is_some())
}
