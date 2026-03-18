import { registerWorker, Logger, TriggerAction } from "iii-sdk";

const iii = registerWorker(
  process.env.III_ENGINE_URL || "ws://localhost:49134",
  {
    workerName: "etl-iii",
  },
);

iii.registerFunction({ id: "etl::start" }, async () => {
  const logger = new Logger();
  const runId = `run-${Date.now()}`;
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "etl-runs",
      key: runId,
      value: {
        _key: runId,
        runId,
        step: "extract",
        status: "queued",
      },
    },
  });
  iii.trigger({
    function_id: "etl::extract-step",
    payload: { runId },
    action: TriggerAction.Enqueue({
      queue: "etl",
    }),
  });
  logger.info("etl.start_run.queued", {
    runId,
  });
  return { runId, status: "queued" };
});

iii.registerFunction({ id: "etl::extract-step" }, async (data: any) => {
  const logger = new Logger();
  const extracted = await iii.trigger({
    function_id: "extract-service::extract-batch",
    payload: { runId: data.runId },
  });
  await iii.trigger({
    function_id: "state::update",
    payload: {
      scope: "etl-runs",
      key: data.runId,
      ops: [
        {
          type: "set",
          path: "step",
          value: "extract",
        },
        {
          type: "set",
          path: "status",
          value: "running",
        },
        {
          type: "set",
          path: "extractedCount",
          value: extracted.length,
        },
      ],
    },
  });
  // ...pull source data...
  logger.info("etl.extract.completed", {
    runId: data.runId,
    count: extracted.length,
  });
  iii.trigger({
    function_id: "etl::transform-step",
    payload: { runId: data.runId, extracted },
    action: TriggerAction.Enqueue({
      queue: "etl",
    }),
  });
  return { extractedCount: extracted.length };
});

iii.registerFunction({ id: "etl::transform-step" }, async (data: any) => {
  const logger = new Logger();
  const transformed = await iii.trigger({
    function_id: "transform-service::normalize-batch",
    payload: {
      runId: data.runId,
      rows: data.extracted,
    },
  });
  await iii.trigger({
    function_id: "state::update",
    payload: {
      scope: "etl-runs",
      key: data.runId,
      ops: [
        {
          type: "set",
          path: "step",
          value: "transform",
        },
        {
          type: "set",
          path: "status",
          value: "running",
        },
        {
          type: "set",
          path: "transformedCount",
          value: transformed.length,
        },
      ],
    },
  });
  logger.info("etl.transform.completed", {
    runId: data.runId,
    count: transformed.length,
  });
  iii.trigger({
    function_id: "etl::load-step",
    payload: { runId: data.runId, transformed },
    action: TriggerAction.Enqueue({
      queue: "etl",
    }),
  });
  return {
    transformedCount: transformed.length,
  };
});

iii.registerFunction({ id: "etl::load-step" }, async (data: any) => {
  const logger = new Logger();
  const loaded = await iii.trigger({
    function_id: "warehouse-service::load-batch",
    payload: {
      runId: data.runId,
      rows: data.transformed,
    },
  });
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "etl-warehouse",
      key: data.runId,
      value: {
        _key: data.runId,
        destination: loaded.destination,
        loadedCount: loaded.loadedCount,
      },
    },
  });
  await iii.trigger({
    function_id: "state::update",
    payload: {
      scope: "etl-runs",
      key: data.runId,
      ops: [
        {
          type: "set",
          path: "step",
          value: "load",
        },
        {
          type: "set",
          path: "status",
          value: "completed",
        },
        {
          type: "set",
          path: "completedAt",
          value: new Date().toISOString(),
        },
      ],
    },
  });
  logger.info("etl.load.completed", {
    runId: data.runId,
    count: loaded.loadedCount,
  });
  return {
    loadedCount: loaded.loadedCount,
  };
});

iii.registerFunction({ id: "etl::status" }, async (request: any) => {
  const snapshot = await iii.trigger({
    function_id: "state::get",
    payload: {
      scope: "etl-runs",
      key: request.params.runId,
    },
  });
  if (!snapshot) {
    const error = new Error("Run not found") as Error & {
      status: number;
    };
    error.status = 404;
    throw error;
  }
  return snapshot;
});

iii.registerFunction({ id: "etl::start-scheduled" }, async () => {
  // ...cron entrypoint...
  return iii.trigger({
    function_id: "etl::start",
    payload: {},
  });
});

iii.registerTrigger({
  type: "http",
  function_id: "etl::start",
  config: {
    api_path: "/etl/run",
    http_method: "POST",
  },
});

iii.registerTrigger({
  type: "http",
  function_id: "etl::status",
  config: {
    api_path: "/etl/:runId",
    http_method: "GET",
  },
});

iii.registerTrigger({
  type: "cron",
  function_id: "etl::start-scheduled",
  config: { expression: "0 0 2 * * * *" },
});
