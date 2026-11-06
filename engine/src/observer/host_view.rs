// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! How a function's host reads its observers' announcements (decision 61):
//! every view names the outcomes the observer fires on (`fires_on`), and a host
//! that did not declare `observe.capture` (0.25, and an rc host that declared
//! `observe.v2` only) decides firing from the legacy `on` alone, so it gets
//! that too. The host's features are read once per announcement of its
//! function (`functionregistered`), never once per observer.

use super::lifecycle::CaptureSet;
use crate::worker_connections::WorkerFeatures;

/// What a host reads from an observer's announcement, from its features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct HostView {
    legacy_on: bool,
}

impl HostView {
    /// The view a connection that declared `features` gets.
    pub(crate) fn of(features: &WorkerFeatures) -> Self {
        #[cfg(test)]
        EVALUATIONS.with(|n| n.set(n.get() + 1));
        Self {
            legacy_on: !features.observe_capture(),
        }
    }

    /// Whether the host also gets the legacy `on` (it lacks
    /// `observe.capture`).
    pub(crate) fn legacy_on(self) -> bool {
        self.legacy_on
    }
}

/// The outcomes `capture` fires on: `success`, `error`, or both in that
/// order.
pub(crate) fn fires(capture: CaptureSet) -> Vec<&'static str> {
    let mut fires = Vec::with_capacity(2);
    if capture.fires(true) {
        fires.push("success");
    }
    if capture.fires(false) {
        fires.push("error");
    }
    fires
}

#[cfg(test)]
thread_local! {
    static EVALUATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// [`HostView::of`] calls on this thread since [`reset_evaluations`].
#[cfg(test)]
pub(crate) fn evaluations() -> usize {
    EVALUATIONS.with(|n| n.get())
}

#[cfg(test)]
pub(crate) fn reset_evaluations() {
    EVALUATIONS.with(|n| n.set(0));
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::mpsc;

    use super::*;
    use crate::{
        engine::{Engine, Outbound},
        observer::{ObserveRequest, events::test_support, lifecycle::CaptureSet},
        protocol::{Capture, Message, ObserveSource, ObserveTarget, ObserverView},
        worker_connections::{WorkerConnection, WorkerFeatures},
    };

    fn features(list: &[&str]) -> WorkerFeatures {
        let features = WorkerFeatures::default();
        let list: Vec<String> = list.iter().map(|f| f.to_string()).collect();
        if !list.is_empty() {
            assert!(features.set(&list));
        }
        features
    }

    #[test]
    fn fires_names_the_outcomes_a_capture_set_fires_on() {
        let set = |list: &[Capture]| CaptureSet::from_list(list);
        assert_eq!(fires(set(&[Capture::Input])), vec!["success"]);
        assert_eq!(fires(set(&[Capture::Output])), vec!["success"]);
        assert_eq!(fires(set(&[])), vec!["success"], "`[]` fires on success");
        assert_eq!(fires(set(&[Capture::Error])), vec!["error"]);
        assert_eq!(
            fires(set(&[Capture::Error, Capture::Caller])),
            vec!["error"]
        );
        assert_eq!(
            fires(set(&[Capture::Input, Capture::Error])),
            vec!["success", "error"]
        );
        assert_eq!(fires(CaptureSet::DEFAULT), vec!["success"]);
    }

    #[test]
    fn only_a_host_that_declared_observe_capture_goes_without_the_legacy_on() {
        let capture = HostView::of(&features(&["observe.v1", "observe.v2", "observe.capture"]));
        assert!(!capture.legacy_on());
        let capture_only = HostView::of(&features(&["observe.capture"]));
        assert!(!capture_only.legacy_on());
        for (what, list) in [
            ("rc host", &["observe.v1", "observe.v2"][..]),
            ("0.25 host", &["observe.v1"][..]),
            ("no features", &[][..]),
        ] {
            assert!(HostView::of(&features(list)).legacy_on(), "{what}");
        }
    }

    fn host(engine: &Engine, list: &[&str]) -> (WorkerConnection, mpsc::Receiver<Outbound>) {
        let (tx, rx) = mpsc::channel(1024);
        let worker = WorkerConnection::new(tx);
        let list: Vec<String> = list.iter().map(|f| f.to_string()).collect();
        assert!(worker.features.set(&list));
        engine.worker_registry.register_worker(worker.clone());
        (worker, rx)
    }

    async fn register(engine: &Arc<Engine>, worker: &WorkerConnection, id: &str) {
        engine
            .router_msg(
                worker,
                &Message::RegisterFunction {
                    id: id.to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                    invocation: None,
                    args_format: None,
                    private: false,
                    observable: true,
                    observe: None,
                },
            )
            .await
            .expect("register function");
    }

    fn request(id: &str, capture: Vec<Capture>) -> ObserveRequest {
        ObserveRequest {
            id: id.to_string(),
            source: ObserveSource {
                function_id: "orders::create".to_string(),
                namespace: None,
                args: None,
                filters: None,
                capture: Some(capture),
                on: None,
                deliver: None,
                raw_input: false,
            },
            target: ObserveTarget {
                function_id: "test::sink".to_string(),
                namespace: None,
            },
            metadata: None,
            max_fires: None,
            expires_in_ms: None,
            max_in_flight: None,
        }
    }

    fn drain(rx: &mut mpsc::Receiver<Outbound>) -> Vec<Message> {
        let mut messages = Vec::new();
        while let Ok(outbound) = rx.try_recv() {
            if let Outbound::Protocol(message) = outbound {
                messages.push(message);
            }
        }
        messages
    }

    /// The rc-host view of an observer by its id: errors fire on `error`,
    /// the rest on success.
    fn assert_rc_view(view: &ObserverView) {
        if view.id.starts_with("obs-error") {
            assert_eq!(view.fires_on, vec!["error"], "{view:?}");
            assert_eq!(view.on.as_deref(), Some("error"), "{view:?}");
        } else {
            assert_eq!(view.fires_on, vec!["success"], "{view:?}");
            assert_eq!(view.on, None, "{view:?}");
        }
    }

    /// guibeira (owner.rs L1455, L1559): the host's capability is read once
    /// per `functionregistered`, not once per observer it lists, and the
    /// `observeradded` that follow reuse it.
    #[tokio::test]
    async fn a_function_registered_with_n_observers_evaluates_its_hosts_capability_once() {
        const N: usize = 6;
        let engine = Arc::new(Engine::new());
        let _sink = test_support::capture(&engine, "test::sink");
        let (observing, _observing_rx) = test_support::worker(&engine);
        for n in 0..N {
            let (id, capture) = if n % 2 == 0 {
                (format!("obs-error-{n}"), vec![Capture::Error])
            } else {
                (format!("obs-input-{n}"), vec![Capture::Input])
            };
            engine
                .observers
                .observe(&engine, &observing, request(&id, capture))
                .await;
            assert!(engine.observers.get(&id).is_some(), "`{id}` registered");
        }

        let (rc_host, mut rc_rx) = host(&engine, &["observe.v1", "observe.v2"]);
        reset_evaluations();
        register(&engine, &rc_host, "orders::create").await;
        assert_eq!(
            evaluations(),
            1,
            "one evaluation for a functionregistered listing {N} observers"
        );
        let listed = drain(&mut rc_rx)
            .into_iter()
            .find_map(|message| match message {
                Message::FunctionRegistered { observers, .. } => Some(observers),
                _ => None,
            })
            .expect("functionregistered");
        assert_eq!(listed.len(), N, "{listed:?}");
        listed.iter().for_each(assert_rc_view);

        // An observer added later is announced with the view the
        // registration evaluated.
        engine
            .observers
            .observe(
                &engine,
                &observing,
                request("obs-error-late", vec![Capture::Error]),
            )
            .await;
        let added = drain(&mut rc_rx)
            .into_iter()
            .find_map(|message| match message {
                Message::ObserverAdded { observer, .. } => Some(observer),
                _ => None,
            })
            .expect("observeradded");
        assert_rc_view(&added);
        assert_eq!(evaluations(), 1, "observeradded evaluates nothing again");

        // Registering again is announced again: one more evaluation for
        // the whole new `functionregistered`, however many it lists.
        register(&engine, &rc_host, "orders::create").await;
        let relisted = drain(&mut rc_rx)
            .into_iter()
            .filter_map(|message| match message {
                Message::FunctionRegistered { observers, .. } => Some(observers),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(evaluations(), relisted.len() + 1, "one per announcement");
        for listed in &relisted {
            assert_eq!(listed.len(), N + 1, "{listed:?}");
            listed.iter().for_each(assert_rc_view);
        }
    }

    /// The observer views of the shared protocol fixtures
    /// (`sdk/fixtures/protocol/observe`), which every SDK decodes and every
    /// SDK's mock engine models, are the views this engine sends: a host that
    /// declared `observe.capture` gets exactly the fixture's fields, its
    /// `fires_on` and its capture, and no `on`; a host without it gets the
    /// same plus the legacy `on` when the observer fires on errors.
    #[tokio::test]
    async fn the_shared_fixture_views_are_the_views_this_engine_sends() {
        use serde_json::Value;
        const ADDED: &str =
            include_str!("../../../sdk/fixtures/protocol/observe/observeradded.json");
        const REGISTERED: &str =
            include_str!("../../../sdk/fixtures/protocol/observe/functionregistered.json");
        let added: Value = serde_json::from_str(ADDED).unwrap();
        let registered: Value = serde_json::from_str(REGISTERED).unwrap();
        let mut fixtures = vec![("observeradded.json", added["observer"].clone())];
        for observer in registered["observers"].as_array().unwrap() {
            fixtures.push(("functionregistered.json", observer.clone()));
        }
        let keys = |view: &Value| {
            let mut keys: Vec<String> = view.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            keys
        };

        for (file, fixture) in fixtures {
            let capture: Vec<Capture> = serde_json::from_value(fixture["capture"].clone()).unwrap();
            for (kind, features) in [
                (
                    "capture host",
                    &["observe.v1", "observe.v2", "observe.capture"][..],
                ),
                ("rc host", &["observe.v1", "observe.v2"][..]),
            ] {
                let engine = Arc::new(Engine::new());
                let _sink = test_support::capture(&engine, "test::sink");
                let (observing, _observing_rx) = test_support::worker(&engine);
                let (host, mut rx) = host(&engine, features);
                register(&engine, &host, "orders::create").await;
                drain(&mut rx);
                let mut request = request("obs", capture.clone());
                request.source.filters = Some(fixture["filters"].clone());
                engine.observers.observe(&engine, &observing, request).await;
                let view = drain(&mut rx)
                    .into_iter()
                    .find_map(|message| match message {
                        Message::ObserverAdded { observer, .. } => Some(observer),
                        _ => None,
                    })
                    .unwrap_or_else(|| panic!("{file} [{kind}]: no observeradded"));
                let sent = serde_json::to_value(&view).unwrap();
                let what = format!("{file} [{kind}]: engine {sent} vs fixture {fixture}");
                assert_eq!(sent["fires_on"], fixture["fires_on"], "{what}");
                assert_eq!(sent["capture"], fixture["capture"], "{what}");
                let mut expected = keys(&fixture);
                if kind == "rc host" && CaptureSet::from_list(&capture).legacy_on().is_some() {
                    expected.push("on".to_string());
                    expected.sort();
                }
                assert_eq!(keys(&sent), expected, "{what}");
            }
        }
    }
}
