// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use clap::Parser;
use iii::modules::config::DEFAULT_PORT;

#[derive(Parser, Debug, Clone)]
pub struct TriggerArgs {
    /// The function ID to invoke (e.g. 'iii::queue::redrive')
    #[arg(long)]
    pub function_id: String,

    /// JSON payload to send to the function
    #[arg(long)]
    pub payload: String,

    /// Engine host address
    #[arg(long, default_value = "localhost")]
    pub address: String,

    /// Engine WebSocket port
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,
}

pub fn build_invoke_message(function_id: &str, data: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "type": "invokefunction",
        "invocation_id": null,
        "function_id": function_id,
        "data": data,
        "traceparent": null,
        "baggage": null,
        "action": null,
    })
}

pub async fn run_trigger(args: &TriggerArgs) -> anyhow::Result<()> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message as WsMessage;

    let data: serde_json::Value = serde_json::from_str(&args.payload)
        .map_err(|e| anyhow::anyhow!("Invalid JSON payload: {}", e))?;

    let url = format!("ws://{}:{}/", args.address, args.port);

    let (mut socket, _): (
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        _,
    ) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to engine at {}: {}", url, e))?;

    while let Some(msg) = socket.next().await {
        let msg = msg.map_err(|e| anyhow::anyhow!("WebSocket error: {}", e))?;
        if let WsMessage::Text(text) = msg {
            let parsed: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            if parsed.get("type").and_then(|t| t.as_str()) == Some("workerregistered") {
                break;
            }
        }
    }

    let invoke_msg = build_invoke_message(&args.function_id, data);
    let msg_text = serde_json::to_string(&invoke_msg)?;
    socket
        .send(WsMessage::Text(msg_text.into()))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to send message: {}", e))?;

    while let Some(msg) = socket.next().await {
        let msg = msg.map_err(|e| anyhow::anyhow!("WebSocket error: {}", e))?;
        if let WsMessage::Text(text) = msg {
            let parsed: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            if parsed.get("type").and_then(|t| t.as_str()) == Some("invocationresult") {
                if let Some(error) = parsed.get("error").filter(|e| !e.is_null()) {
                    eprintln!("Error: {}", serde_json::to_string_pretty(error)?);
                    std::process::exit(1);
                }
                if let Some(result) = parsed.get("result") {
                    println!("{}", serde_json::to_string_pretty(result)?);
                }
                return Ok(());
            }
        }
    }

    Err(anyhow::anyhow!(
        "Connection closed without receiving a result"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_invoke_message_structure() {
        let data = serde_json::json!({"queue": "payment"});
        let msg = build_invoke_message("iii::queue::redrive", data.clone());

        assert_eq!(msg["type"], "invokefunction");
        assert_eq!(msg["function_id"], "iii::queue::redrive");
        assert_eq!(msg["data"], data);
        assert!(msg["invocation_id"].is_null());
        assert!(msg["action"].is_null());
    }

    #[tokio::test]
    async fn run_trigger_rejects_invalid_json_payload() {
        let args = TriggerArgs {
            function_id: "test::fn".to_string(),
            payload: "not-json".to_string(),
            address: "localhost".to_string(),
            port: DEFAULT_PORT,
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
    async fn run_trigger_connection_refused() {
        let args = TriggerArgs {
            function_id: "test::fn".to_string(),
            payload: "{}".to_string(),
            address: "localhost".to_string(),
            port: 19999,
        };
        let result = run_trigger(&args).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Failed to connect"),
            "expected connection error, got: {}",
            err,
        );
    }
}
