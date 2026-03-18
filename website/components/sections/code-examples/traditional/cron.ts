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

async function sendCentralLog(event: string, data: Record<string, unknown>) {
  logger.info({ event, ...data });
  await fetch(`${process.env.OBSERVABILITY_URL}/logs`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ event, data }),
  });
}

async function runReportTask() {
  const span = tracer.startSpan("cron.reports_generate");
  // Report generation logic runs on this schedule.
  await sendCentralLog("cron.reports_generate.run", { task: "reports::generate" });
  span.end();
}

cron.schedule("0 3 * * *", () => {
  void runReportTask();
});
