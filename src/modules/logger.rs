use serde_json::Value;

use crate::modules::observability::LoggerAdapter;

/// Logger implementation that uses tracing for output.
/// This maintains compatibility with the LoggerAdapter trait while
/// leveraging the centralized tracing formatter for consistent output.
///
/// The `function_name` parameter is passed as the `function` field to tracing,
/// which the formatter will use as the display name instead of the module path.
#[derive(Clone)]
pub struct Logger;

impl Logger {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Logger {
    fn default() -> Self {
        Self::new()
    }
}

impl LoggerAdapter for Logger {
    fn info(
        &self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &[(&str, &Value)],
    ) {
        let data = format_args_as_json(args);

        match (trace_id, data.as_deref()) {
            (Some(tid), Some(d)) => {
                tracing::info!(function = %function_name, trace_id = %tid, data = %d, "{}", message);
            }
            (Some(tid), None) => {
                tracing::info!(function = %function_name, trace_id = %tid, "{}", message);
            }
            (None, Some(d)) => {
                tracing::info!(function = %function_name, data = %d, "{}", message);
            }
            (None, None) => {
                tracing::info!(function = %function_name, "{}", message);
            }
        }
    }

    fn warn(
        &self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &[(&str, &Value)],
    ) {
        let data = format_args_as_json(args);

        match (trace_id, data.as_deref()) {
            (Some(tid), Some(d)) => {
                tracing::warn!(function = %function_name, trace_id = %tid, data = %d, "{}", message);
            }
            (Some(tid), None) => {
                tracing::warn!(function = %function_name, trace_id = %tid, "{}", message);
            }
            (None, Some(d)) => {
                tracing::warn!(function = %function_name, data = %d, "{}", message);
            }
            (None, None) => {
                tracing::warn!(function = %function_name, "{}", message);
            }
        }
    }

    fn error(
        &self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &[(&str, &Value)],
    ) {
        let data = format_args_as_json(args);

        match (trace_id, data.as_deref()) {
            (Some(tid), Some(d)) => {
                tracing::error!(function = %function_name, trace_id = %tid, data = %d, "{}", message);
            }
            (Some(tid), None) => {
                tracing::error!(function = %function_name, trace_id = %tid, "{}", message);
            }
            (None, Some(d)) => {
                tracing::error!(function = %function_name, data = %d, "{}", message);
            }
            (None, None) => {
                tracing::error!(function = %function_name, "{}", message);
            }
        }
    }
}

/// Convert args to JSON string if not empty
fn format_args_as_json(args: &[(&str, &Value)]) -> Option<String> {
    if args.is_empty() {
        return None;
    }

    serde_json::to_string(
        &args
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect::<std::collections::HashMap<_, _>>(),
    )
    .ok()
}
