import express from "express";
import { init } from "launchdarkly-node-server-sdk";
import { createClient } from "redis";
import pino from "pino";
import { NodeSDK } from "@opentelemetry/sdk-node";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-http";
import { trace } from "@opentelemetry/api";

const telemetry = new NodeSDK({
  traceExporter: new OTLPTraceExporter({
    url: process.env.OTEL_EXPORTER_OTLP_TRACES_ENDPOINT,
  }),
  serviceName: "feature-flags-traditional",
});
void telemetry.start();

const logger = pino({
  name: "feature-flags-traditional",
  level: process.env.LOG_LEVEL ?? "info",
});
const tracer = trace.getTracer("feature-flags-traditional");
const app = express();
app.use(express.json());

const ldClient = init(process.env.LD_SDK_KEY || "");
const redis = createClient({
  url: process.env.REDIS_URL || "redis://localhost:6379",
});
const subscriber = redis.duplicate();

async function sendCentralLog(event: string, data: Record<string, unknown>) {
  logger.info({ event, ...data });
  await fetch(`${process.env.OBSERVABILITY_URL}/logs`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ event, data }),
  });
}

app.post("/flags/:flagKey/overrides", async (req, res) => {
  const span = tracer.startSpan("flags.override_set");
  const flagKey = req.params.flagKey;
  const override = {
    flagKey,
    value: Boolean(req.body.value),
    updatedAt: new Date().toISOString(),
  };
  await redis.hSet("flags:overrides", flagKey, JSON.stringify(override));
  await redis.publish("flags.updated", JSON.stringify(override));
  await sendCentralLog("flags.override_set", override);
  span.end();
  res.json(override);
});

app.get("/flags/:flagKey/evaluate", async (req, res) => {
  const span = tracer.startSpan("flags.evaluate");
  const flagKey = req.params.flagKey;
  const userId = String(req.query.userId || "anonymous");
  const override = await redis.hGet("flags:overrides", flagKey);
  if (override) {
    const parsed = JSON.parse(override);
    await sendCentralLog("flags.evaluate.override", {
      flagKey,
      userId,
      value: parsed.value,
    });
    span.end();
    res.json({
      flagKey,
      userId,
      value: parsed.value,
      source: "override",
    });
    return;
  }
  // ...segment/user targeting...
  const value = await ldClient.variation(flagKey, { key: userId }, false);
  await sendCentralLog("flags.evaluate.launchdarkly", {
    flagKey,
    userId,
    value,
  });
  span.end();
  res.json({
    flagKey,
    userId,
    value,
    source: "launchdarkly",
  });
});

async function bootFlags() {
  await ldClient.waitForInitialization({
    timeout: 5,
  });
  await redis.connect();
  await subscriber.connect();
  await subscriber.subscribe("flags.updated", async (raw) => {
    const evt = JSON.parse(raw) as { flagKey: string; value: boolean };
    // ...fan out config refresh...
    await sendCentralLog("flags.update_propagated", evt);
  });
}

void bootFlags();
app.listen(3009);
