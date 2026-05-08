// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use anyhow::Result;
use colored::Colorize;
use iii_sdk::{InitOptions, TriggerRequest, register_worker};
use serde_json::{Value, json};

pub async fn print(
    function_path: Option<&str>,
    address: &str,
    port: u16,
    timeout_ms: u64,
) -> Result<()> {
    match function_path {
        None => {
            print_static_help();
            Ok(())
        }
        Some(fn_path) => match fetch_fn_meta(fn_path, address, port, timeout_ms).await {
            Ok(Some(meta)) => {
                render_fn_help(fn_path, &meta);
                Ok(())
            }
            Ok(None) => {
                eprintln!(
                    "{} function `{}` not found in engine registry.",
                    "error:".red(),
                    fn_path
                );
                eprintln!(
                    "  {} run `iii trigger engine::functions::list` to see registered functions.",
                    "hint:".dimmed()
                );
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
) -> Result<Option<Value>> {
    let url = format!("ws://{}:{}", address, port);
    let iii = register_worker(&url, InitOptions::default());

    let result = iii
        .trigger(TriggerRequest {
            function_id: "engine::functions::list".to_string(),
            payload: json!({ "include_internal": true }),
            action: None,
            timeout_ms: Some(timeout_ms),
        })
        .await;

    iii.shutdown_async().await;

    let value = result.map_err(|e| anyhow::anyhow!("{}", e))?;
    let functions = value
        .get("functions")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("unexpected response shape from engine::functions::list"))?;

    Ok(functions
        .iter()
        .find(|f| f.get("function_id").and_then(|s| s.as_str()) == Some(fn_path))
        .cloned())
}

fn render_fn_help(fn_path: &str, meta: &Value) {
    let description = meta
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Drive sections individually via print_template so we can place the
    // schema-driven Parameters table BEFORE the generic Options table that
    // clap-help builds from the trigger flags.
    let Some(trigger) = crate::cli_subcommand("trigger") else {
        return;
    };

    let usage_template = "\n**Usage: ** `${name} [key=value ...] [--json '<obj>']`\n".to_string();
    let parameters_md = parameters_md(meta);

    let mut printer = clap_help::Printer::new(trigger);
    printer
        .expander_mut()
        .set("name", format!("iii trigger {}", fn_path));
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
    // Usage.
    printer.print_template(&usage_template);
    // Parameters (custom, schema-driven).
    if let Some(md) = &parameters_md {
        printer.print_template(md);
    } else {
        printer.print_template("\n**Parameters:**\n\n  *(no request schema published)*\n");
    }
    // Options (clap-help default table for the trigger flags).
    printer.print_template(clap_help::TEMPLATE_OPTIONS);
}

/// Render the schema-driven parameters as a markdown table that termimad
/// can display in the same style as clap-help's Options table. Returns
/// `None` when there is nothing useful to show.
fn parameters_md(meta: &Value) -> Option<String> {
    let schema = meta.get("request_format").filter(|v| !v.is_null())?;
    let props = schema.get("properties").and_then(|p| p.as_object())?;
    if props.is_empty() {
        return None;
    }

    let required: Vec<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let mut md = String::from(
        "\n**Parameters:**\n|:-:|:-:|:-:|:-|\n|name|type|required|description|\n|:-:|:-|:-:|:-|\n",
    );
    for (name, prop) in props {
        // Escape `|` so multi-type values like `string|null` do not split the
        // markdown row into extra columns.
        let ty = schema_type(prop).replace('|', "\\|");
        let req = if required.contains(&name.as_str()) {
            "yes"
        } else {
            "no"
        };
        let desc = prop
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .replace('|', "\\|");
        md.push_str(&format!("|`{}`|{}|{}|{}|\n", name, ty, req, desc));
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
    if let Some(trigger) = crate::cli_subcommand("trigger") {
        crate::render_clap_help(trigger);
    }
    println!(
        "{} `iii trigger <fn-path> --help` queries a running engine for the",
        "Tip:".bold()
    );
    println!("  function's description and request schema.");
}
