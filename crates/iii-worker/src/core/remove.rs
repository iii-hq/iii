// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! `worker::remove` orchestrator. Emits Started events for each name and
//! delegates to a `WorkerHostShim`.

use crate::core::error::WorkerOpError;
use crate::core::events::{EventSink, WorkerOpEvent};
use crate::core::host::WorkerHostShim;
use crate::core::project::ProjectCtx;
use crate::core::types::{RemoveOptions, RemoveOutcome};

pub async fn run(
    opts: RemoveOptions,
    ctx: &ProjectCtx,
    events: &dyn EventSink,
    shim: &dyn WorkerHostShim,
) -> Result<RemoveOutcome, WorkerOpError> {
    for name in &opts.names {
        events.emit(WorkerOpEvent::Started {
            op: "remove",
            worker: name.clone(),
        });
    }
    let outcome = shim.remove(opts, ctx, events).await?;
    for name in &outcome.removed {
        events.emit(WorkerOpEvent::Done {
            op: "remove",
            worker: name.clone(),
        });
    }
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
            opts: RemoveOptions,
            _c: &ProjectCtx,
            _e: &dyn EventSink,
        ) -> Result<RemoveOutcome, WorkerOpError> {
            Ok(RemoveOutcome {
                removed: opts.names,
            })
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
    async fn empty_names_passes_through() {
        let dir = TempDir::new().unwrap();
        let ctx = ProjectCtx::open_unlocked(dir.path().to_path_buf());
        let sink = CapturingSink::new();
        let outcome = run(
            RemoveOptions {
                names: vec![],
                all: false,
                yes: false,
            },
            &ctx,
            &sink,
            &StubShim,
        )
        .await
        .unwrap();
        assert_eq!(outcome.removed.len(), 0);
    }

    #[tokio::test]
    async fn names_pass_through_to_shim() {
        let dir = TempDir::new().unwrap();
        let ctx = ProjectCtx::open_unlocked(dir.path().to_path_buf());
        let sink = CapturingSink::new();
        let outcome = run(
            RemoveOptions {
                names: vec!["a".into(), "b".into()],
                all: false,
                yes: true,
            },
            &ctx,
            &sink,
            &StubShim,
        )
        .await
        .unwrap();
        assert_eq!(outcome.removed, vec!["a".to_string(), "b".to_string()]);
    }
}
