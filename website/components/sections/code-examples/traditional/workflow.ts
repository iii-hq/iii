import express from "express";
import { createClient as createRedisClient } from "redis";
import pino from "pino";
import { NodeSDK } from "@opentelemetry/sdk-node";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-http";
import { trace } from "@opentelemetry/api";
import { Client, Connection } from "@temporalio/client";
import { proxyActivities } from "@temporalio/workflow";
import { Worker } from "@temporalio/worker";

const telemetry = new NodeSDK({
  traceExporter: new OTLPTraceExporter({
    url: process.env.OTEL_EXPORTER_OTLP_TRACES_ENDPOINT,
  }),
  serviceName: "workflow-traditional",
});
void telemetry.start();

const logger = pino({
  name: "workflow-traditional",
  level: process.env.LOG_LEVEL ?? "info",
});
const tracer = trace.getTracer("workflow-traditional");
const redis = createRedisClient({
  url: process.env.REDIS_URL || "redis://localhost:6379",
});
const app = express();
app.use(express.json());
let temporalReady = false;

async function sendCentralLog(event: string, data: Record<string, unknown>) {
  logger.info({ event, ...data });
  await fetch(`${process.env.OBSERVABILITY_URL}/logs`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ event, data }),
  });
}

async function setWorkflowState(orderId: string, step: string, status: string) {
  await redis.hSet(`workflow:${orderId}`, {
    currentStep: step,
    status,
    updatedAt: new Date().toISOString(),
  });
}

const { runStep } = proxyActivities<{
  runStep(orderId: string, step: "validate" | "ship"): Promise<void>;
}>({
  startToCloseTimeout: "1 minute",
  retry: { maximumAttempts: 3 },
});

export async function orderFulfillmentWorkflow(input: {
  orderId: string;
}) {
  await runStep(input.orderId, "validate");
  await runStep(input.orderId, "ship");
  return { orderId: input.orderId, status: "fulfilled" };
}

async function bootTemporal() {
  await redis.connect();
  const connection = await Connection.connect({
    address: process.env.TEMPORAL_ADDRESS || "localhost:7233",
  });
  const worker = await Worker.create({
    connection,
    taskQueue: "order-workflows",
    workflowsPath: require.resolve("./workflow-definitions"),
    activities: {
      runStep: async (orderId: string, step: "validate" | "ship") => {
        await setWorkflowState(orderId, step, "running");
        await setWorkflowState(orderId, step, "complete");
        await sendCentralLog("workflow.step.completed", { orderId, step });
      },
    },
  });
  void worker.run().catch((error) => {
    temporalReady = false;
    logger.error({ err: error }, "workflow.worker_failed");
  });
  temporalReady = true;
}

const temporalReadyPromise = bootTemporal();

app.post("/workflows/order", async (req, res) => {
  const span = tracer.startSpan("workflow.start_order_fulfillment");
  try {
    await temporalReadyPromise;
  } catch (error) {
    span.end();
    res.status(503).json({ error: "workflow service unavailable" });
    return;
  }
  if (!temporalReady) {
    span.end();
    res.status(503).json({ error: "workflow service unavailable" });
    return;
  }

  const orderId = req.body.orderId ?? `ord-${Date.now()}`;
  await setWorkflowState(orderId, "validate", "queued");
  const connection = await Connection.connect({
    address: process.env.TEMPORAL_ADDRESS || "localhost:7233",
  });
  const client = new Client({ connection });
  const handle = await client.workflow.start(orderFulfillmentWorkflow, {
    taskQueue: "order-workflows",
    workflowId: `wf-${orderId}`,
    args: [{ orderId }],
  });
  await sendCentralLog("workflow.start_order_fulfillment.queued", {
    orderId,
    workflowId: handle.workflowId,
  });
  span.end();
  res.status(202).json({
    orderId,
    workflowId: handle.workflowId,
  });
});

temporalReadyPromise
  .then(() => {
    app.listen(3007);
  })
  .catch((error) => {
    logger.error({ err: error }, "workflow.bootstrap_failed");
  });
