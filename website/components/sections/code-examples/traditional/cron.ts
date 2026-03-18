import express from "express";
import cron from "node-cron";
import pino from "pino";
import { NodeSDK } from "@opentelemetry/sdk-node";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-http";
import { trace } from "@opentelemetry/api";

const telemetry = new NodeSDK({
  traceExporter: new OTLPTraceExporter({
    url: process.env.OTEL_EXPORTER_OTLP_TRACES_ENDPOINT,
  }),
  serviceName: "cron-traditional",
});
void telemetry.start();

const logger = pino({
  name: "cron-traditional",
  level: process.env.LOG_LEVEL ?? "info",
});
const tracer = trace.getTracer("cron-traditional");
const app = express();
app.use(express.json());

const schedules: Record<
  string,
  {
    expression: string;
    task: cron.ScheduledTask;
    enabled: boolean;
  }
> = {};

function writeLog(
  level: "info" | "warn" | "error",
  payload: Record<string, unknown>,
) {
  if (level === "error") return logger.error(payload);
  if (level === "warn") return logger.warn(payload);
  return logger.info(payload);
}

async function sendCentralLog(
  level: "info" | "warn" | "error",
  event: string,
  data: Record<string, unknown>,
) {
  writeLog(level, { event, ...data });
  await fetch(`${process.env.OBSERVABILITY_URL}/logs`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
    },
    body: JSON.stringify({
      service: "cron-traditional",
      level,
      event,
      data,
      at: new Date().toISOString(),
    }),
  });
}

async function runReportTask() {
  const span = tracer.startSpan("cron.reports_generate");
  await sendCentralLog("info", "cron.reports_generate.run", {
    task: "reports::generate",
  });
  span.end();
}

schedules.reports = {
  expression: "0 3 * * *",
  enabled: true,
  task: cron.schedule("0 3 * * *", () => {
    void runReportTask();
  }),
};

app.post("/cron/reports/start", async (_req, res) => {
  schedules.reports.task.start();
  schedules.reports.enabled = true;
  await sendCentralLog("info", "cron.reports.start", {});
  res.json({ task: "reports", enabled: true });
});

app.post("/cron/reports/stop", async (_req, res) => {
  schedules.reports.task.stop();
  schedules.reports.enabled = false;
  await sendCentralLog("info", "cron.reports.stop", {});
  res.json({ task: "reports", enabled: false });
});

app.get("/cron/tasks", async (_req, res) => {
  const tasks = Object.entries(schedules).map(([id, schedule]) => ({
    id,
    expression: schedule.expression,
    enabled: schedule.enabled,
  }));
  await sendCentralLog("info", "cron.tasks.list", {
    count: tasks.length,
  });
  res.json({ tasks });
});

app.listen(3005);
