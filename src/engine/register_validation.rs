// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde_json::Value;

use crate::protocol::ErrorBody;

pub(crate) const MAX_ID_LEN: usize = 256;

fn is_valid_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_ID_LEN
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidationIssue {
    pub field: &'static str,
    pub message: &'static str,
}

impl ValidationIssue {
    pub fn join_messages(issues: &[ValidationIssue]) -> String {
        let mut result = String::new();
        for (i, issue) in issues.iter().enumerate() {
            if i > 0 {
                result.push_str("; ");
            }
            result.push_str(issue.field);
            result.push(' ');
            result.push_str(issue.message);
        }
        result
    }
}

pub(crate) fn validate_register_trigger(
    id: &str,
    trigger_type: &str,
    function_id: &str,
    config: &Value,
) -> Result<(), Vec<ValidationIssue>> {
    let mut issues = Vec::new();
    if !is_valid_identifier(id) {
        issues.push(ValidationIssue {
            field: "id",
            message: "must be a non-empty alphanumeric string (max 256 chars, allows - _ . / :)",
        });
    }
    if !is_valid_identifier(trigger_type) {
        issues.push(ValidationIssue {
            field: "trigger_type",
            message: "must be a non-empty alphanumeric string (max 256 chars, allows - _ . / :)",
        });
    }
    if !is_valid_identifier(function_id) {
        issues.push(ValidationIssue {
            field: "function_id",
            message: "must be a non-empty alphanumeric string (max 256 chars, allows - _ . / :)",
        });
    }
    if !config.is_object() {
        issues.push(ValidationIssue {
            field: "config",
            message: "must be a JSON object",
        });
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

pub(crate) fn validate_register_function(
    id: &str,
    request_format: &Option<Value>,
    response_format: &Option<Value>,
    metadata: &Option<Value>,
) -> Result<(), Vec<ValidationIssue>> {
    let mut issues = Vec::new();

    if !is_valid_identifier(id) {
        issues.push(ValidationIssue {
            field: "id",
            message: "must be a non-empty alphanumeric string (max 256 chars, allows - _ . / :)",
        });
    }

    if let Some(m) = metadata
        && !m.is_object()
    {
        issues.push(ValidationIssue {
            field: "metadata",
            message: "must be a JSON object when provided",
        });
    }

    if let Some(r) = request_format
        && !r.is_object()
    {
        issues.push(ValidationIssue {
            field: "request_format",
            message: "must be a JSON object when provided",
        });
    }

    if let Some(r) = response_format
        && !r.is_object()
    {
        issues.push(ValidationIssue {
            field: "response_format",
            message: "must be a JSON object when provided",
        });
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

fn sanitize_id_for_error(id: &str) -> String {
    const MAX_DISPLAY_LEN: usize = 64;
    let sanitized: String = id
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_DISPLAY_LEN)
        .collect();
    if id.len() > MAX_DISPLAY_LEN {
        format!("{}...", sanitized)
    } else {
        sanitized
    }
}

pub(crate) fn format_validation_error(
    operation: &str,
    code: &str,
    id: &str,
    issues: &[ValidationIssue],
) -> ErrorBody {
    ErrorBody {
        code: code.into(),
        message: format!(
            "{} validation failed for id '{}': {}",
            operation,
            sanitize_id_for_error(id),
            ValidationIssue::join_messages(issues)
        ),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        MAX_ID_LEN, ValidationIssue, format_validation_error, validate_register_function,
        validate_register_trigger,
    };

    #[test]
    fn validate_register_trigger_rejects_empty_fields() {
        let err = validate_register_trigger("", "", "", &json!({})).unwrap_err();
        assert!(err.iter().any(|i| i.field == "id"));
        assert!(err.iter().any(|i| i.field == "trigger_type"));
        assert!(err.iter().any(|i| i.field == "function_id"));
    }

    #[test]
    fn validate_register_trigger_rejects_non_object_config() {
        let err =
            validate_register_trigger("t1", "queue", "steps.demo", &json!("bad")).unwrap_err();
        assert!(err.iter().any(|i| i.field == "config"));
    }

    #[test]
    fn validate_register_function_rejects_empty_id_and_bad_metadata() {
        let err = validate_register_function("  ", &None, &None, &Some(json!("bad"))).unwrap_err();
        assert!(err.iter().any(|i| i.field == "id"));
        assert!(err.iter().any(|i| i.field == "metadata"));
    }

    #[test]
    fn format_validation_error_for_function_is_readable() {
        let issues = validate_register_function("", &None, &None, &None).unwrap_err();
        let body = format_validation_error(
            "registerfunction",
            "register_function_validation_failed",
            "fn.bad",
            &issues,
        );
        assert_eq!(body.code, "register_function_validation_failed");
        assert!(body.message.contains("registerfunction validation failed"));
        assert!(body.message.contains("id"));
    }

    #[test]
    fn format_validation_error_produces_correct_body() {
        let issues = vec![ValidationIssue {
            field: "id",
            message: "must be a non-empty string",
        }];
        let body = format_validation_error(
            "registertrigger",
            "register_trigger_validation_failed",
            "t1",
            &issues,
        );
        assert_eq!(body.code, "register_trigger_validation_failed");
        assert!(body.message.contains("registertrigger"));
        assert!(body.message.contains("t1"));
        assert!(body.message.contains("id must be a non-empty string"));
    }

    #[test]
    fn format_validation_error_truncates_long_id() {
        let long_id = "a".repeat(300);
        let issues = vec![ValidationIssue {
            field: "config",
            message: "must be a JSON object",
        }];
        let body = format_validation_error(
            "registertrigger",
            "register_trigger_validation_failed",
            &long_id,
            &issues,
        );
        // id in the message should be truncated, not the full 300 chars
        assert!(body.message.len() < 200);
        assert!(body.message.contains("..."));
    }

    #[test]
    fn format_validation_error_sanitizes_control_chars_in_id() {
        let bad_id = "evil\n\rid";
        let issues = vec![ValidationIssue {
            field: "id",
            message: "must be a non-empty string",
        }];
        let body = format_validation_error(
            "registertrigger",
            "register_trigger_validation_failed",
            bad_id,
            &issues,
        );
        assert!(!body.message.contains('\n'));
        assert!(!body.message.contains('\r'));
    }

    #[test]
    fn validate_register_trigger_rejects_id_exceeding_max_length() {
        let long_id = "a".repeat(MAX_ID_LEN + 1);
        let err = validate_register_trigger(&long_id, "queue", "fn.demo", &json!({})).unwrap_err();
        assert!(err.iter().any(|i| i.field == "id"));
        assert!(err.iter().any(|i| i.message.contains("256")));
    }

    #[test]
    fn validate_register_trigger_rejects_id_with_control_chars() {
        let err =
            validate_register_trigger("evil\nid", "queue", "fn.demo", &json!({})).unwrap_err();
        assert!(err.iter().any(|i| i.field == "id"));
        assert!(err.iter().any(|i| i.message.contains("alphanumeric")));
    }

    #[test]
    fn validate_register_trigger_accepts_valid_identifier_chars() {
        let result =
            validate_register_trigger("my-trigger_v2.0/sub", "queue", "steps.demo", &json!({}));
        assert!(result.is_ok());
    }

    #[test]
    fn validate_register_function_rejects_id_with_spaces() {
        let err = validate_register_function("has space", &None, &None, &None).unwrap_err();
        assert!(err.iter().any(|i| i.field == "id"));
    }

    #[test]
    fn validate_register_trigger_rejects_trigger_type_exceeding_max_length() {
        let long_type = "t".repeat(MAX_ID_LEN + 1);
        let err = validate_register_trigger("t1", &long_type, "fn.demo", &json!({})).unwrap_err();
        assert!(err.iter().any(|i| i.field == "trigger_type"));
    }
}
