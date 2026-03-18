import express from "express";
import cron from "node-cron";
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

const logger = pino({ name: "etl-traditional", level: process.env.LOG_LEVEL ?? "info" });
const tracer = trace.getTracer("etl-traditional");
const redis = createClient({ url: process.env.REDIS_URL || "redis://localhost:6379" });
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
      service: "etl-traditional",
      level,
      event,
      data,
      at: new Date().toISOString(),
    }),
  });
}

async function enqueueStep(runId: string, step: "extract" | "transform" | "load", payload: any) {
  await redis.rPush("etl:queue", JSON.stringify({ runId, step, payload }));
}

async function setRunState(runId: string, fields: Record<string, string>) {
  await redis.hSet(`etl:run:${runId}`, fields);
}

async function processMessage(raw: string) {
  const msg = JSON.parse(raw) as { runId: string; step: "extract" | "transform" | "load"; payload: any };
  if (msg.step === "extract") {
    await setRunState(msg.runId, { step: "extract", status: "running" });
    const extracted = [{ id: "1", value: 10 }, { id: "2", value: 20 }];
    await sendCentralLog("info", "etl.extract.completed", { runId: msg.runId, count: extracted.length });
    await enqueueStep(msg.runId, "transform", { extracted });
    return;
  }
  if (msg.step === "transform") {
    await setRunState(msg.runId, { step: "transform", status: "running" });
    const transformed = msg.payload.extracted.map((row: any) => ({ ...row, value: row.value * 2 }));
    await sendCentralLog("info", "etl.transform.completed", { runId: msg.runId, count: transformed.length });
    await enqueueStep(msg.runId, "load", { transformed });
    return;
  }
  await setRunState(msg.runId, { step: "load", status: "running" });
  await redis.set(`etl:warehouse:${msg.runId}`, JSON.stringify(msg.payload.transformed));
  await setRunState(msg.runId, { step: "load", status: "completed", completedAt: new Date().toISOString() });
  await sendCentralLog("info", "etl.load.completed", {
    runId: msg.runId,
    count: msg.payload.transformed.length,
  });
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
  await setRunState(runId, { runId, step: "extract", status: "queued", createdAt: new Date().toISOString() });
  await enqueueStep(runId, "extract", {});
  await sendCentralLog("info", "etl.start_run.queued", { runId });
  span.end();
  res.status(202).json({ runId, status: "queued" });
});

app.get("/etl/:runId", async (req, res) => {
  const snapshot = await redis.hGetAll(`etl:run:${req.params.runId}`);
  if (Object.keys(snapshot).length === 0) {
    res.status(404).json({ error: "Run not found" });
    return;
  }
  res.json(snapshot);
});

cron.schedule("0 2 * * *", () => {
  void enqueueStep(`run-${Date.now()}`, "extract", {});
});

async function bootEtl() {
  await redis.connect();
  void workerLoop();
}

void bootEtl();
app.listen(3011);
