// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! What compose reports, and who reports it.
//!
//! This crate knows nothing about a telemetry vendor. It builds the event and
//! hands it to a reporter the host binary installs, which for `iii` is
//! `cli::telemetry`. A host that installs nothing sends nothing, which is what
//! an embedded compose and every test does.
//!
//! The report goes out through the process that runs compose rather than
//! through a worker on a durable topic. Compose has no queue worker to depend
//! on, the failures worth reporting happen before any worker is registered,
//! and `down` stops the engine an event would have to travel through.
//!
//! Nothing here carries a path, a container name, a worker reference, or an
//! error message. An error becomes its `ComposeError::code()`, which is a
//! fixed identifier, and a name becomes a count.

use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures::future::BoxFuture;
use serde_json::{Value, json};

use crate::{
    config::{ComposeFile, WorkerSource},
    lifecycle::OpResult,
    state::ChildStatus,
};

/// One foreground `iii compose --up`: the whole project, once.
pub const UP_FINISHED: &str = "compose_up_finished";
/// One container that did not reach `ready` during that startup.
pub const CONTAINER_FAILED: &str = "compose_container_failed";
/// One later mutation: up, down, add, remove, restart, update.
pub const OP_FINISHED: &str = "compose_op_finished";

/// A failed startup reports at most this many containers. The first few carry
/// the cause; the rest are usually the same failure counted again.
const MAX_REPORTED_FAILURES: usize = 3;

/// One event waiting to be sent: its name and its properties.
pub type Report = (String, Value);

/// Sends a batch of events. The future completes when the send does, so a
/// foreground command can wait for it before the process exits.
///
/// A batch rather than one event at a time: a failed startup has several
/// events to send at once, and an operator should wait for one round trip,
/// not one for each container.
pub type Reporter = Arc<dyn Fn(Vec<Report>) -> BoxFuture<'static, ()> + Send + Sync>;

static REPORTER: OnceLock<Reporter> = OnceLock::new();

/// Installs the reporter for this process. The first call wins, so a test that
/// installs one is not overwritten by a later host.
pub fn set_reporter(reporter: Reporter) {
    let _ = REPORTER.set(reporter);
}

/// Sends a batch, or does nothing when no host installed a reporter.
pub(crate) async fn send(reports: Vec<Report>) {
    if reports.is_empty() {
        return;
    }
    if let Some(reporter) = REPORTER.get() {
        reporter(reports).await;
    }
}

/// Sends one event.
pub(crate) async fn report(event: &str, properties: Value) {
    send(vec![(event.to_string(), properties)]).await;
}

/// Sends a batch, and waits for it only when the process is about to leave.
///
/// A process that stays has somewhere for the send to finish and nothing
/// waits for it. One that leaves has to wait, or the report never goes out.
pub(crate) async fn send_waiting(reports: Vec<Report>, wait: bool) {
    if wait {
        send(reports).await;
    } else {
        tokio::spawn(send(reports));
    }
}

/// Set once this process has reported a startup.
///
/// A compose file that does not parse, or a lock that is out of date, fails
/// before the daemon exists, so the only place left to report it is the
/// command's own error path. That path must stay quiet for every failure the
/// daemon already reported itself.
static UP_REPORTED: AtomicBool = AtomicBool::new(false);

/// Builds every report for one startup, and claims the startup report for
/// this process.
pub(crate) fn up_reports(properties: Value, result: Option<&OpResult>) -> Vec<Report> {
    UP_REPORTED.store(true, Ordering::Release);
    let mut reports = vec![(UP_FINISHED.to_string(), properties)];
    if let Some(result) = result {
        reports.extend(
            failed_container_properties(result)
                .into_iter()
                .map(|properties| (CONTAINER_FAILED.to_string(), properties)),
        );
    }
    reports
}

/// Reports a startup that failed before the daemon could report it itself.
pub(crate) async fn report_up_never_started(shape: ProjectShape, frozen: bool, error_kind: &str) {
    if UP_REPORTED.load(Ordering::Acquire) {
        return;
    }
    send(up_reports(
        up_properties(
            UpOutcome::Failed,
            shape,
            frozen,
            // Nothing was read, so the namespace is not known.
            None,
            Duration::ZERO,
            None,
            Some(error_kind),
        ),
        None,
    ))
    .await;
}

/// What a project declares, before anything is resolved or started.
///
/// Counted from the compose file itself, so it is available even when the
/// startup fails on the first container. Dependency depth is not here: a
/// package brings its own dependencies, so the shape of the file is not the
/// shape of the graph that runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ProjectShape {
    pub(crate) declared: usize,
    pub(crate) source_local: usize,
    pub(crate) source_package: usize,
    pub(crate) managed_engine: bool,
    pub(crate) has_lockfile: bool,
}

impl ProjectShape {
    pub(crate) fn of(file: &ComposeFile) -> Self {
        let source_package = file
            .containers
            .values()
            .filter(|container| matches!(container.worker, WorkerSource::Package { .. }))
            .count();
        Self {
            declared: file.containers.len(),
            source_local: file.containers.len() - source_package,
            source_package,
            managed_engine: file.engine.is_some(),
            has_lockfile: crate::lockfile::lock_path(&file.path).exists(),
        }
    }
}

/// The outcome of one startup, in the words the panel used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpOutcome {
    Ready,
    Failed,
    Cancelled,
}

impl UpOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Properties of one `compose_up_finished`.
pub(crate) fn up_properties(
    outcome: UpOutcome,
    shape: ProjectShape,
    frozen: bool,
    namespace: Option<&str>,
    elapsed: Duration,
    result: Option<&OpResult>,
    error_kind: Option<&str>,
) -> Value {
    let ready = result.map_or(0, |result| {
        result
            .containers
            .iter()
            .filter(|container| container.state == ChildStatus::Ready)
            .count()
    });
    json!({
        "outcome": outcome.as_str(),
        "duration_ms": elapsed.as_millis() as u64,
        "containers_declared": shape.declared,
        "containers_ready": ready,
        "containers_failed": result.map_or(0, |result| result.containers.len() - ready),
        "source_local": shape.source_local,
        "source_package": shape.source_package,
        "managed_engine": shape.managed_engine,
        "has_lockfile": shape.has_lockfile,
        "frozen": frozen,
        // The namespace this project serves, as the operator spelled it, or
        // absent when the file that would have named it was never read.
        "namespace": namespace,
        // The code of whichever error ended the startup. Absent on success,
        // and absent on a cancellation, which is the operator's decision
        // rather than a fault.
        "error_kind": error_kind,
    })
}

/// One event per container that did not come up, capped.
///
/// The name is left out on purpose: a container name is the operator's, and
/// the engine already reports the names of the workers that registered. What
/// is missing from a heartbeat is the ones that never got that far, so this
/// carries how far each one got and why it stopped.
pub(crate) fn failed_container_properties(result: &OpResult) -> Vec<Value> {
    result
        .containers
        .iter()
        .filter(|container| container.state != ChildStatus::Ready)
        .take(MAX_REPORTED_FAILURES)
        .enumerate()
        .map(|(index, container)| {
            json!({
                "index": index,
                "state": match container.state {
                    ChildStatus::Starting => "starting",
                    ChildStatus::Ready => "ready",
                    ChildStatus::Restarting => "restarting",
                    ChildStatus::Failed => "failed",
                    ChildStatus::Stopped => "stopped",
                },
                "error_kind": container.error.as_ref().map(|error| error.code.as_str()),
            })
        })
        .collect()
}

/// Properties of one `compose_op_finished`.
pub(crate) fn op_properties(
    op: &str,
    outcome: &str,
    elapsed: Duration,
    error_kind: Option<&str>,
) -> Value {
    json!({
        "op": op,
        "outcome": outcome,
        "duration_ms": elapsed.as_millis() as u64,
        "error_kind": error_kind,
    })
}

/// A reporter that records every batch, after holding each one for a moment.
///
/// One per process, because [`set_reporter`] keeps the first reporter. The
/// hold is what lets a test tell a spawned send from an awaited one: an
/// awaited send returns with its batch already recorded, a spawned send
/// returns before the hold is over.
#[cfg(test)]
pub(crate) mod recorder {
    use std::sync::{Arc, Mutex, OnceLock};

    use super::{Report, set_reporter};

    pub(crate) const HOLD: std::time::Duration = std::time::Duration::from_millis(300);

    #[derive(Default)]
    pub(crate) struct Recorder {
        batches: Mutex<Vec<Vec<Report>>>,
    }

    impl Recorder {
        /// Every recorded report whose properties satisfy `matches`.
        pub(crate) fn reports_where(
            &self,
            matches: impl Fn(&serde_json::Value) -> bool,
        ) -> Vec<Report> {
            self.batches
                .lock()
                .unwrap()
                .iter()
                .flatten()
                .filter(|(_, properties)| matches(properties))
                .cloned()
                .collect()
        }

        /// Polls for a matching report to arrive, for up to two seconds.
        pub(crate) async fn wait_for(
            &self,
            matches: impl Fn(&serde_json::Value) -> bool,
        ) -> Vec<Report> {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
            loop {
                let found = self.reports_where(&matches);
                if !found.is_empty() || tokio::time::Instant::now() > deadline {
                    return found;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
    }

    pub(crate) fn install() -> Arc<Recorder> {
        static RECORDER: OnceLock<Arc<Recorder>> = OnceLock::new();
        RECORDER
            .get_or_init(|| {
                let recorder = Arc::new(Recorder::default());
                let sink = Arc::clone(&recorder);
                set_reporter(Arc::new(move |reports| {
                    let sink = Arc::clone(&sink);
                    Box::pin(async move {
                        tokio::time::sleep(HOLD).await;
                        sink.batches.lock().unwrap().push(reports);
                    })
                }));
                recorder
            })
            .clone()
    }

    pub(crate) fn marker() -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::{ContainerResult, OpError, OpStatus};

    fn container(name: &str, state: ChildStatus, code: Option<&str>) -> ContainerResult {
        ContainerResult {
            container: name.to_string(),
            state,
            changed: true,
            error: code.map(|code| OpError {
                code: code.to_string(),
                message: "a message that must never be reported".to_string(),
            }),
        }
    }

    fn result(containers: Vec<ContainerResult>) -> OpResult {
        OpResult {
            operation_id: "compose:test".to_string(),
            status: OpStatus::Failed,
            changed: true,
            containers,
            primary_error: None,
        }
    }

    #[test]
    fn a_startup_counts_what_came_up_and_what_did_not() {
        let result = result(vec![
            container("api", ChildStatus::Ready, None),
            container("web", ChildStatus::Failed, Some("SPAWN_FAILED")),
        ]);
        let shape = ProjectShape {
            declared: 2,
            source_local: 1,
            source_package: 1,
            managed_engine: true,
            has_lockfile: false,
        };

        let props = up_properties(
            UpOutcome::Failed,
            shape,
            false,
            Some("default"),
            Duration::from_millis(1500),
            Some(&result),
            Some("PROJECT_DID_NOT_START"),
        );

        assert_eq!(props["outcome"], "failed");
        assert_eq!(props["duration_ms"], 1500);
        assert_eq!(props["containers_declared"], 2);
        assert_eq!(props["containers_ready"], 1);
        assert_eq!(props["containers_failed"], 1);
        assert_eq!(props["source_package"], 1);
        assert_eq!(props["error_kind"], "PROJECT_DID_NOT_START");
    }

    #[test]
    fn a_startup_that_never_reached_a_container_still_reports_the_declaration() {
        let props = up_properties(
            UpOutcome::Failed,
            ProjectShape {
                declared: 4,
                source_local: 4,
                ..ProjectShape::default()
            },
            true,
            Some("my-project"),
            Duration::ZERO,
            None,
            Some("COMPOSE_LOCK_REQUIRED"),
        );

        assert_eq!(props["containers_declared"], 4);
        assert_eq!(props["containers_ready"], 0);
        assert_eq!(props["containers_failed"], 0);
        assert_eq!(props["frozen"], true);
        assert_eq!(props["namespace"], "my-project");
    }

    #[test]
    fn a_startup_reports_its_own_event_before_each_failed_container() {
        let reports = up_reports(
            json!({}),
            Some(&result(vec![
                container("api", ChildStatus::Ready, None),
                container("web", ChildStatus::Failed, Some("SPAWN_FAILED")),
            ])),
        );

        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].0, UP_FINISHED);
        assert_eq!(reports[1].0, CONTAINER_FAILED);
    }

    #[test]
    fn only_the_first_failures_report_and_no_name_or_message_goes_with_them() {
        let result = result(vec![
            container("one", ChildStatus::Failed, Some("SPAWN_FAILED")),
            container("two", ChildStatus::Starting, None),
            container("three", ChildStatus::Failed, Some("READINESS_TIMEOUT")),
            container("four", ChildStatus::Failed, Some("SPAWN_FAILED")),
            container("ready", ChildStatus::Ready, None),
        ]);

        let failures = failed_container_properties(&result);

        assert_eq!(failures.len(), MAX_REPORTED_FAILURES);
        assert_eq!(failures[0]["state"], "failed");
        assert_eq!(failures[0]["error_kind"], "SPAWN_FAILED");
        assert_eq!(failures[1]["state"], "starting");
        assert!(failures[1]["error_kind"].is_null());
        assert_eq!(failures[2]["index"], 2);
        let text = serde_json::to_string(&failures).expect("the properties serialize");
        for secret in ["one", "two", "three", "must never be reported"] {
            assert!(!text.contains(secret), "{secret} reached the event");
        }
    }

    #[test]
    fn a_successful_startup_carries_no_error_kind() {
        let props = up_properties(
            UpOutcome::Ready,
            ProjectShape::default(),
            false,
            Some("default"),
            Duration::from_secs(2),
            Some(&result(vec![container("api", ChildStatus::Ready, None)])),
            None,
        );

        assert_eq!(props["outcome"], "ready");
        assert!(props["error_kind"].is_null());
        assert_eq!(props["containers_ready"], 1);
    }

    #[test]
    fn an_operation_reports_its_name_and_outcome() {
        let props = op_properties(
            "add",
            "failed",
            Duration::from_millis(250),
            Some("IO_ERROR"),
        );
        assert_eq!(props["op"], "add");
        assert_eq!(props["outcome"], "failed");
        assert_eq!(props["duration_ms"], 250);
        assert_eq!(props["error_kind"], "IO_ERROR");
    }

    #[tokio::test]
    async fn a_startup_the_daemon_reported_is_not_reported_again() {
        send(up_reports(json!({}), None)).await;
        // Silent: the daemon's own report already covered this startup.
        report_up_never_started(ProjectShape::default(), false, "INVALID_COMPOSE_FILE").await;
        assert!(UP_REPORTED.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn reporting_with_an_empty_batch_does_nothing() {
        send(Vec::new()).await;
        report(UP_FINISHED, json!({})).await;
    }

    #[tokio::test]
    async fn a_send_that_waits_returns_with_its_batch_delivered() {
        let recorder = recorder::install();
        let marker = recorder::marker();

        let began = std::time::Instant::now();
        send_waiting(
            vec![(UP_FINISHED.to_string(), json!({"marker": marker}))],
            true,
        )
        .await;

        assert!(began.elapsed() >= recorder::HOLD, "the send did not wait");
        assert_eq!(recorder.reports_where(|p| p["marker"] == marker).len(), 1);
    }

    #[tokio::test]
    async fn a_send_that_does_not_wait_returns_before_delivery_and_still_delivers() {
        let recorder = recorder::install();
        let marker = recorder::marker();

        let began = std::time::Instant::now();
        send_waiting(
            vec![(UP_FINISHED.to_string(), json!({"marker": marker}))],
            false,
        )
        .await;

        assert!(began.elapsed() < recorder::HOLD, "the send held its caller");
        assert!(recorder.reports_where(|p| p["marker"] == marker).is_empty());
        assert_eq!(recorder.wait_for(|p| p["marker"] == marker).await.len(), 1);
    }
}
