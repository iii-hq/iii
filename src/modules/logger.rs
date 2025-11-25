use chrono::Local;
use colored::Colorize;
use serde_json::Value;

use crate::modules::observability::LoggerAdapter;

pub enum LogLevel {
    Info,
    Warn,
    Error,
}

pub fn log(
    level: LogLevel,
    function_name: &str,
    message: &str,
    args: Option<&[(&str, &Value)]>,
    trace_id: Option<&str>,
) {
    // Get current time [HH:MM:SS]
    let time = Local::now().format("%H:%M:%S").to_string();

    let time_display = format!("[{}]", time).bright_black();
    let trace_display = trace_id
        .map(|id| format!("{} ", id))
        .unwrap_or_default()
        .bright_black();
    let level_str = match level {
        LogLevel::Info => "[INFO]".blue(),
        LogLevel::Warn => "[WARN]".yellow(),
        LogLevel::Error => "[ERROR]".red(),
    };
    let function_display = function_name.cyan().bold();
    let message_display = message.white();
    // header line
    println!(
        "{} {}{} {} {}",
        time_display, trace_display, level_str, function_display, message_display
    );

    // If there are args, pretty print them in a "tree" format
    if let Some(args) = args {
        for (i, (field, value)) in args.iter().enumerate() {
            let is_last = i == args.len() - 1;
            let tree_icon = if is_last { "└" } else { "├" };
            let field_str = format!("{} {}", tree_icon, field.white());
            let rendered = render_tree_value(value, 1, is_last);
            println!("{}{}", field_str, rendered);
        }
    }
}

// Helper function to render Value in "tree" style
fn render_tree_value(value: &Value, indent: usize, _last: bool) -> String {
    let pad = "    ".repeat(indent);

    match value {
        Value::Object(map) => {
            let mut s = format!(": {}{}", "{".bright_black(), "\n");
            let mut iter = map.iter().peekable();
            while let Some((key, v)) = iter.next() {
                let is_last = iter.peek().is_none();
                let branch = if is_last { "└" } else { "│" };
                let field = format!("{}", key.white());
                let rendered = render_tree_value(v, indent + 1, is_last);
                s.push_str(&format!("{}{} {}{}\n", pad, branch, field, rendered));
            }
            s += &format!("{}{}", pad, "}".bright_black());
            s
        }
        Value::Array(arr) => {
            let mut s = format!(": {}{}", "[".bright_black(), "\n");
            for (i, v) in arr.iter().enumerate() {
                let is_last = i == arr.len() - 1;
                let branch = if is_last { "└" } else { "│" };
                let rendered = render_tree_value(v, indent + 1, is_last);
                s.push_str(&format!("{}{} {}\n", pad, branch, rendered));
            }
            s += &format!("{}{}", pad, "]".bright_black());
            s
        }
        Value::String(st) => format!(": {}", format!("{:?}", st).cyan()),
        Value::Number(num) => format!(": {}", num.to_string().yellow()),
        Value::Bool(b) => format!(": {}", b.to_string().purple()),
        Value::Null => format!(": {}", "null".bright_black()),
    }
}

#[derive(Clone)]
pub struct Logger {}

impl LoggerAdapter for Logger {
    fn info(
        &self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &[(&str, &Value)],
    ) {
        log(LogLevel::Info, function_name, message, Some(args), trace_id);
    }

    fn warn(
        &self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &[(&str, &Value)],
    ) {
        log(LogLevel::Warn, function_name, message, Some(args), trace_id);
    }

    fn error(
        &self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &[(&str, &Value)],
    ) {
        log(
            LogLevel::Error,
            function_name,
            message,
            Some(args),
            trace_id,
        );
    }
}
