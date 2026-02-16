// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde_json::Value;

use crate::protocol::ErrorBody;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidationIssue {
    pub field: &'static str,
    pub message: String,
}

impl ValidationIssue {
    fn as_message(&self) -> String {
        format!("{} {}", self.field, self.message)
    }

    pub fn join_messages(issues: &[ValidationIssue]) -> String {
        issues
            .iter()
            .map(ValidationIssue::as_message)
            .collect::<Vec<_>>()
            .join("; ")
    }
}

pub(crate) fn validate_register_trigger(
    id: &str,
    trigger_type: &str,
    function_id: &str,
    config: &Value,
) -> Result<(), Vec<ValidationIssue>> {
    let mut issues = Vec::new();
    if id.trim().is_empty() {
        issues.push(ValidationIssue {
            field: "id",
            message: "must be a non-empty string".into(),
        });
    }
    if trigger_type.trim().is_empty() {
        issues.push(ValidationIssue {
            field: "trigger_type",
            message: "must be a non-empty string".into(),
        });
    }
    if function_id.trim().is_empty() {
        issues.push(ValidationIssue {
            field: "function_id",
            message: "must be a non-empty string".into(),
        });
    }
    if !config.is_object() {
        issues.push(ValidationIssue {
            field: "config",
            message: "must be a JSON object".into(),
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
    _description: &Option<String>,
    request_format: &Option<Value>,
    response_format: &Option<Value>,
    metadata: &Option<Value>,
) -> Result<(), Vec<ValidationIssue>> {
    let mut issues = Vec::new();

    if id.trim().is_empty() {
        issues.push(ValidationIssue {
            field: "id",
            message: "must be a non-empty string".into(),
        });
    }

    if let Some(m) = metadata
        && !m.is_object()
    {
        issues.push(ValidationIssue {
            field: "metadata",
            message: "must be a JSON object when provided".into(),
        });
    }

    if let Some(r) = request_format
        && !r.is_object()
    {
        issues.push(ValidationIssue {
            field: "request_format",
            message: "must be a JSON object when provided".into(),
        });
    }

    if let Some(r) = response_format
        && !r.is_object()
    {
        issues.push(ValidationIssue {
            field: "response_format",
            message: "must be a JSON object when provided".into(),
        });
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

pub(crate) fn format_register_trigger_validation_error(
    id: &str,
    issues: &[ValidationIssue],
) -> ErrorBody {
    ErrorBody {
        code: "register_trigger_validation_failed".into(),
        message: format!(
            "registertrigger validation failed for id '{}': {}",
            id,
            ValidationIssue::join_messages(issues)
        ),
    }
}

pub(crate) fn format_register_function_validation_error(
    id: &str,
    issues: &[ValidationIssue],
) -> ErrorBody {
    ErrorBody {
        code: "register_function_validation_failed".into(),
        message: format!(
            "registerfunction validation failed for id '{}': {}",
            id,
            ValidationIssue::join_messages(issues)
        ),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        format_register_function_validation_error, validate_register_function,
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
        let err =
            validate_register_function("  ", &None, &None, &None, &Some(json!("bad"))).unwrap_err();
        assert!(err.iter().any(|i| i.field == "id"));
        assert!(err.iter().any(|i| i.field == "metadata"));
    }

    #[test]
    fn format_register_function_validation_error_is_readable() {
        let issues = validate_register_function("", &None, &None, &None, &None).unwrap_err();
        let body = format_register_function_validation_error("fn.bad", &issues);
        assert_eq!(body.code, "register_function_validation_failed");
        assert!(body.message.contains("registerfunction validation failed"));
        assert!(body.message.contains("id"));
    }
}
