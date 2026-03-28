use std::sync::Arc;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};

use crate::handler::{JsonRpcResponse, McpHandler};

pub async fn run_stdio(handler: Arc<McpHandler>) -> anyhow::Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut writer = BufWriter::new(stdout);

    tracing::info!("stdio transport started");

    loop {
        while let Some(notification) = handler.take_notification().await {
            let _ = writer.write_all(notification.as_bytes()).await;
            let _ = writer.write_all(b"\n").await;
            let _ = writer.flush().await;
        }

        let mut line = String::new();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                tracing::info!("stdin closed");
                break;
            }
            Ok(_) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                let body: Value = match serde_json::from_str(line) {
                    Ok(v) => v,
                    Err(err) => {
                        tracing::warn!(error = %err, "Parse error");
                        let r =
                            JsonRpcResponse::error(None, -32700, format!("Parse error: {}", err));
                        if let Ok(v) = serde_json::to_value(&r) {
                            write_json(&mut writer, &v).await;
                        }
                        continue;
                    }
                };

                let Some(response) = handler.handle(body).await else {
                    continue;
                };

                write_json(&mut writer, &response).await;
            }
            Err(err) => {
                tracing::error!(error = %err, "stdin read error");
                break;
            }
        }
    }

    Ok(())
}

async fn write_json(writer: &mut BufWriter<tokio::io::Stdout>, v: &Value) {
    if let Ok(json) = serde_json::to_string(v) {
        if writer.write_all(json.as_bytes()).await.is_err()
            || writer.write_all(b"\n").await.is_err()
            || writer.flush().await.is_err()
        {
            tracing::error!("Failed to write response");
        }
    }
}
