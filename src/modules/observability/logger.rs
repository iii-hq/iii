use serde_json::Value;

use super::LoggerAdapter;

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

fn get_args(args: &Option<Value>) -> String {
    match args {
        Some(v) => v
            .as_object()
            .map(|map| serde_json::to_string(map).unwrap_or_default())
            .unwrap_or_default(),
        None => "{}".to_string(),
    }
}

impl LoggerAdapter for Logger {
    fn info(
        &self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &Option<Value>,
    ) {
        match (trace_id, &get_args(args)) {
            (Some(tid), data) => {
                tracing::info!(function = %function_name, trace_id = %tid, data = %data, "{}", message);
            }
            (None, data) => {
                tracing::info!(function = %function_name, data = %data, "{}", message);
            }
        }
    }

    fn warn(
        &self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &Option<Value>,
    ) {
        match (trace_id, &get_args(args)) {
            (Some(tid), data) => {
                tracing::warn!(function = %function_name, trace_id = %tid, data = %data, "{}", message);
            }
            (None, data) => {
                tracing::warn!(function = %function_name, data = %data, "{}", message);
            }
        }
    }

    fn error(
        &self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &Option<Value>,
    ) {
        match (trace_id, &get_args(args)) {
            (Some(tid), data) => {
                tracing::error!(function = %function_name, trace_id = %tid, data = %data, "{}", message);
            }
            (None, data) => {
                tracing::error!(function = %function_name, data = %data, "{}", message);
            }
        }
    }
}
