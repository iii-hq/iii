// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! `worker::stop` orchestrator.

use crate::core::error::WorkerOpError;
use crate::core::events::{EventSink, WorkerOpEvent};
use crate::core::host::WorkerHostShim;
use crate::core::project::ProjectCtx;
use crate::core::types::{StopOptions, StopOutcome};

pub async fn run(
    opts: StopOptions,
    ctx: &ProjectCtx,
    events: &dyn EventSink,
    shim: &dyn WorkerHostShim,
) -> Result<StopOutcome, WorkerOpError> {
    if opts.name.trim().is_empty() {
        return Err(WorkerOpError::invalid_name(
            opts.name.as_str(),
            "name is required",
        ));
    }
    events.emit(WorkerOpEvent::Started {
        op: "stop",
        worker: opts.name.clone(),
    });
    let outcome = shim.stop(opts, ctx, events).await?;
    events.emit(WorkerOpEvent::Done {
        op: "stop",
        worker: outcome.name.clone(),
    });
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::events::CapturingSink;
    use crate::core::types::*;
    use async_trait::async_trait;
    use tempfile::TempDir;

    struct StubShim;

    #[async_trait]
    impl WorkerHostShim for StubShim {
        async fn add(
            &self,
            _o: AddOptions,
            _c: &ProjectCtx,
            _e: &dyn EventSink,
        ) -> Result<AddOutcome, WorkerOpError> {
            unimplemented!()
        }
        async fn remove(
            &self,
            _o: RemoveOptions,
            _c: &ProjectCtx,
            _e: &dyn EventSink,
        ) -> Result<RemoveOutcome, WorkerOpError> {
            unimplemented!()
        }
        async fn update(
            &self,
            _o: UpdateOptions,
            _c: &ProjectCtx,
            _e: &dyn EventSink,
        ) -> Result<UpdateOutcome, WorkerOpError> {
            unimplemented!()
        }
        async fn start(
            &self,
            _o: StartOptions,
            _c: &ProjectCtx,
            _e: &dyn EventSink,
        ) -> Result<StartOutcome, WorkerOpError> {
            unimplemented!()
        }
        async fn stop(
            &self,
            opts: StopOptions,
            _c: &ProjectCtx,
            _e: &dyn EventSink,
        ) -> Result<StopOutcome, WorkerOpError> {
            Ok(StopOutcome {
                name: opts.name,
                stopped: true,
            })
        }
        async fn list(
            &self,
            _o: ListOptions,
            _c: &ProjectCtx,
            _e: &dyn EventSink,
        ) -> Result<ListOutcome, WorkerOpError> {
            unimplemented!()
        }
        async fn clear(
            &self,
            _o: ClearOptions,
            _c: &ProjectCtx,
            _e: &dyn EventSink,
        ) -> Result<ClearOutcome, WorkerOpError> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn empty_name_returns_invalid_name() {
        let dir = TempDir::new().unwrap();
        let ctx = ProjectCtx::open_unlocked(dir.path().to_path_buf());
        let sink = CapturingSink::new();
        let res = run(
            StopOptions {
                name: "".into(),
                yes: true,
            },
            &ctx,
            &sink,
            &StubShim,
        )
        .await;
        assert!(matches!(res, Err(WorkerOpError::InvalidName { .. })));
    }

    #[tokio::test]
    async fn valid_name_passes_through() {
        let dir = TempDir::new().unwrap();
        let ctx = ProjectCtx::open_unlocked(dir.path().to_path_buf());
        let sink = CapturingSink::new();
        let outcome = run(
            StopOptions {
                name: "pdfkit".into(),
                yes: true,
            },
            &ctx,
            &sink,
            &StubShim,
        )
        .await
        .unwrap();
        assert_eq!(outcome.name, "pdfkit");
        assert!(outcome.stopped);
    }
}
