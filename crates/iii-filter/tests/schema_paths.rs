//! `Filter::check_schema_paths` (filters.md §4).

use iii_filter::{ErrorCode, Filter};
use serde_json::{Value, json};

fn filter(src: Value) -> Filter {
    Filter::parse(&src).expect("valid filter")
}

fn check(
    src: Value,
    request: Option<Value>,
    response: Option<Value>,
) -> Result<Vec<String>, String> {
    filter(src)
        .check_schema_paths(request.as_ref(), response.as_ref())
        .map_err(|e| {
            assert_eq!(e.code, ErrorCode::InvalidFilterPath, "{e}");
            e.message
        })
}

fn closed_orders() -> Value {
    json!({
        "type": "object",
        "properties": {
            "table": { "type": "string" },
            "op": { "type": "string", "enum": ["insert", "update", "delete"] },
            "affected_rows": { "type": "integer" },
            "returning": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": { "status": { "type": "string" }, "amount": { "type": "number" } },
                    "additionalProperties": false
                }
            },
            "meta": { "type": "object" },
            "labels": { "type": "object", "additionalProperties": { "type": "string" } }
        },
        "required": ["table", "op"],
        "additionalProperties": false
    })
}

#[test]
fn known_paths_under_a_closed_schema_pass() {
    let warnings = check(
        json!({ "input.table": { "$eq": "orders" }, "input.op": { "$in": ["insert"] } }),
        Some(closed_orders()),
        None,
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn unknown_property_under_a_closed_schema_is_invalid_filter_path() {
    let err = check(
        json!({ "input.tabel": { "$eq": "orders" } }),
        Some(closed_orders()),
        None,
    )
    .unwrap_err();
    assert_eq!(err, "input.tabel is not in request_format");
}

#[test]
fn every_clause_is_checked_including_nested_combinators() {
    let err = check(
        json!({ "$or": [ { "input.op": { "$eq": "insert" } },
                         { "$not": { "input.affected_row": { "$gt": 1 } } } ] }),
        Some(closed_orders()),
        None,
    )
    .unwrap_err();
    assert_eq!(err, "input.affected_row is not in request_format");
}

#[test]
fn exists_false_on_a_typo_is_still_rejected() {
    let err = check(
        json!({ "input.tabel": { "$exists": false } }),
        Some(closed_orders()),
        None,
    )
    .unwrap_err();
    assert_eq!(err, "input.tabel is not in request_format");
}

#[test]
fn open_schema_gives_a_warning() {
    let warnings = check(
        json!({ "output.total": { "$gt": 1 } }),
        None,
        Some(json!({ "type": "object", "properties": { "id": { "type": "string" } } })),
    )
    .unwrap();
    assert_eq!(
        warnings,
        vec!["output.total: response_format is open; path not verified"]
    );
}

#[test]
fn explicit_additional_properties_true_is_open() {
    let warnings = check(
        json!({ "input.x": { "$eq": 1 } }),
        Some(json!({ "type": "object", "properties": {}, "additionalProperties": true })),
        None,
    )
    .unwrap();
    assert_eq!(
        warnings,
        vec!["input.x: request_format is open; path not verified"]
    );
}

#[test]
fn nested_open_object_warns_with_its_location() {
    let warnings = check(
        json!({ "input.meta.source": { "$eq": "api" } }),
        Some(closed_orders()),
        None,
    )
    .unwrap();
    assert_eq!(
        warnings,
        vec!["input.meta.source: request_format is open at input.meta; path not verified"]
    );
}

#[test]
fn additional_properties_schema_is_followed() {
    let warnings = check(
        json!({ "input.labels.team": { "$eq": "core" } }),
        Some(closed_orders()),
        None,
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn input_is_checked_against_request_and_output_against_response() {
    let request =
        json!({ "type": "object", "properties": { "a": {} }, "additionalProperties": false });
    let response =
        json!({ "type": "object", "properties": { "b": {} }, "additionalProperties": false });
    assert!(
        check(
            json!({ "input.a": { "$exists": true }, "output.b": { "$exists": true } }),
            Some(request.clone()),
            Some(response.clone())
        )
        .unwrap()
        .is_empty()
    );
    assert_eq!(
        check(
            json!({ "input.b": { "$exists": true } }),
            Some(request.clone()),
            Some(response.clone())
        )
        .unwrap_err(),
        "input.b is not in request_format"
    );
    assert_eq!(
        check(
            json!({ "output.a": { "$exists": true } }),
            Some(request),
            Some(response)
        )
        .unwrap_err(),
        "output.a is not in response_format"
    );
}

#[test]
fn missing_formats_and_other_roots_are_not_checked() {
    let closed = json!({ "type": "object", "properties": {}, "additionalProperties": false });
    // No formats: nothing to check.
    assert!(
        check(
            json!({ "input.anything": { "$eq": 1 }, "output.x": { "$eq": 1 } }),
            None,
            None
        )
        .unwrap()
        .is_empty()
    );
    // error / caller / metadata are never checked against the formats.
    assert!(
        check(
            json!({ "error.code": { "$eq": "X" }, "caller.parent.worker_name": { "$ne": "a" },
                "metadata.k": { "$exists": true } }),
            Some(closed.clone()),
            Some(closed.clone())
        )
        .unwrap()
        .is_empty()
    );
    // A root-only path has nothing to look up.
    assert!(
        check(json!({ "input": { "$exists": true } }), Some(closed), None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn index_segments_follow_items() {
    assert!(
        check(
            json!({ "input.returning.0.status": { "$eq": "paid" } }),
            Some(closed_orders()),
            None
        )
        .unwrap()
        .is_empty()
    );
    assert_eq!(
        check(
            json!({ "input.returning.0.state": { "$eq": "paid" } }),
            Some(closed_orders()),
            None
        )
        .unwrap_err(),
        "input.returning.0.state is not in request_format"
    );
}

#[test]
fn any_filters_are_checked_against_items() {
    assert!(check(
        json!({ "input.returning": { "$any": { "@.status": { "$eq": "paid" }, "@.amount": { "$gte": 500 } } } }),
        Some(closed_orders()),
        None
    )
    .unwrap()
    .is_empty());
    assert_eq!(
        check(
            json!({ "input.returning": { "$any": { "@.stat": { "$eq": "paid" } } } }),
            Some(closed_orders()),
            None
        )
        .unwrap_err(),
        "input.returning[*].stat is not in request_format"
    );
}

#[test]
fn any_on_an_array_without_items_warns_for_element_paths() {
    let schema = json!({ "type": "object", "properties": { "rows": { "type": "array" } },
                         "additionalProperties": false });
    let warnings = check(
        json!({ "input.rows": { "$any": { "@.id": { "$eq": 1 } } } }),
        Some(schema),
        None,
    )
    .unwrap();
    assert_eq!(
        warnings,
        vec!["input.rows[*].id: request_format is open at input.rows; path not verified"]
    );
}

#[test]
fn nested_any_operator_objects_reach_inner_items() {
    let schema = json!({ "type": "object", "properties": {
        "matrix": { "type": "array", "items": { "type": "array", "items": {
            "type": "object", "properties": { "v": {} }, "additionalProperties": false } } } },
        "additionalProperties": false });
    assert!(
        check(
            json!({ "input.matrix": { "$any": { "$any": { "$exists": true } } } }),
            Some(schema.clone()),
            None
        )
        .unwrap()
        .is_empty()
    );
    assert_eq!(
        check(
            json!({ "input.matrix": { "$any": { "@": { "$any": { "@.w": { "$eq": 1 } } } } } }),
            Some(schema),
            None
        )
        .unwrap_err(),
        "input.matrix[*][*].w is not in request_format"
    );
}

#[test]
fn stepping_into_a_scalar_type_warns() {
    let warnings = check(
        json!({ "input.table.name": { "$eq": "x" } }),
        Some(closed_orders()),
        None,
    )
    .unwrap();
    assert_eq!(
        warnings,
        vec![
            "input.table.name: request_format declares input.table as string; the path is always absent"
        ]
    );
}

#[test]
fn local_refs_are_resolved() {
    let schema = json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {
            "user": { "$ref": "#/definitions/User" },
            "manager": { "allOf": [ { "$ref": "#/definitions/User" } ], "description": "documented ref" },
            "deputy": { "anyOf": [ { "$ref": "#/definitions/User" }, { "type": "null" } ] }
        },
        "additionalProperties": false,
        "definitions": {
            "User": { "type": "object", "properties": { "name": { "type": "string" } },
                      "additionalProperties": false }
        }
    });
    assert!(
        check(
            json!({ "input.user.name": { "$eq": "a" }, "input.manager.name": { "$eq": "b" },
                "input.deputy.name": { "$eq": "c" } }),
            Some(schema.clone()),
            None
        )
        .unwrap()
        .is_empty()
    );
    assert_eq!(
        check(
            json!({ "input.user.nam": { "$eq": "a" } }),
            Some(schema.clone()),
            None
        )
        .unwrap_err(),
        "input.user.nam is not in request_format"
    );
    assert_eq!(
        check(
            json!({ "input.deputy.nme": { "$eq": "a" } }),
            Some(schema),
            None
        )
        .unwrap_err(),
        "input.deputy.nme is not in request_format"
    );
}

#[test]
fn unresolvable_constructs_are_treated_as_open() {
    let cases = [
        (
            json!({ "$ref": "https://example.com/schema.json" }),
            "uses `$ref` at input",
        ),
        (
            json!({ "$ref": "#/definitions/Missing" }),
            "uses `$ref` at input",
        ),
        (json!({ "$ref": "#" }), "uses `$ref` at input"),
        (
            json!({ "$ref": "#/definitions/A", "properties": {} , "definitions": { "A": {} } }),
            "uses `$ref` at input",
        ),
        (
            json!({ "anyOf": [ { "type": "object" }, { "type": "string" } ] }),
            "uses `anyOf` at input",
        ),
        (
            json!({ "oneOf": [ { "properties": { "a": {} } }, { "properties": { "b": {} } } ] }),
            "uses `oneOf` at input",
        ),
        (
            json!({ "allOf": [ { "properties": { "a": {} } }, { "properties": { "b": {} } } ],
                 "additionalProperties": false }),
            "uses `allOf` at input",
        ),
        (
            json!({ "type": "object", "patternProperties": { "^x": {} }, "additionalProperties": false }),
            "uses `patternProperties` at input",
        ),
        (
            json!({ "type": "object", "if": { "properties": { "a": {} } }, "then": {} }),
            "uses `if` at input",
        ),
    ];
    for (schema, expected) in cases {
        let warnings = check(
            json!({ "input.x": { "$eq": 1 } }),
            Some(schema.clone()),
            None,
        )
        .unwrap_or_else(|e| panic!("{schema}: {e}"));
        assert_eq!(
            warnings,
            vec![format!(
                "input.x: request_format {expected}; path not verified"
            )],
            "{schema}"
        );
    }
}

#[test]
fn declared_properties_win_over_unresolved_keywords() {
    let schema = json!({ "type": "object", "properties": { "a": { "type": "object", "properties": {},
                         "additionalProperties": false } },
                         "patternProperties": { "^x": {} } });
    assert_eq!(
        check(json!({ "input.a.b": { "$eq": 1 } }), Some(schema), None).unwrap_err(),
        "input.a.b is not in request_format"
    );
}

#[test]
fn boolean_schemas() {
    let schema = json!({ "type": "object", "properties": { "any": true, "never": false },
                         "additionalProperties": false });
    assert_eq!(
        check(
            json!({ "input.any.x": { "$eq": 1 } }),
            Some(schema.clone()),
            None
        )
        .unwrap(),
        vec!["input.any.x: request_format is open at input.any; path not verified"]
    );
    assert_eq!(
        check(json!({ "input.never.x": { "$eq": 1 } }), Some(schema), None).unwrap_err(),
        "input.never is not in request_format"
    );
}

#[test]
fn escaped_keys_are_looked_up_unescaped_and_reported_escaped() {
    let schema =
        json!({ "type": "object", "properties": { "a.b": {} }, "additionalProperties": false });
    assert!(
        check(
            json!({ "input.a\\.b": { "$exists": true } }),
            Some(schema.clone()),
            None
        )
        .unwrap()
        .is_empty()
    );
    assert_eq!(
        check(
            json!({ "input.a\\.c": { "$exists": true } }),
            Some(schema),
            None
        )
        .unwrap_err(),
        "input.a\\.c is not in request_format"
    );
}

#[test]
fn warnings_are_deduplicated() {
    let warnings = check(
        json!({ "$or": [ { "input.x": { "$eq": 1 } }, { "input.x": { "$eq": 2 } } ] }),
        Some(json!({ "type": "object" })),
        None,
    )
    .unwrap();
    assert_eq!(
        warnings,
        vec!["input.x: request_format is open; path not verified"]
    );
}

#[test]
fn false_schemas_reject_the_path_that_reaches_them() {
    let schema = json!({
        "type": "object",
        "properties": {
            "never": false,
            "via_ref": { "$ref": "#/definitions/Never" },
            "empty_list": { "type": "array", "items": false },
            "list": { "type": "array", "items": { "type": "object", "properties": { "x": false } } }
        },
        "definitions": { "Never": false }
    });
    let cases = [
        (
            json!({ "input.never": { "$exists": false } }),
            "input.never is not in request_format",
        ),
        (
            json!({ "input.via_ref": { "$eq": 1 } }),
            "input.via_ref is not in request_format",
        ),
        (
            json!({ "input.never.x": { "$eq": 1 } }),
            "input.never is not in request_format",
        ),
        (
            json!({ "input.empty_list.0": { "$eq": 1 } }),
            "input.empty_list.0 is not in request_format",
        ),
        (
            json!({ "input.list.0.x": { "$eq": 1 } }),
            "input.list.0.x is not in request_format",
        ),
        (
            json!({ "input.empty_list": { "$any": { "@.y": { "$eq": 1 } } } }),
            "input.empty_list[*].y is not in request_format",
        ),
        (
            json!({ "input.list": { "$any": { "@.x": { "$eq": 1 } } } }),
            "input.list[*].x is not in request_format",
        ),
    ];
    for (src, expected) in cases {
        assert_eq!(
            check(src.clone(), Some(schema.clone()), None).unwrap_err(),
            expected,
            "{src}"
        );
    }
    // The array itself is fine, and so is `$any` without element paths.
    assert!(
        check(
            json!({ "input.empty_list": { "$exists": true, "$any": { "$eq": 1 } } }),
            Some(schema),
            None
        )
        .unwrap()
        .is_empty()
    );
}
