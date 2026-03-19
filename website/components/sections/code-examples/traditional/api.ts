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

const posts: Post[] = [];

const createPost = z.object({
  title: z.string().min(1),
  body: z.string().min(1),
});

async function sendCentralLog(event: string, data: Record<string, unknown>) {
  logger.info({ event, ...data });
  await fetch(`${process.env.OBSERVABILITY_URL}/logs`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ event, data }),
  });
}

app.get("/posts", async (_req, res) => {
  const span = tracer.startSpan("api.list_posts");
  try {
    await sendCentralLog("api.list_posts", { count: posts.length });
    return res.json({ posts });
  } finally {
    span.end();
  }
});
});

app.post("/posts", async (req, res) => {
  const span = tracer.startSpan("api.create_post");
  try {
    const parsed = createPost.safeParse(req.body);
    if (!parsed.success) {
      return res.status(400).json({ error: "Invalid post payload" });
    }

    const post: Post = { title: parsed.data.title, body: parsed.data.body };
    posts.unshift(post);
    await sendCentralLog("api.create_post.created", { title: post.title });
    return res.status(201).json({ post });
  } finally {
    span.end();
  }
});
});

app.listen(3000);
