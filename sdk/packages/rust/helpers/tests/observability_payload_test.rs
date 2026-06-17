use iii_helpers::observability::{REDACTED_PLACEHOLDER, redact};

#[test]
fn redact_replaces_sensitive_keys() {
    let input = serde_json::json!({ "password": "hunter2", "name": "ok" });
    let out = redact(&input);
    assert_eq!(out["password"], REDACTED_PLACEHOLDER);
    assert_eq!(out["name"], "ok");
}
