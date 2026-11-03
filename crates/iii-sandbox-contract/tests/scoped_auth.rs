//! The scoped `worker::*` token (contract 1.2) against independently computed vectors; the
//! adapter checks its own copy against the same fixture.

use iii_sandbox_contract::{scoped_auth_matches, scoped_auth_token};
use serde::Deserialize;

#[derive(Deserialize)]
struct Vector {
    project_token: String,
    state_key: String,
    token: String,
}

fn vectors() -> Vec<Vector> {
    serde_json::from_str(include_str!("fixtures/scoped_auth.json")).expect("fixture parses")
}

#[test]
fn scoped_tokens_match_the_fixture() {
    for vector in vectors() {
        assert_eq!(
            scoped_auth_token(&vector.project_token, &vector.state_key),
            vector.token,
            "state key {}",
            vector.state_key
        );
    }
}

#[test]
fn a_scoped_token_matches_only_its_own_vm_and_project() {
    for vector in vectors() {
        assert!(scoped_auth_matches(
            &vector.project_token,
            &vector.state_key,
            &vector.token
        ));
        let other_vm = format!("{}0", vector.state_key);
        assert!(!scoped_auth_matches(
            &vector.project_token,
            &other_vm,
            &vector.token
        ));
        let other_project = format!("{}x", vector.project_token);
        assert!(!scoped_auth_matches(
            &other_project,
            &vector.state_key,
            &vector.token
        ));
    }
}

#[test]
fn a_malformed_or_truncated_token_never_matches() {
    let vector = &vectors()[0];
    for presented in ["", "not-hex", &vector.token[..10], &vector.project_token] {
        assert!(
            !scoped_auth_matches(&vector.project_token, &vector.state_key, presented),
            "{presented:?}"
        );
    }
}
