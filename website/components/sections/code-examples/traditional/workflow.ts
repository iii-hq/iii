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

const logger = pino({ name: "workflow-traditional", level: process.env.LOG_LEVEL ?? "info" });
const tracer = trace.getTracer("workflow-traditional");
const redis = createRedisClient({ url: process.env.REDIS_URL || "redis://localhost:6379" });
const app = express();
app.use(express.json());

function writeLog(level: "info" | "warn" | "error", payload: Record<string, unknown>) {
  if (level === "error") return logger.error(payload);
  if (level === "warn") return logger.warn(payload);
  return logger.info(payload);
}

async function sendCentralLog(level: "info" | "warn" | "error", event: string, data: Record<string, unknown>) {
  writeLog(level, { event, ...data });
  await fetch(`${process.env.OBSERVABILITY_URL}/logs`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      service: "workflow-traditional",
      level,
      event,
      data,
      at: new Date().toISOString(),
    }),
  });
}

async function setWorkflowStep(orderId: string, step: string, status: string) {
  await redis.hSet(`workflow:${orderId}`, { currentStep: step, status, updatedAt: new Date().toISOString() });
}

async function validateOrder(orderId: string) {
  await setWorkflowStep(orderId, "validate", "running");
  await sendCentralLog("info", "workflow.step.validate", { orderId });
  await setWorkflowStep(orderId, "validate", "complete");
}

async function chargePayment(orderId: string, total: number) {
  await setWorkflowStep(orderId, "payment", "running");
  await sendCentralLog("info", "workflow.step.payment", { orderId, total });
  await setWorkflowStep(orderId, "payment", "complete");
  return { transactionId: `txn-${Date.now()}` };
}

async function shipOrder(orderId: string) {
  await setWorkflowStep(orderId, "ship", "running");
  await sendCentralLog("info", "workflow.step.ship", { orderId });
  await setWorkflowStep(orderId, "ship", "complete");
  return { trackingNumber: `trk-${Date.now()}` };
}

const { validateOrderActivity, chargePaymentActivity, shipOrderActivity } = proxyActivities<{
  validateOrderActivity(orderId: string): Promise<void>;
  chargePaymentActivity(orderId: string, total: number): Promise<{ transactionId: string }>;
  shipOrderActivity(orderId: string): Promise<{ trackingNumber: string }>;
}>({
  startToCloseTimeout: "1 minute",
  retry: { maximumAttempts: 3 },
});

export async function orderFulfillmentWorkflow(input: { orderId: string; total: number }) {
  await validateOrderActivity(input.orderId);
  await chargePaymentActivity(input.orderId, input.total);
  const shipment = await shipOrderActivity(input.orderId);
  return { orderId: input.orderId, status: "fulfilled", trackingNumber: shipment.trackingNumber };
}

async function bootTemporal() {
  await redis.connect();
  const connection = await Connection.connect({ address: process.env.TEMPORAL_ADDRESS || "localhost:7233" });
  const worker = await Worker.create({
    connection,
    taskQueue: "order-workflows",
    workflowsPath: require.resolve("./workflow-definitions"),
    activities: {
      validateOrderActivity: validateOrder,
      chargePaymentActivity: chargePayment,
      shipOrderActivity: shipOrder,
    },
  });
  void worker.run();
}

app.post("/workflows/order", async (req, res) => {
  const span = tracer.startSpan("workflow.start_order_fulfillment");
  const orderId = req.body.orderId ?? `ord-${Date.now()}`;
  await setWorkflowStep(orderId, "created", "queued");
  const connection = await Connection.connect({ address: process.env.TEMPORAL_ADDRESS || "localhost:7233" });
  const client = new Client({ connection });
  const handle = await client.workflow.start(orderFulfillmentWorkflow, {
    taskQueue: "order-workflows",
    workflowId: `wf-${orderId}`,
    args: [{ orderId, total: req.body.total }],
  });
  await sendCentralLog("info", "workflow.start_order_fulfillment.queued", { orderId, workflowId: handle.workflowId });
  span.end();
  res.status(202).json({ orderId, workflowId: handle.workflowId });
});

app.get("/workflows/order/:orderId", async (req, res) => {
  const snapshot = await redis.hGetAll(`workflow:${req.params.orderId}`);
  if (Object.keys(snapshot).length === 0) {
    res.status(404).json({ error: "Workflow not found" });
    return;
  }
  await sendCentralLog("info", "workflow.snapshot.loaded", { orderId: req.params.orderId, status: snapshot.status });
  res.json({ orderId: req.params.orderId, ...snapshot });
});

void bootTemporal();
app.listen(3007);
