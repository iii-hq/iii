// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

use async_trait::async_trait;

use crate::core::error::WorkerOpError;
use crate::core::events::EventSink;
use crate::core::project::ProjectCtx;
use crate::core::types::{
    AddOptions, AddOutcome, ClearOptions, ClearOutcome, ListOptions, ListOutcome, RemoveOptions,
    RemoveOutcome, StartOptions, StartOutcome, StopOptions, StopOutcome, UpdateOptions,
    UpdateOutcome,
};

/// Host-side implementation of the heavy operations. `CliHostShim` is the
/// production impl wired to `cli::managed::*` / `cli::registry` / etc.;
/// tests provide stubs. The orchestrators in `core::*` validate, acquire
/// locks, emit events, and delegate every fs/network/process effect here.
#[async_trait]
pub trait WorkerHostShim: Send + Sync {
    async fn add(
        &self,
        opts: AddOptions,
        ctx: &ProjectCtx,
        events: &dyn EventSink,
    ) -> Result<AddOutcome, WorkerOpError>;
    async fn remove(
        &self,
        opts: RemoveOptions,
        ctx: &ProjectCtx,
        events: &dyn EventSink,
    ) -> Result<RemoveOutcome, WorkerOpError>;
    async fn update(
        &self,
        opts: UpdateOptions,
        ctx: &ProjectCtx,
        events: &dyn EventSink,
    ) -> Result<UpdateOutcome, WorkerOpError>;
    async fn start(
        &self,
        opts: StartOptions,
        ctx: &ProjectCtx,
        events: &dyn EventSink,
    ) -> Result<StartOutcome, WorkerOpError>;
    async fn stop(
        &self,
        opts: StopOptions,
        ctx: &ProjectCtx,
        events: &dyn EventSink,
    ) -> Result<StopOutcome, WorkerOpError>;
    async fn list(
        &self,
        opts: ListOptions,
        ctx: &ProjectCtx,
        events: &dyn EventSink,
    ) -> Result<ListOutcome, WorkerOpError>;
    async fn clear(
        &self,
        opts: ClearOptions,
        ctx: &ProjectCtx,
        events: &dyn EventSink,
    ) -> Result<ClearOutcome, WorkerOpError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::events::NullSink;
    use crate::core::types::{AddStatus, WorkerSource};
    use tempfile::TempDir;

    struct StubShim;

    #[async_trait]
    impl WorkerHostShim for StubShim {
        async fn add(
            &self,
            _o: AddOptions,
            ctx: &ProjectCtx,
            _e: &dyn EventSink,
        ) -> Result<AddOutcome, WorkerOpError> {
            Ok(AddOutcome {
                name: "stub".into(),
                version: Some("0.0.0".into()),
                status: AddStatus::Installed,
                awaited_ready: false,
                config_path: ctx.config_path(),
            })
        }
        async fn remove(
            &self,
            _o: RemoveOptions,
            _c: &ProjectCtx,
            _e: &dyn EventSink,
        ) -> Result<RemoveOutcome, WorkerOpError> {
            Ok(RemoveOutcome { removed: vec![] })
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
            Ok(StartOutcome {
                name: "x".into(),
                pid: Some(1),
                port: None,
            })
        }
        async fn stop(
            &self,
            _o: StopOptions,
            _c: &ProjectCtx,
            _e: &dyn EventSink,
        ) -> Result<StopOutcome, WorkerOpError> {
            Ok(StopOutcome {
                name: "x".into(),
                stopped: true,
            })
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
            Ok(ClearOutcome { cleared_bytes: 0 })
        }
    }

    #[tokio::test]
    async fn dyn_workerhostshim_is_callable_through_a_stub() {
        let dir = TempDir::new().unwrap();
        let ctx = ProjectCtx::open_unlocked(dir.path().to_path_buf());
        let shim: &dyn WorkerHostShim = &StubShim;
        let opts = AddOptions {
            source: WorkerSource::Registry {
                name: "x".into(),
                version: None,
            },
            force: false,
            reset_config: false,
            wait: false,
        };
        let outcome = shim.add(opts, &ctx, &NullSink).await.unwrap();
        assert_eq!(outcome.name, "stub");
        assert_eq!(outcome.status, AddStatus::Installed);
    }
}
