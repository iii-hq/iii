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
    function_id: "etl::extract",
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

iii.registerFunction({ id: "etl::extract" }, async (data: any) => {
  const logger = new Logger();
  const extracted = [{ id: "1", value: 10 }];
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
    function_id: "etl::transform",
    payload: { runId: data.runId, extracted },
    action: TriggerAction.Enqueue({
      queue: "etl",
    }),
  });
  return { extractedCount: extracted.length };
});

iii.registerFunction({ id: "etl::transform" }, async (data: any) => {
  const logger = new Logger();
  const transformed = data.extracted.map((row: any) => ({ ...row, value: row.value * 2 }));
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
    function_id: "etl::load",
    payload: { runId: data.runId, transformed },
    action: TriggerAction.Enqueue({
      queue: "etl",
    }),
  });
  return {
    transformedCount: transformed.length,
  };
});

iii.registerFunction({ id: "etl::load" }, async (data: any) => {
  const logger = new Logger();
  // ...write transformed rows...
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "etl-warehouse",
      key: data.runId,
      value: {
        _key: data.runId,
        rows: data.transformed,
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
    count: data.transformed.length,
  });
  return {
    loadedCount: data.transformed.length,
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
