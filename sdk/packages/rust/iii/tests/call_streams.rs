//! Call-native streams against a real engine (`III_URL`): `InputStream` and
//! `OutputStream` as values in call payloads and results.
//!
//! Every remote case calls with `Dispatch::Remote`, so the stream crosses an
//! engine pipe even though the caller hosts the function. Callers bind the
//! streams they deserialize outside a handler (several clients are alive in
//! this binary); `stream_bind.rs` covers the lazy binding.
//!
//! Requires a running III engine. Set III_URL, or use the default.

mod common;

use std::{
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWrite};
use tokio_tungstenite::tungstenite::Message as WsMessage;

use common::engine_ws_url;
use iii_helpers::observability::OtelConfig;
use iii_sdk::protocol::{
    ChannelDirection, ChannelItem, STREAM_ABORTED, STREAM_IN_USE, STREAM_NOT_ALLOWED,
};
use iii_sdk::runtime::{FunctionRef, WorkerMetadata};
use iii_sdk::{
    CallAction, CallRequest, CallRequestWithMetadata, Capture, Dispatch, Error, IIIClient,
    InitOptions, InputStream, Observed, Observer, OutputStream, RegisterFunction, Source,
    StreamInfo, WorkerIdentityMode, register_worker,
};

const WAIT: Duration = Duration::from_secs(10);

fn uid() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

async fn client(name: &str) -> IIIClient {
    let iii = register_worker(
        &engine_ws_url(),
        InitOptions {
            metadata: Some(WorkerMetadata {
                name: name.to_string(),
                ..WorkerMetadata::default()
            }),
            otel: Some(OtelConfig {
                enabled: Some(false),
                ..OtelConfig::default()
            }),
            identity: WorkerIdentityMode::Explicit,
            ..InitOptions::default()
        },
    );
    iii.wait_until_registered(WAIT)
        .await
        .expect("engine reachable");
    iii
}

async fn until(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// The engine confirmed `function`: it can be called, through the engine or
/// in-process.
async fn ready(function: &FunctionRef) {
    until("the function's registration", || {
        function.local_dispatch_active()
    })
    .await;
}

/// A call that goes through the engine even to a function this worker hosts.
fn remote(function_id: &str, payload: Value) -> CallRequestWithMetadata {
    CallRequest::new(function_id, payload).dispatch(Dispatch::Remote)
}

/// A call in-process (`local`) or through the engine.
fn via(local: bool, function_id: &str, payload: Value) -> CallRequestWithMetadata {
    let dispatch = if local {
        Dispatch::Auto
    } else {
        Dispatch::Remote
    };
    CallRequest::new(function_id, payload).dispatch(dispatch)
}

#[derive(Deserialize, schemars::JsonSchema)]
struct Upload {
    name: String,
    body: InputStream,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct Render {
    #[serde(default)]
    pages: u32,
    out: OutputStream,
}

/// A byte sink that records what it got and whether it was shut down (a
/// close) or only dropped (an abort).
#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<(Vec<u8>, bool)>>);

impl Recorder {
    fn bytes(&self) -> Vec<u8> {
        self.0.lock().unwrap().0.clone()
    }

    fn closed(&self) -> bool {
        self.0.lock().unwrap().1
    }
}

impl AsyncWrite for Recorder {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.0.lock().unwrap().0.extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.0.lock().unwrap().1 = true;
        Poll::Ready(Ok(()))
    }
}

fn chunks(parts: &[&'static [u8]]) -> InputStream {
    InputStream::from_stream(futures_util::stream::iter(
        parts
            .iter()
            .map(|part| Ok::<_, std::io::Error>(Bytes::from_static(part)))
            .collect::<Vec<_>>(),
    ))
}

/// Send: a source in the payload reaches the handler as an `InputStream`,
/// whole, from a reader (split into 64 KiB frames), a stream and bytes.
#[tokio::test]
async fn a_source_in_the_payload_reaches_the_handler() {
    let id = uid();
    let iii = client(&format!("send-{id}")).await;
    let function_id = format!("st{id}::count");
    let function = iii.register_function(
        &function_id,
        RegisterFunction::new_async(|upload: Upload| async move {
            let bytes = upload.body.read_all().await?;
            let sum: u64 = bytes.iter().map(|b| u64::from(*b)).sum();
            Ok::<_, Error>(json!({ "name": upload.name, "len": bytes.len(), "sum": sum }))
        }),
    );
    ready(&function).await;

    let big: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let sum: u64 = big.iter().map(|b| u64::from(*b)).sum();
    let out = iii
        .call(remote(
            &function_id,
            json!({
                "name": "big",
                "body": InputStream::from_reader(std::io::Cursor::new(big.clone())),
            }),
        ))
        .await
        .unwrap();
    assert_eq!(out, json!({ "name": "big", "len": big.len(), "sum": sum }));

    let out = iii
        .call(remote(
            &function_id,
            json!({ "name": "chunks", "body": chunks(&[b"ab", b"cd", b"e"]) }),
        ))
        .await
        .unwrap();
    assert_eq!(out["len"], 5);

    let out = iii
        .call(remote(
            &function_id,
            json!({ "name": "empty", "body": InputStream::from_bytes(Vec::new()) }),
        ))
        .await
        .unwrap();
    assert_eq!(out["len"], 0);
    iii.shutdown_async().await;
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct Export {
    report_id: String,
    csv: InputStream,
}

/// Return: a stream in a handler's result arrives as a ref the caller
/// deserializes into an `InputStream`.
#[tokio::test]
async fn a_stream_in_the_result_reaches_the_caller() {
    let id = uid();
    let iii = client(&format!("return-{id}")).await;
    let function_id = format!("st{id}::export");
    let function = iii.register_function(
        &function_id,
        RegisterFunction::new_async(|req: Value| async move {
            let rows = req["rows"].as_u64().unwrap_or(0);
            let lines = futures_util::stream::iter(
                (0..rows)
                    .map(|i| Ok::<_, std::io::Error>(Bytes::from(format!("{i},{}\n", i * 10)))),
            );
            Ok::<_, Error>(Export {
                report_id: "r-42".into(),
                csv: InputStream::from_stream(lines),
            })
        }),
    );
    ready(&function).await;

    let result = iii
        .call(remote(&function_id, json!({ "rows": 1000 })))
        .await
        .unwrap();
    // On the wire: a pipe ref with the three keys every SDK reads.
    let csv = result["csv"].as_object().unwrap();
    assert_eq!(csv.len(), 3, "{csv:?}");
    assert_eq!(csv["direction"], "read");
    assert!(!csv["channel_id"].as_str().unwrap().starts_with("local:"));

    let export: Export = serde_json::from_value(result).unwrap();
    export.csv.bind(&iii);
    assert_eq!(export.report_id, "r-42");
    let lines: Vec<String> = export.csv.lines().try_collect().await.unwrap();
    assert_eq!(lines.len(), 1000);
    assert_eq!(lines[1], "1,10");
    assert_eq!(lines[999], "999,9990");
    iii.shutdown_async().await;
}

/// Sink: the caller passes where the bytes go; the call resolves once the
/// sink got the whole stream, which the handler's return closed.
#[tokio::test]
async fn a_sink_gets_what_the_function_writes_before_the_call_resolves() {
    let id = uid();
    let iii = client(&format!("sink-{id}")).await;
    let function_id = format!("st{id}::render");
    let function = iii.register_function(
        &function_id,
        RegisterFunction::new_async(|req: Render| async move {
            for page in 0..req.pages {
                req.out.write(format!("page {page}\n").as_bytes()).await?;
            }
            // Ok closes `out`.
            Ok::<_, Error>(())
        }),
    );
    ready(&function).await;

    let (writer, mut reader) = tokio::io::duplex(1 << 20);
    let out = iii
        .call(remote(
            &function_id,
            json!({ "pages": 50, "out": OutputStream::to_writer(writer) }),
        ))
        .await
        .unwrap();
    assert_eq!(out, Value::Null);
    let mut got = String::new();
    tokio::time::timeout(Duration::from_secs(1), reader.read_to_string(&mut got))
        .await
        .expect("the sink was closed before the call resolved")
        .unwrap();
    assert_eq!(got.lines().count(), 50);
    assert_eq!(got.lines().last(), Some("page 49"));
    iii.shutdown_async().await;
}

/// Forward: a handler passes on a stream it did not read: its own ref goes,
/// no byte passes through it. Read from first, the rest is pumped through a
/// new pipe.
#[tokio::test]
async fn a_received_stream_is_forwarded_by_ref_or_pumped_once_read() {
    let id = uid();
    let iii = client(&format!("forward-{id}")).await;
    let (a, b) = (format!("st{id}::a"), format!("st{id}::b"));
    let b_fn = iii.register_function(
        &b,
        RegisterFunction::new_async(|input: Value| async move {
            let wire = input["body"].clone();
            let body: InputStream = serde_json::from_value(wire.clone())?;
            Ok::<_, Error>(json!({
                "channel_id": wire["channel_id"],
                "keys": wire.as_object().map(|o| o.len()),
                "text": body.text().await?,
            }))
        }),
    );
    let caller = iii.clone();
    let b_id = b.clone();
    let a_fn = iii.register_function(
        &a,
        RegisterFunction::new_async(move |input: Value| {
            let caller = caller.clone();
            let b_id = b_id.clone();
            async move {
                let received = input["body"]["channel_id"].clone();
                let mut body: InputStream = serde_json::from_value(input["body"].clone())?;
                let mut head = Value::Null;
                if input["read_first"] == true {
                    let first = body.next().await.transpose()?.unwrap_or_default();
                    head = json!(String::from_utf8_lossy(&first));
                }
                let forwarded = caller.call(remote(&b_id, json!({ "body": body }))).await?;
                Ok::<_, Error>(json!({ "received": received, "head": head, "b": forwarded }))
            }
        }),
    );
    ready(&a_fn).await;
    ready(&b_fn).await;

    let out = iii
        .call(remote(
            &a,
            json!({ "body": chunks(&[b"first|", b"second|", b"third"]) }),
        ))
        .await
        .unwrap();
    assert_eq!(out["b"]["text"], "first|second|third");
    assert_eq!(out["b"]["channel_id"], out["received"], "forwarded by ref");
    assert_eq!(out["b"]["keys"], 3);

    let out = iii
        .call(remote(
            &a,
            json!({ "read_first": true, "body": chunks(&[b"first|", b"second|", b"third"]) }),
        ))
        .await
        .unwrap();
    assert_eq!(out["head"], "first|");
    assert_eq!(out["b"]["text"], "second|third");
    assert_ne!(
        out["b"]["channel_id"], out["received"],
        "pumped through a new pipe"
    );
    for channel_id in [&out["b"]["channel_id"], &out["received"]] {
        assert!(!channel_id.as_str().unwrap().starts_with("local:"));
    }
    iii.shutdown_async().await;
}

/// Abort from the sender: a source that fails aborts the stream; the reader
/// gets the data sent before it, then `STREAM_ABORTED`.
#[tokio::test]
async fn a_failing_source_aborts_the_reader() {
    let id = uid();
    let iii = client(&format!("abort-src-{id}")).await;
    let function_id = format!("st{id}::read");
    let function = iii.register_function(
        &function_id,
        RegisterFunction::new_async(|upload: Upload| async move {
            let mut body = upload.body;
            let mut got = 0;
            let error = loop {
                match body.next().await {
                    Some(Ok(chunk)) => got += chunk.len(),
                    Some(Err(error)) => break Some(error),
                    None => break None,
                }
            };
            Ok::<_, Error>(json!({
                "got": got,
                "code": error.as_ref().and_then(|e| e.code().map(str::to_string)),
            }))
        }),
    );
    ready(&function).await;

    let failing = InputStream::from_stream(futures_util::stream::iter([
        Ok(Bytes::from_static(b"12345")),
        Err("disk gone"),
    ]));
    let out = iii
        .call(remote(
            &function_id,
            json!({ "name": "x", "body": failing }),
        ))
        .await
        .unwrap();
    assert_eq!(out, json!({ "got": 5, "code": STREAM_ABORTED }));
    iii.shutdown_async().await;
}

/// Abort from the receiver: a handler that fails aborts the sink it wrote
/// to, and the call fails with the handler's error; one that aborts the sink
/// and succeeds fails the call with `STREAM_ABORTED`; a call that fails
/// before anything was written aborts the sink too.
#[tokio::test]
async fn a_sink_is_aborted_when_the_function_fails_or_aborts_it() {
    let id = uid();
    let iii = client(&format!("abort-sink-{id}")).await;
    let failing = format!("st{id}::fails");
    let aborting = format!("st{id}::aborts");
    let f1 = iii.register_function(
        &failing,
        RegisterFunction::new_async(|req: Render| async move {
            req.out.write(b"partial").await?;
            Err::<(), _>(Error::Handler("render failed".into()))
        }),
    );
    let f2 = iii.register_function(
        &aborting,
        RegisterFunction::new_async(|req: Render| async move {
            req.out.write(b"partial").await?;
            req.out.abort("out of paper").await?;
            Ok::<_, Error>(json!({ "ok": true }))
        }),
    );
    ready(&f1).await;
    ready(&f2).await;

    let sink = Recorder::default();
    let error = iii
        .call(remote(
            &failing,
            json!({ "out": OutputStream::to_writer(sink.clone()) }),
        ))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("render failed"), "{error}");
    assert!(!sink.closed(), "an aborted sink is not closed");

    let sink = Recorder::default();
    let error = iii
        .call(remote(
            &aborting,
            json!({ "out": OutputStream::to_writer(sink.clone()) }),
        ))
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some(STREAM_ABORTED), "{error}");
    assert_eq!(sink.bytes(), b"partial");
    assert!(!sink.closed());

    let sink = Recorder::default();
    let error = iii
        .call(remote(
            &format!("st{id}::missing"),
            json!({ "out": OutputStream::to_writer(sink.clone()) }),
        ))
        .await
        .unwrap_err();
    assert_ne!(error.code(), Some(STREAM_ABORTED), "{error}");
    assert!(!sink.closed());
    iii.shutdown_async().await;
}

/// The handler of an untouched-output case: it takes `out` and leaves it,
/// then fails, returns `null`, or returns a value.
fn leaves_its_output(input: Value) -> Result<Value, Error> {
    let _out: OutputStream = serde_json::from_value(input["out"].clone())?;
    ignores_its_output(input)
}

/// An untyped handler: it never deserializes the ref it gets.
fn ignores_its_output(input: Value) -> Result<Value, Error> {
    match input["then"].as_str() {
        Some("fail") => Err(Error::Handler("failed".into())),
        Some("value") => Ok(json!({ "status_code": 200, "body": "buffered" })),
        _ => Ok(Value::Null),
    }
}

/// Whether the read end of a pipe ends (cleanly, empty) soon.
async fn ends_soon(input: &InputStream) -> bool {
    match tokio::time::timeout(Duration::from_millis(800), input.read_all()).await {
        Ok(read) => {
            assert_eq!(read.unwrap(), Vec::<u8>::new());
            true
        }
        Err(_) => {
            input.cancel().await;
            false
        }
    }
}

/// The guard on an output the handler never used, in a plain call: closed
/// when the handler succeeded, whatever it returned, so a caller's sink always
/// ends; left as is when it failed. The same for a handler that takes its
/// input untyped and never looks at the ref.
#[tokio::test]
async fn an_untouched_output_is_closed_when_a_plain_call_succeeds() {
    untouched_output_in_a_plain_call(leaves_its_output).await;
    untouched_output_in_a_plain_call(ignores_its_output).await;
}

async fn untouched_output_in_a_plain_call(handler: fn(Value) -> Result<Value, Error>) {
    let id = uid();
    let iii = client(&format!("guard-out-{id}")).await;
    let function_id = format!("st{id}::untouched");
    let function = iii.register_function(&function_id, RegisterFunction::new(handler));
    ready(&function).await;

    for (then, ends) in [("value", true), ("null", true), ("fail", false)] {
        let pipe = iii.pipe().await.unwrap();
        let out = iii
            .call(remote(
                &function_id,
                json!({ "then": then, "out": pipe.output }),
            ))
            .await;
        assert_eq!(out.is_ok(), then != "fail", "{then}: {out:?}");
        assert_eq!(ends_soon(&pipe.input).await, ends, "{then}");
    }

    // A caller's sink: the call resolves, and the sink is closed.
    let sink = Recorder::default();
    let out = tokio::time::timeout(
        Duration::from_secs(5),
        iii.call(remote(
            &function_id,
            json!({ "then": "value", "out": OutputStream::to_writer(sink.clone()) }),
        )),
    )
    .await
    .expect("the call resolved")
    .unwrap();
    assert_eq!(out["body"], "buffered");
    assert!(sink.closed());
    iii.shutdown_async().await;
}

/// The guard on an output the handler never used, in an observer activation
/// (how the HTTP worker calls a route target): closed only when the handler
/// returned `null`, left as is with a value (the HTTP worker reads that as a
/// buffered response) or a failure. Typed or untyped handler alike.
#[tokio::test]
async fn an_untouched_output_in_an_observer_activation_is_closed_only_on_null() {
    untouched_output_in_an_activation(leaves_its_output).await;
    untouched_output_in_an_activation(ignores_its_output).await;
}

async fn untouched_output_in_an_activation(handler: fn(Value) -> Result<Value, Error>) {
    let id = uid();
    let host = client(&format!("route-host-{id}")).await;
    let watcher = client(&format!("route-target-{id}")).await;
    let (source, target) = (format!("st{id}::requests"), format!("st{id}::route"));
    let added: Arc<Mutex<Option<Observer>>> = Arc::default();
    host.on_observer_added_for(&source, {
        let added = added.clone();
        move |obs: Observer| {
            *added.lock().unwrap() = Some(obs);
            async { Ok(()) }
        }
    });
    let function =
        host.register_function(&source, RegisterFunction::new(|v: Value| Ok::<_, Error>(v)));
    ready(&function).await;
    watcher.register_function(&target, RegisterFunction::new(handler));
    let observer = watcher.observe(Source::function(&source), &target);
    tokio::time::timeout(WAIT, observer.ready())
        .await
        .unwrap()
        .unwrap();
    until("the added hook", || added.lock().unwrap().is_some()).await;
    let obs = added.lock().unwrap().clone().unwrap();
    tokio::time::timeout(WAIT, obs.accepted())
        .await
        .unwrap()
        .unwrap();

    for (then, ends) in [("value", false), ("null", true), ("fail", false)] {
        let pipe = host.pipe().await.unwrap();
        let out = host
            .call(CallRequest::observer(
                &obs,
                json!({ "then": then, "out": pipe.output }),
            ))
            .await;
        assert_eq!(out.is_ok(), then != "fail", "{then}: {out:?}");
        assert_eq!(ends_soon(&pipe.input).await, ends, "{then}");
    }
    for c in [host, watcher] {
        c.shutdown_async().await;
    }
}

/// The guard on input: one the handler started reading and did not finish
/// is cancelled when it returns, even when a task still holds it; the engine
/// then closes the writer. One never read is left alone: another function
/// can read it after.
#[tokio::test]
async fn the_guard_cancels_a_partly_read_input_and_leaves_an_unread_one() {
    let id = uid();
    let iii = client(&format!("guard-in-{id}")).await;
    let partial = format!("st{id}::partial");
    let ignore = format!("st{id}::ignore");
    let read = format!("st{id}::read");
    let f1 = iii.register_function(
        &partial,
        RegisterFunction::new_async(|upload: Upload| async move {
            let mut body = upload.body;
            let first = body.next().await.transpose()?.map(|chunk| chunk.len());
            // A task keeps the stream: the guard still cancels it.
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(30)).await;
                drop(body);
            });
            Ok::<_, Error>(json!({ "first": first }))
        }),
    );
    let f2 = iii.register_function(
        &ignore,
        RegisterFunction::new_async(|upload: Upload| async move {
            let _unread = upload.body;
            Ok::<_, Error>(json!({ "ignored": upload.name }))
        }),
    );
    let f3 = iii.register_function(
        &read,
        RegisterFunction::new_async(|upload: Upload| async move {
            Ok::<_, Error>(json!({ "text": upload.body.text().await? }))
        }),
    );
    for f in [&f1, &f2, &f3] {
        ready(f).await;
    }

    let pipe = iii.pipe().await.unwrap();
    let call = iii.call(remote(&partial, json!({ "name": "p", "body": pipe.input })));
    let (out, written) = tokio::join!(call, pipe.output.write(&[1u8; 1000]));
    written.unwrap();
    assert_eq!(out.unwrap(), json!({ "first": 1000 }));
    // The reader left: the engine closes the writer's socket, so writes stop
    // being taken.
    let deadline = Instant::now() + WAIT;
    let failed = loop {
        if pipe.output.write(&[2u8; 64 * 1024]).await.is_err() {
            break true;
        }
        if Instant::now() > deadline {
            break false;
        }
    };
    assert!(failed, "the partly read input was not cancelled");

    let pipe = iii.pipe().await.unwrap();
    let wire = serde_json::to_value(&pipe.input).unwrap();
    let out = iii
        .call(remote(
            &ignore,
            json!({ "name": "first", "body": wire.clone() }),
        ))
        .await
        .unwrap();
    assert_eq!(out["ignored"], "first");
    pipe.output.write(b"still here").await.unwrap();
    pipe.output.close().await.unwrap();
    let out = iii
        .call(remote(&read, json!({ "name": "second", "body": wire })))
        .await
        .unwrap();
    assert_eq!(out["text"], "still here");
    iii.shutdown_async().await;
}

/// One reader: a second read of an `InputStream` fails.
#[tokio::test]
async fn an_input_stream_has_one_reader() {
    let id = uid();
    let iii = client(&format!("one-reader-{id}")).await;
    let function_id = format!("st{id}::twice");
    let function = iii.register_function(
        &function_id,
        RegisterFunction::new_async(|upload: Upload| async move {
            let first = upload.body.read_all().await?;
            let second = upload.body.read_all().await.map_err(|e| e.to_string());
            Ok::<_, Error>(json!({ "first": first.len(), "second": second.err() }))
        }),
    );
    ready(&function).await;
    let out = iii
        .call(remote(
            &function_id,
            json!({ "name": "x", "body": InputStream::from_bytes(&b"abc"[..]) }),
        ))
        .await
        .unwrap();
    assert_eq!(out["first"], 3);
    assert!(
        out["second"]
            .as_str()
            .unwrap()
            .contains("already has a reader"),
        "{out}"
    );
    iii.shutdown_async().await;
}

/// Messages and bytes keep their order: `frames()` yields both, and
/// `messages()` the text frames, each decoded.
#[tokio::test]
async fn messages_and_bytes_keep_their_order() {
    let id = uid();
    let iii = client(&format!("frames-{id}")).await;
    let function_id = format!("st{id}::respond");
    let function = iii.register_function(
        &function_id,
        RegisterFunction::new_async(|req: Render| async move {
            req.out
                .send(&json!({ "type": "set_status", "status_code": 201 }))
                .await?;
            req.out.write(b"body-1").await?;
            req.out.send_text("raw text").await?;
            req.out.write(b"body-2").await?;
            Ok::<_, Error>(())
        }),
    );
    ready(&function).await;

    let pipe = iii.pipe().await.unwrap();
    iii.call(remote(&function_id, json!({ "out": pipe.output })))
        .await
        .unwrap();
    let frames: Vec<ChannelItem> = pipe.input.frames().try_collect().await.unwrap();
    let ChannelItem::Text(status) = &frames[0] else {
        panic!("{frames:?}")
    };
    assert_eq!(
        serde_json::from_str::<Value>(status).unwrap(),
        json!({ "type": "set_status", "status_code": 201 })
    );
    assert_eq!(
        frames[1..],
        [
            ChannelItem::Binary(b"body-1".to_vec()),
            ChannelItem::Text("raw text".into()),
            ChannelItem::Binary(b"body-2".to_vec()),
        ]
    );

    let pipe = iii.pipe().await.unwrap();
    iii.call(remote(&function_id, json!({ "out": pipe.output })))
        .await
        .unwrap();
    let messages: Vec<Value> = pipe.input.messages().try_collect().await.unwrap();
    assert_eq!(
        messages,
        vec![
            json!({ "type": "set_status", "status_code": 201 }),
            json!("raw text")
        ]
    );
    iii.shutdown_async().await;
}

/// R9: a payload with a stream (a value or a raw ref) is not enqueued;
/// `Void` is allowed.
#[tokio::test]
async fn enqueue_refuses_streams_and_void_takes_them() {
    let id = uid();
    let iii = client(&format!("enqueue-{id}")).await;
    let function_id = format!("st{id}::sink");
    let got = Arc::new(Mutex::new(None));
    let seen = got.clone();
    let function = iii.register_function(
        &function_id,
        RegisterFunction::new_async(move |upload: Upload| {
            let seen = seen.clone();
            async move {
                let text = upload.body.text().await?;
                *seen.lock().unwrap() = Some(text);
                Ok::<_, Error>(())
            }
        }),
    );
    ready(&function).await;

    let enqueue = CallAction::Enqueue {
        queue: "default".into(),
    };
    for body in [
        serde_json::to_value(InputStream::from_bytes(&b"x"[..])).unwrap(),
        json!({ "channel_id": "c", "access_key": "k", "direction": "read" }),
    ] {
        let error = iii
            .call(CallRequest {
                action: Some(enqueue.clone()),
                ..CallRequest::new(&function_id, json!({ "name": "q", "body": body }))
            })
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(STREAM_NOT_ALLOWED), "{error}");
        assert!(matches!(error, Error::StreamNotAllowed(_)));
    }

    let out = iii
        .call(
            remote(
                &function_id,
                json!({ "name": "v", "body": InputStream::from_bytes(&b"fire and forget"[..]) }),
            )
            .action(CallAction::Void),
        )
        .await
        .unwrap();
    assert_eq!(out, Value::Null);
    until("the void call's stream", || got.lock().unwrap().is_some()).await;
    assert_eq!(got.lock().unwrap().as_deref(), Some("fire and forget"));
    iii.shutdown_async().await;
}

/// In-process: a call this worker dispatches to its own function passes
/// its streams as is (no pipe): the handler reads the caller's source and
/// writes into its sink. A returned stream reaches the caller as a stream
/// ref, as from a remote call: no placeholder reaches the caller.
#[tokio::test]
async fn an_in_process_call_passes_streams_without_a_pipe() {
    let id = uid();
    let iii = client(&format!("local-{id}")).await;
    let function_id = format!("st{id}::echo");
    let function = iii.register_function(
        &function_id,
        RegisterFunction::new_async(|input: Value| async move {
            let source = input["body"]["channel_id"].clone();
            let body: InputStream = serde_json::from_value(input["body"].clone())?;
            let out: OutputStream = serde_json::from_value(input["out"].clone())?;
            let text = body.text().await?;
            out.write(format!("echo: {text}").as_bytes()).await?;
            Ok::<_, Error>(json!({
                "source": source,
                "back": InputStream::from_bytes(format!("back: {text}")),
            }))
        }),
    );
    ready(&function).await;

    let sink = Recorder::default();
    let out = iii
        .call(CallRequest::new(
            &function_id,
            json!({
                "body": InputStream::from_bytes(&b"in-process"[..]),
                "out": OutputStream::to_writer(sink.clone()),
            }),
        ))
        .await
        .unwrap();
    assert!(
        out["source"].as_str().unwrap().starts_with("local:"),
        "no pipe: {out}"
    );
    assert_eq!(sink.bytes(), b"echo: in-process");
    assert!(
        sink.closed(),
        "the sink was closed before the call resolved"
    );
    let back: InputStream = serde_json::from_value(out["back"].clone()).unwrap();
    back.bind(&iii);
    assert_eq!(back.text().await.unwrap(), "back: in-process");
    iii.shutdown_async().await;
}

#[derive(Deserialize, schemars::JsonSchema)]
struct UploadSeen {
    name: String,
    body: StreamInfo,
}

/// Observed calls: an observer's target gets a `StreamInfo` for each stream
/// the call carried, through the engine and in-process alike.
#[tokio::test]
async fn an_observer_sees_stream_info_not_the_stream() {
    let id = uid();
    let host = client(&format!("observed-{id}")).await;
    let watcher = client(&format!("watcher-{id}")).await;
    let (source, target) = (format!("st{id}::upload"), format!("st{id}::audit"));
    let function = host.register_function(
        &source,
        RegisterFunction::new_async(|upload: Upload| async move {
            Ok::<_, Error>(json!({ "len": upload.body.read_all().await?.len() }))
        }),
    );
    let seen: Arc<Mutex<Vec<(String, StreamInfo)>>> = Arc::default();
    let inbox = seen.clone();
    watcher.register_function(
        &target,
        RegisterFunction::new(move |event: Observed<UploadSeen>| {
            if let Some(upload) = event.input {
                inbox.lock().unwrap().push((upload.name, upload.body));
            }
            Ok::<_, Error>(())
        }),
    );
    ready(&function).await;
    let observer = watcher.observe(Source::function(&source).capture([Capture::Input]), &target);
    tokio::time::timeout(WAIT, observer.ready())
        .await
        .unwrap()
        .unwrap();

    host.call(remote(
        &source,
        json!({ "name": "routed", "body": InputStream::from_bytes(&b"abc"[..]) }),
    ))
    .await
    .unwrap();
    host.call(CallRequest::new(
        &source,
        json!({ "name": "local", "body": InputStream::from_bytes(&b"abc"[..]) }),
    ))
    .await
    .unwrap();
    until("both observed calls", || seen.lock().unwrap().len() == 2).await;
    let seen = seen.lock().unwrap().clone();
    for (name, info) in &seen {
        assert!(info.stream, "{name}: {info:?}");
        assert_eq!(info.direction, ChannelDirection::Read);
        assert!(!info.id.is_empty());
        let local = info.id.starts_with("local:");
        assert_eq!(local, name == "local", "{name}: {info:?}");
    }
    for c in [host, watcher] {
        c.shutdown_async().await;
    }
}

/// The URL of a channel end, as a 0.26 SDK builds it.
fn raw_url(end: &Value) -> String {
    format!(
        "{}/ws/channels/{}?key={}&dir={}",
        engine_ws_url().trim_end_matches('/'),
        end["channel_id"].as_str().unwrap(),
        end["access_key"].as_str().unwrap(),
        end["direction"].as_str().unwrap()
    )
}

/// Reads an end with a raw socket, as a 0.26 `ChannelReader` does.
async fn raw_read(end: &Value) -> Vec<u8> {
    let (mut ws, _) = tokio_tungstenite::connect_async(raw_url(end))
        .await
        .unwrap();
    let mut bytes = Vec::new();
    while let Some(message) = ws.next().await {
        match message.unwrap() {
            WsMessage::Binary(data) => bytes.extend_from_slice(&data),
            WsMessage::Close(frame) => {
                assert!(
                    frame.as_ref().is_none_or(|f| u16::from(f.code) == 1000),
                    "{frame:?}"
                );
                break;
            }
            _ => {}
        }
    }
    bytes
}

/// Writes an end with a raw socket, as a 0.26 `ChannelWriter` does.
async fn raw_write(end: &Value, data: &'static [u8]) {
    let (mut ws, _) = tokio_tungstenite::connect_async(raw_url(end))
        .await
        .unwrap();
    ws.send(WsMessage::Binary(data.into())).await.unwrap();
    ws.send(WsMessage::Close(None)).await.unwrap();
}

/// Interop: refs built by hand (what a 0.26 SDK sends and expects) work both
/// ways: a raw read ref is an `InputStream`, a raw write ref an
/// `OutputStream`, and the refs this SDK sends are the plain three keys a
/// 0.26 handler reads with its own socket.
#[tokio::test]
async fn raw_refs_from_an_old_sdk_interoperate() {
    let id = uid();
    let iii = client(&format!("interop-{id}")).await;
    let read = format!("st{id}::read");
    let old_reader = format!("st{id}::old-reader");
    let old_writer = format!("st{id}::old-writer");
    let f1 = iii.register_function(
        &read,
        RegisterFunction::new_async(|upload: Upload| async move {
            Ok::<_, Error>(json!({ "text": upload.body.text().await? }))
        }),
    );
    let f2 = iii.register_function(
        &old_reader,
        RegisterFunction::new_async(|input: Value| async move {
            let end = input["body"].clone();
            let keys: Vec<String> = end.as_object().unwrap().keys().cloned().collect();
            let bytes = raw_read(&end).await;
            Ok::<_, Error>(json!({ "keys": keys, "text": String::from_utf8_lossy(&bytes) }))
        }),
    );
    let f3 = iii.register_function(
        &old_writer,
        RegisterFunction::new_async(|input: Value| async move {
            raw_write(&input["out"], b"written by 0.26").await;
            Ok::<_, Error>(Value::Null)
        }),
    );
    for f in [&f1, &f2, &f3] {
        ready(f).await;
    }

    // An old caller: its own pipe, written with a raw socket, the raw read ref
    // in the payload.
    let pipe = iii
        .call(CallRequest::new(
            "engine::channels::create",
            json!({ "buffer_size": null }),
        ))
        .await
        .unwrap();
    let (writer, reader) = (pipe["writer"].clone(), pipe["reader"].clone());
    let (_, out) = tokio::join!(
        raw_write(&writer, b"sent by 0.26"),
        iii.call(remote(&read, json!({ "name": "old", "body": reader })))
    );
    assert_eq!(out.unwrap()["text"], "sent by 0.26");

    // An old handler reads what this SDK sends.
    let out = iii
        .call(remote(
            &old_reader,
            json!({ "body": InputStream::from_bytes(&b"sent by 0.27"[..]) }),
        ))
        .await
        .unwrap();
    assert_eq!(out["text"], "sent by 0.27");
    assert_eq!(
        out["keys"],
        json!(["access_key", "channel_id", "direction"])
    );

    // An old handler writes into this SDK's sink.
    let sink = Recorder::default();
    iii.call(remote(
        &old_writer,
        json!({ "out": OutputStream::to_writer(sink.clone()) }),
    ))
    .await
    .unwrap();
    assert_eq!(sink.bytes(), b"written by 0.26");
    assert!(sink.closed());
    iii.shutdown_async().await;
}

/// A handler that keeps its output open and then drops it unfinished: the
/// caller's sink is aborted and the call fails with `STREAM_ABORTED`, through
/// the engine and in-process alike (it does not wait forever).
#[tokio::test]
async fn an_output_kept_open_and_dropped_aborts_the_callers_sink() {
    let id = uid();
    let iii = client(&format!("kept-open-{id}")).await;
    let function_id = format!("st{id}::keep");
    let function = iii.register_function(
        &function_id,
        RegisterFunction::new_async(|req: Render| async move {
            req.out.keep_open();
            req.out.write(b"partial").await?;
            let out = req.out;
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                drop(out);
            });
            Ok::<_, Error>(())
        }),
    );
    ready(&function).await;
    for local in [false, true] {
        let sink = Recorder::default();
        let error = tokio::time::timeout(
            Duration::from_secs(5),
            iii.call(via(
                local,
                &function_id,
                json!({ "out": OutputStream::to_writer(sink.clone()) }),
            )),
        )
        .await
        .expect("the call resolved")
        .unwrap_err();
        assert_eq!(error.code(), Some(STREAM_ABORTED), "{local}: {error}");
        assert!(!sink.closed(), "{local}");
    }
    iii.shutdown_async().await;
}

/// A handler that keeps its output open and never ends it: the call's
/// timeout bounds the wait for the caller's sink. The call fails with
/// `TIMEOUT` and the sink is already aborted when it does, through the engine
/// and in-process alike.
#[tokio::test]
async fn the_call_timeout_bounds_the_wait_for_a_sink() {
    let id = uid();
    let iii = client(&format!("sink-timeout-{id}")).await;
    let function_id = format!("st{id}::hold");
    let function = iii.register_function(
        &function_id,
        RegisterFunction::new_async(|req: Render| async move {
            req.out.keep_open();
            req.out.write(b"partial").await?;
            let out = req.out;
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(20)).await;
                drop(out);
            });
            Ok::<_, Error>(())
        }),
    );
    ready(&function).await;
    for local in [false, true] {
        let sink = Recorder::default();
        let out = OutputStream::to_writer(sink.clone());
        let started = std::time::Instant::now();
        let error = tokio::time::timeout(
            Duration::from_secs(10),
            iii.call(
                via(local, &function_id, json!({ "out": out.clone() }))
                    .timeout(Duration::from_millis(1500)),
            ),
        )
        .await
        .expect("the call resolved")
        .unwrap_err();
        let elapsed = started.elapsed();
        assert!(matches!(error, Error::Timeout), "{local}: {error}");
        assert_eq!(error.code(), Some("TIMEOUT"), "{local}");
        assert!(
            elapsed >= Duration::from_millis(1000) && elapsed < Duration::from_secs(5),
            "{local}: {elapsed:?}"
        );
        // Aborted before the call failed: a write right after it fails.
        assert!(out.write(b"late").await.is_err(), "{local}");
        assert!(!sink.closed(), "{local}");
    }
    iii.shutdown_async().await;
}

/// A stream read to the end is refused before it is sent, in-process as
/// through the engine.
#[tokio::test]
async fn a_stream_read_to_the_end_is_refused_on_every_path() {
    let id = uid();
    let iii = client(&format!("read-to-end-{id}")).await;
    let function_id = format!("st{id}::consume");
    let function = iii.register_function(
        &function_id,
        RegisterFunction::new_async(|upload: Upload| async move {
            let text = upload.body.text().await?;
            Ok::<_, Error>(json!({ "text": text }))
        }),
    );
    ready(&function).await;
    for local in [false, true] {
        let body = InputStream::from_bytes(&b"once"[..]);
        body.read_all().await.unwrap();
        let error = iii
            .call(via(
                local,
                &function_id,
                json!({ "name": "n", "body": body }),
            ))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(STREAM_IN_USE), "{local}: {error}");
        assert!(
            error.to_string().contains("read to the end"),
            "{local}: {error}"
        );
    }
    iii.shutdown_async().await;
}

const PARTS: &[&[u8]] = &[b"one", b"two", b"three"];

/// A source whose chunks come 50 ms apart.
fn slow(parts: &'static [&'static [u8]]) -> InputStream {
    InputStream::from_stream(futures_util::stream::iter(parts.iter().copied()).then(
        |part| async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok::<_, std::io::Error>(Bytes::from_static(part))
        },
    ))
}

/// A handler that read part of its input and returns it: the caller reads
/// the rest, through the engine and in-process. The rest is sent on before
/// the guard settles the input, as a stream ref.
#[tokio::test]
async fn a_partly_read_input_returned_in_the_result_keeps_its_rest() {
    let id = uid();
    let iii = client(&format!("partial-{id}")).await;
    let function_id = format!("st{id}::partial");
    let function = iii.register_function(
        &function_id,
        RegisterFunction::new_async(|upload: Upload| async move {
            let mut body = upload.body;
            let first = body.next().await.transpose()?.unwrap_or_default();
            Ok::<_, Error>(json!({
                "first": String::from_utf8_lossy(&first),
                "rest": body,
            }))
        }),
    );
    ready(&function).await;
    for local in [false, true] {
        let out = iii
            .call(via(
                local,
                &function_id,
                json!({ "name": "p", "body": slow(PARTS) }),
            ))
            .await
            .unwrap();
        assert_eq!(out["first"], "one", "{local}");
        let channel = out["rest"]["channel_id"].as_str().unwrap();
        assert!(!channel.starts_with("local:"), "{local}: {out}");
        let rest: InputStream = serde_json::from_value(out["rest"].clone()).unwrap();
        rest.bind(&iii);
        assert_eq!(rest.text().await.unwrap(), "twothree", "{local}");
    }
    iii.shutdown_async().await;
}

/// One stream twice in one payload is refused before anything is sent, with
/// `STREAM_IN_USE` and the same message, through the engine and in-process.
#[tokio::test]
async fn the_same_stream_twice_is_refused_on_every_path() {
    let id = uid();
    let iii = client(&format!("twice-{id}")).await;
    let function_id = format!("st{id}::twice");
    let function = iii.register_function(
        &function_id,
        RegisterFunction::new(|_: Value| Ok::<_, Error>(Value::Null)),
    );
    ready(&function).await;
    let mut messages = Vec::new();
    for local in [false, true] {
        let source = InputStream::from_bytes(&b"x"[..]);
        let sink = OutputStream::to_writer(tokio::io::sink());
        for payload in [
            json!({ "a": source, "b": source }),
            json!({ "a": sink, "b": [sink] }),
        ] {
            let error = iii
                .call(via(local, &function_id, payload))
                .await
                .unwrap_err();
            assert_eq!(error.code(), Some(STREAM_IN_USE), "{local}: {error}");
            messages.push(error.to_string());
        }
    }
    messages.dedup();
    assert_eq!(
        messages,
        ["a stream can be sent once: the same stream appears twice"]
    );
    iii.shutdown_async().await;
}

/// A handler that returns the sink it got, written to or not: the call does
/// not wait for that sink (the caller writes it now), and what the caller
/// writes through the returned stream reaches its own sink. In-process too,
/// where the caller gets a stream ref, never a placeholder.
#[tokio::test]
async fn a_sink_returned_in_the_result_is_the_callers_to_write() {
    let id = uid();
    let iii = client(&format!("sink-back-{id}")).await;
    let function_id = format!("st{id}::back");
    let function = iii.register_function(
        &function_id,
        RegisterFunction::new_async(|req: Render| async move {
            if req.pages > 0 {
                req.out.write(b"hi ").await?;
            }
            Ok::<_, Error>(json!({ "out": req.out }))
        }),
    );
    ready(&function).await;
    for local in [false, true] {
        for (pages, expected) in [(0, "there"), (1, "hi there")] {
            let sink = Recorder::default();
            let out = tokio::time::timeout(
                Duration::from_secs(5),
                iii.call(via(
                    local,
                    &function_id,
                    json!({ "pages": pages, "out": OutputStream::to_writer(sink.clone()) }),
                )),
            )
            .await
            .expect("the call does not wait for the sink it returns")
            .unwrap();
            let channel = out["out"]["channel_id"].as_str().unwrap();
            assert!(!channel.starts_with("local:"), "{local} {pages}: {out}");
            let back: OutputStream = serde_json::from_value(out["out"].clone()).unwrap();
            back.bind(&iii);
            back.write(b"there").await.unwrap();
            back.close().await.unwrap();
            until("the sink's close", || sink.closed()).await;
            assert_eq!(sink.bytes(), expected.as_bytes(), "{local} {pages}");
        }
    }
    iii.shutdown_async().await;
}

/// An untyped handler that passes the ref it got on, as raw JSON, to another
/// function: its guard leaves that stream to the other function, which
/// writes it after the first one returned (called through the engine or
/// in-process).
#[tokio::test]
async fn an_untyped_handler_passes_a_raw_ref_on() {
    let id = uid();
    let iii = client(&format!("raw-forward-{id}")).await;
    let (a, b) = (format!("st{id}::a"), format!("st{id}::b"));
    let b_fn = iii.register_function(
        &b,
        RegisterFunction::new_async(|req: Render| async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            req.out.write(b"late").await?;
            Ok::<_, Error>(())
        }),
    );
    let a_fn = iii.register_function(
        &a,
        RegisterFunction::new_async({
            let iii = iii.clone();
            let b = b.clone();
            move |input: Value| {
                let iii = iii.clone();
                let b = b.clone();
                async move {
                    let local = input["local"].as_bool().unwrap_or_default();
                    iii.call(
                        via(local, &b, json!({ "out": input["out"] })).action(CallAction::Void),
                    )
                    .await?;
                    Ok::<_, Error>(json!({ "passed": true }))
                }
            }
        }),
    );
    ready(&a_fn).await;
    ready(&b_fn).await;
    for local in [false, true] {
        let pipe = iii.pipe().await.unwrap();
        let out = iii
            .call(remote(&a, json!({ "local": local, "out": pipe.output })))
            .await
            .unwrap();
        assert_eq!(out["passed"], true);
        let got = tokio::time::timeout(WAIT, pipe.input.read_all())
            .await
            .expect("the stream ended")
            .unwrap();
        assert_eq!(got, b"late", "{local}");
    }
    iii.shutdown_async().await;
}

/// A handler that read part of its input and sends the rest in-process to
/// another function, plain or void: the receiver reads the rest, even after
/// the sender returned (the sender's guard leaves it to the receiver).
#[tokio::test]
async fn a_partly_read_input_sent_in_process_is_the_receivers() {
    let id = uid();
    let iii = client(&format!("partial-local-{id}")).await;
    let (a, b) = (format!("st{id}::a"), format!("st{id}::b"));
    let got: Arc<Mutex<Option<String>>> = Arc::default();
    let b_fn = iii.register_function(
        &b,
        RegisterFunction::new_async({
            let got = got.clone();
            move |upload: Upload| {
                let got = got.clone();
                async move {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    let rest = upload.body.text().await?;
                    *got.lock().unwrap() = Some(rest.clone());
                    Ok::<_, Error>(json!({ "rest": rest }))
                }
            }
        }),
    );
    let a_fn = iii.register_function(
        &a,
        RegisterFunction::new_async({
            let iii = iii.clone();
            let b = b.clone();
            move |upload: Upload| {
                let iii = iii.clone();
                let b = b.clone();
                async move {
                    let mut body = upload.body;
                    let first = body.next().await.transpose()?.unwrap_or_default();
                    let mut request = CallRequest::new(&b, json!({ "name": "rest", "body": body }))
                        .dispatch(Dispatch::Auto);
                    if upload.name == "void" {
                        request = request.action(CallAction::Void);
                    }
                    let out = iii.call(request).await?;
                    Ok::<_, Error>(json!({ "first": String::from_utf8_lossy(&first), "b": out }))
                }
            }
        }),
    );
    ready(&a_fn).await;
    ready(&b_fn).await;
    for name in ["plain", "void"] {
        got.lock().unwrap().take();
        let out = iii
            .call(remote(&a, json!({ "name": name, "body": slow(PARTS) })))
            .await
            .unwrap();
        assert_eq!(out["first"], "one", "{name}");
        until("the receiver's read", || got.lock().unwrap().is_some()).await;
        assert_eq!(got.lock().unwrap().as_deref(), Some("twothree"), "{name}");
    }
    iii.shutdown_async().await;
}
