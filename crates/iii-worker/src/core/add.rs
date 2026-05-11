// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! `worker::add` orchestrator. Validates inputs, applies trigger-mode
//! policies (e.g. local-path rejection), and delegates to a `WorkerHostShim`.
//! Lock acquisition is the caller's responsibility.

use crate::core::error::WorkerOpError;
use crate::core::events::{EventSink, WorkerOpEvent};
use crate::core::host::WorkerHostShim;
use crate::core::project::ProjectCtx;
use crate::core::types::{AddOptions, AddOutcome, WorkerSource};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallerMode {
    Cli,
    Trigger,
}

pub async fn run(
    opts: AddOptions,
    ctx: &ProjectCtx,
    events: &dyn EventSink,
    shim: &dyn WorkerHostShim,
    mode: CallerMode,
) -> Result<AddOutcome, WorkerOpError> {
    if mode == CallerMode::Trigger {
        if let WorkerSource::Local { path } = &opts.source {
            return Err(WorkerOpError::local_path_not_allowed_via_trigger(
                path.display().to_string(),
            ));
        }
    }
    let label = source_label(&opts.source);
    events.emit(WorkerOpEvent::Started {
        op: "add",
        worker: label,
    });
    let outcome = shim.add(opts, ctx, events).await?;
    events.emit(WorkerOpEvent::Done {
        op: "add",
        worker: outcome.name.clone(),
    });
    Ok(outcome)
}

fn source_label(s: &WorkerSource) -> String {
    match s {
        WorkerSource::Registry { name, .. } => name.clone(),
        WorkerSource::Oci { reference } => reference.clone(),
        WorkerSource::Local { path } => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::events::CapturingSink;
    use crate::core::types::{AddStatus, WorkerSource};
    use async_trait::async_trait;
    use tempfile::TempDir;

    struct StubShim;

    #[async_trait]
    impl WorkerHostShim for StubShim {
        async fn add(
            &self,
            opts: AddOptions,
            ctx: &ProjectCtx,
            _e: &dyn EventSink,
        ) -> Result<AddOutcome, WorkerOpError> {
            let name = match opts.source {
                WorkerSource::Registry { name, .. } => name,
                WorkerSource::Oci { reference } => reference,
                WorkerSource::Local { path } => path.display().to_string(),
            };
            Ok(AddOutcome {
                name,
                version: Some("1.0.0".into()),
                status: AddStatus::Installed,
                awaited_ready: false,
                config_path: ctx.config_path(),
            })
        }
        async fn remove(
            &self,
            _o: crate::core::RemoveOptions,
            _c: &ProjectCtx,
            _e: &dyn EventSink,
        ) -> Result<crate::core::RemoveOutcome, WorkerOpError> {
            unimplemented!()
        }
        async fn update(
            &self,
            _o: crate::core::UpdateOptions,
            _c: &ProjectCtx,
            _e: &dyn EventSink,
        ) -> Result<crate::core::UpdateOutcome, WorkerOpError> {
            unimplemented!()
        }
        async fn start(
            &self,
            _o: crate::core::StartOptions,
            _c: &ProjectCtx,
            _e: &dyn EventSink,
        ) -> Result<crate::core::StartOutcome, WorkerOpError> {
            unimplemented!()
        }
        async fn stop(
            &self,
            _o: crate::core::StopOptions,
            _c: &ProjectCtx,
            _e: &dyn EventSink,
        ) -> Result<crate::core::StopOutcome, WorkerOpError> {
            unimplemented!()
        }
        async fn list(
            &self,
            _o: crate::core::ListOptions,
            _c: &ProjectCtx,
            _e: &dyn EventSink,
        ) -> Result<crate::core::ListOutcome, WorkerOpError> {
            unimplemented!()
        }
        async fn clear(
            &self,
            _o: crate::core::ClearOptions,
            _c: &ProjectCtx,
            _e: &dyn EventSink,
        ) -> Result<crate::core::ClearOutcome, WorkerOpError> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn trigger_mode_rejects_local_path() {
        let dir = TempDir::new().unwrap();
        let ctx = ProjectCtx::open_unlocked(dir.path().to_path_buf());
        let sink = CapturingSink::new();
        let opts = AddOptions {
            source: WorkerSource::Local {
                path: "./my-worker".into(),
            },
            force: false,
            reset_config: false,
            wait: true,
        };
        let res = run(opts, &ctx, &sink, &StubShim, CallerMode::Trigger).await;
        assert!(matches!(
            res,
            Err(WorkerOpError::LocalPathNotAllowedViaTrigger { .. })
        ));
    }

    #[tokio::test]
    async fn cli_mode_allows_local_path() {
        let dir = TempDir::new().unwrap();
        let ctx = ProjectCtx::open_unlocked(dir.path().to_path_buf());
        let sink = CapturingSink::new();
        let opts = AddOptions {
            source: WorkerSource::Local {
                path: "./my-worker".into(),
            },
            force: false,
            reset_config: false,
            wait: true,
        };
        let outcome = run(opts, &ctx, &sink, &StubShim, CallerMode::Cli)
            .await
            .unwrap();
        assert_eq!(outcome.status, AddStatus::Installed);
    }

    #[tokio::test]
    async fn registry_source_passes_through_to_shim() {
        let dir = TempDir::new().unwrap();
        let ctx = ProjectCtx::open_unlocked(dir.path().to_path_buf());
        let sink = CapturingSink::new();
        let opts = AddOptions {
            source: WorkerSource::Registry {
                name: "pdfkit".into(),
                version: None,
            },
            force: false,
            reset_config: false,
            wait: true,
        };
        let outcome = run(opts, &ctx, &sink, &StubShim, CallerMode::Trigger)
            .await
            .unwrap();
        assert_eq!(outcome.name, "pdfkit");
    }

    #[tokio::test]
    async fn emits_started_and_done_events() {
        let dir = TempDir::new().unwrap();
        let ctx = ProjectCtx::open_unlocked(dir.path().to_path_buf());
        let sink = CapturingSink::new();
        let opts = AddOptions {
            source: WorkerSource::Registry {
                name: "pdfkit".into(),
                version: None,
            },
            force: false,
            reset_config: false,
            wait: true,
        };
        let _ = run(opts, &ctx, &sink, &StubShim, CallerMode::Cli)
            .await
            .unwrap();
        let events = sink.events.lock().unwrap();
        assert!(matches!(
            &events[0],
            WorkerOpEvent::Started { op: "add", .. }
        ));
        assert!(matches!(
            &events.last().unwrap(),
            WorkerOpEvent::Done { op: "add", .. }
        ));
    }
}
