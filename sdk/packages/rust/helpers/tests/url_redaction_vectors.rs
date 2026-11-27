//! Runs the shared URL redaction vectors in `sdk/fixtures/redaction/`, which
//! the engine and every SDK run too.

use std::path::Path;

use iii_helpers::observability::{redact, redact_and_truncate};
use serde_json::{Value, json};

fn wrap(value: &Value) -> Value {
    json!({ "headers": { "location": value }, "links": [value] })
}

#[test]
fn redacts_url_credentials_from_the_shared_vectors() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../fixtures/redaction/url-credentials.json");
    let text = std::fs::read_to_string(&path).expect("read the shared vectors");
    let vectors: Value = serde_json::from_str(&text).expect("parse the shared vectors");
    let cases = vectors["cases"].as_array().expect("cases");
    assert!(
        cases.len() > 10,
        "{} cases in {}",
        cases.len(),
        path.display()
    );
    for case in cases {
        let note = case["note"].as_str().expect("note");
        let expected = wrap(&case["expect"]);
        assert_eq!(redact(&wrap(&case["value"])), expected, "{note}");
        let (serialized, _) = redact_and_truncate(&wrap(&case["value"]), None);
        assert_eq!(
            serde_json::from_str::<Value>(&serialized).expect("valid JSON"),
            expected,
            "{note}: serialized"
        );
        assert_eq!(redact(&expected), expected, "{note}: idempotent");
    }
}
