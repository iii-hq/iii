use iii_helpers::observability::{REDACTED_PLACEHOLDER, redact};

#[test]
fn redact_replaces_sensitive_keys() {
    let input = serde_json::json!({ "password": "hunter2", "name": "ok" });
    let out = redact(&input);
    assert_eq!(out["password"], REDACTED_PLACEHOLDER);
    assert_eq!(out["name"], "ok");
}

#[test]
fn redact_replaces_credentials_in_url_values() {
    let input = serde_json::json!({
        "status_code": 302,
        "headers": { "location": "https://h.example/key?X-Amz-Date=1&X-Amz-Signature=abc" },
        "links": ["https://deploy:hunter2@git.example.com/repo.git", "/workers?page=2"],
    });
    let out = redact(&input);
    assert_eq!(
        out["headers"]["location"],
        format!("https://h.example/key?X-Amz-Date=1&X-Amz-Signature={REDACTED_PLACEHOLDER}")
    );
    assert_eq!(
        out["links"][0],
        format!("https://deploy:{REDACTED_PLACEHOLDER}@git.example.com/repo.git")
    );
    assert_eq!(out["links"][1], "/workers?page=2");
    assert_eq!(redact(&out), out);
}
