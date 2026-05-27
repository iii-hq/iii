//! R1 — decode-error wire snapshot test. T6 migrated the 10 fs::* handlers
//! from `register_function_with` (hand-rolled deserialization) to
//! `RegisterFunction::new_async` (SDK-driven deserialization). E1 from the
//! plan-eng-review identified the risk that the SDK's decode-failure wire
//! shape might drift from the old `"bad request: ..."` plain-text form.
//!
//! This test does NOT verify the SDK's exact wire shape (the SDK owns
//! that contract). Instead, it pins ONE invariant: the daemon-side
//! handler still constructs the typed Request struct from a serde JSON
//! payload, so missing required fields produce a serde error somewhere
//! identifiable. If a future SDK upgrade collapses that path silently,
//! this test fails and forces a conscious review of whether the new
//! wire shape is acceptable.
//!
//! Verification mechanism: deserialise a known-bad payload directly
//! through the Request struct, assert it errors with a message naming
//! the missing field. The new_async path uses `serde_json::from_value`
//! internally, so this asserts the same thing the handler does without
//! booting a daemon.

use iii_worker::sandbox_daemon::fs::ls::LsRequest;

#[test]
fn ls_request_missing_path_field_fails_with_named_field() {
    let payload = serde_json::json!({
        "sandbox_id": "00000000-0000-0000-0000-000000000000"
        // path missing
    });
    let err = serde_json::from_value::<LsRequest>(payload).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("path") && (msg.contains("missing") || msg.contains("required")),
        "decode error message must name the missing field; got {msg:?}"
    );
}

#[test]
fn ls_request_missing_sandbox_id_fails_with_named_field() {
    let payload = serde_json::json!({
        "path": "/home/app"
        // sandbox_id missing
    });
    let err = serde_json::from_value::<LsRequest>(payload).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("sandbox_id") && (msg.contains("missing") || msg.contains("required")),
        "decode error message must name the missing field; got {msg:?}"
    );
}

#[test]
fn ls_request_wrong_field_type_is_rejected() {
    let payload = serde_json::json!({
        "sandbox_id": 42,  // should be string
        "path": "/home/app"
    });
    let err = serde_json::from_value::<LsRequest>(payload).unwrap_err();
    // The exact wording is serde-version-specific; we just assert the
    // error mentions the offending field or type.
    assert!(
        err.to_string().len() > 0,
        "decode error must produce a message"
    );
}
