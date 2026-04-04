import { registerWorker, Logger, TriggerAction } from "iii-sdk";

const iii = registerWorker(
  process.env.III_ENGINE_URL || "ws://localhost:49134",
  {
    workerName: "cron-iii",
  },
);

iii.registerFunction("reports::generate", async () => {
  const logger = new Logger();
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

iii.registerTrigger({
  type: "cron",
  function_id: "reports::generate",
  config: { expression: "0 0 3 * * * *" },
});
