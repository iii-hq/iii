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

function sendCentralLog(event: string, data: Record<string, unknown>) {
  logger.info({ event, ...data });
  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), 750);

  void (async () => {
    try {
      await fetch(`${process.env.OBSERVABILITY_URL}/logs`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ event, data }),
        signal: controller.signal,
      });
    } catch (error) {
      logger.error({ err: error, event }, "feature_flags.central_log_failed");
    } finally {
      clearTimeout(timeoutId);
    }
  })();
}

app.post("/flags/:flagKey/overrides", async (req, res) => {
  const span = tracer.startSpan("flags.override_set");
  const flagKey = req.params.flagKey;
  try {
    if (typeof req.body?.value !== "boolean") {
      res.status(400).json({ error: "value must be a boolean" });
      return;
    }

    const override = {
      flagKey,
      value: req.body.value,
      updatedAt: new Date().toISOString(),
    };

    await redis.hSet("flags:overrides", flagKey, JSON.stringify(override));
    sendCentralLog("flags.override_set", override);
    res.json(override);
  } catch (error) {
    span.recordException(error as Error);
    const message = error instanceof Error ? error.message : "Unknown error";
    sendCentralLog("flags.override_set.error", { flagKey, error: message });
    res.status(500).json({
      error: "Failed to set flag override",
      details: message,
    });
  } finally {
    span.end();
  }
});

app.get("/flags/:flagKey/evaluate", async (req, res) => {
  const span = tracer.startSpan("flags.evaluate");
  const flagKey = req.params.flagKey;
  const userId = String(req.query.userId || "anonymous");
  try {
    const override = await redis.hGet("flags:overrides", flagKey);
    if (override) {
      const parsed = JSON.parse(override);
      sendCentralLog("flags.evaluate.override", {
        flagKey,
        userId,
        value: parsed.value,
      });
      res.json({
        flagKey,
        userId,
        value: parsed.value,
        source: "override",
      });
      return;
    }

    const value = await ldClient.variation(flagKey, { key: userId }, false);
    sendCentralLog("flags.evaluate.launchdarkly", {
      flagKey,
      userId,
      value,
    });
    res.json({
      flagKey,
      userId,
      value,
      source: "launchdarkly",
    });
  } catch (error) {
    span.recordException(error as Error);
    const message = error instanceof Error ? error.message : "Unknown error";
    sendCentralLog("flags.evaluate.error", { flagKey, userId, error: message });
    res.status(500).json({
      error: "Failed to evaluate flag",
      details: message,
    });
  } finally {
    span.end();
  }
});

async function bootFlags() {
  await ldClient.waitForInitialization({
    timeout: 5,
  });
  await redis.connect();
}

bootFlags()
  .then(() => {
    app.listen(3009);
  })
  .catch((error) => {
    logger.error({ err: error }, "feature_flags.bootstrap_failed");
  });
