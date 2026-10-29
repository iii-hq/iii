//! observe against a real engine (`III_URL`): delivery of routed calls
//! (phase 1), and hooks, local dispatch, reports and calls through an
//! observer (phase 2).
//!
//! Requires a running III engine. Set III_URL, or use the default.

mod common;

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use serde_json::{Value, json};

use common::{engine_ws_url, proxy::CountingProxy};
use iii_helpers::observability::OtelConfig;
use iii_sdk::runtime::WorkerMetadata;
use iii_sdk::{
    AddedHook, Call, CallAction, CallRequest, Capture, Dispatch, Error, Filter, IIIClient,
    InitOptions, Match, ObserveRejection, Observed, Observer, ObserverOptions, ObserverState, On,
    RegisterFunction, RemovedHook, Source, WorkerIdentityMode, current_invocation, register_worker,
    scope,
};

const WAIT: Duration = Duration::from_secs(10);

fn uid() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

async fn client_at(url: &str, name: &str) -> IIIClient {
    let iii = register_worker(
        url,
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

async fn client(name: &str) -> IIIClient {
    client_at(&engine_ws_url(), name).await
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

async fn until(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// A target function that records what it receives.
#[derive(Clone, Default)]
struct Inbox(Arc<Mutex<Vec<Delivery>>>);

/// One activation: the target's input and its metadata sidecar.
type Delivery = (Value, Option<Value>);

impl Inbox {
    fn function(&self) -> RegisterFunction {
        let inbox = self.clone();
        RegisterFunction::new(move |input: Value, metadata: Option<Value>| {
            inbox.0.lock().unwrap().push((input, metadata));
            Ok::<_, Error>(json!({ "ok": true }))
        })
    }

    fn len(&self) -> usize {
        self.0.lock().unwrap().len()
    }

    fn inputs(&self) -> Vec<Value> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|(i, _)| i.clone())
            .collect()
    }

    async fn wait(&self, n: usize) -> Vec<Value> {
        until(&format!("{n} activations"), || self.len() >= n).await;
        self.inputs()
    }
}

/// Envelope fields that differ between two deliveries of the same event.
fn stable(mut envelope: Value) -> Value {
    for key in [
        "invocation_id",
        "activation_id",
        "started_at_ms",
        "duration_ms",
        "observer_id",
    ] {
        envelope.as_object_mut().unwrap().remove(key);
    }
    envelope
}

fn call_req(function_id: &str, payload: Value) -> CallRequest {
    CallRequest::new(function_id, payload)
}

// ═════════════════════════ phase 1: routed calls ═════════════════════════

#[tokio::test]
async fn routed_calls_activate_the_matching_observers() {
    let id = uid();
    let (charge, target) = (format!("it{id}::charge"), format!("it{id}::alert"));
    let owner = client(&format!("payments-{id}")).await;
    owner.register_function(
        &charge,
        RegisterFunction::new(|input: Value| Ok::<_, Error>(json!({ "charged": input["amount"] }))),
    );
    let observer = client(&format!("alerts-{id}")).await;
    let inbox = Inbox::default();
    observer.register_function(&target, inbox.function());
    let handle = observer.observe(
        Source::function(&charge).filters(json!({
            "input.amount": { "$gte": 500 },
            "output.charged": { "$exists": true },
            "caller.worker_name": { "$prefix": "checkout-" },
        })),
        &target,
    );
    tokio::time::timeout(WAIT, handle.ready())
        .await
        .unwrap()
        .unwrap();

    let caller = client(&format!("checkout-{id}")).await;
    for amount in [100, 900] {
        caller
            .call(call_req(&charge, json!({ "amount": amount })))
            .await
            .unwrap();
    }
    // another caller: the caller filter says no
    observer
        .call(call_req(&charge, json!({ "amount": 950 })))
        .await
        .unwrap();
    let got = inbox.wait(1).await;
    settle().await;
    assert_eq!(inbox.len(), 1, "only the matching call activates: {got:#?}");
    let envelope = &got[0];
    assert_eq!(envelope["function_id"], charge.as_str());
    assert_eq!(envelope["status"], "success");
    assert_eq!(
        envelope["input"],
        json!({ "amount": 900 }),
        "no _caller_worker_id"
    );
    assert_eq!(envelope["output"], json!({ "charged": 900 }));
    assert_eq!(envelope["caller"]["worker_name"], format!("checkout-{id}"));
    assert_eq!(envelope["observer_id"], handle.id());
    let typed: Observed = serde_json::from_value(envelope.clone()).unwrap();
    assert_eq!(typed.function_id, charge);
    for c in [owner, observer, caller] {
        c.shutdown_async().await;
    }
}

#[tokio::test]
async fn on_error_delivers_the_error_with_its_capture() {
    let id = uid();
    let (charge, target) = (format!("it{id}::fail"), format!("it{id}::on-fail"));
    let owner = client(&format!("payments-{id}")).await;
    owner.register_function(
        &charge,
        RegisterFunction::new(|_: Value| {
            Err::<Value, _>(Error::Remote {
                code: "CARD_DECLINED".into(),
                message: "declined".into(),
                stacktrace: None,
            })
        }),
    );
    let inbox = Inbox::default();
    owner.register_function(&target, inbox.function());
    let handle = owner.observe_with(
        Source::function(&charge)
            .on(On::Error)
            .capture([Capture::Input, Capture::Error]),
        &target,
        ObserverOptions::default().metadata(json!({ "team": "oncall" })),
    );
    tokio::time::timeout(WAIT, handle.ready())
        .await
        .unwrap()
        .unwrap();
    let _ = owner
        .call(call_req(&charge, json!({ "amount": 1 })).dispatch(Dispatch::Remote))
        .await
        .unwrap_err();
    inbox.wait(1).await;
    let (envelope, metadata) = inbox.0.lock().unwrap()[0].clone();
    assert_eq!(envelope["status"], "error");
    assert_eq!(envelope["error"]["code"], "CARD_DECLINED");
    assert!(envelope.get("output").is_none());
    assert!(envelope.get("caller").is_none(), "not captured");
    assert_eq!(
        metadata,
        Some(json!({ "team": "oncall" })),
        "the metadata sidecar"
    );
    owner.shutdown_async().await;
}

#[tokio::test]
async fn targets_keep_observers_pending_and_self_targets_fail() {
    let id = uid();
    let (source, target) = (format!("it{id}::source"), format!("it{id}::late-target"));
    let iii = client(&format!("w-{id}")).await;
    iii.register_function(&source, RegisterFunction::new(|v: Value| Ok::<_, Error>(v)));
    settle().await;
    let handle = iii.observe(Source::function(&source), &target);
    settle().await;
    assert_eq!(handle.state(), ObserverState::Pending);
    let inbox = Inbox::default();
    iii.register_function(&target, inbox.function());
    tokio::time::timeout(WAIT, handle.ready())
        .await
        .unwrap()
        .unwrap();

    let own = iii.observe(Source::function(&source), &source);
    let err = tokio::time::timeout(WAIT, own.ready())
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(err.invocation_error().unwrap().code, "SELF_TARGET");
    iii.shutdown_async().await;
}

#[tokio::test]
async fn args_on_a_function_without_args_format_are_invalid() {
    let id = uid();
    let (source, target) = (format!("it{id}::plain"), format!("it{id}::t"));
    let iii = client(&format!("w-{id}")).await;
    iii.register_function(&source, RegisterFunction::new(|v: Value| Ok::<_, Error>(v)));
    iii.register_function(&target, Inbox::default().function());
    let handle = iii.observe(
        Source::function(&source).args(json!({ "expression": "* * * * * *" })),
        &target,
    );
    let err = tokio::time::timeout(WAIT, handle.ready())
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(err.invocation_error().unwrap().code, "INVALID_ARGS");
    iii.shutdown_async().await;
}

#[tokio::test]
async fn max_fires_and_dispose_end_an_observer() {
    let id = uid();
    let (source, target) = (format!("it{id}::src"), format!("it{id}::t"));
    let iii = client(&format!("w-{id}")).await;
    iii.register_function(&source, RegisterFunction::new(|v: Value| Ok::<_, Error>(v)));
    let inbox = Inbox::default();
    iii.register_function(&target, inbox.function());
    let once = iii.observe_with(
        Source::function(&source).filters(json!({ "input.n": { "$exists": true } })),
        &target,
        ObserverOptions::default().max_fires(1),
    );
    tokio::time::timeout(WAIT, once.ready())
        .await
        .unwrap()
        .unwrap();

    // engine::observers::get shows the observer's filters, normalized and unchanged.
    let got = iii
        .call(call_req(
            "engine::observers::get",
            json!({ "id": once.id() }),
        ))
        .await
        .unwrap();
    let normalized = Filter::parse(&json!({ "input.n": { "$exists": true } })).unwrap();
    let shown = got.get("observer").unwrap_or(&got);
    assert_eq!(&shown["filters"], normalized.normalized(), "{got:#}");

    for n in 0..3 {
        iii.call(call_req(&source, json!({ "n": n })).dispatch(Dispatch::Remote))
            .await
            .unwrap();
    }
    inbox.wait(1).await;
    until("max_fires disposal", || {
        once.state() == ObserverState::Disposed
    })
    .await;
    assert_eq!(once.reason().unwrap().code, "MAX_FIRES");
    settle().await;
    assert_eq!(inbox.len(), 1);

    let open = iii.observe(Source::function(&source), &target);
    tokio::time::timeout(WAIT, open.ready())
        .await
        .unwrap()
        .unwrap();
    open.dispose().await.unwrap();
    assert_eq!(open.state(), ObserverState::Disposed);
    iii.call(call_req(&source, json!({ "n": 9 })).dispatch(Dispatch::Remote))
        .await
        .unwrap();
    settle().await;
    assert_eq!(inbox.len(), 1, "no activation after dispose");
    iii.shutdown_async().await;
}

/// A call made from a handler names it as `caller`, also from a task the
/// handler spawned with `scope`; the engine stamps the worker.
#[tokio::test]
async fn caller_propagates_from_handlers_and_through_scope() {
    let id = uid();
    let (outer, probe) = (format!("it{id}::outer"), format!("it{id}::probe"));
    let seen: Arc<Mutex<Vec<iii_sdk::InvocationCtx>>> = Arc::default();
    let remote = client(&format!("probe-{id}")).await;
    remote.register_function(
        &probe,
        RegisterFunction::new({
            let seen = seen.clone();
            move |v: Value| {
                seen.lock().unwrap().push(current_invocation().unwrap());
                Ok::<_, Error>(v)
            }
        }),
    );
    let caller = client(&format!("caller-{id}")).await;
    caller.register_function(
        &outer,
        RegisterFunction::new_async({
            let caller = caller.clone();
            let probe = probe.clone();
            move |v: Value| {
                let caller = caller.clone();
                let probe = probe.clone();
                async move {
                    let ctx = current_invocation();
                    tokio::spawn(scope(ctx, async move {
                        caller.call(call_req(&probe, json!({}))).await
                    }))
                    .await
                    .unwrap()?;
                    Ok::<_, Error>(v)
                }
            }
        }),
    );
    settle().await;
    caller
        .call(call_req(&outer, json!({})).dispatch(Dispatch::Remote))
        .await
        .unwrap();
    let ctx = seen.lock().unwrap()[0].clone();
    let by = ctx.caller.expect("caller");
    assert_eq!(
        by.worker_name.as_deref(),
        Some(format!("caller-{id}").as_str())
    );
    assert_eq!(by.function_id.as_deref(), Some(outer.as_str()));
    assert!(by.invocation_id.is_some());
    assert_eq!(ctx.observe_depth, 0);
    for c in [remote, caller] {
        c.shutdown_async().await;
    }
}

#[tokio::test]
async fn an_expired_observer_is_disposed() {
    let id = uid();
    let (source, target) = (format!("it{id}::src"), format!("it{id}::t"));
    let iii = client(&format!("w-{id}")).await;
    iii.register_function(&source, RegisterFunction::new(|v: Value| Ok::<_, Error>(v)));
    let inbox = Inbox::default();
    iii.register_function(&target, inbox.function());
    let handle = iii.observe_with(
        Source::function(&source),
        &target,
        ObserverOptions::default().expires_in(Duration::from_millis(300)),
    );
    tokio::time::timeout(WAIT, handle.ready())
        .await
        .unwrap()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    iii.call(call_req(&source, json!({})).dispatch(Dispatch::Remote))
        .await
        .unwrap();
    until("expiry", || handle.state() == ObserverState::Disposed).await;
    assert_eq!(handle.reason().unwrap().code, "EXPIRED");
    settle().await;
    assert_eq!(inbox.len(), 0);
    iii.shutdown_async().await;
}

/// Observers are replayed with the same id after a reconnect and keep
/// delivering.
#[tokio::test]
async fn observers_survive_a_reconnect_with_the_same_id() {
    let id = uid();
    let (source, target) = (format!("it{id}::src"), format!("it{id}::t"));
    let owner = client(&format!("owner-{id}")).await;
    owner.register_function(&source, RegisterFunction::new(|v: Value| Ok::<_, Error>(v)));
    let proxy = CountingProxy::start(&engine_ws_url()).await;
    let observer = client_at(proxy.url(), &format!("observer-{id}")).await;
    let inbox = Inbox::default();
    observer.register_function(&target, inbox.function());
    let handle = observer.observe(Source::function(&source), &target);
    tokio::time::timeout(WAIT, handle.ready())
        .await
        .unwrap()
        .unwrap();

    proxy.drop_connections();
    until("disconnect", || {
        observer.get_connection_state() != iii_sdk::runtime::IIIConnectionState::Connected
    })
    .await;
    until("reconnect", || {
        observer.get_connection_state() == iii_sdk::runtime::IIIConnectionState::Connected
    })
    .await;
    tokio::time::timeout(WAIT, handle.ready())
        .await
        .unwrap()
        .unwrap();
    let listed = owner
        .call(call_req(
            "engine::observers::get",
            json!({ "id": handle.id() }),
        ))
        .await
        .unwrap();
    assert!(listed.to_string().contains(handle.id()), "{listed:#}");
    // the target is registered again too: activations resume
    let deadline = Instant::now() + WAIT;
    while inbox.len() == 0 {
        assert!(
            Instant::now() < deadline,
            "no activation after the reconnect"
        );
        owner
            .call(call_req(&source, json!({ "after": "reconnect" })).dispatch(Dispatch::Remote))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert_eq!(inbox.inputs()[0]["input"], json!({ "after": "reconnect" }));
    for c in [owner, observer] {
        c.shutdown_async().await;
    }
}

/// A target that calls the function it observes makes a cycle; the depth the
/// SDK carries on every call lets the engine stop it.
#[tokio::test]
async fn the_loop_guard_stops_a_cycle() {
    let id = uid();
    let (source, target) = (format!("it{id}::f"), format!("it{id}::g"));
    let iii = client(&format!("w-{id}")).await;
    iii.register_function(&source, RegisterFunction::new(|v: Value| Ok::<_, Error>(v)));
    let depths: Arc<Mutex<Vec<u32>>> = Arc::default();
    iii.register_function(
        &target,
        RegisterFunction::new_async({
            let iii = iii.clone();
            let source = source.clone();
            let depths = depths.clone();
            move |_: Value| {
                let iii = iii.clone();
                let source = source.clone();
                depths
                    .lock()
                    .unwrap()
                    .push(current_invocation().map_or(0, |c| c.observe_depth));
                async move {
                    iii.call(call_req(&source, json!({})).dispatch(Dispatch::Remote))
                        .await?;
                    Ok::<_, Error>(Value::Null)
                }
            }
        }),
    );
    let handle = iii.observe(Source::function(&source), &target);
    tokio::time::timeout(WAIT, handle.ready())
        .await
        .unwrap()
        .unwrap();
    iii.call(call_req(&source, json!({})).dispatch(Dispatch::Remote))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    let seen = depths.lock().unwrap().clone();
    assert!(!seen.is_empty() && seen.len() <= 8, "{seen:?}");
    assert_eq!(seen, (1..=seen.len() as u32).collect::<Vec<_>>());
    iii.shutdown_async().await;
}

// ═════════════ phase 2: local dispatch, hooks, calls through observers ═════

/// The equivalence suite: the same calls, routed through the engine and run
/// in-process, give identical observer-visible envelopes (ids, timings and
/// stacktraces aside).
#[tokio::test]
async fn local_and_remote_calls_deliver_the_same_envelope() {
    let id = uid();
    let (source, target) = (format!("it{id}::double"), format!("it{id}::t"));
    let owner = client(&format!("owner-{id}")).await;
    let function = owner.register_function(
        &source,
        RegisterFunction::new(|v: Value| {
            if v["fail"] == json!(true) {
                return Err(Error::Remote {
                    code: "NOPE".into(),
                    message: "refused".into(),
                    stacktrace: None,
                });
            }
            Ok(json!({ "doubled": v["n"].as_i64().unwrap_or(0) * 2 }))
        }),
    );
    let observer = client(&format!("observer-{id}")).await;
    let inbox = Inbox::default();
    observer.register_function(&target, inbox.function());
    let handle = observer.observe_with(
        Source::function(&source)
            .on(On::Complete)
            .filters(json!({ "input.n": { "$gte": 10 }, "caller.worker_name": { "$eq": format!("owner-{id}") } }))
            .capture([
                Capture::Input,
                Capture::Output,
                Capture::Error,
                Capture::Caller,
                Capture::Metadata,
            ]),
        &target,
        ObserverOptions::default().max_in_flight(1),
    );
    tokio::time::timeout(WAIT, handle.ready())
        .await
        .unwrap()
        .unwrap();
    until("local dispatch", || function.local_dispatch_active()).await;

    let mut expected = 0;
    for payload in [
        json!({ "n": 21 }),
        json!({ "n": 1 }),
        json!({ "n": 30, "fail": true }),
    ] {
        let matches = payload["n"].as_i64().unwrap() >= 10;
        for dispatch in [Dispatch::Remote, Dispatch::Auto] {
            let request = call_req(&source, payload.clone())
                .metadata(json!({ "tenant": "acme" }))
                .dispatch(dispatch);
            let _ = owner.call(request).await;
            if matches {
                expected += 1;
                inbox.wait(expected).await;
            }
        }
    }
    settle().await;
    let got = inbox.inputs();
    assert_eq!(got.len(), 4, "{got:#?}");
    assert_eq!(
        stable(got[0].clone()),
        stable(got[1].clone()),
        "success: {got:#?}"
    );
    assert_eq!(
        stable(got[2].clone()),
        stable(got[3].clone()),
        "error: {got:#?}"
    );
    assert_eq!(got[2]["error"]["code"], "NOPE");
    for c in [owner, observer] {
        c.shutdown_async().await;
    }
}

/// Local calls through the counting proxy: the proxy runs on other threads
/// and the loop yields, so nothing sent during the loop goes uncounted.
async fn local_calls(iii: &IIIClient, function_id: &str, n: u64) {
    for i in 0..n {
        iii.call(call_req(function_id, json!(i))).await.unwrap();
        if i % 1_000 == 0 {
            tokio::task::yield_now().await;
        }
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_million_unobserved_local_calls_put_nothing_on_the_wire() {
    let id = uid();
    let (quiet, loud, target) = (
        format!("it{id}::quiet"),
        format!("it{id}::loud"),
        format!("it{id}::t"),
    );
    let proxy = CountingProxy::start(&engine_ws_url()).await;
    let owner = client_at(proxy.url(), &format!("owner-{id}")).await;
    let function =
        owner.register_function(&quiet, RegisterFunction::new(|v: Value| Ok::<_, Error>(v)));
    let observed =
        owner.register_function(&loud, RegisterFunction::new(|v: Value| Ok::<_, Error>(v)));
    owner.register_function(&target, Inbox::default().function());
    let handle = owner.observe(Source::function(&loud).capture([Capture::Input]), &target);
    tokio::time::timeout(WAIT, handle.ready())
        .await
        .unwrap()
        .unwrap();
    until("local dispatch", || {
        function.local_dispatch_active() && observed.local_dispatch_active()
    })
    .await;
    settle().await;

    // Positive control: the same harness counts what an observed function sends.
    let before = proxy.sent_bytes();
    local_calls(&owner, &loud, 1_000).await;
    let reported = proxy.sent_bytes() - before;
    assert!(reported > 10_000, "only {reported} bytes for 1000 reports");

    let before = proxy.sent_bytes();
    local_calls(&owner, &quiet, 1_000_000).await;
    let sent = proxy.sent_bytes() - before;
    // One periodic `localinvocationstats` (or a keepalive pong) may fall in the
    // window; a single invocation or report frame per call would be > 50 MB.
    assert!(
        sent < 512,
        "{sent} bytes sent during 1M unobserved local calls"
    );
    owner.shutdown_async().await;
}

/// A function recursing into itself 1000 calls deep: local calls are capped
/// per stack and the deepest go through the engine, so it just returns.
#[tokio::test]
async fn deep_recursion_returns() {
    let id = uid();
    let source = format!("it{id}::recurse");
    let owner = client(&format!("owner-{id}")).await;
    let function = owner.register_function(
        &source,
        RegisterFunction::new_async({
            let owner = owner.clone();
            let source = source.clone();
            move |input: Value| {
                let owner = owner.clone();
                let source = source.clone();
                async move {
                    let n = input.as_u64().unwrap_or(0);
                    if n == 0 {
                        return Ok::<_, Error>(json!("done"));
                    }
                    Box::pin(owner.call(call_req(&source, json!(n - 1)))).await
                }
            }
        }),
    );
    until("local dispatch", || function.local_dispatch_active()).await;
    let out = tokio::time::timeout(
        Duration::from_secs(60),
        owner.call(CallRequest {
            timeout_ms: Some(60_000),
            ..call_req(&source, json!(1_000))
        }),
    )
    .await
    .expect("returns")
    .unwrap();
    assert_eq!(out, json!("done"));
    owner.shutdown_async().await;
}

#[tokio::test]
async fn every_local_call_after_ready_is_activated() {
    let id = uid();
    let (source, target) = (format!("it{id}::race"), format!("it{id}::t"));
    let owner = client(&format!("owner-{id}")).await;
    let function =
        owner.register_function(&source, RegisterFunction::new(|v: Value| Ok::<_, Error>(v)));
    until("local dispatch", || function.local_dispatch_active()).await;
    let observer = client(&format!("observer-{id}")).await;
    let inbox = Inbox::default();
    observer.register_function(&target, inbox.function());
    let handle = observer.observe_with(
        Source::function(&source).capture([Capture::Input]),
        &target,
        ObserverOptions::default().max_in_flight(1024),
    );
    tokio::time::timeout(WAIT, handle.ready())
        .await
        .unwrap()
        .unwrap();
    const N: usize = 500;
    for n in 0..N {
        owner
            .call(call_req(&source, json!({ "n": n })))
            .await
            .unwrap();
    }
    inbox.wait(N).await;
    settle().await;
    assert_eq!(inbox.len(), N);
    for c in [owner, observer] {
        c.shutdown_async().await;
    }
}

/// A source with args: each observer names a topic, the added hook rejects
/// one, and the match hook gives each event to its topic's observers. With
/// `answer`, a deliver hook answers each request through its topic's first
/// observer instead.
#[derive(Clone, Default)]
struct Topics {
    added: Arc<Mutex<Vec<Value>>>,
    removed: Arc<Mutex<Vec<String>>>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct TopicArgs {
    topic: String,
}

#[async_trait]
impl AddedHook for Topics {
    async fn on_added(&self, obs: Observer) -> Result<(), ObserveRejection> {
        self.added.lock().unwrap().push(obs.args_value().clone());
        let TopicArgs { topic } = obs.args()?;
        if topic == "forbidden" {
            return Err(ObserveRejection::new("TOPIC_FORBIDDEN", "not this one"));
        }
        Ok(())
    }
}

#[async_trait]
impl RemovedHook for Topics {
    async fn on_removed(&self, obs: Observer) {
        self.removed.lock().unwrap().push(obs.id().to_string());
    }
}

async fn topics_host(name: &str, source: &str, topics: &Topics, answer: bool) -> IIIClient {
    let host = client(name).await;
    host.on_observer_added_for(source, topics.clone())
        .on_observer_removed_for(source, topics.clone())
        .on_observer_match_for(
            source,
            Match::keys(
                |obs| obs.args_value()["topic"].as_str().into(),
                |call| call.input["topic"].as_str().into(),
            ),
        );
    if answer {
        host.on_observer_deliver_for(source, {
            let host = host.clone();
            move |call: Call, observers: Vec<Observer>| {
                let host = host.clone();
                async move {
                    let obs = observers.first().ok_or("no route")?;
                    if !obs.matches(&call) {
                        return Err(Error::Remote {
                            code: "ROUTE_FILTERED".into(),
                            message: "the route's filters say no".into(),
                            stacktrace: None,
                        });
                    }
                    host.call(CallRequest::observer(obs, call.input.clone()))
                        .await
                }
            }
        });
    }
    host.register_function(
        source,
        RegisterFunction::new(|event: Value| Ok::<_, Error>(event))
            .args_format::<TopicArgs>()
            .private(),
    );
    host
}

#[tokio::test]
async fn hosts_get_validated_args_route_by_them_and_can_reject() {
    let id = uid();
    let source = format!("it{id}::topics");
    let topics = Topics::default();
    let host = topics_host(&format!("host-{id}"), &source, &topics, false).await;
    let observer = client(&format!("observer-{id}")).await;
    let (a, b) = (Inbox::default(), Inbox::default());
    observer.register_function(format!("it{id}::a"), a.function());
    observer.register_function(format!("it{id}::b"), b.function());
    settle().await;

    // A missing required arg fails in the engine, before the host is asked.
    let missing = observer.observe(
        Source::function(&source).args(json!({})),
        format!("it{id}::a"),
    );
    let err = tokio::time::timeout(WAIT, missing.ready())
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(err.invocation_error().unwrap().code, "INVALID_ARGS");
    assert!(
        topics.added.lock().unwrap().is_empty(),
        "the hook never ran"
    );

    // The added hook's rejection reaches ready().
    let rejected = observer.observe(
        Source::function(&source).args(json!({ "topic": "forbidden" })),
        format!("it{id}::a"),
    );
    let err = tokio::time::timeout(WAIT, rejected.ready())
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(err.invocation_error().unwrap().code, "TOPIC_FORBIDDEN");

    let to_a = observer.observe(
        Source::function(&source).args(json!({ "topic": "a" })),
        format!("it{id}::a"),
    );
    let to_b = observer.observe(
        Source::function(&source)
            .args(json!({ "topic": "b" }))
            .filters(json!({ "input.n": { "$gt": 1 } })),
        format!("it{id}::b"),
    );
    tokio::time::timeout(WAIT, to_a.ready())
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(WAIT, to_b.ready())
        .await
        .unwrap()
        .unwrap();

    // Called right after ready(): the observer was active before its ack.
    for event in [
        json!({ "topic": "a", "n": 1 }),
        json!({ "topic": "b", "n": 1 }),
        json!({ "topic": "b", "n": 2 }),
    ] {
        let out = host.call(call_req(&source, event.clone())).await.unwrap();
        assert_eq!(out, event, "the caller gets the handler's return value");
    }
    let got_a = a.wait(1).await;
    let got_b = b.wait(1).await;
    settle().await;
    assert_eq!((a.len(), b.len()), (1, 1), "b's filters said no to n: 1");
    assert_eq!(got_a[0]["input"], json!({ "topic": "a", "n": 1 }));
    assert_eq!(got_a[0]["output"], json!({ "topic": "a", "n": 1 }));
    assert_eq!(got_b[0]["input"], json!({ "topic": "b", "n": 2 }));
    assert_eq!(got_a[0]["caller"]["worker_name"], format!("host-{id}"));

    // dispose → the removed hook
    to_a.dispose().await.unwrap();
    until("on_observer_removed", || {
        topics
            .removed
            .lock()
            .unwrap()
            .contains(&to_a.id().to_string())
    })
    .await;
    for c in [host, observer] {
        c.shutdown_async().await;
    }
}

#[tokio::test]
async fn calls_through_an_observer_return_the_targets_result() {
    let id = uid();
    let source = format!("it{id}::requests");
    let topics = Topics::default();
    let host = topics_host(&format!("http-{id}"), &source, &topics, true).await;
    let users = client(&format!("users-{id}")).await;
    let handler = format!("it{id}::get-user");
    users.register_function(
        &handler,
        RegisterFunction::new(|req: Value, metadata: Option<Value>| {
            Ok::<_, Error>(json!({ "status": 200, "path": req["path"], "binding": metadata }))
        }),
    );
    settle().await;
    let route = users.observe_with(
        Source::function(&source)
            .args(json!({ "topic": "users" }))
            .filters(json!({ "input.tenant": { "$eq": "acme" } })),
        &handler,
        ObserverOptions::default().metadata(json!({ "__binding": "route-1" })),
    );
    tokio::time::timeout(WAIT, route.ready())
        .await
        .unwrap()
        .unwrap();

    let res = host
        .call(call_req(
            &source,
            json!({ "topic": "users", "tenant": "acme", "path": "/users/42" }),
        ))
        .await
        .unwrap();
    assert_eq!(res["status"], 200);
    assert_eq!(res["path"], "/users/42");
    assert_eq!(
        res["binding"],
        json!({ "__binding": "route-1" }),
        "metadata sidecar"
    );

    let err = host
        .call(call_req(
            &source,
            json!({ "topic": "users", "tenant": "other" }),
        ))
        .await
        .unwrap_err();
    assert_eq!(err.invocation_error().unwrap().code, "ROUTE_FILTERED");

    // Only the host of the observer's function may call through it.
    let obs = host.observers(&source)[0].clone();
    let err = users
        .call(CallRequest::observer(&obs, json!({ "tenant": "acme" })))
        .await
        .unwrap_err();
    assert_eq!(err.invocation_error().unwrap().code, "NOT_FUNCTION_OWNER");

    route.dispose().await.unwrap();
    until("removal", || !obs.is_active()).await;
    let err = host
        .call(CallRequest::observer(&obs, json!({ "tenant": "acme" })))
        .await
        .unwrap_err();
    let code = err.invocation_error().unwrap().code.clone();
    assert!(
        code == "OBSERVER_NOT_ACTIVE" || code == "OBSERVER_NOT_FOUND",
        "{code}"
    );
    for c in [host, users] {
        c.shutdown_async().await;
    }
}

#[tokio::test]
async fn the_host_sees_its_observers_and_their_filters() {
    let id = uid();
    let (source, target) = (format!("it{id}::plain"), format!("it{id}::t"));
    let host = client(&format!("host-{id}")).await;
    let function =
        host.register_function(&source, RegisterFunction::new(|v: Value| Ok::<_, Error>(v)));
    host.register_function(&target, Inbox::default().function());
    let handle = host.observe(
        Source::function(&source).filters(json!({ "input.n": { "$gt": 1 } })),
        &target,
    );
    tokio::time::timeout(WAIT, handle.ready())
        .await
        .unwrap()
        .unwrap();
    until("the observer reaches its host", || function.is_observed()).await;
    let obs = function.observers()[0].clone();
    assert_eq!(obs.id(), handle.id());
    assert_eq!(obs.target(), target);
    let call = |n: i64| Call {
        input: json!({ "n": n }),
        output: Some(json!({ "n": n })),
        ..Call::default()
    };
    assert!(obs.matches(&call(2)));
    assert!(!obs.matches(&call(1)));
    host.shutdown_async().await;
}

#[tokio::test]
async fn reconnect_keeps_serving_locally_and_flushes_reports() {
    let id = uid();
    let (source, target) = (format!("it{id}::offline"), format!("it{id}::t"));
    let proxy = CountingProxy::start(&engine_ws_url()).await;
    let owner = client_at(proxy.url(), &format!("owner-{id}")).await;
    let function =
        owner.register_function(&source, RegisterFunction::new(|v: Value| Ok::<_, Error>(v)));
    let observer = client(&format!("observer-{id}")).await;
    let inbox = Inbox::default();
    observer.register_function(&target, inbox.function());
    let handle = observer.observe(Source::function(&source).capture([Capture::Input]), &target);
    tokio::time::timeout(WAIT, handle.ready())
        .await
        .unwrap()
        .unwrap();
    until("local dispatch", || function.local_dispatch_active()).await;

    proxy.drop_connections();
    until("disconnect", || {
        owner.get_connection_state() != iii_sdk::runtime::IIIConnectionState::Connected
    })
    .await;
    assert_ne!(
        owner.get_connection_state(),
        iii_sdk::runtime::IIIConnectionState::Connected,
        "the call below is made while disconnected"
    );
    let out = owner
        .call(call_req(&source, json!({ "while": "offline" })))
        .await
        .unwrap();
    assert_eq!(
        out,
        json!({ "while": "offline" }),
        "local dispatch keeps serving"
    );
    let got = inbox.wait(1).await;
    assert_eq!(
        got[0]["input"],
        json!({ "while": "offline" }),
        "reported after reconnect"
    );
    for c in [owner, observer] {
        c.shutdown_async().await;
    }
}

#[tokio::test]
async fn void_local_calls_keep_their_context() {
    let id = uid();
    let source = format!("it{id}::void");
    let seen: Arc<Mutex<Option<iii_sdk::InvocationCtx>>> = Arc::default();
    let owner = client(&format!("owner-{id}")).await;
    let function = owner.register_function(
        &source,
        RegisterFunction::new({
            let seen = seen.clone();
            move |v: Value| {
                *seen.lock().unwrap() = current_invocation();
                Ok::<_, Error>(v)
            }
        }),
    );
    until("local dispatch", || function.local_dispatch_active()).await;
    owner
        .call(CallRequest {
            function_id: source.clone(),
            payload: json!({}),
            action: Some(CallAction::Void),
            timeout_ms: None,
        })
        .await
        .unwrap();
    until("the void call", || seen.lock().unwrap().is_some()).await;
    let ctx = seen.lock().unwrap().clone().unwrap();
    assert_eq!(ctx.function_id, source);
    assert_eq!(
        ctx.caller.unwrap().worker_name.as_deref(),
        Some(format!("owner-{id}").as_str())
    );
    owner.shutdown_async().await;
}
