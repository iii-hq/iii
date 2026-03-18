import { registerWorker, Logger, TriggerAction } from "iii-sdk";

const iii = registerWorker(
  process.env.III_ENGINE_URL || "ws://localhost:49134",
  {
    workerName: "cron-iii",
  },
);

iii.registerFunction({ id: "reports::generate" }, async () => {
  const logger = new Logger();
  const settings = await iii.trigger({
    function_id: "state::get",
    payload: {
      scope: "cron-settings",
      key: "reports",
    },
  });
  const enabled = settings?.enabled ?? true;
  if (!enabled) {
    logger.info("cron.reports_generate.skipped", {});
    return { skipped: true };
  }
  const enqueueReceipt = await iii.trigger({
    function_id: "reporting-service::generate-daily-report",
    payload: {
      reportId: "reports",
      requestedAt: new Date().toISOString(),
    },
    action: TriggerAction.Enqueue({
      queue: "reports",
    }),
  });
  logger.info("cron.reports_generate.run", {
    task: "reports::generate",
    receipt: enqueueReceipt?.messageReceiptId,
  });
  return { queued: true };
});

iii.registerFunction({ id: "cron::reports-start" }, async () => {
  const logger = new Logger();
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "cron-settings",
      key: "reports",
      value: {
        _key: "reports",
        enabled: true,
        expression: "0 0 3 * * * *",
      },
    },
  });
  logger.info("cron.reports.start", {});
  return { task: "reports", enabled: true };
});

iii.registerFunction({ id: "cron::reports-stop" }, async () => {
  const logger = new Logger();
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "cron-settings",
      key: "reports",
      value: {
        _key: "reports",
        enabled: false,
        expression: "0 0 3 * * * *",
      },
    },
  });
  logger.info("cron.reports.stop", {});
  return { task: "reports", enabled: false };
});

iii.registerFunction({ id: "cron::tasks-list" }, async () => {
  const logger = new Logger();
  const settings = await iii.trigger({
    function_id: "state::get",
    payload: {
      scope: "cron-settings",
      key: "reports",
    },
  });
  const task = settings ?? {
    _key: "reports",
    enabled: true,
    expression: "0 0 3 * * * *",
  };
  const metadata = await iii.trigger({
    function_id: "reporting-service::describe-report",
    payload: {
      reportId: "reports",
    },
  });
  logger.info("cron.tasks.list", { count: 1 });
  return {
    tasks: [
      {
        id: "reports",
        expression: task.expression,
        enabled: task.enabled,
        owner: metadata.owner ?? "reporting-service",
      },
    ],
  };
});

iii.registerTrigger({
  type: "cron",
  function_id: "reports::generate",
  config: { expression: "0 0 3 * * * *" },
});

iii.registerTrigger({
  type: "http",
  function_id: "cron::reports-start",
  config: {
    api_path: "/cron/reports/start",
    http_method: "POST",
  },
});

iii.registerTrigger({
  type: "http",
  function_id: "cron::reports-stop",
  config: {
    api_path: "/cron/reports/stop",
    http_method: "POST",
  },
});

iii.registerTrigger({
  type: "http",
  function_id: "cron::tasks-list",
  config: {
    api_path: "/cron/tasks",
    http_method: "GET",
  },
});
