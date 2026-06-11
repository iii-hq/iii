// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Integration tests for the `worker::*` trigger surface — everything from
//! `tmp/test-worker-cli/test.sh` that doesn't require a live engine. Lives
//! here (not as a unit test under `core/`) so the assertions speak to the
//! public API agents will see at runtime.
//!
//! Excluded by design (need a live engine + daemon): registry pulls, OCI
//! pulls, CLI ↔ trigger parity, liveness post-fuzz, unknown function_id
//! engine-side rejection.

use iii_worker::cli::host_shim::{
    classify_handler_error, resolve_clear_targets, resolve_remove_targets,
};
use iii_worker::cli::worker_manager_daemon::{
    bad_request_payload, err_payload, op_description, op_metadata,
};
use iii_worker::core::{
    AddOptions, ClearOptions, ListOptions, RemoveOptions, StartOptions, StopOptions, UpdateOptions,
    WorkerOpError, WorkerOpErrorKind, WorkerSource,
};
use schemars::schema_for;
use serde_json::{Value, json};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Parse the wire envelope an SDK consumer would see. Returns `(code, type, details)`.
fn parse_envelope(envelope: &str) -> (String, String, Value) {
    let v: Value = serde_json::from_str(envelope).unwrap_or_else(|e| {
        panic!("envelope is not JSON: {e}\n---\n{envelope}");
    });
    let code = v
        .get("code")
        .and_then(|c| c.as_str())
        .unwrap_or_default()
        .to_string();
    let type_ = v
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or_default()
        .to_string();
    let details = v.get("details").cloned().unwrap_or(Value::Null);
    (code, type_, details)
}

/// Try to deserialize `payload` as `T`. On failure, return the W105 envelope
/// the daemon would emit. Mirrors `register_*` handler body.
fn try_deserialize<T: serde::de::DeserializeOwned>(op: &str, payload: Value) -> Result<T, String> {
    serde_json::from_value(payload).map_err(|e| bad_request_payload(op, &e))
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. WorkerSource adversarial serde — every malformed shape lands on W105
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn worker_source_missing_kind_is_w105() {
    let err =
        try_deserialize::<AddOptions>("worker::add", json!({"source": {"name": "x"}})).unwrap_err();
    let (code, type_, details) = parse_envelope(&err);
    assert_eq!(code, "W105");
    assert_eq!(type_, "WorkerOpError");
    assert_eq!(details["function_id"], "worker::add");
    assert!(
        details["reason"].as_str().unwrap().contains("kind"),
        "details.reason mentions the missing field"
    );
}

#[test]
fn worker_source_unknown_kind_is_w105() {
    let err = try_deserialize::<AddOptions>(
        "worker::add",
        json!({"source": {"kind": "magic", "name": "x"}}),
    )
    .unwrap_err();
    let (code, _, _) = parse_envelope(&err);
    assert_eq!(code, "W105");
}

#[test]
fn worker_source_capitalized_kind_is_w105() {
    // Tag enum is `rename_all = "snake_case"` — Registry must be rejected.
    let err = try_deserialize::<AddOptions>(
        "worker::add",
        json!({"source": {"kind": "Registry", "name": "x"}}),
    )
    .unwrap_err();
    let (code, _, _) = parse_envelope(&err);
    assert_eq!(code, "W105");
}

#[test]
fn worker_source_registry_without_name_is_w105() {
    let err = try_deserialize::<AddOptions>("worker::add", json!({"source": {"kind": "registry"}}))
        .unwrap_err();
    let (code, _, details) = parse_envelope(&err);
    assert_eq!(code, "W105");
    assert!(details["reason"].as_str().unwrap().contains("name"));
}

#[test]
fn worker_source_oci_without_reference_is_w105() {
    let err = try_deserialize::<AddOptions>("worker::add", json!({"source": {"kind": "oci"}}))
        .unwrap_err();
    let (code, _, details) = parse_envelope(&err);
    assert_eq!(code, "W105");
    assert!(details["reason"].as_str().unwrap().contains("reference"));
}

#[test]
fn worker_source_oci_with_name_field_is_w105() {
    // `name` is the registry-variant field; OCI variant requires `reference`.
    let err = try_deserialize::<AddOptions>(
        "worker::add",
        json!({"source": {"kind": "oci", "name": "x"}}),
    )
    .unwrap_err();
    let (code, _, details) = parse_envelope(&err);
    assert_eq!(code, "W105");
    assert!(details["reason"].as_str().unwrap().contains("reference"));
}

#[test]
fn worker_source_local_without_path_is_w105() {
    let err = try_deserialize::<AddOptions>("worker::add", json!({"source": {"kind": "local"}}))
        .unwrap_err();
    let (code, _, details) = parse_envelope(&err);
    assert_eq!(code, "W105");
    assert!(details["reason"].as_str().unwrap().contains("path"));
}

#[test]
fn add_payload_with_no_source_is_w105() {
    let err = try_deserialize::<AddOptions>("worker::add", json!({})).unwrap_err();
    let (code, _, details) = parse_envelope(&err);
    assert_eq!(code, "W105");
    assert!(details["reason"].as_str().unwrap().contains("source"));
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Type strictness — wrong primitive types map to W105
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn source_as_string_is_w105() {
    let err =
        try_deserialize::<AddOptions>("worker::add", json!({"source": "iii-state"})).unwrap_err();
    assert_eq!(parse_envelope(&err).0, "W105");
}

#[test]
fn yes_as_string_is_w105() {
    let err = try_deserialize::<StopOptions>(
        "worker::stop",
        json!({"name": "image-resize", "yes": "true"}),
    )
    .unwrap_err();
    assert_eq!(parse_envelope(&err).0, "W105");
}

#[test]
fn names_as_string_is_w105() {
    let err =
        try_deserialize::<RemoveOptions>("worker::remove", json!({"names": "x", "yes": true}))
            .unwrap_err();
    assert_eq!(parse_envelope(&err).0, "W105");
}

#[test]
fn all_as_string_is_w105() {
    let err = try_deserialize::<ClearOptions>("worker::clear", json!({"all": "yes", "yes": true}))
        .unwrap_err();
    assert_eq!(parse_envelope(&err).0, "W105");
}

#[test]
fn wait_as_number_is_w105() {
    let err = try_deserialize::<AddOptions>(
        "worker::add",
        json!({"source": {"kind": "registry", "name": "x"}, "wait": 1}),
    )
    .unwrap_err();
    assert_eq!(parse_envelope(&err).0, "W105");
}

#[test]
fn registry_name_null_is_w105() {
    let err = try_deserialize::<AddOptions>(
        "worker::add",
        json!({"source": {"kind": "registry", "name": null}}),
    )
    .unwrap_err();
    assert_eq!(parse_envelope(&err).0, "W105");
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. RemoveOptions / ClearOptions consent + ambiguity (W103/W104)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn remove_without_yes_is_w104() {
    let opts = RemoveOptions {
        names: vec!["x".into()],
        all: false,
        yes: false,
    };
    let err = resolve_remove_targets(&opts).unwrap_err();
    assert_eq!(err.kind(), WorkerOpErrorKind::ConsentRequired);
    let payload = err_payload(&err);
    let (code, type_, details) = parse_envelope(&payload);
    assert_eq!(code, "W104");
    assert_eq!(type_, "WorkerOpError");
    assert_eq!(details["op"], "remove");
}

#[test]
fn clear_without_yes_is_w104() {
    let opts = ClearOptions {
        names: vec!["x".into()],
        all: false,
        yes: false,
    };
    let err = resolve_clear_targets(&opts).unwrap_err();
    assert_eq!(err.kind(), WorkerOpErrorKind::ConsentRequired);
    assert_eq!(parse_envelope(&err_payload(&err)).0, "W104");
}

#[test]
fn remove_empty_payload_is_w104() {
    // {} → all defaults → consent comes first.
    let opts = RemoveOptions {
        names: vec![],
        all: false,
        yes: false,
    };
    let err = resolve_remove_targets(&opts).unwrap_err();
    assert_eq!(err.kind(), WorkerOpErrorKind::ConsentRequired);
}

#[test]
fn remove_yes_only_is_w103_missing_target() {
    let opts = RemoveOptions {
        names: vec![],
        all: false,
        yes: true,
    };
    let err = resolve_remove_targets(&opts).unwrap_err();
    assert_eq!(err.kind(), WorkerOpErrorKind::MissingTarget);
    let (code, _, details) = parse_envelope(&err_payload(&err));
    assert_eq!(code, "W103");
    assert_eq!(details["op"], "remove");
}

#[test]
fn clear_yes_only_is_w103_missing_target() {
    let opts = ClearOptions {
        names: vec![],
        all: false,
        yes: true,
    };
    let err = resolve_clear_targets(&opts).unwrap_err();
    assert_eq!(err.kind(), WorkerOpErrorKind::MissingTarget);
    assert_eq!(parse_envelope(&err_payload(&err)).0, "W103");
}

#[test]
fn remove_all_plus_names_is_w103_ambiguous() {
    let opts = RemoveOptions {
        names: vec!["x".into()],
        all: true,
        yes: true,
    };
    let err = resolve_remove_targets(&opts).unwrap_err();
    assert_eq!(err.kind(), WorkerOpErrorKind::MissingTarget);
    assert_eq!(parse_envelope(&err_payload(&err)).0, "W103");
}

#[test]
fn clear_all_plus_names_is_w103_ambiguous() {
    let opts = ClearOptions {
        names: vec!["x".into()],
        all: true,
        yes: true,
    };
    let err = resolve_clear_targets(&opts).unwrap_err();
    assert_eq!(err.kind(), WorkerOpErrorKind::MissingTarget);
    assert_eq!(parse_envelope(&err_payload(&err)).0, "W103");
}

#[test]
fn remove_names_with_yes_succeeds() {
    let opts = RemoveOptions {
        names: vec!["a".into(), "b".into()],
        all: false,
        yes: true,
    };
    let resolved = resolve_remove_targets(&opts).unwrap();
    assert_eq!(resolved, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn clear_all_with_yes_succeeds() {
    let opts = ClearOptions {
        names: vec![],
        all: true,
        yes: true,
    };
    // Resolves to empty Vec (caller interprets empty + all=true as "wipe everything").
    let resolved = resolve_clear_targets(&opts).unwrap();
    assert!(resolved.is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. classify_handler_error — stderr lifting to typed errors
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn classify_invalid_name_lifts_to_w100() {
    let stderr = "error: Worker name 'foo;rm -rf /' contains invalid characters\n";
    let err = classify_handler_error(1, stderr, "add", "foo;rm -rf /");
    assert_eq!(err.kind(), WorkerOpErrorKind::InvalidName);
    let (code, _, details) = parse_envelope(&err_payload(&err));
    assert_eq!(code, "W100");
    assert_eq!(details["name"], "foo;rm -rf /");
}

#[test]
fn classify_not_found_lifts_to_w110() {
    let stderr = "error: Worker 'pdfkit' not found in registry\n";
    let err = classify_handler_error(1, stderr, "add", "pdfkit");
    assert_eq!(err.kind(), WorkerOpErrorKind::NotFound);
    let (code, _, details) = parse_envelope(&err_payload(&err));
    assert_eq!(code, "W110");
    assert_eq!(details["name"], "pdfkit");
}

#[test]
fn classify_unknown_failure_is_w900_without_rc_leak() {
    let stderr = "error: HTTP 503 service unavailable\n";
    let err = classify_handler_error(2, stderr, "add", "anything");
    assert_eq!(err.kind(), WorkerOpErrorKind::Internal);
    let (code, _, _) = parse_envelope(&err_payload(&err));
    assert_eq!(code, "W900");
    // The message must NOT carry the rc — that's the agent-DX contract.
    let msg = err.to_string();
    assert!(
        !msg.contains("(rc "),
        "Internal message must not leak rc: {msg}"
    );
}

#[test]
fn classify_strips_internal_prefix() {
    // The CLI handler may emit `internal: Worker 'x' contains invalid chars`.
    // The classifier must strip `internal:` before lifting to W100.
    let stderr = "internal: Worker name 'x' contains invalid characters\n";
    let err = classify_handler_error(1, stderr, "stop", "x");
    let payload = err_payload(&err);
    assert!(
        !payload.contains("\"internal:"),
        "payload should not leak the 'internal:' prefix: {payload}"
    );
    assert_eq!(parse_envelope(&payload).0, "W100");
}

#[test]
fn classify_ansi_stripped_from_payload() {
    // Captured stderr often has color escapes from `colored`. classify_handler_error
    // must strip them before placing the line in the error envelope.
    let stderr = "\u{1b}[31merror:\u{1b}[0m Worker 'pdfkit' not found\n";
    let err = classify_handler_error(1, stderr, "add", "pdfkit");
    let payload = err_payload(&err);
    assert!(
        !payload.contains("\u{1b}["),
        "payload should not contain ANSI escapes: {payload:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. WorkerOpError → wire envelope completeness for every variant
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn every_kind_has_distinct_code() {
    use WorkerOpErrorKind::*;
    let kinds = [
        InvalidName,
        InvalidSource,
        LocalPathNotAllowedViaTrigger,
        MissingTarget,
        ConsentRequired,
        BadRequest,
        NotFound,
        AlreadyExists,
        NotInstalled,
        NotRunning,
        AlreadyRunning,
        LockBusy,
        LockIo,
        ConfigIo,
        ConfigParse,
        Registry,
        OciPull,
        Download,
        LockfileMismatch,
        Spawn,
        StartTimeout,
        StopTimeout,
        Cancelled,
        Internal,
    ];
    let codes: Vec<&'static str> = kinds.iter().map(|k| k.code()).collect();
    let mut sorted = codes.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        codes.len(),
        sorted.len(),
        "duplicate W-codes detected: {codes:?}"
    );
    for code in &codes {
        assert!(
            code.starts_with('W') && code.len() == 4,
            "code {code:?} must be Wxxx"
        );
    }
}

// W102 is reserved: local-path installs are now allowed over the trigger,
// so nothing produces this variant in practice. This pins its wire envelope
// shape in case the code is ever re-used.
#[test]
fn local_path_not_allowed_via_trigger_envelope_carries_path() {
    let err = WorkerOpError::LocalPathNotAllowedViaTrigger {
        path: "/tmp/foo".into(),
    };
    let (code, type_, details) = parse_envelope(&err_payload(&err));
    assert_eq!(code, "W102");
    assert_eq!(type_, "WorkerOpError");
    assert_eq!(details["path"], "/tmp/foo");
}

#[test]
fn not_found_envelope_carries_name() {
    let err = WorkerOpError::NotFound {
        name: "definitely-does-not-exist".into(),
    };
    let (code, _, details) = parse_envelope(&err_payload(&err));
    assert_eq!(code, "W110");
    assert_eq!(details["name"], "definitely-does-not-exist");
}

#[test]
fn already_running_envelope_carries_name_and_pid() {
    let err = WorkerOpError::AlreadyRunning {
        name: "pdfkit".into(),
        pid: 42,
    };
    let (code, _, details) = parse_envelope(&err_payload(&err));
    assert_eq!(code, "W114");
    assert_eq!(details["name"], "pdfkit");
    assert_eq!(details["pid"], 42);
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. JSON Schema completeness — what `worker::schema` exposes to agents
// ─────────────────────────────────────────────────────────────────────────────

/// Walks a schema's `properties` and returns the names of fields lacking a `description`.
fn fields_missing_description(schema_json: &Value) -> Vec<String> {
    let mut missing = Vec::new();
    let Some(props) = schema_json.get("properties").and_then(|p| p.as_object()) else {
        return missing;
    };
    for (name, def) in props {
        if def.get("description").is_none() {
            missing.push(name.clone());
        }
    }
    missing
}

#[test]
fn add_options_every_field_has_description() {
    let schema = serde_json::to_value(schema_for!(AddOptions)).unwrap();
    let missing = fields_missing_description(&schema);
    assert!(
        missing.is_empty(),
        "AddOptions fields missing description: {missing:?}"
    );
}

#[test]
fn remove_options_every_field_has_description() {
    let schema = serde_json::to_value(schema_for!(RemoveOptions)).unwrap();
    let missing = fields_missing_description(&schema);
    assert!(
        missing.is_empty(),
        "RemoveOptions fields missing description: {missing:?}"
    );
}

#[test]
fn logs_options_every_field_has_description() {
    let schema = serde_json::to_value(schema_for!(iii_worker::core::LogsOptions)).unwrap();
    let missing = fields_missing_description(&schema);
    assert!(
        missing.is_empty(),
        "LogsOptions fields missing description: {missing:?}"
    );
}

#[test]
fn logs_outcome_every_field_has_description() {
    let schema = serde_json::to_value(schema_for!(iii_worker::core::LogsOutcome)).unwrap();
    let missing = fields_missing_description(&schema);
    assert!(
        missing.is_empty(),
        "LogsOutcome fields missing description: {missing:?}"
    );
}

#[test]
fn status_options_every_field_has_description() {
    let schema = serde_json::to_value(schema_for!(iii_worker::core::StatusOptions)).unwrap();
    let missing = fields_missing_description(&schema);
    assert!(
        missing.is_empty(),
        "StatusOptions fields missing description: {missing:?}"
    );
}

#[test]
fn status_outcome_every_field_has_description() {
    let schema = serde_json::to_value(schema_for!(iii_worker::core::StatusOutcome)).unwrap();
    let missing = fields_missing_description(&schema);
    assert!(
        missing.is_empty(),
        "StatusOutcome fields missing description: {missing:?}"
    );
}

#[test]
fn validate_options_every_field_has_description() {
    let schema = serde_json::to_value(schema_for!(
        iii_worker::cli::worker_manifest::ValidateOptions
    ))
    .unwrap();
    let missing = fields_missing_description(&schema);
    assert!(
        missing.is_empty(),
        "ValidateOptions fields missing description: {missing:?}"
    );
}

#[test]
fn manifest_report_every_field_has_description() {
    let schema = serde_json::to_value(schema_for!(
        iii_worker::cli::worker_manifest::ManifestReport
    ))
    .unwrap();
    let missing = fields_missing_description(&schema);
    assert!(
        missing.is_empty(),
        "ManifestReport fields missing description: {missing:?}"
    );
}

#[test]
fn worker_manifest_schema_is_closed_world_with_descriptions() {
    // The iii.worker.yaml schema served via worker::schema must reject
    // unknown keys, require `name`, and describe every field — it is the
    // authoring contract LLMs build manifests from.
    let schema = iii_worker::cli::worker_manifest::manifest_schema_json();
    assert_eq!(schema["additionalProperties"], serde_json::json!(false));
    assert_eq!(schema["required"], serde_json::json!(["name"]));
    let missing = fields_missing_description(&schema);
    assert!(
        missing.is_empty(),
        "WorkerManifest fields missing description: {missing:?}"
    );
}

#[test]
fn clear_options_every_field_has_description() {
    let schema = serde_json::to_value(schema_for!(ClearOptions)).unwrap();
    let missing = fields_missing_description(&schema);
    assert!(
        missing.is_empty(),
        "ClearOptions fields missing description: {missing:?}"
    );
}

#[test]
fn update_options_every_field_has_description() {
    let schema = serde_json::to_value(schema_for!(UpdateOptions)).unwrap();
    let missing = fields_missing_description(&schema);
    assert!(
        missing.is_empty(),
        "UpdateOptions fields missing description: {missing:?}"
    );
}

#[test]
fn start_options_every_field_has_description() {
    let schema = serde_json::to_value(schema_for!(StartOptions)).unwrap();
    let missing = fields_missing_description(&schema);
    assert!(
        missing.is_empty(),
        "StartOptions fields missing description: {missing:?}"
    );
}

#[test]
fn stop_options_every_field_has_description() {
    let schema = serde_json::to_value(schema_for!(StopOptions)).unwrap();
    let missing = fields_missing_description(&schema);
    assert!(
        missing.is_empty(),
        "StopOptions fields missing description: {missing:?}"
    );
}

#[test]
fn list_options_field_has_description() {
    let schema = serde_json::to_value(schema_for!(ListOptions)).unwrap();
    let missing = fields_missing_description(&schema);
    assert!(
        missing.is_empty(),
        "ListOptions fields missing description: {missing:?}"
    );
}

#[test]
fn worker_source_schema_has_three_kinds() {
    // Definition lives in either `definitions` (draft-07) or inline `oneOf` — try both.
    let schema = serde_json::to_value(schema_for!(WorkerSource)).unwrap();

    let oneof = schema
        .get("oneOf")
        .or_else(|| schema.pointer("/definitions/WorkerSource/oneOf"))
        .and_then(|v| v.as_array())
        .expect("WorkerSource schema exposes a oneOf with one branch per kind");

    let mut kinds: Vec<String> = oneof
        .iter()
        .filter_map(|branch| {
            branch
                .pointer("/properties/kind/enum/0")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect();
    kinds.sort();
    assert_eq!(
        kinds,
        vec![
            "local".to_string(),
            "oci".to_string(),
            "registry".to_string()
        ]
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. op_metadata table — every registered op declares timeout + idempotency
// ─────────────────────────────────────────────────────────────────────────────

const ALL_OPS: &[&str] = &[
    "worker::add",
    "worker::remove",
    "worker::update",
    "worker::start",
    "worker::stop",
    "worker::list",
    "worker::clear",
    "worker::logs",
    "worker::schema",
    "worker::status",
    "worker::validate",
];

#[test]
fn every_op_has_nonempty_description() {
    for op in ALL_OPS {
        assert!(
            !op_description(op).is_empty(),
            "{op} has an empty description — it would surface blank in \
             engine::functions::info and worker::schema"
        );
    }
}

#[test]
fn every_op_has_positive_timeout() {
    for op in ALL_OPS {
        let (timeout, _) = op_metadata(op);
        assert!(timeout > 0, "{op} has zero/negative timeout");
    }
}

#[test]
fn add_timeout_at_least_five_minutes() {
    // Registry pull + binary fetch routinely exceeds the SDK's 30s default.
    let (timeout, _) = op_metadata("worker::add");
    assert!(
        timeout >= 300_000,
        "worker::add timeout too short: {timeout}ms"
    );
}

#[test]
fn list_timeout_at_most_thirty_seconds() {
    let (timeout, _) = op_metadata("worker::list");
    assert!(
        timeout <= 30_000,
        "worker::list timeout too long: {timeout}ms"
    );
}

#[test]
fn read_only_ops_are_idempotent() {
    for op in [
        "worker::add",
        "worker::list",
        "worker::schema",
        "worker::clear",
        "worker::logs",
        "worker::status",
        "worker::validate",
    ] {
        let (_, idempotent) = op_metadata(op);
        assert!(idempotent, "{op} should be declared idempotent");
    }
}

#[test]
fn stateful_ops_are_not_idempotent() {
    for op in ["worker::start", "worker::stop"] {
        let (_, idempotent) = op_metadata(op);
        assert!(
            !idempotent,
            "{op} should NOT be declared idempotent (process lifecycle)"
        );
    }
}

#[test]
fn unknown_op_falls_back_to_safe_defaults() {
    let (timeout, idempotent) = op_metadata("worker::definitely-new-2027");
    assert!(timeout > 0 && timeout <= 60_000);
    assert!(!idempotent, "unknown ops default to non-idempotent (safer)");
}

// ─────────────────────────────────────────────────────────────────────────────
// 8. Forward / backward compat — unknown fields and omitted optionals
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn list_options_ignores_unknown_fields() {
    // serde defaults to ignoring unknown fields. Locks the contract.
    let opts: ListOptions = serde_json::from_value(json!({
        "running_only": false,
        "future_field_2027": "hello",
        "another": 42
    }))
    .unwrap();
    assert!(!opts.running_only);
}

#[test]
fn add_options_with_only_source_uses_defaults() {
    // force / reset_config / wait all have #[serde(default)] semantics.
    let opts: AddOptions = serde_json::from_value(json!({
        "source": {"kind": "registry", "name": "x"}
    }))
    .unwrap();
    assert!(!opts.force);
    assert!(!opts.reset_config);
    assert!(opts.wait, "wait defaults to true (block until ready)");
}

#[test]
fn update_options_empty_payload_means_update_all() {
    let opts: UpdateOptions = serde_json::from_value(json!({})).unwrap();
    assert!(opts.names.is_empty());
}

#[test]
fn list_options_null_or_missing_defaults_cleanly() {
    // The daemon's list handler uses unwrap_or_default — verify Default exists.
    let opts: ListOptions = Default::default();
    assert!(!opts.running_only);
}

#[test]
fn list_wire_path_option_wrapper_preserves_leniency_and_w105() {
    // The daemon's list handler deserializes `Option<ListOptions>` through
    // the SDK typed path and falls back to defaults. Pin the wire behavior:
    // null / {} / [] stay lenient, valid objects parse, and malformed shapes
    // produce the same W105 reason text the bare `ListOptions` path did.
    for payload in [json!(null), json!({}), json!([])] {
        let opts = try_deserialize::<Option<ListOptions>>("worker::list", payload.clone())
            .unwrap_or_else(|e| panic!("payload {payload} must stay lenient, got {e}"))
            .unwrap_or_default();
        assert!(!opts.running_only);
    }

    let opts =
        try_deserialize::<Option<ListOptions>>("worker::list", json!({"running_only": true}))
            .unwrap()
            .unwrap_or_default();
    assert!(opts.running_only);

    for payload in [
        json!({"running_only": "yes"}),
        json!("hello"),
        json!([1, 2]),
    ] {
        let bare_err = serde_json::from_value::<ListOptions>(payload.clone()).unwrap_err();
        let envelope = try_deserialize::<Option<ListOptions>>("worker::list", payload).unwrap_err();
        let (code, _, details) = parse_envelope(&envelope);
        assert_eq!(code, "W105");
        // `Option<T>` forwards non-null payloads to T's deserializer
        // verbatim, so the W105 reason matches the pre-typed-handler text.
        assert_eq!(
            details.get("reason").and_then(|r| r.as_str()),
            Some(bare_err.to_string().as_str())
        );
    }
}

#[test]
fn typed_handler_draft07_extraction_matches_schema_for_bytes() {
    // The daemon's request schemas now come from the SDK's typed-handler
    // auto-extraction (SchemaSettings::draft07), while worker::schema serves
    // schemars::schema_for! output. Pin that the two generators agree for
    // every options struct so the two introspection surfaces never drift.
    fn draft07<T: schemars::JsonSchema>() -> Value {
        serde_json::to_value(
            schemars::r#gen::SchemaSettings::draft07()
                .into_generator()
                .into_root_schema_for::<T>(),
        )
        .unwrap()
    }
    macro_rules! assert_schema_parity {
        ($($t:ty),+ $(,)?) => {$(
            assert_eq!(
                serde_json::to_value(schema_for!($t)).unwrap(),
                draft07::<$t>(),
                concat!("schema drift for ", stringify!($t)),
            );
        )+};
    }
    assert_schema_parity!(
        AddOptions,
        RemoveOptions,
        UpdateOptions,
        StartOptions,
        StopOptions,
        ClearOptions,
        ListOptions,
        iii_worker::core::LogsOptions,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 9. WorkerSource serde round-trips (happy path)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn worker_source_registry_round_trips_with_version() {
    let v = json!({"kind": "registry", "name": "pdfkit", "version": "1.0.0"});
    let parsed: WorkerSource = serde_json::from_value(v.clone()).unwrap();
    assert!(matches!(
        &parsed,
        WorkerSource::Registry { name, version }
            if name == "pdfkit" && version.as_deref() == Some("1.0.0")
    ));
    assert_eq!(serde_json::to_value(&parsed).unwrap(), v);
}

#[test]
fn worker_source_oci_round_trips() {
    let v = json!({"kind": "oci", "reference": "docker.io/andersonofl/todo-worker:latest"});
    let parsed: WorkerSource = serde_json::from_value(v.clone()).unwrap();
    assert!(matches!(
        &parsed,
        WorkerSource::Oci { reference } if reference == "docker.io/andersonofl/todo-worker:latest"
    ));
    assert_eq!(serde_json::to_value(&parsed).unwrap(), v);
}

#[test]
fn worker_source_local_round_trips() {
    let v = json!({"kind": "local", "path": "./my-worker"});
    let parsed: WorkerSource = serde_json::from_value(v.clone()).unwrap();
    assert!(
        matches!(&parsed, WorkerSource::Local { path } if path.to_str() == Some("./my-worker"))
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 10. bad_request_payload produces parseable W105 for arbitrary serde failures
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn bad_request_payload_is_always_valid_json() {
    let err = serde_json::from_value::<AddOptions>(json!({})).unwrap_err();
    let envelope = bad_request_payload("worker::add", &err);
    let parsed: Value = serde_json::from_str(&envelope).expect("envelope is JSON");
    assert_eq!(parsed["type"], "WorkerOpError");
    assert_eq!(parsed["code"], "W105");
    assert_eq!(parsed["details"]["function_id"], "worker::add");
    assert!(parsed["details"]["reason"].is_string());
}

#[test]
fn bad_request_payload_propagates_op_label() {
    let err = serde_json::from_value::<StopOptions>(json!({})).unwrap_err();
    let envelope = bad_request_payload("worker::stop", &err);
    let parsed: Value = serde_json::from_str(&envelope).unwrap();
    assert_eq!(parsed["details"]["function_id"], "worker::stop");
}

// ─────────────────────────────────────────────────────────────────────────────
// 11. Adversarial name handling at the type layer (W100 InvalidName)
// ─────────────────────────────────────────────────────────────────────────────
//
// These are smoke checks: bad names must deserialize (the JSON layer is
// non-judgmental) but the orchestrators / shim layers reject them with W100
// at run time. Live behavior is asserted by `tmp/test-worker-cli/test.sh`
// §14 + A7; the unit-level orchestrator tests cover empty-name.

#[test]
fn shell_metacharacter_name_deserializes_unaltered() {
    let opts: StopOptions =
        serde_json::from_value(json!({"name": "foo;rm -rf /", "yes": true})).unwrap();
    assert_eq!(opts.name, "foo;rm -rf /");
}

#[test]
fn unicode_emoji_rtl_name_deserializes_unaltered() {
    // \u{202e} is the RTL override codepoint — escaped here so Rust's
    // text_direction_codepoint_in_literal lint stays happy.
    let opts: StopOptions =
        serde_json::from_value(json!({"name": "emoji-\u{1F680}-rtl-\u{202e}", "yes": true}))
            .unwrap();
    assert!(opts.name.contains('\u{1F680}'));
    assert!(opts.name.contains('\u{202e}'));
}

#[test]
fn one_kib_name_deserializes() {
    let long: String = "a".repeat(1024);
    let opts: StopOptions =
        serde_json::from_value(json!({"name": long.clone(), "yes": true})).unwrap();
    assert_eq!(opts.name.len(), 1024);
}

#[test]
fn invalid_name_envelope_echoes_input() {
    // Direct construction asserts the envelope shape the daemon emits when
    // the orchestrator / shim layers reject a bad name.
    let err = WorkerOpError::InvalidName {
        name: "foo;rm -rf /".into(),
        reason: "contains shell metacharacters".into(),
    };
    let (code, type_, details) = parse_envelope(&err_payload(&err));
    assert_eq!(code, "W100");
    assert_eq!(type_, "WorkerOpError");
    assert_eq!(details["name"], "foo;rm -rf /");
    assert!(details["reason"].is_string());
}

// ─────────────────────────────────────────────────────────────────────────────
// 12. err_payload is robust to weird inputs
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn err_payload_for_internal_does_not_panic() {
    let err = WorkerOpError::Internal {
        message: "unexpected: \"quoted\" 'apostrophes' \\backslash".into(),
    };
    let payload = err_payload(&err);
    let _: Value = serde_json::from_str(&payload).expect("internal envelope is JSON");
}

#[test]
fn err_payload_includes_type_discriminator_for_every_variant() {
    // Spot-check several variants — every one must carry `"type": "WorkerOpError"`
    // so consumers can route on it without inspecting `code`.
    let variants = [
        WorkerOpError::Cancelled,
        WorkerOpError::ConsentRequired { op: "stop".into() },
        WorkerOpError::NotInstalled { name: "x".into() },
        WorkerOpError::Registry {
            message: "boom".into(),
        },
    ];
    for v in &variants {
        let parsed: Value = serde_json::from_str(&err_payload(v)).unwrap();
        assert_eq!(
            parsed["type"], "WorkerOpError",
            "{:?} envelope must carry type discriminator",
            v
        );
    }
}
