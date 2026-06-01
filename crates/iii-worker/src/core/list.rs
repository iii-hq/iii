// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! `worker::list` orchestrator.

use crate::core::error::WorkerOpError;
use crate::core::events::EventSink;
use crate::core::host::WorkerHostShim;
use crate::core::project::ProjectCtx;
use crate::core::types::{ListOptions, ListOutcome};

pub async fn run(
    opts: ListOptions,
    ctx: &ProjectCtx,
    events: &dyn EventSink,
    shim: &dyn WorkerHostShim,
) -> Result<ListOutcome, WorkerOpError> {
    shim.list(opts, ctx, events).await
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
            Ok(ListOutcome { workers: vec![] })
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
    async fn list_pass_through() {
        let dir = TempDir::new().unwrap();
        let ctx = ProjectCtx::open_unlocked(dir.path().to_path_buf());
        let sink = CapturingSink::new();
        let outcome = run(ListOptions::default(), &ctx, &sink, &StubShim)
            .await
            .unwrap();
        assert_eq!(outcome.workers.len(), 0);
    }
}
