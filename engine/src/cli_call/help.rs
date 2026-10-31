// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use anyhow::Result;
use colored::Colorize;
use iii_sdk::protocol::CallRequest;
use iii_sdk::{InitOptions, register_worker};
use serde_json::{Value, json};

pub async fn print(
    function_path: Option<&str>,
    address: &str,
    port: u16,
    timeout_ms: u64,
    namespace: Option<&str>,
) -> Result<()> {
    match function_path {
        None => {
            print_static_help();
            Ok(())
        }
        Some(fn_path) => match fetch_fn_meta(fn_path, address, port, timeout_ms, namespace).await {
            Ok(Some(meta)) => {
                render_fn_help(fn_path, &meta);
                Ok(())
            }
            Ok(None) => {
                if let Some(message) = crate::legacy_worker_functions::migration_message(fn_path) {
                    eprintln!("{} {message}", "error:".red());
                } else {
                    eprintln!(
                        "{} function `{}` not found in engine registry.",
                        "error:".red(),
                        fn_path
                    );
                    eprintln!(
                        "  {} run `iii call engine::functions::list` to see registered functions.",
                        "hint:".dimmed()
                    );
                }
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!(
                    "{} could not query engine for `{}`: {}",
                    "warning:".yellow(),
                    fn_path,
                    e
                );
                eprintln!("Showing static CLI help only.");
                eprintln!();
                print_static_help();
                Ok(())
            }
        },
    }
}

async fn fetch_fn_meta(
    fn_path: &str,
    address: &str,
    port: u16,
    timeout_ms: u64,
    namespace: Option<&str>,
) -> Result<Option<Value>> {
    let url = format!("ws://{}:{}", address, port);
    let iii = register_worker(&url, InitOptions::default());

    // `engine::functions::info` is the only surface that carries the
    // request/response schemas — `engine::functions::list` returns slim
    // summaries (id + description) and would leave the Parameters table
    // permanently empty.
    // `engine::functions::info` itself lives in `default`, but it takes the
    // target namespace in its payload: the same id can exist in several.
    let mut payload = json!({ "function_id": fn_path });
    if let Some(namespace) = namespace {
        payload["namespace"] = json!(namespace);
    }

    let result = iii
        .call(CallRequest {
            function_id: "engine::functions::info".to_string(),
            payload,
            action: None,
            timeout_ms: Some(timeout_ms),
        })
        .await;

    iii.shutdown_async().await;

    match result {
        Ok(value) => Ok(Some(value)),
        Err(e) => {
            // Distinguish two "not found" shapes (case-insensitive):
            // 1. The dispatcher couldn't find `engine::functions::info`
            //    itself — an engine that predates the introspection fn.
            //    Surface as Err so `print()` degrades to static help
            //    instead of falsely claiming the TARGET function is absent.
            // 2. The target function isn't registered — map to Ok(None)
            //    so the caller prints the friendly "not found" hint.
            let msg = e.to_string().to_ascii_lowercase();
            let looks_not_found = msg.contains("not_found")
                || msg.contains("not found")
                || msg.contains("not registered");
            if looks_not_found && msg.contains("engine::functions::info") {
                Err(anyhow::anyhow!(
                    "engine does not support engine::functions::info; showing static help"
                ))
            } else if looks_not_found {
                Ok(None)
            } else {
                Err(anyhow::anyhow!("{}", e))
            }
        }
    }
}

/// True for characters a worker-supplied string must never carry onto the
/// terminal: control chars (ANSI ESC, C1, DEL) plus the Unicode bidi
/// override/isolate range (U+202A–202E, U+2066–2069 — the Trojan-Source
/// class) and the line/paragraph separators U+2028/U+2029. `\n` is handled
/// by callers (kept for the about block, collapsed in table cells).
fn is_unsafe_terminal_char(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{2028}' | '\u{2029}')
}

/// Engine-provided strings originate from whatever worker registered the
/// function — strip unsafe characters before printing to the user's terminal
/// so a malicious worker can't smuggle cursor/clipboard/hyperlink escape
/// sequences (or bidi spoofing) into `--help` output. Newlines are kept;
/// descriptions may legitimately span lines.
fn sanitize_terminal(s: &str) -> String {
    s.chars()
        .filter(|c| !is_unsafe_terminal_char(*c) || *c == '\n')
        .collect()
}

/// Like `sanitize_terminal`, but for a single markdown table cell: every
/// field rendered into the Parameters table (`name`, `type`, `description`)
/// is worker-controlled, so newlines/tabs are collapsed to spaces (a cell
/// must stay on one row) and all remaining control characters are stripped.
/// Without this, a property keyed with an ANSI/OSC sequence — or a multi-line
/// description — reaches the operator's terminal or splits the table.
fn sanitize_cell(s: &str) -> String {
    s.chars()
        .map(|c| if c == '\n' || c == '\t' { ' ' } else { c })
        .filter(|c| !is_unsafe_terminal_char(*c))
        .collect()
}

fn render_fn_help(fn_path: &str, meta: &Value) {
    let description = sanitize_terminal(
        meta.get("description")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
    );
    let description = description.as_str();

    // Drive sections individually via print_template so we can place the
    // schema-driven sections BEFORE the generic Options table that clap-help
    // builds from the call flags.
    let Some(call) = crate::cli_subcommand("call") else {
        return;
    };

    let mut printer = clap_help::Printer::new(call);
    printer
        .expander_mut()
        .set("name", help_title(fn_path, meta));
    printer.expander_mut().set("function", fn_path);
    if !description.is_empty() {
        printer.expander_mut().set("about", description.to_string());
    }

    // Title.
    printer.print_template(clap_help::TEMPLATE_TITLE);
    println!();
    // About.
    if !description.is_empty() {
        printer.print_template("\n${about}\n");
    }
    for section in help_sections(meta) {
        printer.print_template(&section);
    }
    // Options (clap-help default table for the call flags): only for a
    // function the CLI can call.
    if !is_private(meta) {
        printer.print_template(clap_help::TEMPLATE_OPTIONS);
    }
}

/// `private: true` on a function row: only the worker that hosts the function
/// calls it, and everyone else observes it (an event source).
fn is_private(meta: &Value) -> bool {
    meta.get("private").and_then(Value::as_bool) == Some(true)
}

/// `observable: false` on a function row: observing it fails with
/// `NOT_OBSERVABLE`. Absent means observable.
fn is_observable(meta: &Value) -> bool {
    meta.get("observable").and_then(Value::as_bool) != Some(false)
}

/// The help title: the command that calls the function, or the bare id of a
/// private one.
fn help_title(fn_path: &str, meta: &Value) -> String {
    if is_private(meta) {
        fn_path.to_string()
    } else {
        format!("iii call {fn_path}")
    }
}

/// The sections between the about block and the Options table, as the
/// markdown templates `clap_help` prints. `${function}` expands to the
/// function id.
///
/// A callable function gets its call usage and its Parameters. A private
/// function is observed rather than called: its help says so, shows how to
/// observe it, and lists its request schema as the Input every call carries.
/// Both show the observer args (`args_format`) and the observer count of an
/// observable function.
fn help_sections(meta: &Value) -> Vec<String> {
    let mut sections = Vec::new();
    if is_private(meta) {
        sections.push(
            "\n**Private:** only the worker that hosts `${function}` calls it. Observe it to \
             run a function on each of its calls:\n\n  `iii.observe('${function}', target)`\n"
                .to_string(),
        );
        sections.push(
            schema_table_md("Input", request_schema(meta))
                .unwrap_or_else(|| "\n**Input:**\n\n  *(no request schema published)*\n".into()),
        );
    } else {
        sections.push(
            "\n**Usage: ** `iii call ${function} [key=value ...] [--json '<obj>']`\n".to_string(),
        );
        sections.push(parameters_md(meta).unwrap_or_else(|| {
            "\n**Parameters:**\n\n  *(no request schema published)*\n".to_string()
        }));
    }

    if !is_observable(meta) {
        sections.push("\n**Not observable:** observing it fails with `NOT_OBSERVABLE`.\n".into());
        return sections;
    }
    if let Some(args_format) = meta.get("args_format").filter(|v| !v.is_null()) {
        sections.push(
            schema_table_md("Observer args", Some(args_format)).unwrap_or_else(|| {
                format!(
                    "\n**Observer args:** {}\n",
                    sanitize_cell(&schema_type(args_format)).replace('|', "\\|")
                )
            }),
        );
    }
    if let Some(count) = meta.get("observer_count").and_then(Value::as_u64) {
        sections.push(format!(
            "\n**Observers:** {count}\n\n  `iii call engine::observers::list function_id=${{function}}` lists them.\n"
        ));
    }
    sections
}

/// `functions::info` names the field `request_schema`; the legacy
/// `request_format` key is the fallback for older engines.
fn request_schema(meta: &Value) -> Option<&Value> {
    meta.get("request_schema")
        .or_else(|| meta.get("request_format"))
        .filter(|v| !v.is_null())
}

/// Render the schema-driven parameters as a markdown table that termimad
/// can display in the same style as clap-help's Options table. Returns
/// `None` when there is nothing useful to show.
fn parameters_md(meta: &Value) -> Option<String> {
    schema_table_md("Parameters", request_schema(meta))
}

/// A JSON Schema's properties as a `heading` table. `None` when the schema is
/// absent or declares no properties.
fn schema_table_md(heading: &str, schema: Option<&Value>) -> Option<String> {
    let schema = schema?;
    let props = schema.get("properties").and_then(|p| p.as_object())?;
    if props.is_empty() {
        return None;
    }

    let required: Vec<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let mut md = format!(
        "\n**{heading}:**\n|:-:|:-:|:-:|:-|\n|name|type|required|description|\n|:-:|:-|:-:|:-|\n"
    );
    for (name, prop) in props {
        // Escape `|` so multi-type values like `string|null` do not split the
        // markdown row into extra columns.
        let ty = sanitize_cell(&schema_type(prop)).replace('|', "\\|");
        let req = if required.contains(&name.as_str()) {
            "yes"
        } else {
            "no"
        };
        let desc = sanitize_cell(
            prop.get("description")
                .and_then(|d| d.as_str())
                .unwrap_or(""),
        )
        .replace('|', "\\|");
        // The property key is worker-controlled too — sanitize it before it
        // reaches the terminal.
        md.push_str(&format!(
            "|`{}`|{}|{}|{}|\n",
            sanitize_cell(name),
            ty,
            req,
            desc
        ));
    }
    md.push_str("|-\n");
    Some(md)
}

fn schema_type(schema: &Value) -> String {
    match schema.get("type") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join("|"),
        _ => "any".to_string(),
    }
}

fn print_static_help() {
    if let Some(call) = crate::cli_subcommand("call") {
        crate::render_clap_help(call);
    }
    println!(
        "{} `iii call <fn-path> --help` queries a running engine for the",
        "Tip:".bold()
    );
    println!("  function's description, request schema and observer args.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sanitize_terminal_strips_controls_but_keeps_newlines_and_multibyte() {
        assert_eq!(sanitize_terminal("a\u{1b}[31mb\u{7f}c"), "a[31mbc");
        assert_eq!(sanitize_terminal("line1\nline2"), "line1\nline2");
        assert_eq!(sanitize_terminal("\u{1b}\u{0007}"), "");
        assert_eq!(sanitize_terminal(""), "");
        assert_eq!(
            sanitize_terminal("caf\u{e9} \u{1f600}"),
            "caf\u{e9} \u{1f600}"
        );
        // Bidi overrides/isolates (Trojan-Source) are stripped, newline kept.
        assert_eq!(sanitize_terminal("a\u{202e}b\u{2066}c"), "abc");
    }

    #[test]
    fn sanitize_cell_collapses_newlines_and_strips_controls() {
        // Newlines/tabs become spaces so a worker-supplied value can't split
        // the markdown row; other control chars (ESC/DEL/BEL) and bidi
        // overrides are dropped.
        assert_eq!(sanitize_cell("a\nb\tc"), "a b c");
        assert_eq!(sanitize_cell("x\u{1b}]8;;evil\u{7}y"), "x]8;;evily");
        assert_eq!(sanitize_cell("\u{7f}\u{0007}"), "");
        assert_eq!(sanitize_cell("a\u{202e}b"), "ab");
        assert_eq!(sanitize_cell("plain"), "plain");
    }

    #[test]
    fn parameters_md_sanitizes_worker_controlled_name_and_type() {
        // A malicious worker controls its own request_schema, including
        // property keys and the `type` string. Neither may carry control
        // bytes into the operator's terminal via `--help`.
        let meta = json!({
            "request_schema": {
                "properties": {
                    "ev\u{1b}[31mil": { "type": "str\u{7f}ing", "description": "d" }
                }
            }
        });
        let md = parameters_md(&meta).unwrap();
        assert!(!md.contains('\u{1b}'), "ESC must not reach the table: {md}");
        assert!(!md.contains('\u{7f}'), "DEL must not reach the table: {md}");
    }

    #[test]
    fn parameters_md_prefers_request_schema_then_falls_back_to_request_format() {
        let with_new = json!({
            "request_schema": { "properties": { "name": { "type": "string" } } }
        });
        assert!(parameters_md(&with_new).is_some());

        let with_legacy = json!({
            "request_format": { "properties": { "name": { "type": "string" } } }
        });
        assert!(parameters_md(&with_legacy).is_some());
    }

    #[test]
    fn parameters_md_none_when_empty_null_or_missing() {
        assert!(parameters_md(&json!({ "request_schema": { "properties": {} } })).is_none());
        assert!(parameters_md(&json!({ "request_schema": null })).is_none());
        assert!(parameters_md(&json!({})).is_none());
    }

    /// `engine::functions::info` for `engine::on-observers-changed` on a 0.26
    /// engine, trimmed: private, no args, its event as the request schema.
    fn private_source_row() -> Value {
        json!({
            "function_id": "engine::on-observers-changed",
            "description": "Engine-hosted source: an observer was created or changed state.",
            "private": true,
            "observer_count": 3,
            "request_schema": {
                "type": "object",
                "properties": {
                    "observer_id": { "type": "string" },
                    "state": { "type": "string" }
                },
                "required": ["observer_id", "state"]
            }
        })
    }

    fn rendered(meta: &Value) -> String {
        help_sections(meta)
            .concat()
            .replace("${function}", "x::on-y")
    }

    #[test]
    fn a_private_function_is_observed_not_called() {
        let meta = private_source_row();
        let help = rendered(&meta);

        assert!(is_private(&meta));
        assert_eq!(
            help_title("engine::on-observers-changed", &meta),
            "engine::on-observers-changed",
            "the title names the function, not a call"
        );
        assert!(
            !help.contains("**Usage"),
            "no call usage for a private function: {help}"
        );
        assert!(!help.contains("**Parameters"), "{help}");
        assert!(help.contains("**Private:** only the worker that hosts `x::on-y` calls it"));
        assert!(
            help.contains("  `iii.observe('x::on-y', target)`"),
            "{help}"
        );
        assert!(
            help.contains("**Input:**") && help.contains("|`observer_id`|string|yes|"),
            "the request schema is each call's input: {help}"
        );
        assert!(help.contains("**Observers:** 3"), "{help}");
        assert!(
            help.contains("`iii call engine::observers::list function_id=x::on-y`"),
            "{help}"
        );
    }

    #[test]
    fn args_format_is_shown_as_the_observer_args() {
        let mut meta = private_source_row();
        meta["args_format"] = json!({
            "type": "object",
            "properties": {
                "expression": { "type": "string", "description": "7-field cron expression" }
            },
            "required": ["expression"]
        });
        let help = rendered(&meta);
        assert!(help.contains("**Observer args:**"), "{help}");
        assert!(
            help.contains("|`expression`|string|yes|7-field cron expression|"),
            "{help}"
        );

        // A callable function can take observer args too.
        meta["private"] = json!(false);
        let help = rendered(&meta);
        assert!(help.contains("**Usage: ** `iii call x::on-y"), "{help}");
        assert!(help.contains("|`expression`|string|yes|"), "{help}");

        // An args_format without properties still says what args are.
        meta["args_format"] = json!({ "type": "object" });
        assert!(rendered(&meta).contains("**Observer args:** object"));
    }

    #[test]
    fn a_callable_function_keeps_its_usage_and_parameters() {
        let meta = json!({
            "function_id": "orders::create",
            "observer_count": 0,
            "request_schema": { "properties": { "sku": { "type": "string" } } }
        });
        let help = rendered(&meta);

        assert!(!is_private(&meta));
        assert_eq!(
            help_title("orders::create", &meta),
            "iii call orders::create"
        );
        assert!(
            help.starts_with("\n**Usage: ** `iii call x::on-y [key=value ...] [--json '<obj>']`"),
            "{help}"
        );
        assert!(help.contains("**Parameters:**") && help.contains("|`sku`|string|no|"));
        assert!(!help.contains("**Private"), "{help}");
        assert!(!help.contains("**Observer args"), "{help}");
        assert!(help.contains("**Observers:** 0"), "{help}");
    }

    #[test]
    fn a_function_that_is_not_observable_says_so() {
        let meta = json!({
            "function_id": "secrets::read",
            "observable": false,
            "observer_count": 0,
            "args_format": { "properties": { "a": { "type": "string" } } }
        });
        let help = rendered(&meta);
        assert!(help.contains("**Not observable:**"), "{help}");
        assert!(!help.contains("**Observer args"), "{help}");
        assert!(!help.contains("**Observers:**"), "{help}");
    }

    #[test]
    fn a_row_without_observe_fields_renders_as_before() {
        // An engine that predates the observe fields on function rows.
        let meta = json!({ "function_id": "a::b" });
        let help = rendered(&meta);
        assert!(help.contains("**Usage: **"), "{help}");
        assert!(help.contains("*(no request schema published)*"), "{help}");
        assert!(!help.contains("**Observers"), "{help}");
    }

    #[test]
    fn parameters_md_marks_required_fields_and_escapes_pipes() {
        let meta = json!({
            "request_schema": {
                "properties": {
                    "source": { "description": "where from", "type": "object" },
                    "wait": { "type": ["boolean", "null"] }
                },
                "required": ["source"]
            }
        });
        let md = parameters_md(&meta).unwrap();
        assert!(md.contains("source"));
        assert!(md.contains("yes"), "required column marks `source` as yes");
        assert!(
            !md.contains("boolean|null"),
            "multi-type values must escape `|` so the markdown table doesn't split"
        );
    }
}
