// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use clap::Parser;
use iii::modules::worker::DEFAULT_PORT;
use iii_sdk::{IIIError, InitOptions, TriggerRequest, register_worker};

#[derive(Parser, Debug, Clone)]
pub struct TriggerArgs {
    /// The function ID to invoke (e.g. 'iii::queue::redrive')
    #[arg(long)]
    pub function_id: String,

    /// JSON payload to send to the function
    #[arg(long, default_value = "{}")]
    pub payload: String,

    /// Engine host address
    #[arg(long, default_value = "localhost")]
    pub address: String,

    /// Engine WebSocket port
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,

    /// Max time to wait for the invocation result (milliseconds)
    #[arg(long, default_value_t = 30_000)]
    pub timeout_ms: u64,
}

pub async fn run_trigger(args: &TriggerArgs) -> anyhow::Result<()> {
    let data: serde_json::Value = serde_json::from_str(&args.payload)
        .map_err(|e| anyhow::anyhow!("Invalid JSON payload: {}", e))?;

    let url = format!("ws://{}:{}", args.address, args.port);
    let iii = register_worker(&url, InitOptions::default());

    let trigger_result = iii
        .trigger(TriggerRequest {
            function_id: args.function_id.clone(),
            payload: data,
            action: None,
            timeout_ms: Some(args.timeout_ms),
        })
        .await;

    iii.shutdown_async().await;

    match trigger_result {
        Ok(value) => {
            if !value.is_null() {
                println!("{}", serde_json::to_string_pretty(&value)?);
            }
            Ok(())
        }
        Err(IIIError::Remote {
            code,
            message,
            stacktrace,
        }) => {
            let err_obj = serde_json::json!({
                "code": code,
                "message": message,
                "stacktrace": stacktrace,
            });
            eprintln!("Error: {}", serde_json::to_string_pretty(&err_obj)?);
            std::process::exit(1);
        }
        Err(e) => Err(map_trigger_error(e)),
    }
}

fn map_trigger_error(e: IIIError) -> anyhow::Error {
    match e {
        IIIError::Timeout => {
            anyhow::anyhow!(
                "Timed out waiting for the engine (no response within the timeout). Is the engine running at the given address and port?"
            )
        }
        IIIError::WebSocket(msg) => {
            anyhow::anyhow!("WebSocket error: {}", msg)
        }
        other => anyhow::Error::new(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_trigger_rejects_invalid_json_payload() {
        let args = TriggerArgs {
            function_id: "test::fn".to_string(),
            payload: "not-json".to_string(),
            address: "localhost".to_string(),
            port: DEFAULT_PORT,
            timeout_ms: 30_000,
        };
        let result = run_trigger(&args).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Invalid JSON payload"),
            "expected JSON validation error, got: {}",
            err,
        );
    }

    #[tokio::test]
    async fn run_trigger_unreachable_engine_times_out() {
        let args = TriggerArgs {
            function_id: "test::fn".to_string(),
            payload: "{}".to_string(),
            address: "localhost".to_string(),
            port: 19999,
            timeout_ms: 800,
        };
        let result = run_trigger(&args).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Timed out") || err.contains("timeout"),
            "expected timeout when engine is unreachable, got: {}",
            err,
        );
    }
}
