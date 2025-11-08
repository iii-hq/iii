use anyhow::{Context, Result, bail};
use bytes::{Bytes, BytesMut};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{env, sync::Arc};
use tokio::{
    net::TcpStream,
    sync::{Notify, mpsc},
    time::{self, Duration, MissedTickBehavior},
};
use tokio_util::codec::{Decoder, LengthDelimitedCodec};
use uuid::Uuid;

mod protocol;
use protocol::*;

#[derive(Debug, Clone)]
struct Config {
    addr: String,
    heartbeat: Option<Duration>,
    run_for: Option<Duration>,
}

impl Config {
    fn from_env_and_args() -> Result<Self> {
        let mut addr = env::var("ENGINE_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());
        let mut heartbeat_secs = env::var("ENGINE_HEARTBEAT")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(5.0);
        let mut run_for_secs = env::var("ENGINE_RUN_FOR")
            .ok()
            .and_then(|v| v.parse::<f64>().ok());

        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--addr" => {
                    addr = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--addr requires a value"))?;
                }
                _ if arg.starts_with("--addr=") => {
                    addr = arg["--addr=".len()..].to_string();
                }
                "--heartbeat" => {
                    let val = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--heartbeat requires a value"))?;
                    heartbeat_secs = parse_secs(&val, "--heartbeat")?;
                }
                _ if arg.starts_with("--heartbeat=") => {
                    let val = &arg["--heartbeat=".len()..];
                    heartbeat_secs = parse_secs(val, "--heartbeat")?;
                }
                "--run-for" => {
                    let val = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--run-for requires a value"))?;
                    run_for_secs = Some(parse_secs(&val, "--run-for")?);
                }
                _ if arg.starts_with("--run-for=") => {
                    let val = &arg["--run-for=".len()..];
                    run_for_secs = Some(parse_secs(val, "--run-for")?);
                }
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                unknown => {
                    bail!("unknown argument: {unknown}");
                }
            }
        }

        let heartbeat = if heartbeat_secs > 0.0 {
            Some(Duration::from_secs_f64(heartbeat_secs))
        } else {
            None
        };
        let run_for = run_for_secs.map(Duration::from_secs_f64);

        Ok(Self {
            addr,
            heartbeat,
            run_for,
        })
    }
}

fn parse_secs(value: &str, flag: &str) -> Result<f64> {
    value
        .parse::<f64>()
        .with_context(|| format!("invalid value for {flag}: {value}"))
}

fn print_usage() {
    eprintln!(
        "Usage: cargo run --bin client -- [--addr HOST:PORT] [--heartbeat SECONDS] [--run-for SECONDS]"
    );
    eprintln!("Environment overrides: ENGINE_ADDR, ENGINE_HEARTBEAT, ENGINE_RUN_FOR");
}

#[tokio::main]
async fn main() {
    let config = match Config::from_env_and_args() {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("client config error: {err:?}");
            return;
        }
    };

    if let Err(err) = run_client(config).await {
        eprintln!("client error: {err:?}");
    }
}

async fn run_client(config: Config) -> Result<()> {
    println!("Client connecting to {}", config.addr);
    let stream = TcpStream::connect(&config.addr).await?;
    let codec = LengthDelimitedCodec::builder()
        .max_frame_length(64 * 1024 * 1024)
        .new_codec();
    let framed = codec.framed(stream);
    let (mut wr, rd) = framed.split::<Bytes>();

    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

    // writer task owns the sink half
    let writer_handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            match serde_json::to_vec(&msg) {
                Ok(raw) => {
                    if let Err(e) = wr.send(Bytes::from(raw)).await {
                        eprintln!("send error: {e:?}");
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("serialize error: {e:?}");
                }
            }
        }
    });

    // Registrar "math.add"
    let register = Message::Register {
        methods: vec![MethodDef {
            name: "math.add".into(),
            params_schema: json!({
              "$schema":"https://json-schema.org/draft/2020-12/schema",
              "type":"object",
              "properties": { "a": {"type":"number"}, "b":{"type":"number"} },
              "required": ["a","b"],
              "additionalProperties": false
            }),
            result_schema: json!({ "type":"number" }),
        }],
    };
    tx.send(register)
        .context("writer dropped before sending register")?;

    let shutdown = Arc::new(Notify::new());

    // Task de leitura/atendimento
    let reader_shutdown = shutdown.clone();
    let reader_tx = tx.clone();
    let mut reader_stream = rd;
    let reader = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = reader_shutdown.notified() => {
                    break;
                }
                frame = reader_stream.next() => {
                    match frame {
                        Some(Ok(bytes)) => {
                            if let Err(err) = handle_message(bytes, &reader_tx) {
                                eprintln!("message error: {err:?}");
                            }
                        }
                        Some(Err(e)) => {
                            eprintln!("read error: {e:?}");
                            break;
                        }
                        None => {
                            println!("server closed connection");
                            break;
                        }
                    }
                }
            }
        }
        reader_shutdown.notify_waiters();
    });

    // Heartbeat loop (optional)
    let heartbeat_handle = if let Some(period) = config.heartbeat {
        let hb_shutdown = shutdown.clone();
        let hb_tx = tx.clone();
        Some(tokio::spawn(async move {
            let mut interval = time::interval(period);
            interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = hb_shutdown.notified() => break,
                    _ = interval.tick() => {
                        let notify = Message::Notify {
                            to: None,
                            method: "client.heartbeat".into(),
                            params: json!({"ts": Utc::now().to_rfc3339()}),
                        };
                        if hb_tx.send(notify).is_err() {
                            break;
                        }
                    }
                }
            }
        }))
    } else {
        None
    };

    if let Some(limit) = config.run_for {
        let timer_shutdown = shutdown.clone();
        tokio::spawn(async move {
            time::sleep(limit).await;
            println!(
                "runtime limit of {:.2}s reached, shutting down client",
                limit.as_secs_f64()
            );
            timer_shutdown.notify_waiters();
        });
    }

    shutdown.notified().await;
    drop(tx);

    if let Some(handle) = heartbeat_handle {
        let _ = handle.await;
    }
    let _ = reader.await;
    let _ = writer_handle.await;

    Ok(())
}

fn handle_message(bytes: BytesMut, tx: &mpsc::UnboundedSender<Message>) -> Result<()> {
    let msg: Message = serde_json::from_slice(&bytes)?;
    match msg {
        Message::Call {
            id, method, params, ..
        } => handle_call(method, params, id, tx),
        Message::Notify { method, params, .. } => {
            println!("notify<{method}>: {params}");
        }
        Message::Result {
            id,
            ok,
            result,
            error,
            ..
        } => {
            println!("result<{id}> ok={ok} result={result:?} error={error:?}");
        }
        Message::Error {
            id, code, message, ..
        } => {
            eprintln!("engine error for {id}: {code} - {message}");
        }
        Message::Ping => {
            println!("ping received; replying with pong");
            send_message(tx, Message::Pong);
        }
        Message::Pong => {
            println!("pong received");
        }
        Message::Register { methods } => {
            println!("register ack: {} methods", methods.len());
        }
    }
    Ok(())
}

fn handle_call(method: String, params: Value, id: Uuid, tx: &mpsc::UnboundedSender<Message>) {
    match method.as_str() {
        "math.add" => {
            let a = params.get("a").and_then(Value::as_f64);
            let b = params.get("b").and_then(Value::as_f64);
            match (a, b) {
                (Some(a), Some(b)) => {
                    let result = json!(a + b);
                    respond_with_result(tx, id, result);
                }
                _ => respond_with_error(
                    tx,
                    id,
                    "invalid_params",
                    "expected numeric fields 'a' and 'b'",
                ),
            }
        }
        other => {
            respond_with_error(
                tx,
                id,
                "method_not_found",
                format!("no handler for method {other}"),
            );
        }
    }
}

fn respond_with_result(tx: &mpsc::UnboundedSender<Message>, id: Uuid, payload: Value) {
    send_message(
        tx,
        Message::Result {
            id,
            ok: true,
            result: Some(payload),
            error: None,
        },
    );
}

fn respond_with_error(
    tx: &mpsc::UnboundedSender<Message>,
    id: Uuid,
    code: &str,
    message: impl Into<String>,
) {
    send_message(
        tx,
        Message::Result {
            id,
            ok: false,
            result: None,
            error: Some(ErrorBody {
                code: code.to_string(),
                message: message.into(),
            }),
        },
    );
}

fn send_message(tx: &mpsc::UnboundedSender<Message>, msg: Message) {
    if tx.send(msg).is_err() {
        eprintln!("writer channel closed; dropping outbound message");
    }
}
