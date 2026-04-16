// This is the client - it makes calls to data-worker and compute-worker.
// It can be thought of as an orchestrator, but iii does not require one.

import { registerWorker, Logger } from "iii-sdk";
const iii = registerWorker(process.env.III_URL ?? "ws://localhost:49134");
const logger = new Logger();

// All workers behave as a single application — scoped state set
// in one worker is retrievable from any other.
const WORKER_VERSION = 1;
await iii.trigger({
  function_id: "state::set",
  payload: { scope: "shared", key: "WORKER_VERSION", data: WORKER_VERSION },
});

// registerFunction declares functionality to the engine.
// Once registered, any connected process can trigger this function.
const health = iii.registerFunction("client::health", async () => {
  logger.info("Health check OK");
  return { status: 200, body: { healthy: true, timestamp: Date.now() } };
});

// registerTrigger independently creates callables — HTTP endpoints, cron jobs, etc.
iii.registerTrigger({
  type: "http",
  function_id: health.id,
  config: { api_path: "/health", http_method: "GET" },
});

iii.registerTrigger({
  type: "cron",
  function_id: health.id,
  config: { expression: "*/30 * * * * * *" },
});

// Trigger functions in other workers across languages.
const orchestrate = iii.registerFunction(
  "client::orchestrate",
  async (payload) => {
    logger.info("Handling request", { payload: JSON.stringify(payload) });

    const results: { client: string; errors: any[]; [key: string]: unknown } = {
      client: "ok",
      errors: [],
    };

    const body = payload.body ?? payload;
    const data = body.data ?? body;

    const dataRequest = iii.trigger({
      function_id: "data-worker::transform",
      payload: { data },
    });
    const computeRequest = iii.trigger({
      function_id: "compute-worker::compute",
      payload: { n: body.n },
    });

    const [dataResult, computeResult] = await Promise.allSettled([
      dataRequest,
      computeRequest,
    ]);

    if (dataResult.status === "fulfilled") {
      results.dataWorker = dataResult.value;
    } else {
      logger.error("data-worker error", dataResult.reason);
      results.errors.push(dataResult.reason);
    }

    if (computeResult.status === "fulfilled") {
      results.computeWorker = computeResult.value;
    } else {
      logger.error("compute-worker error", computeResult.reason);
      results.errors.push(computeResult.reason);
    }

    try {
      results.externalWorker = await iii.trigger({
        function_id: "payment-worker::record",
        payload: { charge: 0.0001 },
      });
    } catch (error) {
      logger.error("payment-worker error", error);
      results.errors.push(error);
    }

    results.success =
      "Success! Open workers/client/src/worker.ts and config.yaml to learn how this worked, or visit https://iii.dev/docs/concepts";

    return { status: results.errors.length > 0 ? 500 : 200, body: results };
  },
);

iii.registerTrigger({
  type: "http",
  function_id: orchestrate.id,
  config: { api_path: "/orchestrate", http_method: "POST" },
});

console.log("Client started - listening for calls");
