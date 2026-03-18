import express from "express";
import { z } from "zod";
import pino from "pino";
import { NodeSDK } from "@opentelemetry/sdk-node";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-http";
import { trace } from "@opentelemetry/api";

const telemetry = new NodeSDK({
  traceExporter: new OTLPTraceExporter({
    url: process.env.OTEL_EXPORTER_OTLP_TRACES_ENDPOINT,
  }),
  serviceName: "blog-api-traditional",
});
void telemetry.start();

const logger = pino({
  name: "blog-api-traditional",
  level: process.env.LOG_LEVEL ?? "info",
});
const tracer = trace.getTracer("blog-api-traditional");

const app = express();
app.use(express.json());

type Post = {
  title: string;
  body: string;
};

const posts: Post[] = [
  {
    title: "Hello API",
    body: "HTTP and app logic are implemented in one service.",
  },
];

const createPost = z.object({
  title: z.string().min(1),
  body: z.string().min(1),
});

function writeLog(level: "info" | "warn" | "error", payload: Record<string, unknown>) {
  if (level === "error") {
    logger.error(payload);
    return;
  }
  if (level === "warn") {
    logger.warn(payload);
    return;
  }
  logger.info(payload);
}

async function sendCentralLog(level: "info" | "warn" | "error", event: string, data: Record<string, unknown>) {
  writeLog(level, { event, ...data });
  await fetch(`${process.env.OBSERVABILITY_URL}/logs`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      service: "blog-api-traditional",
      level,
      event,
      data,
      at: new Date().toISOString(),
    }),
  });
}

app.get("/posts", async (_req, res) => {
  const span = tracer.startSpan("api.list_posts");
  await sendCentralLog("info", "api.list_posts", { count: posts.length });
  span.end();
  res.json({ posts });
});

app.post("/posts", async (req, res) => {
  const span = tracer.startSpan("api.create_post");
  const data = createPost.parse(req.body);
  const post: Post = { title: data.title, body: data.body };
  posts.unshift(post);
  await sendCentralLog("info", "api.create_post.created", { title: post.title });
  span.end();
  res.status(201).json({ post });
});

app.listen(3000);
