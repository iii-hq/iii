use std::{env, fmt, sync::OnceLock};

use chrono::Local;
use colored::Colorize;
use regex::Regex;
use tracing::{
    Event, Level, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{
    EnvFilter,
    fmt::{self as tracing_fmt, FmtContext, FormatEvent, FormatFields},
    registry::LookupSpan,
};

use crate::modules::config::EngineConfig;

/// Collected field from tracing event
#[derive(Debug, Clone)]
enum FieldValue {
    String(String),
    I64(i64),
    U64(u64),
    F64(f64),
    Bool(bool),
    Debug(String),
}

/// Visitor that collects tracing fields into a Vec
struct FieldCollector {
    fields: Vec<(String, FieldValue)>,
    message: Option<String>,
    function: Option<String>,
}

impl FieldCollector {
    fn new() -> Self {
        Self {
            fields: Vec::new(),
            message: None,
            function: None,
        }
    }

    /// Extract the function field value if it exists
    fn get_function(&self) -> Option<&str> {
        self.function.as_deref()
    }

    /// Get fields excluding the "function" field (since it's shown in the header)
    fn get_display_fields(&self) -> Vec<(&String, &FieldValue)> {
        self.fields
            .iter()
            .filter(|(name, _)| name != "function")
            .map(|(name, value)| (name, value))
            .collect()
    }
}

impl Visit for FieldCollector {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "message" => self.message = Some(value.to_string()),
            "function" => self.function = Some(value.to_string()),
            _ => self.fields.push((
                field.name().to_string(),
                FieldValue::String(value.to_string()),
            )),
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .push((field.name().to_string(), FieldValue::I64(value)));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .push((field.name().to_string(), FieldValue::U64(value)));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.fields
            .push((field.name().to_string(), FieldValue::F64(value)));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .push((field.name().to_string(), FieldValue::Bool(value)));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        match field.name() {
            "message" => self.message = Some(format!("{:?}", value)),
            "function" => {
                self.function = Some(format!("{:?}", value).trim_matches('"').to_string())
            }
            _ => self.fields.push((
                field.name().to_string(),
                FieldValue::Debug(format!("{:?}", value)),
            )),
        }
    }
}

/// Renders a field value with appropriate coloring
fn render_field_value(value: &FieldValue) -> String {
    match value {
        FieldValue::String(s) => format!("{}", format!("\"{}\"", s).cyan()),
        FieldValue::I64(n) => format!("{}", n.to_string().yellow()),
        FieldValue::U64(n) => format!("{}", n.to_string().yellow()),
        FieldValue::F64(n) => format!("{}", n.to_string().yellow()),
        FieldValue::Bool(b) => format!("{}", b.to_string().purple()),
        FieldValue::Debug(s) => {
            // Try to parse as JSON for pretty printing
            if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(s) {
                render_json_value(&json_val, 2)
            } else {
                format!("{}", s.bright_black())
            }
        }
    }
}

/// Renders a serde_json::Value in tree format with colors
fn render_json_value(value: &serde_json::Value, indent: usize) -> String {
    let pad = "    ".repeat(indent);

    match value {
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                return format!("{}", "{}".bright_black());
            }
            let mut s = format!("{}\n", "{".bright_black());
            let mut iter = map.iter().peekable();
            while let Some((key, v)) = iter.next() {
                let is_last = iter.peek().is_none();
                let branch = if is_last { "└" } else { "├" };
                let field = format!("{}", key.white());
                let rendered = render_json_value(v, indent + 1);
                s.push_str(&format!("{}{} {}: {}", pad, branch, field, rendered));
                if !is_last {
                    s.push('\n');
                }
            }
            s.push_str(&format!(
                "\n{}{}",
                "    ".repeat(indent - 1),
                "}".bright_black()
            ));
            s
        }
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                return format!("{}", "[]".bright_black());
            }
            let mut s = format!("{}\n", "[".bright_black());
            for (i, v) in arr.iter().enumerate() {
                let is_last = i == arr.len() - 1;
                let branch = if is_last { "└" } else { "├" };
                let rendered = render_json_value(v, indent + 1);
                s.push_str(&format!("{}{} {}", pad, branch, rendered));
                if !is_last {
                    s.push('\n');
                }
            }
            s.push_str(&format!(
                "\n{}{}",
                "    ".repeat(indent - 1),
                "]".bright_black()
            ));
            s
        }
        serde_json::Value::String(st) => format!("{}", format!("\"{}\"", st).cyan()),
        serde_json::Value::Number(num) => format!("{}", num.to_string().yellow()),
        serde_json::Value::Bool(b) => format!("{}", b.to_string().purple()),
        serde_json::Value::Null => format!("{}", "null".bright_black()),
    }
}

/// Renders collected fields in a tree-like format
fn render_fields_tree(fields: &[(&String, &FieldValue)]) -> String {
    if fields.is_empty() {
        return String::new();
    }

    let mut result = String::from("\n");
    let pad = "    ";

    for (i, (name, value)) in fields.iter().enumerate() {
        let is_last = i == fields.len() - 1;
        let branch = if is_last { "└" } else { "├" };
        let field_name = name.white();
        let field_value = render_field_value(value);

        result.push_str(&format!(
            "{}{} {}: {}",
            pad, branch, field_name, field_value
        ));

        if !is_last {
            result.push('\n');
        }
    }

    result
}

/// Format timestamp as [HH:MM:SS.mmm AM/PM]
fn format_timestamp() -> String {
    let now = Local::now();
    now.format("[%I:%M:%S%.3f %p]").to_string()
}

struct IIILogFormatter;

impl<S, N> FormatEvent<S, N> for IIILogFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: tracing_fmt::format::Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();

        // Collect fields first to check for "function" field
        let mut collector = FieldCollector::new();
        event.record(&mut collector);

        // timestamp in format [09:19:23.241 AM]
        write!(writer, "{} ", format_timestamp().dimmed())?;

        // level with colors
        let level = meta.level();
        let level_str = match *level {
            Level::TRACE => "TRACE".purple(),
            Level::DEBUG => "DEBUG".green(),
            Level::INFO => "INFO".blue(),
            Level::WARN => "WARN".yellow(),
            Level::ERROR => "ERROR".red(),
        };
        write!(writer, "[{}] ", level_str)?;

        // Use "function" field if present, otherwise use target (module path)
        let display_name = collector.get_function().unwrap_or(meta.target());
        write!(writer, "{} ", display_name.cyan().bold())?;

        // Write message if present
        if let Some(msg) = &collector.message {
            write!(writer, "{}", msg.white())?;
        }

        // Render fields as tree (excluding "function" since it's in the header)
        let display_fields = collector.get_display_fields();
        let tree = render_fields_tree(&display_fields);
        write!(writer, "{}", tree)?;

        writeln!(writer)
    }
}

static TRACING: OnceLock<()> = OnceLock::new();

fn expand_env_vars(yaml_content: &str) -> String {
    let re = Regex::new(r"\$\{([^}:]+)(?::([^}]*))?\}").unwrap();

    re.replace_all(yaml_content, |caps: &regex::Captures| {
        let var_name = &caps[1];
        let default_value = caps.get(2).map(|m| m.as_str());

        match env::var(var_name) {
            Ok(value) => value,
            Err(_) => match default_value {
                Some(default) => default.to_string(),
                None => {
                    tracing::error!(
                        "Environment variable '{}' not set and no
    default provided",
                        var_name
                    );
                    panic!(
                        "Environment variable '{}' not set and no default provided",
                        var_name
                    );
                }
            },
        }
    })
    .to_string()
}

pub fn init_log(path: &str) {
    println!("Initializing logging from config file: {}", path);
    match std::fs::read_to_string(path) {
        Ok(yaml_content) => {
            let yaml_content = expand_env_vars(&yaml_content);
            let config = serde_yaml::from_str::<EngineConfig>(&yaml_content);
            match config {
                Ok(cfg) => {
                    println!("Parsed config file: {}", path);
                    let log_module_name = "modules::observability::LoggingModule";
                    let log_module_cfg = cfg.modules.iter().find(|m| m.class == log_module_name);
                    match log_module_cfg {
                        Some(_) => {
                            let log_level = log_module_cfg
                                .and_then(|m| {
                                    m.config
                                        .as_ref()?
                                        .get("level")
                                        .or_else(|| m.config.as_ref()?.get("log_level"))
                                })
                                .map(|v| {
                                    v.as_str()
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| v.to_string())
                                })
                                .unwrap_or_else(|| "info".to_string());

                            let log_format = log_module_cfg
                                .and_then(|m| m.config.as_ref()?.get("format"))
                                .map(|v| {
                                    v.as_str()
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| v.to_string())
                                })
                                .unwrap_or_else(|| "default".to_string());

                            println!(
                                "Log level from config: {}, Log format: {}",
                                log_level, log_format
                            );

                            if log_format.to_lowercase() == "json" {
                                init_prod_log(log_level.as_str());
                            } else {
                                init_local_log(log_level.as_str());
                            }
                        }
                        None => {
                            println!(
                                "LoggingModule not found in config, using default local logging"
                            );
                            init_local_log("info");
                        }
                    }
                }
                Err(err) => {
                    eprintln!("Failed to parse config file {}: {}", path, err);
                    init_local_log("info");
                }
            }
        }
        Err(_) => {
            println!("No config for logging found at {}, using default", path);
            init_local_log("info");
        }
    }
}

fn init_prod_log(log_level: &str) {
    TRACING.get_or_init(|| {
        let filter = EnvFilter::new(log_level);
        tracing_subscriber::fmt::Subscriber::builder()
            .with_env_filter(filter)
            .json()
            .with_current_span(true)
            .with_span_list(true)
            .init();
    });
}

fn init_local_log(log_level: &str) {
    TRACING.get_or_init(|| {
        let filter = EnvFilter::new(log_level);
        tracing_subscriber::fmt::Subscriber::builder()
            .with_env_filter(filter)
            .event_format(IIILogFormatter)
            .init();
    });
}
