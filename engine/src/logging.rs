// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::{fmt, sync::OnceLock};

use chrono::Local;
use colored::Colorize;
use tracing::{
    Event, Level, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{
    EnvFilter,
    fmt::{self as tracing_fmt, FmtContext, FormatEvent, FormatFields},
    layer::SubscriberExt,
    registry::LookupSpan,
    util::SubscriberInitExt,
};

use crate::telemetry::{ExporterType, OtelConfig, init_otel};
use crate::workers::config::EngineConfig;
use crate::workers::observability::logs_layer::OtelLogsLayer;
use crate::workers::observability::otel::{
    OTEL_PASSTHROUGH_TARGET, get_log_storage, get_otel_config, init_log_storage, logs_enabled,
};

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

    /// Get fields excluding the "function" field (always hidden since it's
    /// shown in the header). When `is_passthrough` is true, also hides the
    /// "service" and "function_name" fields (rendered as the header) and
    /// any "data" field whose string value is empty (would otherwise render
    /// as `""`).
    fn get_display_fields_filtered(&self, is_passthrough: bool) -> Vec<(&String, &FieldValue)> {
        self.fields
            .iter()
            .filter(|(name, value)| {
                if name == "function" {
                    return false;
                }
                if is_passthrough {
                    if name == "service" || name == "function_name" {
                        return false;
                    }
                    if name == "data" && matches!(value, FieldValue::String(s) if s.is_empty()) {
                        return false;
                    }
                }
                true
            })
            .map(|(name, value)| (name, value))
            .collect()
    }

    /// Look up a field by name and return its value as a plain string.
    fn field_as_string(&self, key: &str) -> Option<String> {
        self.fields.iter().find_map(|(name, value)| {
            if name != key {
                return None;
            }
            Some(match value {
                FieldValue::String(s) => s.clone(),
                FieldValue::Debug(s) => s.trim_matches('"').to_string(),
                FieldValue::I64(n) => n.to_string(),
                FieldValue::U64(n) => n.to_string(),
                FieldValue::F64(n) => n.to_string(),
                FieldValue::Bool(b) => b.to_string(),
            })
        })
    }
}

/// The worker (service) name and step (service.name) name for an OTEL
/// passthrough event, each optional. Extracted from tracing fields emitted by
/// `emit_log_to_console`.
#[derive(Debug, Default)]
struct PassthroughNames {
    /// Worker service name, e.g. `todo-worker-python`.
    worker: Option<String>,
    /// Step/function name, e.g. `api.get./todos`. Parsed out of the `data`
    /// JSON blob under the `service.name` attribute.
    step: Option<String>,
}

impl PassthroughNames {
    fn is_empty(&self) -> bool {
        self.worker.is_none() && self.step.is_none()
    }
}

/// Extract the passthrough worker + step names from the collected fields.
///
/// Primary source: the `function_name` field emitted directly by
/// `emit_log_to_console`. Falls back to parsing `service.name` out of the
/// `data` JSON blob for events produced before the `function_name` field
/// was added.
fn extract_passthrough_names(collector: &FieldCollector) -> PassthroughNames {
    let worker = collector
        .field_as_string("service")
        .filter(|s| !s.is_empty());

    let step = collector
        .field_as_string("function_name")
        .filter(|s| !s.is_empty())
        .or_else(|| {
            collector.field_as_string("data").and_then(|data| {
                let parsed: serde_json::Value = serde_json::from_str(&data).ok()?;
                parsed
                    .get("service.name")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
        });

    PassthroughNames { worker, step }
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

        // For OTEL-ingested logs from SDK workers, render two separately-
        // colored header tokens: worker (blue) and step/service.name
        // (purple). Falls back gracefully when one or both are missing.
        let is_passthrough = meta.target() == OTEL_PASSTHROUGH_TARGET;
        let passthrough = if is_passthrough {
            extract_passthrough_names(&collector)
        } else {
            PassthroughNames::default()
        };

        if is_passthrough && !passthrough.is_empty() {
            if let Some(w) = passthrough.worker.as_deref() {
                write!(writer, "{} ", w.blue().bold())?;
            }
            if let Some(s) = passthrough.step.as_deref() {
                write!(writer, "{} ", s.purple().bold())?;
            }
        } else {
            let display_name = if is_passthrough {
                OTEL_PASSTHROUGH_TARGET
            } else {
                collector.get_function().unwrap_or(meta.target())
            };
            write!(writer, "{} ", display_name.cyan().bold())?;
        }

        // Write message if present
        if let Some(msg) = &collector.message {
            write!(writer, "{}", msg.white())?;
        }

        // Render fields as tree. Hide "function" always; hide "service" and
        // empty "data" on passthrough events since they're already in the
        // header / vestigial after service.name stripping in emit_log_to_console.
        let display_fields = collector.get_display_fields_filtered(is_passthrough);
        let tree = render_fields_tree(&display_fields);
        write!(writer, "{}", tree)?;

        writeln!(writer)
    }
}

static TRACING: OnceLock<()> = OnceLock::new();

/// Extract OTEL configuration from the ObservabilityWorker config in the config file.
/// This is called early during startup, before modules are loaded.
fn extract_otel_config(cfg: &EngineConfig) -> OtelConfig {
    use crate::workers::observability::config::ObservabilityWorkerConfig;

    let otel_module_name = "iii-observability";
    let otel_module_cfg = cfg
        .modules
        .iter()
        .chain(cfg.workers.iter())
        .find(|m| m.name == otel_module_name);

    let module_config: ObservabilityWorkerConfig = match otel_module_cfg {
        Some(entry) => match &entry.config {
            Some(cfg) => serde_json::from_value(cfg.clone()).unwrap_or_default(),
            None => ObservabilityWorkerConfig::default(),
        },
        None => return OtelConfig::default(),
    };

    let mut otel_cfg = OtelConfig::default();

    if let Some(enabled) = module_config.enabled {
        otel_cfg.enabled = enabled;
    }
    if let Some(service_name) = module_config.service_name {
        otel_cfg.service_name = service_name;
    }
    if let Some(exporter) = module_config.exporter {
        otel_cfg.exporter = match exporter {
            crate::workers::observability::config::OtelExporterType::Memory => ExporterType::Memory,
            crate::workers::observability::config::OtelExporterType::Otlp => ExporterType::Otlp,
            crate::workers::observability::config::OtelExporterType::Both => ExporterType::Both,
        };
    }
    if let Some(endpoint) = module_config.endpoint {
        otel_cfg.endpoint = endpoint;
    }
    if let Some(sampling) = module_config.sampling_ratio {
        otel_cfg.sampling_ratio = sampling;
    }
    if let Some(max_spans) = module_config.memory_max_spans {
        otel_cfg.memory_max_spans = max_spans;
    }

    otel_cfg
}

pub fn init_log_from_engine_config(cfg: &EngineConfig) {
    let otel_cfg = extract_otel_config(cfg);
    let otel_module_name = "iii-observability";
    let otel_module_cfg = cfg
        .modules
        .iter()
        .chain(cfg.workers.iter())
        .find(|m| m.name == otel_module_name);

    let log_level = otel_module_cfg
        .and_then(|m| m.config.as_ref())
        .and_then(|c| c.get("level").or_else(|| c.get("log_level")))
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "info".to_string());

    let log_format = otel_module_cfg
        .and_then(|m| m.config.as_ref())
        .and_then(|c| c.get("format"))
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "default".to_string());

    println!(
        "Log level from config: {}, Log format: {}, OTel enabled: {}",
        log_level, log_format, otel_cfg.enabled
    );

    if log_format.to_lowercase() == "json" {
        init_prod_log(log_level.as_str(), &otel_cfg);
    } else {
        init_local_log(log_level.as_str(), &otel_cfg);
    }
}

/// Initializes logging, strictly loading config from the given path.
/// If `config_path` is `None`, initializes with default local logging.
/// If the file is missing or unparseable, falls back to default local logging
/// (logging init should never crash the process).
pub fn init_log_from_config(config_path: Option<&str>) {
    match config_path {
        None => {
            println!("No config file specified, using default local logging");
            init_local_log("info", &OtelConfig::default());
        }
        Some(path) => {
            println!("Initializing logging from config file: {}", path);
            let cfg = EngineConfig::config_file(path);
            if let Err(e) = cfg {
                println!(
                    "Failed to load config file for logging: {}, using default local logging. Error: {}",
                    path, e
                );
                init_local_log("info", &OtelConfig::default());
                return;
            }

            let cfg = cfg.expect("already checked");
            println!("Parsed config file: {}", path);
            init_log_from_engine_config(&cfg);
        }
    }
}

/// Disable ANSI color output in the `colored` crate process-wide.
///
/// Why: the engine's `tracing::info!(...)` macros interpolate colored
/// strings (e.g. `"[UNREGISTERED]".red()`) that the `colored` crate
/// resolves to ANSI escapes at call time. When the JSON formatter then
/// serializes the `message` field, those escape bytes are preserved
/// verbatim, producing log lines like `"\u001b[31m[UNREGISTERED]\u001b[0m"`
/// that break downstream JSON log consumers (MOT-2812).
///
/// Setting the `colored` override to `false` makes every subsequent
/// `.red()` / `.bold()` / etc. a no-op string wrapper, so JSON logs stay
/// plain ASCII. Local text logging never calls this, so human-readable
/// logs keep their colors.
fn disable_ansi_for_json_logs() {
    colored::control::set_override(false);
}

fn init_prod_log(log_level: &str, otel_cfg: &OtelConfig) {
    TRACING.get_or_init(|| {
        // Prevent ANSI escape codes from leaking into JSON-formatted logs.
        // See `disable_ansi_for_json_logs` for rationale (MOT-2812).
        disable_ansi_for_json_logs();

        let filter = EnvFilter::new(log_level);

        // JSON formatting layer
        let fmt_layer = tracing_subscriber::fmt::layer()
            .json()
            .with_current_span(true)
            .with_span_list(true);

        // Build the subscriber with optional OTel layers
        // We need to initialize OTel first to get the layers with correct types
        let otel_trace_layer = init_otel(otel_cfg);

        // Initialize OTEL logs layer if enabled
        let otel_logs_layer = if otel_cfg.enabled && logs_enabled(get_otel_config()) {
            // Get max logs from global config (if set) or use default
            let max_logs = get_otel_config()
                .and_then(|cfg| cfg.logs_max_count)
                .or(Some(1000));

            // Initialize log storage
            init_log_storage(max_logs);

            // Create logs layer
            get_log_storage()
                .map(|storage| OtelLogsLayer::new(storage, otel_cfg.service_name.clone()))
        } else {
            None
        };

        tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .with(otel_trace_layer)
            .with(otel_logs_layer)
            .init();
    });
}

fn init_local_log(log_level: &str, otel_cfg: &OtelConfig) {
    TRACING.get_or_init(|| {
        let filter = EnvFilter::new(log_level);

        // Custom formatting layer
        let fmt_layer = tracing_subscriber::fmt::layer().event_format(IIILogFormatter);

        // Build the subscriber with optional OTel layers
        let otel_trace_layer = init_otel(otel_cfg);

        // Initialize OTEL logs layer if enabled
        let otel_logs_layer = if otel_cfg.enabled && logs_enabled(get_otel_config()) {
            // Get max logs from global config (if set) or use default
            let max_logs = get_otel_config()
                .and_then(|cfg| cfg.logs_max_count)
                .or(Some(1000));

            // Initialize log storage
            init_log_storage(max_logs);

            // Create logs layer
            get_log_storage()
                .map(|storage| OtelLogsLayer::new(storage, otel_cfg.service_name.clone()))
        } else {
            None
        };

        tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .with(otel_trace_layer)
            .with(otel_logs_layer)
            .init();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::config::{EngineConfig, WorkerEntry};
    use serial_test::serial;
    use tracing::callsite::Identifier;
    use tracing::field::{FieldSet, Visit};
    use tracing::metadata::{Kind, Metadata};

    /// Helper: builds a `FieldSet` (and backing static metadata) that contains
    /// the given field names, and returns a closure that can look up any of
    /// those names to produce a `Field`.
    ///
    /// We need a `'static` callsite / metadata, so we use `Box::leak` once per
    /// test-helper invocation.  This is fine for tests.
    macro_rules! make_fields {
        ($($name:expr),+ $(,)?) => {{
            // Static array of field names – leaked so that it lives for 'static.
            let names: &'static [&'static str] = Box::leak(Box::new([$($name),+]));

            // We need a Callsite impl that lives for 'static.
            struct TestCallsite;
            static TEST_CALLSITE: TestCallsite = TestCallsite;
            impl tracing::Callsite for TestCallsite {
                fn set_interest(&self, _: tracing::subscriber::Interest) {}
                fn metadata(&self) -> &Metadata<'_> {
                    static META: std::sync::OnceLock<Metadata<'static>> = std::sync::OnceLock::new();
                    META.get_or_init(|| {
                        Metadata::new(
                            "test",
                            "test_target",
                            tracing::Level::INFO,
                            None,
                            None,
                            None,
                            FieldSet::new(&[], Identifier(&TEST_CALLSITE)),
                            Kind::EVENT,
                        )
                    })
                }
            }

            let field_set = FieldSet::new(names, Identifier(&TEST_CALLSITE));
            field_set
        }};
    }

    #[test]
    fn test_init_log_from_config_with_none_does_not_panic() {
        // With None, should use default local logging
        init_log_from_config(None);
    }

    #[test]
    fn test_init_log_from_config_with_missing_file_does_not_panic() {
        // Should still init logging even if file doesn't exist —
        // logging init should not crash the process, just fall back
        init_log_from_config(Some("/tmp/iii_no_such_logging_config_98765.yaml"));
    }

    #[test]
    #[serial]
    fn colored_emits_ansi_when_override_true() {
        // Baseline: when we force the colored override on, ANSI escapes appear in the
        // output. Regression guard so Task 2's JSON-mode test has something to negate.
        colored::control::set_override(true);
        let s = format!("{}", "hello".red());
        assert!(
            s.contains('\u{1b}'),
            "expected ANSI escape in colored output with override=true, got {:?}",
            s
        );
        colored::control::unset_override();
    }

    #[test]
    #[serial]
    fn init_prod_log_disables_ansi_in_colored_crate() {
        // Arrange: force ANSI on, then sanity-check the precondition.
        colored::control::set_override(true);
        assert!(
            format!("{}", "x".red()).contains('\u{1b}'),
            "precondition failed: colored should be emitting ANSI when override is true"
        );

        // Act: apply just the color-override step from the JSON init path.
        // We cannot call `init_prod_log` directly in a unit test because it
        // installs a global tracing subscriber via OnceCell — only one process-
        // wide init is allowed. Instead, we call the small extracted helper.
        disable_ansi_for_json_logs();

        // Assert: any subsequent `.red()` / `.purple()` produces plain text.
        let red = format!("{}", "[UNREGISTERED]".red());
        let purple = format!("{}", "discord::send_message".purple());
        assert_eq!(red, "[UNREGISTERED]", "red() must not inject ANSI");
        assert_eq!(
            purple, "discord::send_message",
            "purple() must not inject ANSI"
        );
        assert!(!red.contains('\u{1b}'));
        assert!(!purple.contains('\u{1b}'));

        // Cleanup so other #[serial] tests start from a known state.
        colored::control::unset_override();
    }

    // =========================================================================
    // FieldCollector tests
    // =========================================================================

    #[test]
    fn test_field_collector_new() {
        let collector = FieldCollector::new();
        assert!(collector.fields.is_empty());
        assert!(collector.message.is_none());
        assert!(collector.function.is_none());
    }

    #[test]
    fn test_field_collector_records_fields() {
        let mut collector = FieldCollector::new();

        let fs = make_fields!("alpha", "beta", "gamma", "delta", "epsilon");

        // record_str
        let f_alpha = fs.field("alpha").expect("alpha field");
        collector.record_str(&f_alpha, "hello");

        // record_f64
        let f_beta = fs.field("beta").expect("beta field");
        collector.record_f64(&f_beta, std::f64::consts::PI);

        // record_i64
        let f_gamma = fs.field("gamma").expect("gamma field");
        collector.record_i64(&f_gamma, -42);

        // record_u64
        let f_delta = fs.field("delta").expect("delta field");
        collector.record_u64(&f_delta, 99);

        // record_bool
        let f_epsilon = fs.field("epsilon").expect("epsilon field");
        collector.record_bool(&f_epsilon, true);

        assert_eq!(collector.fields.len(), 5);

        // Verify each stored value
        assert!(
            matches!(&collector.fields[0], (name, FieldValue::String(s)) if name == "alpha" && s == "hello")
        );
        assert!(
            matches!(&collector.fields[1], (name, FieldValue::F64(v)) if name == "beta" && (*v - std::f64::consts::PI).abs() < f64::EPSILON)
        );
        assert!(
            matches!(&collector.fields[2], (name, FieldValue::I64(v)) if name == "gamma" && *v == -42)
        );
        assert!(
            matches!(&collector.fields[3], (name, FieldValue::U64(v)) if name == "delta" && *v == 99)
        );
        assert!(
            matches!(&collector.fields[4], (name, FieldValue::Bool(v)) if name == "epsilon" && *v)
        );
    }

    #[test]
    fn test_field_collector_get_function() {
        let mut collector = FieldCollector::new();
        let fs = make_fields!("function");
        let f = fs.field("function").unwrap();
        collector.record_str(&f, "my_fn");

        assert_eq!(collector.get_function(), Some("my_fn"));
        // "function" should NOT be stored in `fields` – it goes to the
        // dedicated `function` field.
        assert!(collector.fields.is_empty());
    }

    #[test]
    fn test_field_collector_get_function_missing() {
        let collector = FieldCollector::new();
        assert_eq!(collector.get_function(), None);
    }

    #[test]
    fn test_field_collector_get_display_fields() {
        let mut collector = FieldCollector::new();
        let fs = make_fields!("function", "extra", "other");

        // record "function" – should be excluded from display
        let f_fn = fs.field("function").unwrap();
        collector.record_str(&f_fn, "my_fn");

        // record normal fields
        let f_extra = fs.field("extra").unwrap();
        collector.record_str(&f_extra, "value1");

        let f_other = fs.field("other").unwrap();
        collector.record_i64(&f_other, 7);

        let display = collector.get_display_fields_filtered(false);
        // "function" is stored on the dedicated field, not in `fields`,
        // so the filtered view just returns whatever is in `fields`.
        assert_eq!(display.len(), 2);
        assert_eq!(display[0].0, "extra");
        assert_eq!(display[1].0, "other");
    }

    #[test]
    fn test_field_collector_record_debug_message() {
        let mut collector = FieldCollector::new();
        let fs = make_fields!("message");
        let f = fs.field("message").unwrap();
        collector.record_debug(&f, &"debug message");

        assert!(collector.message.is_some());
        assert!(
            collector
                .message
                .as_ref()
                .unwrap()
                .contains("debug message")
        );
    }

    #[test]
    fn test_field_collector_record_debug_function() {
        let mut collector = FieldCollector::new();
        let fs = make_fields!("function");
        let f = fs.field("function").unwrap();
        collector.record_debug(&f, &"my_debug_fn");

        assert_eq!(collector.get_function(), Some("my_debug_fn"));
    }

    #[test]
    fn test_field_collector_record_debug_regular() {
        let mut collector = FieldCollector::new();
        let fs = make_fields!("custom");
        let f = fs.field("custom").unwrap();
        collector.record_debug(&f, &42_i32);

        assert_eq!(collector.fields.len(), 1);
        assert!(matches!(&collector.fields[0], (name, FieldValue::Debug(_)) if name == "custom"));
    }

    // =========================================================================
    // render_field_value tests
    // =========================================================================

    /// Helper to strip ANSI escape codes from colored output so we can assert
    /// on the plain text content.
    fn strip_ansi(s: &str) -> String {
        let re = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
        re.replace_all(s, "").to_string()
    }

    #[test]
    fn test_render_field_value() {
        // String
        let s = render_field_value(&FieldValue::String("hello".into()));
        assert_eq!(strip_ansi(&s), "\"hello\"");

        // I64
        let s = render_field_value(&FieldValue::I64(-7));
        assert_eq!(strip_ansi(&s), "-7");

        // U64
        let s = render_field_value(&FieldValue::U64(42));
        assert_eq!(strip_ansi(&s), "42");

        // F64
        let s = render_field_value(&FieldValue::F64(1.5));
        assert_eq!(strip_ansi(&s), "1.5");

        // Bool
        let s = render_field_value(&FieldValue::Bool(true));
        assert_eq!(strip_ansi(&s), "true");

        let s = render_field_value(&FieldValue::Bool(false));
        assert_eq!(strip_ansi(&s), "false");
    }

    #[test]
    fn test_render_field_value_debug_json() {
        // A Debug value that happens to be valid JSON should be pretty-printed
        let json_str = r#"{"key":"val"}"#;
        let s = render_field_value(&FieldValue::Debug(json_str.to_string()));
        let plain = strip_ansi(&s);
        assert!(plain.contains("key"));
        assert!(plain.contains("val"));
    }

    #[test]
    fn test_render_field_value_debug_non_json() {
        // Non-JSON debug value is rendered as-is
        let s = render_field_value(&FieldValue::Debug("just text".into()));
        assert_eq!(strip_ansi(&s), "just text");
    }

    // =========================================================================
    // render_json_value tests
    // =========================================================================

    #[test]
    fn test_render_json_value_object() {
        let obj = serde_json::json!({
            "name": "Alice",
            "nested": {
                "deep": true
            }
        });
        let rendered = render_json_value(&obj, 2);
        let plain = strip_ansi(&rendered);

        // Should contain opening / closing braces
        assert!(plain.contains('{'));
        assert!(plain.contains('}'));
        // Should contain the key names
        assert!(plain.contains("name"));
        assert!(plain.contains("nested"));
        assert!(plain.contains("deep"));
        // Should contain tree branch chars
        assert!(plain.contains("├") || plain.contains("└"));
    }

    #[test]
    fn test_render_json_value_object_empty() {
        let obj = serde_json::json!({});
        let rendered = render_json_value(&obj, 2);
        let plain = strip_ansi(&rendered);
        assert_eq!(plain, "{}");
    }

    #[test]
    fn test_render_json_value_array() {
        let arr = serde_json::json!([1, "two", false]);
        let rendered = render_json_value(&arr, 2);
        let plain = strip_ansi(&rendered);

        assert!(plain.contains('['));
        assert!(plain.contains(']'));
        assert!(plain.contains('1'));
        assert!(plain.contains("\"two\""));
        assert!(plain.contains("false"));
    }

    #[test]
    fn test_render_json_value_array_empty() {
        let arr = serde_json::json!([]);
        let rendered = render_json_value(&arr, 2);
        let plain = strip_ansi(&rendered);
        assert_eq!(plain, "[]");
    }

    #[test]
    fn test_render_json_value_primitives() {
        // String
        let v = serde_json::json!("hello");
        assert_eq!(strip_ansi(&render_json_value(&v, 0)), "\"hello\"");

        // Number
        let v = serde_json::json!(42);
        assert_eq!(strip_ansi(&render_json_value(&v, 0)), "42");

        let v = serde_json::json!(std::f64::consts::PI);
        assert_eq!(
            strip_ansi(&render_json_value(&v, 0)),
            std::f64::consts::PI.to_string()
        );

        // Bool
        let v = serde_json::json!(true);
        assert_eq!(strip_ansi(&render_json_value(&v, 0)), "true");

        // Null
        let v = serde_json::json!(null);
        assert_eq!(strip_ansi(&render_json_value(&v, 0)), "null");
    }

    // =========================================================================
    // render_fields_tree tests
    // =========================================================================

    #[test]
    fn test_render_fields_tree_empty() {
        let fields: Vec<(&String, &FieldValue)> = vec![];
        let result = render_fields_tree(&fields);
        assert_eq!(result, String::new());
    }

    #[test]
    fn test_render_fields_tree() {
        let name1 = "alpha".to_string();
        let val1 = FieldValue::String("hello".into());
        let name2 = "beta".to_string();
        let val2 = FieldValue::I64(42);

        let fields: Vec<(&String, &FieldValue)> = vec![(&name1, &val1), (&name2, &val2)];
        let result = render_fields_tree(&fields);
        let plain = strip_ansi(&result);

        // Starts with a newline
        assert!(plain.starts_with('\n'));
        // First field uses ├ (not last), second uses └ (last)
        assert!(plain.contains("├"));
        assert!(plain.contains("└"));
        // Contains field names
        assert!(plain.contains("alpha"));
        assert!(plain.contains("beta"));
        // Contains rendered values
        assert!(plain.contains("\"hello\""));
        assert!(plain.contains("42"));
    }

    #[test]
    fn test_render_fields_tree_single_field() {
        let name = "only".to_string();
        let val = FieldValue::Bool(true);
        let fields: Vec<(&String, &FieldValue)> = vec![(&name, &val)];
        let result = render_fields_tree(&fields);
        let plain = strip_ansi(&result);

        // Single field should use └ (last/only)
        assert!(plain.contains("└"));
        assert!(!plain.contains("├"));
        assert!(plain.contains("only"));
        assert!(plain.contains("true"));
    }

    // =========================================================================
    // format_timestamp tests
    // =========================================================================

    #[test]
    fn test_format_timestamp() {
        let ts = format_timestamp();
        // Expected format: [HH:MM:SS.mmm AM/PM]
        // Examples: [09:19:23.241 AM], [12:00:00.000 PM]
        let re = regex::Regex::new(r"^\[\d{2}:\d{2}:\d{2}\.\d{3,} [AP]M\]$").unwrap();
        assert!(
            re.is_match(&ts),
            "Timestamp '{}' does not match expected format [HH:MM:SS.mmm AM/PM]",
            ts
        );
    }

    #[test]
    fn test_extract_otel_config_reads_observability_module_config() {
        let cfg = EngineConfig {
            modules: vec![WorkerEntry {
                name: "iii-observability".to_string(),
                image: None,
                config: Some(serde_json::json!({
                    "enabled": true,
                    "service_name": "test-service",
                    "exporter": "memory",
                    "endpoint": "http://collector:4317",
                    "sampling_ratio": 0.25,
                    "memory_max_spans": 321
                })),
            }],
            workers: vec![],
        };

        let otel = extract_otel_config(&cfg);
        assert!(otel.enabled);
        assert_eq!(otel.service_name, "test-service");
        assert!(matches!(otel.exporter, ExporterType::Memory));
        assert_eq!(otel.endpoint, "http://collector:4317");
        assert_eq!(otel.sampling_ratio, 0.25);
        assert_eq!(otel.memory_max_spans, 321);
    }

    #[test]
    fn test_extract_otel_config_reads_observability_from_workers_key() {
        let cfg = EngineConfig {
            modules: vec![],
            workers: vec![WorkerEntry {
                name: "iii-observability".to_string(),
                image: None,
                config: Some(serde_json::json!({
                    "enabled": true,
                    "service_name": "workers-key-test",
                    "exporter": "memory",
                })),
            }],
        };

        let otel = extract_otel_config(&cfg);
        assert!(
            otel.enabled,
            "should find observability config under workers key"
        );
        assert_eq!(otel.service_name, "workers-key-test");
        assert!(matches!(otel.exporter, ExporterType::Memory));
    }

    #[test]
    fn test_extract_otel_config_defaults_when_module_missing() {
        let cfg = EngineConfig {
            modules: vec![],
            workers: vec![],
        };

        let otel = extract_otel_config(&cfg);
        assert!(!otel.enabled);
        assert!(matches!(otel.exporter, ExporterType::Otlp));
    }

    #[test]
    fn test_extract_otel_config_reads_default_engine_config() {
        let cfg = EngineConfig::default_config();

        let otel = extract_otel_config(&cfg);
        assert!(otel.enabled);
        assert_eq!(otel.service_name, "iii");
        assert!(matches!(otel.exporter, ExporterType::Memory));
    }

    #[test]
    fn test_init_log_from_engine_config_uses_default_otel_config() {
        let cfg = EngineConfig::default_config();

        init_log_from_engine_config(&cfg);
    }

    #[test]
    #[serial]
    fn test_init_log_with_config_file_initializes_tracing_once() {
        let path = std::env::temp_dir().join(format!(
            "iii-logging-{}-{}.yaml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let yaml = r#"
modules:
  - class: workers::observability::ObservabilityWorker
    config:
      enabled: false
      level: debug
      format: default
      service_name: logging-test
      exporter: memory
      endpoint: http://localhost:4317
      sampling_ratio: 0.5
      memory_max_spans: 64
"#;

        std::fs::write(&path, yaml).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(TRACING.get().is_some());
    }

    // =========================================================================
    // OTEL passthrough header tests
    // =========================================================================

    #[test]
    fn test_extract_passthrough_names_reads_function_name_field_directly() {
        let mut collector = FieldCollector::new();
        let fs = make_fields!("service", "function_name", "data");

        collector.record_str(&fs.field("service").unwrap(), "todo-worker-python");
        collector.record_str(&fs.field("function_name").unwrap(), "api.get./todos");
        collector.record_str(&fs.field("data").unwrap(), r#"{"log.data":{"count":0}}"#);

        let names = extract_passthrough_names(&collector);
        assert_eq!(names.worker.as_deref(), Some("todo-worker-python"));
        assert_eq!(names.step.as_deref(), Some("api.get./todos"));
        assert!(!names.is_empty());
    }

    #[test]
    fn test_extract_passthrough_names_falls_back_to_parsing_data() {
        let mut collector = FieldCollector::new();
        let fs = make_fields!("service", "function_name", "data");

        collector.record_str(&fs.field("service").unwrap(), "todo-worker-python");
        // function_name field present but empty (what emit_log_to_console
        // emits when the `service.name` attribute isn't set).
        collector.record_str(&fs.field("function_name").unwrap(), "");
        // Legacy: function name lives only in the data JSON.
        collector.record_str(
            &fs.field("data").unwrap(),
            r#"{"service.name":"api.legacy"}"#,
        );

        let names = extract_passthrough_names(&collector);
        assert_eq!(names.worker.as_deref(), Some("todo-worker-python"));
        assert_eq!(names.step.as_deref(), Some("api.legacy"));
    }

    #[test]
    fn test_extract_passthrough_names_step_only_when_worker_missing() {
        let mut collector = FieldCollector::new();
        let fs = make_fields!("data");

        collector.record_str(
            &fs.field("data").unwrap(),
            r#"{"service.name":"api.get./todos"}"#,
        );

        let names = extract_passthrough_names(&collector);
        assert!(names.worker.is_none());
        assert_eq!(names.step.as_deref(), Some("api.get./todos"));
    }

    #[test]
    fn test_extract_passthrough_names_empty_when_nothing_present() {
        let collector = FieldCollector::new();
        let names = extract_passthrough_names(&collector);
        assert!(names.is_empty());
        assert!(names.worker.is_none());
        assert!(names.step.is_none());
    }

    #[test]
    fn test_get_display_fields_filtered_hides_header_fields_and_empty_data_on_passthrough() {
        let mut collector = FieldCollector::new();
        let fs = make_fields!("service", "function_name", "data", "function", "extra");

        collector.record_str(&fs.field("service").unwrap(), "worker");
        collector.record_str(&fs.field("function_name").unwrap(), "api.get./todos");
        // Empty string — mirrors the case where emit_log_to_console stripped
        // the only attribute (`service.name`) leaving nothing to render.
        collector.record_str(&fs.field("data").unwrap(), "");
        collector.record_str(&fs.field("function").unwrap(), "fn-name");
        collector.record_str(&fs.field("extra").unwrap(), "keep");

        // Non-passthrough case: only "function" is hidden.
        let visible: Vec<&String> = collector
            .get_display_fields_filtered(false)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert!(visible.iter().any(|n| *n == "service"));
        assert!(visible.iter().any(|n| *n == "function_name"));
        assert!(visible.iter().any(|n| *n == "data"));
        assert!(visible.iter().any(|n| *n == "extra"));
        assert!(!visible.iter().any(|n| *n == "function"));

        // Passthrough case: function, service, function_name, AND empty data hidden.
        let visible: Vec<&String> = collector
            .get_display_fields_filtered(true)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert!(!visible.iter().any(|n| *n == "service"));
        assert!(!visible.iter().any(|n| *n == "function_name"));
        assert!(!visible.iter().any(|n| *n == "function"));
        assert!(!visible.iter().any(|n| *n == "data"));
        assert!(visible.iter().any(|n| *n == "extra"));
    }

    #[test]
    fn test_get_display_fields_filtered_keeps_nonempty_data_on_passthrough() {
        let mut collector = FieldCollector::new();
        let fs = make_fields!("data");

        collector.record_str(&fs.field("data").unwrap(), r#"{"log.data":"x"}"#);

        let visible: Vec<&String> = collector
            .get_display_fields_filtered(true)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert!(visible.iter().any(|n| *n == "data"));
    }
}
