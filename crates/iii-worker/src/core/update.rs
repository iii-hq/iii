// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! `worker::update` orchestrator.

use crate::core::error::WorkerOpError;
use crate::core::events::{EventSink, WorkerOpEvent};
use crate::core::host::WorkerHostShim;
use crate::core::project::ProjectCtx;
use crate::core::types::{UpdateOptions, UpdateOutcome};

pub async fn run(
    opts: UpdateOptions,
    ctx: &ProjectCtx,
    events: &dyn EventSink,
    shim: &dyn WorkerHostShim,
) -> Result<UpdateOutcome, WorkerOpError> {
    let label = if opts.names.is_empty() {
        "<all>".to_string()
    } else {
        opts.names.join(",")
    };
    events.emit(WorkerOpEvent::Started {
        op: "update",
        worker: label.clone(),
    });
    let outcome = shim.update(opts, ctx, events).await?;
    events.emit(WorkerOpEvent::Done {
        op: "update",
        worker: label,
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
            Ok(UpdateOutcome { updated: vec![] })
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
            _o: StopOptions,
            _c: &ProjectCtx,
            _e: &dyn EventSink,
        ) -> Result<StopOutcome, WorkerOpError> {
            unimplemented!()
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
    async fn update_pass_through() {
        let dir = TempDir::new().unwrap();
        let ctx = ProjectCtx::open_unlocked(dir.path().to_path_buf());
        let sink = CapturingSink::new();
        let outcome = run(
            UpdateOptions {
                names: vec!["pdfkit".into()],
            },
            &ctx,
            &sink,
            &StubShim,
        )
        .await
        .unwrap();
        assert_eq!(outcome.updated.len(), 0);
    }

    #[tokio::test]
    async fn update_all_when_names_empty() {
        let dir = TempDir::new().unwrap();
        let ctx = ProjectCtx::open_unlocked(dir.path().to_path_buf());
        let sink = CapturingSink::new();
        let outcome = run(UpdateOptions { names: vec![] }, &ctx, &sink, &StubShim)
            .await
            .unwrap();
        assert_eq!(outcome.updated.len(), 0);
    }
}
