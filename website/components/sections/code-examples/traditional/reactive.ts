import express from "express";
import { createServer } from "http";
import { WebSocketServer } from "ws";
import { Client as PgClient } from "pg";
import { createClient as createRedisClient } from "redis";
import pino from "pino";
import { NodeSDK } from "@opentelemetry/sdk-node";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-http";
import { trace } from "@opentelemetry/api";

const telemetry = new NodeSDK({
  traceExporter: new OTLPTraceExporter({
    url: process.env.OTEL_EXPORTER_OTLP_TRACES_ENDPOINT,
  }),
  serviceName: "reactive-traditional",
});
void telemetry.start();

const logger = pino({
  name: "reactive-traditional",
  level: process.env.LOG_LEVEL ?? "info",
});
const tracer = trace.getTracer("reactive-traditional");
const pg = new PgClient({
  connectionString: process.env.POSTGRES_URL,
});
const redisPub = createRedisClient({
  url: process.env.REDIS_URL || "redis://localhost:6379",
});
const redisSub = redisPub.duplicate();

const app = express();
app.use(express.json());
const server = createServer(app);
const wss = new WebSocketServer({ server });

async function sendCentralLog(event: string, data: Record<string, unknown>) {
  logger.info({ event, ...data });
  await fetch(`${process.env.OBSERVABILITY_URL}/logs`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ event, data }),
  });
}

app.post("/reactive/accounts/:accountId/status", async (req, res) => {
  const span = tracer.startSpan("reactive.status_update");
  const accountId = req.params.accountId;
  const status = String(req.body.status);
  await pg.query(
    "insert into accounts (id, status) values ($1, $2) on conflict (id) do update set status = excluded.status",
    [accountId, status],
  );
  await pg.query("notify account_changes, $1", [
    JSON.stringify({ accountId, status }),
  ]);
  await sendCentralLog("reactive.status_update", {
    accountId,
    status,
  });
  span.end();
  res.json({ accountId, status });
});

async function bootReactive() {
  await pg.connect();
  await redisPub.connect();
  await redisSub.connect();
  await pg.query("listen account_changes");
  pg.on("notification", async (msg) => {
    if (msg.channel !== "account_changes" || !msg.payload) return;
    await redisPub.publish("account_changes", msg.payload);
    await sendCentralLog("reactive.pg_to_pubsub", JSON.parse(msg.payload));
  });
  await redisSub.subscribe("account_changes", async (raw) => {
    for (const socket of wss.clients) {
      socket.send(raw);
    }
    await sendCentralLog("reactive.pubsub_to_ws", JSON.parse(raw));
  });
}

void bootReactive();
server.listen(3012);
