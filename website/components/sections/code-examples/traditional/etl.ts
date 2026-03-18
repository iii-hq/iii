import express from "express";
import { createClient } from "redis";
import pino from "pino";
import { NodeSDK } from "@opentelemetry/sdk-node";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-http";
import { trace } from "@opentelemetry/api";

const telemetry = new NodeSDK({
  traceExporter: new OTLPTraceExporter({
    url: process.env.OTEL_EXPORTER_OTLP_TRACES_ENDPOINT,
  }),
  serviceName: "etl-traditional",
});
void telemetry.start();

const logger = pino({
  name: "etl-traditional",
  level: process.env.LOG_LEVEL ?? "info",
});
const tracer = trace.getTracer("etl-traditional");
const redis = createClient({
  url: process.env.REDIS_URL || "redis://localhost:6379",
});
const app = express();
app.use(express.json());

async function sendCentralLog(event: string, data: Record<string, unknown>) {
  logger.info({ event, ...data });
  await fetch(`${process.env.OBSERVABILITY_URL}/logs`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ event, data }),
  });
}

async function enqueueStep(
  runId: string,
  step: "extract" | "transform" | "load",
  payload: any,
) {
  await redis.rPush("etl:queue", JSON.stringify({ runId, step, payload }));
}

async function setRunState(runId: string, fields: Record<string, string>) {
  await redis.hSet(`etl:run:${runId}`, fields);
}

async function processMessage(raw: string) {
  const msg = JSON.parse(raw) as {
    runId: string;
    step: "extract" | "transform" | "load";
    payload: any;
  };
  if (msg.step === "extract") {
    await setRunState(msg.runId, { step: "extract", status: "running" });
    await enqueueStep(msg.runId, "transform", { rows: [{ id: "1", value: 10 }] });
    await sendCentralLog("etl.extract.completed", { runId: msg.runId });
    return;
  }
  if (msg.step === "transform") {
    await setRunState(msg.runId, { step: "transform", status: "running" });
    await enqueueStep(msg.runId, "load", { rows: msg.payload.rows });
    await sendCentralLog("etl.transform.completed", { runId: msg.runId });
    return;
  }
  await setRunState(msg.runId, { step: "load", status: "running" });
  await redis.set(`etl:warehouse:${msg.runId}`, JSON.stringify(msg.payload.rows));
  await setRunState(msg.runId, {
    step: "load",
    status: "completed",
    completedAt: new Date().toISOString(),
  });
  await sendCentralLog("etl.load.completed", { runId: msg.runId });
}

async function workerLoop() {
  while (true) {
    const result = await redis.blPop("etl:queue", 0);
    if (result?.element) {
      await processMessage(result.element);
    }
  }
}

app.post("/etl/run", async (_req, res) => {
  const span = tracer.startSpan("etl.start_run");
  const runId = `run-${Date.now()}`;
  await setRunState(runId, {
    runId,
    step: "extract",
    status: "queued",
    createdAt: new Date().toISOString(),
  });
  await enqueueStep(runId, "extract", {});
  await sendCentralLog("etl.start_run.queued", { runId });
  span.end();
  res.status(202).json({ runId, status: "queued" });
});

async function bootEtl() {
  await redis.connect();
  void workerLoop();
}

void bootEtl();
app.listen(3011);
