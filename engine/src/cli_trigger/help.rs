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

    println!("{} {}", "iii trigger".bold(), fn_path.bold());
    if !description.is_empty() {
        println!();
        println!("  {}", description);
    }

    println!();
    println!("{}", "Usage:".bold());
    println!("  iii trigger {} [key=value ...] [--json '<obj>']", fn_path);

    let request_format = meta.get("request_format");
    println!();
    println!("{}", "Parameters:".bold());

    let Some(schema) = request_format.filter(|v| !v.is_null()) else {
        println!("  (no request schema published)");
        return;
    };

    let Some(props) = schema.get("properties").and_then(|p| p.as_object()) else {
        println!("  (request body is {})", schema_type(schema));
        return;
    };

    if props.is_empty() {
        println!("  (none)");
        return;
    }

    let required: Vec<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    // Collect rows so we can compute column widths up front and align everything.
    let rows: Vec<Row> = props
        .iter()
        .map(|(name, prop)| Row {
            name: name.clone(),
            ty: schema_type(prop),
            required: required.contains(&name.as_str()),
            desc: prop
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string(),
        })
        .collect();

    // Display name carries a trailing `*` for required fields, so include that
    // in the width calc to keep the type column aligned.
    let max_name = rows
        .iter()
        .map(|r| r.name.len() + if r.required { 1 } else { 0 })
        .max()
        .unwrap_or(0);
    let max_ty = rows.iter().map(|r| r.ty.len()).max().unwrap_or(0);

    let mut has_required = false;
    for row in &rows {
        let display_name = if row.required {
            has_required = true;
            format!("{}*", row.name)
        } else {
            row.name.clone()
        };
        let padded_name = format!("{:<width$}", display_name, width = max_name);
        let padded_ty = format!("{:<width$}", row.ty, width = max_ty);
        if row.desc.is_empty() {
            println!("  {}  {}", padded_name.bold(), padded_ty.dimmed());
        } else {
            println!(
                "  {}  {}  {}",
                padded_name.bold(),
                padded_ty.dimmed(),
                row.desc
            );
        }
    }

    if has_required {
        println!();
        println!("  {} required", "*".bold());
    }
}

struct Row {
    name: String,
    ty: String,
    required: bool,
    desc: String,
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
    println!("Invoke a function on a running iii engine.");
    println!();
    println!("{}", "Usage:".bold());
    println!("  iii trigger [OPTIONS] [FUNCTION_PATH] [KV]... [--json '<obj>']");
    println!();
    println!("{}", "Arguments:".bold());
    println!("  [FUNCTION_PATH]  Function path (e.g. `my::fn`). Positional.");
    println!("  [KV]...          Key=value payload tokens (`a=10 b=\"hello\"`).");
    println!();
    println!(
        "{} `iii trigger <fn-path> --help` queries a running engine for the",
        "Tip:".bold()
    );
    println!("  function's description and request schema.");
}
