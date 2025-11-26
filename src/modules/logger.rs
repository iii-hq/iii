use serde_json::Value;

use crate::modules::observability::LoggerAdapter;

/// Logger implementation that uses tracing for output.
/// This maintains compatibility with the LoggerAdapter trait while
/// leveraging the centralized tracing formatter for consistent output.
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
        if args.is_empty() {
            if let Some(tid) = trace_id {
                tracing::info!(trace_id = %tid, function = %function_name, "{}", message);
            } else {
                tracing::info!(function = %function_name, "{}", message);
            }
        } else {
            let args_json = serde_json::to_string(&args.iter()
                .map(|(k, v)| (*k, *v))
                .collect::<std::collections::HashMap<_, _>>())
                .unwrap_or_default();
            
            if let Some(tid) = trace_id {
                tracing::info!(trace_id = %tid, function = %function_name, data = %args_json, "{}", message);
            } else {
                tracing::info!(function = %function_name, data = %args_json, "{}", message);
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
        if args.is_empty() {
            if let Some(tid) = trace_id {
                tracing::warn!(trace_id = %tid, function = %function_name, "{}", message);
            } else {
                tracing::warn!(function = %function_name, "{}", message);
            }
        } else {
            let args_json = serde_json::to_string(&args.iter()
                .map(|(k, v)| (*k, *v))
                .collect::<std::collections::HashMap<_, _>>())
                .unwrap_or_default();
            
            if let Some(tid) = trace_id {
                tracing::warn!(trace_id = %tid, function = %function_name, data = %args_json, "{}", message);
            } else {
                tracing::warn!(function = %function_name, data = %args_json, "{}", message);
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
        if args.is_empty() {
            if let Some(tid) = trace_id {
                tracing::error!(trace_id = %tid, function = %function_name, "{}", message);
            } else {
                tracing::error!(function = %function_name, "{}", message);
            }
        } else {
            let args_json = serde_json::to_string(&args.iter()
                .map(|(k, v)| (*k, *v))
                .collect::<std::collections::HashMap<_, _>>())
                .unwrap_or_default();
            
            if let Some(tid) = trace_id {
                tracing::error!(trace_id = %tid, function = %function_name, data = %args_json, "{}", message);
            } else {
                tracing::error!(function = %function_name, data = %args_json, "{}", message);
            }
        }
    }
}
