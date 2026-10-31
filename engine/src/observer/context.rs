// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::sync::{Arc, OnceLock};

use uuid::Uuid;

use crate::{
    protocol::{Activation, Caller},
    worker_connections::WorkerConnectionRegistry,
};

/// The worker name the engine uses as `caller.worker_name` on what it starts
/// itself (activations).
pub const ENGINE_CALLER_NAME: &str = "iii-engine";

tokio::task_local! {
    /// The invocation whose handler is running. Scoped around the handler call
    /// in `InvocationHandler::handle_invocation_inner`, so the code that builds
    /// the `InvokeFunction` frame for a worker-hosted function
    /// (`WorkerConnection::handle_function`) reads it synchronously, inside the
    /// same task.
    static CURRENT: Arc<InvocationCtx>;
}

/// Who started an invocation and where it sits in a chain of observers
/// (protocol.md §2.4): what the engine stamps on the `InvokeFunction` it
/// forwards to an `observe.v1` worker and what an observer's filters see as
/// `caller`.
pub struct InvocationCtx {
    origin: CallerOrigin,
    resolved: OnceLock<Option<Caller>>,
    /// Observe hops above the invocation: 0 for a call no observer caused.
    pub observe_depth: u32,
    /// Set on an observer activation.
    pub activation: Option<Activation>,
}

enum CallerOrigin {
    /// Engine orchestration (hooks, middleware, engine-hosted sources): no
    /// caller.
    None,
    /// A call from a worker connection. `worker_id` / `worker_name` are
    /// stamped from the connection and cannot be forged; the rest is what the
    /// calling SDK asserted. Resolved lazily: most invocations never need it.
    Connection {
        worker_id: Uuid,
        workers: Arc<WorkerConnectionRegistry>,
        asserted: Option<Box<Caller>>,
    },
    /// Built by the engine itself (activations), used verbatim.
    Engine(Caller),
}

impl InvocationCtx {
    /// Engine orchestration: no caller, depth 0.
    pub fn engine_internal() -> Self {
        Self::with_origin(CallerOrigin::None, 0, None)
    }

    /// Engine orchestration inside a chain of observers: no caller, the
    /// enclosing depth kept, so a chain that passes through an engine-hosted
    /// function or an engine-hosted source keeps counting toward the loop
    /// guard.
    pub fn engine_internal_at(observe_depth: u32) -> Self {
        Self::with_origin(CallerOrigin::None, observe_depth, None)
    }

    /// A call of an engine-hosted source: the engine is the caller
    /// (`{ worker_name: "iii-engine" }`), at the depth of what caused it.
    pub fn engine_source(observe_depth: u32) -> Self {
        Self::with_origin(
            CallerOrigin::Engine(Caller {
                worker_name: Some(ENGINE_CALLER_NAME.to_string()),
                ..Caller::default()
            }),
            observe_depth,
            None,
        )
    }

    /// The depth of the invocation whose handler is running; 0 outside one.
    pub fn current_depth() -> u32 {
        CURRENT
            .try_with(|ctx| ctx.observe_depth)
            .unwrap_or_default()
    }

    /// A call routed from a worker connection.
    pub fn from_connection(
        worker_id: Uuid,
        workers: Arc<WorkerConnectionRegistry>,
        asserted: Option<Box<Caller>>,
        observe_depth: Option<u32>,
    ) -> Self {
        Self::with_origin(
            CallerOrigin::Connection {
                worker_id,
                workers,
                asserted,
            },
            observe_depth.unwrap_or(0),
            None,
        )
    }

    /// A call through an observer (`iii.call({ observer, payload })`): the
    /// host's connection is the caller, as for any call it makes, and the
    /// call carries the activation.
    pub fn observer_call(
        worker_id: Uuid,
        workers: Arc<WorkerConnectionRegistry>,
        asserted: Option<Box<Caller>>,
        observe_depth: u32,
        activation: Activation,
    ) -> Self {
        Self::with_origin(
            CallerOrigin::Connection {
                worker_id,
                workers,
                asserted,
            },
            observe_depth,
            Some(activation),
        )
    }

    /// An observer activation: the engine is the caller.
    pub fn activation(caller: Caller, observe_depth: u32, activation: Activation) -> Self {
        Self::with_origin(
            CallerOrigin::Engine(caller),
            observe_depth,
            Some(activation),
        )
    }

    fn with_origin(
        origin: CallerOrigin,
        observe_depth: u32,
        activation: Option<Activation>,
    ) -> Self {
        Self {
            origin,
            resolved: OnceLock::new(),
            observe_depth,
            activation,
        }
    }

    /// Engine orchestration at depth 0, outside any activation: nothing to
    /// carry.
    pub fn is_blank(&self) -> bool {
        matches!(self.origin, CallerOrigin::None)
            && self.observe_depth == 0
            && self.activation.is_none()
    }

    /// The caller as the engine reports it: the connection's `worker_id` and
    /// `worker_name` over whatever the SDK asserted for them.
    pub fn caller(&self) -> Option<&Caller> {
        self.resolved
            .get_or_init(|| match &self.origin {
                CallerOrigin::None => None,
                CallerOrigin::Engine(caller) => Some(caller.clone()),
                CallerOrigin::Connection {
                    worker_id,
                    workers,
                    asserted,
                } => {
                    let mut caller = asserted.as_deref().cloned().unwrap_or_default();
                    caller.worker_id = Some(worker_id.to_string());
                    caller.worker_name = workers.get_worker_name(worker_id);
                    Some(caller)
                }
            })
            .as_ref()
    }

    /// The context of the invocation whose handler is running, if any.
    pub fn current() -> Option<Arc<InvocationCtx>> {
        CURRENT.try_with(Arc::clone).ok()
    }

    /// Runs `fut` (an invocation's handler) with `ctx` as the current context.
    pub async fn scope<F: std::future::Future>(ctx: Arc<InvocationCtx>, fut: F) -> F::Output {
        CURRENT.scope(ctx, fut).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ParentCaller;

    #[test]
    fn a_connection_caller_is_stamped_over_what_the_sdk_asserted() {
        crate::workers::observability::metrics::ensure_default_meter();
        let workers = Arc::new(WorkerConnectionRegistry::new());
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let mut worker = crate::worker_connections::WorkerConnection::new(tx);
        worker.name = Some("billing".to_string());
        let worker_id = worker.id;
        workers.register_worker(worker);

        let asserted = Caller {
            worker_id: Some("forged".into()),
            worker_name: Some("forged".into()),
            function_id: Some("billing::close-invoice".into()),
            invocation_id: Some("i-1".into()),
            parent: Some(ParentCaller {
                worker_name: Some("checkout".into()),
                ..ParentCaller::default()
            }),
        };
        let ctx =
            InvocationCtx::from_connection(worker_id, workers, Some(Box::new(asserted)), Some(2));
        let caller = ctx.caller().expect("a caller");
        assert_eq!(caller.worker_id, Some(worker_id.to_string()));
        assert_eq!(caller.worker_name.as_deref(), Some("billing"));
        assert_eq!(
            caller.function_id.as_deref(),
            Some("billing::close-invoice")
        );
        assert_eq!(
            caller.parent.as_ref().unwrap().worker_name.as_deref(),
            Some("checkout")
        );
        assert_eq!(ctx.observe_depth, 2);
    }

    #[test]
    fn engine_orchestration_has_no_caller() {
        assert!(InvocationCtx::engine_internal().caller().is_none());
    }

    #[tokio::test]
    async fn the_current_context_is_visible_inside_the_scope_only() {
        assert!(InvocationCtx::current().is_none());
        let ctx = Arc::new(InvocationCtx::engine_internal());
        let depth = InvocationCtx::scope(ctx, async {
            InvocationCtx::current().map(|ctx| ctx.observe_depth)
        })
        .await;
        assert_eq!(depth, Some(0));
        assert!(InvocationCtx::current().is_none());
    }
}
