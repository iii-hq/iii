// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.

//! Wire ABI pin for sandbox::* error codes.
//!
//! Loads `tests/fixtures/sandbox_error_codes.json` and asserts that every
//! row in the fixture matches what `SandboxError::*` actually emits via
//! `to_payload()`. The fixture is the single source of truth shared across
//! Rust and the Node + Python SDKs — if you change the Rust enum without
//! updating the fixture (or vice versa), this test fails.
//!
//! Companion check: this test also asserts every `SandboxErrorCode` variant
//! has a fixture entry (no Rust drift), and every fixture entry has a
//! corresponding constructor exercised below (no fixture drift).

use std::collections::BTreeMap;

use serde_json::Value;

use iii_worker::sandbox_daemon::errors::{SandboxError, SandboxErrorCode};

const FIXTURE_PATH: &str = "tests/fixtures/sandbox_error_codes.json";

#[derive(Debug, serde::Deserialize)]
struct CodeFixture {
    code: String,
    #[serde(rename = "type")]
    error_type: String,
    retryable: bool,
    rust_variant: String,
    summary: String,
}

#[derive(Debug, serde::Deserialize)]
struct FixtureFile {
    version: u32,
    docs_base: String,
    codes: Vec<CodeFixture>,
}

fn load_fixture() -> FixtureFile {
    let raw = std::fs::read_to_string(FIXTURE_PATH).unwrap_or_else(|e| {
        panic!("could not read {FIXTURE_PATH}: {e} (run `cargo test` from the workspace root)")
    });
    serde_json::from_str(&raw).expect("fixture must be valid JSON")
}

/// One representative SandboxError per code — used to drive `.code()` /
/// `.to_payload()` and check the payload matches the fixture row.
fn representative(code_str: &str) -> Option<SandboxError> {
    Some(match code_str {
        "S001" => SandboxError::InvalidRequest("x".into()),
        "S002" => SandboxError::NotFound("x".into()),
        "S003" => SandboxError::ConcurrentExec("x".into()),
        "S004" => SandboxError::AlreadyStopped("x".into()),
        "S100" => SandboxError::image_not_in_catalog("x"),
        "S101" => SandboxError::RootfsMissing { image: "x".into() },
        "S102" => SandboxError::auto_install_failed("x", "y"),
        "S200" => SandboxError::exec_timed_out(1),
        "S210" => SandboxError::FsInvalidRequest("x".into()),
        "S211" => SandboxError::FsNotFound { path: "x".into() },
        "S212" => SandboxError::FsWrongType { path: "x".into() },
        "S213" => SandboxError::FsAlreadyExists { path: "x".into() },
        "S214" => SandboxError::FsNotEmpty { path: "x".into() },
        "S215" => SandboxError::FsPermission("x".into()),
        "S216" => SandboxError::FsIo("x".into()),
        "S217" => SandboxError::FsRegex("x".into()),
        "S218" => SandboxError::FsChannelAborted("x".into()),
        "S219" => SandboxError::FsUnsupported,
        "S300" => SandboxError::BootFailed("x".into()),
        "S400" => SandboxError::ResourceLimit("x".into()),
        _ => return None,
    })
}

#[test]
fn fixture_version_is_supported() {
    let f = load_fixture();
    assert_eq!(f.version, 1, "test asserts against fixture version 1");
    assert_eq!(f.docs_base, "https://docs.iii.dev/errors/sandbox/");
}

#[test]
fn every_fixture_row_matches_rust_to_payload() {
    let f = load_fixture();
    for row in &f.codes {
        let err = representative(&row.code).unwrap_or_else(|| {
            panic!(
                "fixture has code {} ({}) with no representative SandboxError; \
                 add one in `representative()` if a new variant landed",
                row.code, row.rust_variant
            )
        });
        assert_eq!(
            err.code().as_str(),
            row.code,
            "fixture says variant {} -> {} but Rust emits {} for {:?}",
            row.rust_variant,
            row.code,
            err.code().as_str(),
            err
        );

        let payload = err.to_payload();
        assert_eq!(payload["code"], Value::String(row.code.clone()));
        assert_eq!(payload["type"], Value::String(row.error_type.clone()));
        assert_eq!(payload["retryable"], Value::Bool(row.retryable));
        let docs_url = payload["docs_url"]
            .as_str()
            .expect("docs_url must be a string");
        assert!(
            docs_url.ends_with(&row.code),
            "docs_url {docs_url} must end with code {}",
            row.code
        );
        assert!(
            !row.summary.is_empty(),
            "fixture summary must not be empty for {}",
            row.code
        );
    }
}

/// Compile-time exhaustiveness guard. If a new `SandboxErrorCode` variant
/// is added to `errors.rs`, this match fails to compile, forcing the
/// developer to extend `ALL_VARIANTS`, this match, and the JSON fixture
/// in lockstep. Without this, a hardcoded-list test would silently miss
/// new variants.
fn enforce_exhaustive(c: SandboxErrorCode) -> SandboxErrorCode {
    match c {
        SandboxErrorCode::S001
        | SandboxErrorCode::S002
        | SandboxErrorCode::S003
        | SandboxErrorCode::S004
        | SandboxErrorCode::S100
        | SandboxErrorCode::S101
        | SandboxErrorCode::S102
        | SandboxErrorCode::S200
        | SandboxErrorCode::S210
        | SandboxErrorCode::S211
        | SandboxErrorCode::S212
        | SandboxErrorCode::S213
        | SandboxErrorCode::S214
        | SandboxErrorCode::S215
        | SandboxErrorCode::S216
        | SandboxErrorCode::S217
        | SandboxErrorCode::S218
        | SandboxErrorCode::S219
        | SandboxErrorCode::S300
        | SandboxErrorCode::S400 => c,
    }
}

const ALL_VARIANTS: &[SandboxErrorCode] = &[
    SandboxErrorCode::S001,
    SandboxErrorCode::S002,
    SandboxErrorCode::S003,
    SandboxErrorCode::S004,
    SandboxErrorCode::S100,
    SandboxErrorCode::S101,
    SandboxErrorCode::S102,
    SandboxErrorCode::S200,
    SandboxErrorCode::S210,
    SandboxErrorCode::S211,
    SandboxErrorCode::S212,
    SandboxErrorCode::S213,
    SandboxErrorCode::S214,
    SandboxErrorCode::S215,
    SandboxErrorCode::S216,
    SandboxErrorCode::S217,
    SandboxErrorCode::S218,
    SandboxErrorCode::S219,
    SandboxErrorCode::S300,
    SandboxErrorCode::S400,
];

#[test]
fn every_rust_code_has_a_fixture_row() {
    let f = load_fixture();
    let mut by_code: BTreeMap<&str, &CodeFixture> = BTreeMap::new();
    for row in &f.codes {
        by_code.insert(row.code.as_str(), row);
    }
    for &code in ALL_VARIANTS {
        let code = enforce_exhaustive(code);
        assert!(
            by_code.contains_key(code.as_str()),
            "Rust SandboxErrorCode::{} has no fixture row in {FIXTURE_PATH}; \
             add it to the fixture, ALL_VARIANTS, AND enforce_exhaustive() to keep \
             the contract pinned",
            code.as_str()
        );
    }
}
