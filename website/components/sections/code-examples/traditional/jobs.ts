import express from "express";
import { Queue, Worker, QueueEvents } from "bullmq";
import IORedis from "ioredis";
import pino from "pino";
import { NodeSDK } from "@opentelemetry/sdk-node";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-http";
import { trace } from "@opentelemetry/api";

const telemetry = new NodeSDK({
  traceExporter: new OTLPTraceExporter({
    url: process.env.OTEL_EXPORTER_OTLP_TRACES_ENDPOINT,
  }),
  serviceName: "jobs-traditional",
});
void telemetry.start();

const logger = pino({
  name: "jobs-traditional",
  level: process.env.LOG_LEVEL ?? "info",
});
const tracer = trace.getTracer("jobs-traditional");
const app = express();
app.use(express.json());

const redis = new IORedis(process.env.REDIS_URL || "redis://localhost:6379");
const queue = new Queue("video-transcode", {
  connection: redis,
});
const queueEvents = new QueueEvents("video-transcode", {
  connection: redis,
});

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
      service: "jobs-traditional",
      level,
      event,
      data,
      at: new Date().toISOString(),
    }),
  });
}

new Worker(
  "video-transcode",
  async (job) => {
    const span = tracer.startSpan("jobs.video_transcode");
    await sendCentralLog("info", "jobs.video_transcode.started", {
      jobId: job.id,
      assetId: job.data.assetId,
    });
    const result = {
      assetId: job.data.assetId,
      profile: job.data.profile,
      output: `${job.data.assetId}.mp4`,
    };
    await sendCentralLog("info", "jobs.video_transcode.completed", {
      jobId: job.id,
      output: result.output,
    });
    span.end();
    return result;
  },
  { connection: redis, concurrency: 5 },
);

queueEvents.on("failed", async ({ jobId, failedReason }) => {
  await sendCentralLog("error", "jobs.video_transcode.failed", {
    jobId,
    failedReason,
  });
});

app.post("/jobs/transcode", async (req, res) => {
  const span = tracer.startSpan("jobs.enqueue_transcode");
  const job = await queue.add(
    "video.transcode",
    {
      assetId: req.body.assetId,
      profile: req.body.profile ?? "1080p",
    },
    {
      attempts: 3,
      backoff: {
        type: "exponential",
        delay: 1000,
      },
      removeOnComplete: 1000,
    },
  );
  await sendCentralLog("info", "jobs.enqueue_transcode.queued", {
    jobId: job.id,
    assetId: req.body.assetId,
  });
  span.end();
  res.status(202).json({
    jobId: job.id,
    queue: "video-transcode",
  });
});

app.get("/jobs/:jobId", async (req, res) => {
  const job = await queue.getJob(req.params.jobId);
  if (!job) {
    await sendCentralLog("warn", "jobs.lookup.not_found", {
      jobId: req.params.jobId,
    });
    res.status(404).json({ error: "Job not found" });
    return;
  }
  const state = await job.getState();
  const result = job.returnvalue ?? null;
  await sendCentralLog("info", "jobs.lookup.found", {
    jobId: job.id,
    state,
  });
  res.json({ jobId: job.id, state, result });
});

app.listen(3001);
