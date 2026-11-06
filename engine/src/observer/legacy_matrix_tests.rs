//! CONTRACT D4: an older SDK's `source.on` / `source.deliver` are translated
//! at registration instead of refused, and every one of the 528 old
//! `on` x `deliver` x `capture` combinations fires on exactly the outcomes it
//! fired on before and hands the target exactly the payload it got before.
//!
//! The expected behavior is the v0.25 engine's, twice over: transcribed from
//! its `accepts` / `project` ([`old_fires`], [`old_payload`]), and checked
//! against the machine-made matrix of the pr2471 investigation
//! (`testdata/legacy_on_deliver_matrix.jsonl`, one row per combination).
//!
//! Two differences are accepted and documented:
//! - on a success that reported no output, the old envelope carried
//!   `"output": null`, the new one leaves the key out (every absent field is
//!   omitted);
//! - the 33 rows that carry neither `on` nor `deliver` are, on the wire, new
//!   style sources: `capture` alone decides. 19 of them behave differently by
//!   design (a capture naming `error` fires on failures; `[caller]`,
//!   `[metadata]`, `[caller,metadata]` are refused) and are pinned as such.
//!   That holds only for a worker that declared `observe.capture`: every row
//!   is also sent from an rc SDK (`observe.v2` only) and a 0.25 SDK (no
//!   features), and from those the 19 rows keep the v0.25 result too
//!   (CONTRACT D8).

use std::sync::{Arc, Mutex, atomic::AtomicUsize};

use serde_json::{Map, Value, json};
use tokio::sync::mpsc;

use super::{Completed, Delivery, InvocationId, Payload, project};
use crate::{
    engine::{Engine, EngineTrait, Handler, Outbound, RegisterFunctionRequest},
    function::FunctionResult,
    observer::{ObserveRequest, events::test_support},
    protocol::{DEFAULT_NAMESPACE, Message, ObserveSource, ObserveTarget, ObserverState},
};

const MATRIX: &str = include_str!("testdata/legacy_on_deliver_matrix.jsonl");
const FIELDS: [&str; 5] = ["input", "output", "error", "caller", "metadata"];
/// The old default capture: `[input, output, caller]`.
const OLD_DEFAULT: [bool; 5] = [true, true, false, true, false];
/// The observing connections every row is sent from: a current SDK, an rc
/// SDK (`observe.v2` only) and a 0.25 SDK (no features).
const CONNECTIONS: [(&str, &[&str]); 3] = [
    ("observe.capture", test_support::CURRENT_SDK),
    ("rc", test_support::RC_SDK),
    ("0.25", &[]),
];

fn completed(success: bool, with_output: bool) -> Completed {
    Completed {
        delivery: Delivery::Engine,
        function: Arc::new((DEFAULT_NAMESPACE.to_string(), "test::src".to_string())),
        invocation_id: InvocationId::Reported("1".to_string()),
        success,
        output_is_input: false,
        input: Some(Payload::Value(Arc::new(json!({ "in": 1 })))),
        output: (success && with_output).then(|| Payload::Value(Arc::new(json!({ "out": 2 })))),
        error: (!success)
            .then(|| Payload::Value(Arc::new(json!({ "code": "E", "message": "boom" })))),
        caller: Some(Arc::new(json!({ "function_id": "caller::fn" }))),
        metadata: Some(Payload::Value(Arc::new(json!({ "m": 3 })))),
        started_at_ms: 0,
        duration_ms: 0.0,
        depth: 0,
        traceparent: None,
        baggage: None,
        queues: AtomicUsize::new(0),
        charged: AtomicUsize::new(0),
        encoded: once_cell::race::OnceBox::new(),
    }
}

/// v0.25 `accepts(on, success)`.
fn old_fires(on: &str) -> (bool, bool) {
    (
        matches!(on, "" | "success" | "complete"),
        matches!(on, "error" | "complete"),
    )
}

/// v0.25 `dispatch::project`, transcribed.
fn old_payload(deliver: &str, capture: [bool; 5], c: &Completed) -> Value {
    let value = |p: &Option<Payload>| p.as_ref().and_then(Payload::value).map(|v| Value::clone(v));
    let shipped = |p: &Option<Payload>| value(p).unwrap_or(Value::Null);
    match deliver {
        "input" => return shipped(&c.input),
        "output" => return shipped(&c.output),
        _ => {}
    }
    let mut m = Map::new();
    m.insert("observer_id".into(), "obs".into());
    m.insert("activation_id".into(), "a".into());
    m.insert("function_id".into(), c.function.1.clone().into());
    m.insert("namespace".into(), c.function.0.clone().into());
    m.insert("invocation_id".into(), c.invocation_id.to_string().into());
    m.insert(
        "status".into(),
        if c.success { "success" } else { "error" }.into(),
    );
    for (wanted, key, value) in [
        (capture[0], "input", value(&c.input)),
        (capture[2], "error", value(&c.error)),
        (capture[3], "caller", c.caller.as_deref().cloned()),
        (capture[4], "metadata", value(&c.metadata)),
    ] {
        if wanted && let Some(value) = value {
            m.insert(key.into(), value);
        }
    }
    if capture[1] && c.success {
        m.insert("output".into(), shipped(&c.output));
    }
    m.insert("started_at_ms".into(), c.started_at_ms.into());
    m.insert(
        "duration_ms".into(),
        serde_json::Number::from_f64(c.duration_ms)
            .map(Value::Number)
            .unwrap_or(Value::Null),
    );
    Value::Object(m)
}

/// The matrix's payload summary: `envelope{keys}` without the ids and
/// timings, or `raw <value>` for a non-object.
fn summary(v: &Value) -> String {
    const TOP: [&str; 7] = [
        "observer_id",
        "activation_id",
        "function_id",
        "namespace",
        "invocation_id",
        "started_at_ms",
        "duration_ms",
    ];
    match v.as_object() {
        Some(o) => format!(
            "envelope{{{}}}",
            o.keys()
                .filter(|k| !TOP.contains(&k.as_str()))
                .cloned()
                .collect::<Vec<_>>()
                .join(",")
        ),
        None => format!("raw {v}"),
    }
}

fn parse_capture(label: &str) -> Option<Vec<&str>> {
    if label == "(absent)" {
        return None;
    }
    let inner = label.trim_start_matches('[').trim_end_matches(']');
    Some(if inner.is_empty() {
        Vec::new()
    } else {
        inner.split(',').collect()
    })
}

fn absent_as_empty(label: &str) -> &str {
    if label == "(absent)" { "" } else { label }
}

fn register(engine: &Engine, function_id: &str) {
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: function_id.to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|input: Value| async move { FunctionResult::Success(Some(input)) }),
    );
}

/// An `observe` from an older SDK: `source` as it arrived on the wire.
fn legacy_request(id: &str, source: Value) -> ObserveRequest {
    ObserveRequest {
        id: id.to_string(),
        source: serde_json::from_value::<ObserveSource>(source).expect("decodes"),
        target: ObserveTarget {
            function_id: "test::dst".to_string(),
            namespace: None,
        },
        metadata: None,
        max_fires: None,
        expires_in_ms: None,
        max_in_flight: None,
    }
}

/// The `observeresult`s queued on the observing worker's socket.
fn results(socket: &mut mpsc::Receiver<Outbound>) -> Vec<(ObserverState, Option<String>)> {
    let mut out = Vec::new();
    while let Ok(frame) = socket.try_recv() {
        if let Outbound::Protocol(Message::ObserveResult { state, error, .. }) = frame {
            out.push((state, error.map(|e| format!("{}: {}", e.code, e.message))));
        }
    }
    out
}

#[tokio::test]
async fn every_old_on_deliver_capture_combination_keeps_its_firing_and_payload() {
    let rows: Vec<Value> = MATRIX
        .lines()
        .map(|line| serde_json::from_str(line).expect("matrix row"))
        .collect();
    assert_eq!(rows.len(), 528, "4 on x 4 deliver x 33 capture shapes");
    let (ok, err, void) = (
        completed(true, true),
        completed(false, false),
        completed(true, false),
    );

    let mut failures = Vec::new();
    let mut new_model = Vec::new();
    let mut legacy_checked = 0;
    for row in &rows {
        let field = |key: &str| row[key].as_str().expect("string column").to_string();
        let (on, deliver, capture_label) = (field("on"), field("deliver"), field("capture"));
        let (on, deliver) = (absent_as_empty(&on), absent_as_empty(&deliver));
        let capture = parse_capture(&capture_label);
        let old_capture: [bool; 5] = match &capture {
            None => OLD_DEFAULT,
            Some(list) => std::array::from_fn(|i| list.contains(&FIELDS[i])),
        };
        let label = format!("on={on:?} deliver={deliver:?} capture={capture_label}");

        // The oracle agrees with the matrix the investigation produced.
        let (fires_ok, fires_err) = old_fires(on);
        let shape = if deliver.is_empty() {
            "envelope"
        } else {
            deliver
        };
        let (old_ok, old_err, old_void) = (
            old_payload(shape, old_capture, &ok),
            old_payload(shape, old_capture, &err),
            old_payload(shape, old_capture, &void),
        );
        let fires_label = match (fires_ok, fires_err) {
            (true, true) => "success+error",
            (true, false) => "success",
            _ => "error",
        };
        assert_eq!(field("old_fires"), fires_label, "{label}");
        let expect_summary = |fires: bool, payload: &Value| {
            if fires {
                summary(payload)
            } else {
                String::new()
            }
        };
        assert_eq!(
            field("old_success_payload"),
            expect_summary(fires_ok, &old_ok),
            "{label}"
        );
        assert_eq!(
            field("old_error_payload"),
            expect_summary(fires_err, &old_err),
            "{label}"
        );
        // From a worker that declared `observe.capture`, a source without
        // `on` and `deliver` is new-style: `capture` alone decides what
        // fires, and the gate refuses a non-empty capture of `caller` /
        // `metadata` alone. From any other worker (an rc SDK, a 0.25 SDK)
        // every source keeps the v0.25 semantics (CONTRACT D8).
        let new_style = on.is_empty() && deliver.is_empty();
        let (new_ok, new_err, new_refused) = match (&capture, new_style) {
            (Some(list), true) => {
                let io = old_capture[0] || old_capture[1];
                let refused = !list.is_empty() && !io && !old_capture[2];
                (!refused && (list.is_empty() || io), old_capture[2], refused)
            }
            _ => (fires_ok, fires_err, false),
        };
        let differs = (new_ok, new_err, new_refused) != (fires_ok, fires_err, false);
        if differs {
            new_model.push(label.clone());
        }

        for (kind, features) in CONNECTIONS {
            let (expect_ok, expect_err, expect_refused) = if kind == "observe.capture" {
                (new_ok, new_err, new_refused)
            } else {
                if differs {
                    legacy_checked += 1;
                }
                (fires_ok, fires_err, false)
            };
            let label = format!("[{kind}] {label}");

            // The same `observe`, as that SDK sends it, to this engine.
            let mut source = json!({ "function_id": "test::src" });
            if !on.is_empty() {
                source["on"] = on.into();
            }
            if !deliver.is_empty() {
                source["deliver"] = deliver.into();
            }
            if let Some(list) = &capture {
                source["capture"] = json!(list);
            }
            let engine = Arc::new(Engine::new());
            register(&engine, "test::src");
            let _target = test_support::capture(&engine, "test::dst");
            let (worker, mut socket) = test_support::worker_with(&engine, features);
            let request = legacy_request("obs", source);
            engine
                .observers
                .observe(&engine, &worker, request.clone())
                .await;
            let created = results(&mut socket);
            let refused = created
                .iter()
                .any(|(state, _)| *state == ObserverState::Error);
            let stored = engine.observers.get("obs");
            if refused != expect_refused || stored.is_some() == refused {
                failures.push(format!("{label}: answered {created:?}"));
                continue;
            }
            let Some(observer) = stored else {
                continue;
            };

            // Its replay (a reconnect) is the same spec, and the observer it
            // answers for is the one it made.
            engine.observers.observe(&engine, &worker, request).await;
            let replayed = results(&mut socket);
            if replayed
                .iter()
                .any(|(state, _)| *state == ObserverState::Error)
            {
                failures.push(format!("{label}: replay {replayed:?}"));
            }
            let observer = engine
                .observers
                .get("obs")
                .filter(|after| {
                    after.capture == observer.capture && after.deliver == observer.deliver
                })
                .unwrap_or_else(|| {
                    failures.push(format!("{label}: the replay changed the observer"));
                    observer
                });

            let (fires_ok, fires_err) = (expect_ok, expect_err);
            let mut wrong = Vec::new();
            if observer.capture.fires(true) != fires_ok {
                wrong.push(format!("fires on success: {}", !fires_ok));
            }
            if observer.capture.fires(false) != fires_err {
                wrong.push(format!("fires on error: {}", !fires_err));
            }
            if fires_ok {
                let new_ok = project(&observer, &ok, "a");
                if new_ok != old_ok {
                    wrong.push(format!("success payload {new_ok} != old {old_ok}"));
                }
                // Accepted: no `"output": null` when the report carried none.
                let mut old_void = old_void.clone();
                if let Some(envelope) = old_void.as_object_mut()
                    && envelope.get("output") == Some(&Value::Null)
                {
                    envelope.remove("output");
                }
                let new_void = project(&observer, &void, "a");
                if new_void != old_void {
                    wrong.push(format!("void payload {new_void} != old {old_void}"));
                }
            }
            if fires_err {
                let new_err = project(&observer, &err, "a");
                if new_err != old_err {
                    wrong.push(format!("error payload {new_err} != old {old_err}"));
                }
            }
            // The host of the observed function ships what the payload needs.
            let needs = observer.capture.with_delivered(&observer.deliver);
            let shipped = [
                needs.input,
                needs.output,
                needs.error,
                needs.caller,
                needs.metadata,
            ];
            let wanted: [bool; 5] = match shape {
                "input" => [true, false, false, false, false],
                "output" => [false, true, false, false, false],
                _ => old_capture,
            };
            for i in 0..5 {
                if wanted[i] && !shipped[i] {
                    wrong.push(format!("the host does not ship `{}`", FIELDS[i]));
                }
            }
            if !wrong.is_empty() {
                failures.push(format!("{label}: {}", wrong.join("; ")));
            }
        }
    }
    // The rows without `on` and `deliver` are new-style sources from a worker
    // that declared `observe.capture`: `capture` alone decides. Pinned, by
    // design: the 16 capture shapes naming `error` fire on failures (and only
    // on successes too with `input` / `output`), and `[caller]`, `[metadata]`,
    // `[caller,metadata]` are refused. From an rc or 0.25 SDK the same 19
    // rows keep the v0.25 result (CONTRACT D8).
    assert_eq!(new_model.len(), 19, "{}", new_model.join("\n"));
    assert_eq!(
        legacy_checked,
        19 * 2,
        "each pinned row, from an rc and a 0.25 SDK"
    );
    assert!(
        failures.is_empty(),
        "{} of {} checks differ:\n{}",
        failures.len(),
        rows.len() * CONNECTIONS.len(),
        failures.join("\n")
    );
}

/// Values an older engine refused are still refused: `INVALID_ARGS`, and
/// nothing is stored.
#[tokio::test]
async fn invalid_legacy_on_or_deliver_values_are_refused() {
    for (n, extra) in [
        json!({ "on": "bogus" }),
        json!({ "on": 5 }),
        json!({ "on": ["error"] }),
        json!({ "deliver": "both" }),
        json!({ "deliver": true }),
        json!({ "on": "error", "deliver": "payload" }),
    ]
    .into_iter()
    .enumerate()
    {
        let engine = Arc::new(Engine::new());
        register(&engine, "test::src");
        let _target = test_support::capture(&engine, "test::dst");
        let (worker, mut socket) = test_support::worker(&engine);
        let mut source = json!({ "function_id": "test::src" });
        for (key, value) in extra.as_object().unwrap() {
            source[key] = value.clone();
        }
        let id = format!("bad-{n}");
        engine
            .observers
            .observe(&engine, &worker, legacy_request(&id, source))
            .await;
        let answered = results(&mut socket);
        assert!(
            matches!(answered.as_slice(), [(ObserverState::Error, Some(e))] if e.starts_with("INVALID_ARGS")),
            "{extra} -> {answered:?}"
        );
        assert!(engine.observers.get(&id).is_none(), "{extra}: stored");
    }
}

/// A translated observer is announced as the observer it now is: `on: error`
/// with a capture of `caller` alone (refused from a new SDK) is accepted from
/// an older one, since its firing comes from `on`.
#[tokio::test]
async fn an_old_capture_the_new_gate_refuses_is_accepted_with_on() {
    let engine = Arc::new(Engine::new());
    register(&engine, "test::src");
    let _target = test_support::capture(&engine, "test::dst");
    let (worker, mut socket) = test_support::worker(&engine);
    let source = json!({ "function_id": "test::src", "on": "error", "capture": ["caller"] });
    engine
        .observers
        .observe(&engine, &worker, legacy_request("obs", source))
        .await;
    assert!(
        results(&mut socket)
            .iter()
            .all(|(state, _)| *state != ObserverState::Error)
    );
    let observer = engine.observers.get("obs").expect("stored");
    assert!(!observer.capture.fires(true) && observer.capture.fires(false));
    assert_eq!(
        observer.capture.to_list(),
        vec![crate::protocol::Capture::Caller]
    );

    // The same capture without `on` is a new-style source: still refused.
    let source = json!({ "function_id": "test::src", "capture": ["caller"] });
    engine
        .observers
        .observe(&engine, &worker, legacy_request("obs-new", source))
        .await;
    assert!(matches!(
        results(&mut socket).as_slice(),
        [(ObserverState::Error, Some(e))] if e.starts_with("INVALID_ARGS")
    ));
}

#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Logs {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Each registration of a translated observer (its creation and every
/// replay) logs one deprecation warning naming the observer, the fields and
/// the `capture` that replaces them. A new-style source logs none.
#[tokio::test(flavor = "current_thread")]
async fn a_translated_observer_logs_one_deprecation_warning_per_registration() {
    let logs = Logs::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    // Other tests on other threads may have cached the warning's callsite
    // as uninteresting before this subscriber existed.
    tracing::callsite::rebuild_interest_cache();

    let engine = Arc::new(Engine::new());
    register(&engine, "test::src");
    let _target = test_support::capture(&engine, "test::dst");
    let (worker, _socket) = test_support::worker(&engine);
    let text = || String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    let warnings = |id: &str| {
        text()
            .lines()
            .filter(|line| line.contains("WARN") && line.contains(id))
            .count()
    };

    let legacy = json!({ "function_id": "test::src", "on": "complete", "deliver": "input" });
    engine
        .observers
        .observe(
            &engine,
            &worker,
            legacy_request("obs-legacy", legacy.clone()),
        )
        .await;
    assert_eq!(warnings("obs-legacy"), 1, "{}", text());
    let line = text();
    for needle in ["deprecated", "source.on", "source.deliver", "capture"] {
        assert!(line.contains(needle), "`{needle}` in {line}");
    }
    engine
        .observers
        .observe(&engine, &worker, legacy_request("obs-legacy", legacy))
        .await;
    assert_eq!(warnings("obs-legacy"), 2, "one per registration");

    let modern = json!({ "function_id": "test::src", "capture": ["input", "error"] });
    engine
        .observers
        .observe(&engine, &worker, legacy_request("obs-modern", modern))
        .await;
    assert_eq!(warnings("obs-modern"), 0, "{}", text());
}

/// CONTRACT D8: a worker that did not declare `observe.capture` (an rc SDK
/// with `observe.v2` only, a 0.25 SDK with no features) gets the v0.25
/// semantics even without `on` / `deliver`: it fires on successes alone, its
/// payload carries exactly its capture, and the new gate refuses nothing.
/// Its creation and its replay each log one warning naming the observer and
/// `observe.capture`. A worker that declared it gets the capture model and
/// no warning.
#[tokio::test(flavor = "current_thread")]
async fn a_worker_without_observe_capture_keeps_the_old_semantics() {
    let logs = Logs::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    tracing::callsite::rebuild_interest_cache();
    let text = || String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    let warnings = |id: &str| {
        text()
            .lines()
            .filter(|line| line.contains("WARN") && line.contains(&format!("`{id}`")))
            .count()
    };

    for (kind, features) in CONNECTIONS {
        let engine = Arc::new(Engine::new());
        register(&engine, "test::src");
        let _target = test_support::capture(&engine, "test::dst");
        let (worker, mut socket) = test_support::worker_with(&engine, features);
        let legacy = kind != "observe.capture";
        for (id, capture) in [
            ("obs-error", json!(["error"])),
            ("obs-input-error", json!(["input", "error"])),
            ("obs-caller", json!(["caller"])),
            ("obs-metadata", json!(["metadata"])),
        ] {
            let id = format!("{id}-{kind}");
            let source = json!({ "function_id": "test::src", "capture": capture });
            for registration in 1..=2 {
                engine
                    .observers
                    .observe(&engine, &worker, legacy_request(&id, source.clone()))
                    .await;
                let answered = results(&mut socket);
                let refused = answered
                    .iter()
                    .any(|(state, _)| *state == ObserverState::Error);
                let gate_refuses = capture == json!(["caller"]) || capture == json!(["metadata"]);
                assert_eq!(
                    refused,
                    !legacy && gate_refuses,
                    "[{kind}] {id} registration {registration}: {answered:?}"
                );
                assert_eq!(
                    warnings(&id),
                    if legacy { registration } else { 0 },
                    "[{kind}] {id}: one warning per registration from a legacy SDK\n{}",
                    text()
                );
            }
            if refused_by_gate(&engine, &id) {
                continue;
            }
            let observer = engine.observers.get(&id).expect("stored");
            let names_error = capture.as_array().unwrap().contains(&json!("error"));
            let (on_ok, on_err) = (observer.capture.fires(true), observer.capture.fires(false));
            if legacy {
                assert!(
                    on_ok && !on_err,
                    "[{kind}] {id}: v0.25 fires on success only"
                );
            } else {
                assert_eq!(on_err, names_error, "[{kind}] {id}");
            }
            let fields: Vec<String> = observer
                .capture
                .to_list()
                .iter()
                .map(|c| c.as_str().to_string())
                .collect();
            let wanted: Vec<String> = serde_json::from_value(capture.clone()).unwrap();
            assert_eq!(
                fields, wanted,
                "[{kind}] {id}: the payload carries the capture"
            );
        }
        if legacy {
            let line = text();
            assert!(
                line.contains("observe.capture"),
                "names the feature: {line}"
            );
        }
    }
}

fn refused_by_gate(engine: &Engine, id: &str) -> bool {
    engine.observers.get(id).is_none()
}
