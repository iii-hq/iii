// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

pub mod exec;
pub mod help;
pub mod payload;

use clap::Parser;
use iii::workers::worker::DEFAULT_PORT;

#[derive(Parser, Debug, Clone)]
#[command(disable_help_flag = true)]
pub struct TriggerArgs {
    /// Function path (e.g. `my::fn`, `sandbox::run`). Positional.
    #[arg(value_name = "FUNCTION_PATH")]
    pub function_path: Option<String>,

    /// Key=value payload tokens (`a=10 b="hello world"`).
    /// Combinable with `--json`: kv pairs override individual keys of the json object.
    #[arg(value_name = "KV", num_args = 0..)]
    pub kv: Vec<String>,

    /// JSON payload (`--json '{"a":1}'`). When combined with kv pairs the json must be an object;
    /// kv pairs override its keys (shallow merge).
    #[arg(long)]
    pub json: Option<String>,

    /// Engine host address.
    #[arg(long, default_value = "localhost")]
    pub address: String,

    /// Engine WebSocket port.
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,

    /// Max time to wait for the invocation result (milliseconds).
    #[arg(long, default_value_t = 30_000)]
    pub timeout_ms: u64,

    /// Print help. With a FUNCTION_PATH, queries a running engine for that
    /// function's description and request schema.
    #[arg(short = 'h', long = "help", action = clap::ArgAction::SetTrue)]
    pub help: bool,
}

pub async fn run_trigger(args: &TriggerArgs) -> anyhow::Result<()> {
    if args.help {
        return help::print(
            args.function_path.as_deref(),
            &args.address,
            args.port,
            args.timeout_ms,
        )
        .await;
    }
    let function_path = args.function_path.as_deref().ok_or_else(|| {
        anyhow::anyhow!("iii trigger: missing FUNCTION_PATH. Try: `iii trigger <fn-path> [args]`")
    })?;
    let payload = payload::parse(&args.kv, args.json.as_deref())?;
    exec::invoke(
        function_path,
        payload,
        &args.address,
        args.port,
        args.timeout_ms,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_trigger_missing_fn_path_errors() {
        let args = TriggerArgs {
            function_path: None,
            kv: vec![],
            json: None,
            address: "localhost".to_string(),
            port: DEFAULT_PORT,
            timeout_ms: 800,
            help: false,
        };
        let err = run_trigger(&args).await.unwrap_err().to_string();
        assert!(
            err.contains("missing FUNCTION_PATH"),
            "expected missing fn-path error, got: {}",
            err,
        );
    }

    #[tokio::test]
    async fn run_trigger_unreachable_engine_times_out() {
        let args = TriggerArgs {
            function_path: Some("test::fn".to_string()),
            kv: vec![],
            json: None,
            address: "localhost".to_string(),
            port: 19999,
            timeout_ms: 800,
            help: false,
        };
        let err = run_trigger(&args).await.unwrap_err().to_string();
        assert!(
            err.contains("Timed out") || err.contains("timeout"),
            "expected timeout when engine is unreachable, got: {}",
            err,
        );
    }

    #[tokio::test]
    async fn run_trigger_rejects_invalid_json() {
        let args = TriggerArgs {
            function_path: Some("test::fn".to_string()),
            kv: vec![],
            json: Some("not-json".to_string()),
            address: "localhost".to_string(),
            port: DEFAULT_PORT,
            timeout_ms: 30_000,
            help: false,
        };
        let err = run_trigger(&args).await.unwrap_err().to_string();
        assert!(
            err.contains("--json: invalid JSON"),
            "expected json validation error, got: {}",
            err,
        );
    }
}
