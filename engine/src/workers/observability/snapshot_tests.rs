// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use super::*;
use crate::{
    engine::{EngineTrait, Handler, RegisterFunctionRequest},
    invocation::InvocationHandler,
    observer::InvocationCtx,
};

// Exercise the same service macro, result type and list implementation with an
// isolated accumulator. No global reset or production injection API is needed.
#[derive(Clone)]
struct MetricsService {
    worker: ObservabilityWorker,
    acc: Arc<metrics::MetricsAccumulator>,
}

#[service(name = "snapshot-test")]
impl MetricsService {
    #[function(
        id = "engine::metrics::list",
        description = "Isolated metrics snapshot regression"
    )]
    async fn list_metrics(
        &self,
        input: MetricsListInput,
    ) -> FunctionResult<MetricsListResult, ErrorBody> {
        self.worker
            .list_metrics_with_accumulator(input, &self.acc)
            .await
    }
}

#[tokio::test]
async fn serialized_selfmetrics_excludes_current_and_includes_previous_call() {
    metrics::ensure_default_meter();
    let engine = Arc::new(Engine::new());
    let (shutdown_tx, _) = tokio::sync::watch::channel(false);
    let worker = ObservabilityWorker {
        _config: config::ObservabilityWorkerConfig::default(),
        engine: engine.clone(),
        shutdown_tx: Arc::new(shutdown_tx),
        worker_shutdown_rx: Arc::new(std::sync::Mutex::new(None)),
        logs_retention_stop: Arc::new(std::sync::Mutex::new(None)),
        logs_exporter_stop: Arc::new(std::sync::Mutex::new(None)),
        logs_subscriber_stop: Arc::new(std::sync::Mutex::new(None)),
        apply_lock: Arc::new(tokio::sync::Mutex::new(())),
    };
    let acc = Arc::new(metrics::MetricsAccumulator::default());
    MetricsService {
        worker,
        acc: acc.clone(),
    }
    .register_functions(engine.clone());
    let dispatcher = InvocationHandler::with_metrics_for_test(acc.clone());
    let handler = engine
        .functions
        .get_handler("default", "engine::metrics::list")
        .unwrap();
    let mut returned = Vec::new();
    for previous in 0..2_u64 {
        let result = dispatcher
            .handle_invocation_with_handler(
                Some(uuid::Uuid::new_v4()),
                None,
                "engine::metrics::list".into(),
                "default",
                serde_json::json!({}),
                handler.clone(),
                None,
                None,
                None,
                None,
                InvocationCtx::engine_internal(),
            )
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let invocations = &result["engine_metrics"]["invocations"];
        assert_eq!(invocations["total"], previous);
        assert_eq!(invocations["success"], previous);
        assert_eq!(invocations["error"], 0);
        assert_eq!(invocations["deferred"], 0);
        assert_eq!(
            invocations.as_object().unwrap().len(),
            5,
            "JSON shape is unchanged"
        );
        if previous == 0 {
            assert!(invocations["by_function"].as_object().unwrap().is_empty());
        } else {
            assert_eq!(
                invocations["by_function"],
                serde_json::json!({"engine::metrics::list": 1})
            );
        }
        assert_eq!(
            acc.get_function_count("engine::metrics::list"),
            Some(previous + 1)
        );
        returned.push(result);
    }
    assert_eq!(returned[0]["engine_metrics"]["invocations"]["total"], 0);
    assert!(
        returned[0]["engine_metrics"]["invocations"]["by_function"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    assert!(acc.first_user_success_fn.get().is_none());
}
