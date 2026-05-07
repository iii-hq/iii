// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use iii_sdk::{IIIError, InitOptions, TriggerRequest, register_worker};
use serde_json::Value;

pub async fn invoke(
    function_path: &str,
    payload: Value,
    address: &str,
    port: u16,
    timeout_ms: u64,
) -> anyhow::Result<()> {
    let url = format!("ws://{}:{}", address, port);
    let iii = register_worker(&url, InitOptions::default());

    let result = iii
        .trigger(TriggerRequest {
            function_id: function_path.to_string(),
            payload,
            action: None,
            timeout_ms: Some(timeout_ms),
        })
        .await;

    iii.shutdown_async().await;

    match result {
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
        IIIError::Timeout => anyhow::anyhow!(
            "Timed out waiting for the engine (no response within the timeout). Is the engine running at the given address and port?"
        ),
        IIIError::WebSocket(msg) => anyhow::anyhow!("WebSocket error: {}", msg),
        other => anyhow::Error::new(other),
    }
}
