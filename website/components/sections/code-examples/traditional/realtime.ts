import express from "express";
import { createServer } from "http";
import { Server } from "socket.io";
import { createClient } from "redis";
import pino from "pino";
import { NodeSDK } from "@opentelemetry/sdk-node";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-http";
import { trace } from "@opentelemetry/api";

const telemetry = new NodeSDK({
  traceExporter: new OTLPTraceExporter({
    url: process.env.OTEL_EXPORTER_OTLP_TRACES_ENDPOINT,
  }),
  serviceName: "realtime-traditional",
});
void telemetry.start();

const logger = pino({ name: "realtime-traditional", level: process.env.LOG_LEVEL ?? "info" });
const tracer = trace.getTracer("realtime-traditional");

const app = express();
app.use(express.json());
const server = createServer(app);
const io = new Server(server, { cors: { origin: "*" } });
const redisPublisher = createClient({ url: process.env.REDIS_URL || "redis://localhost:6379" });
const redisSubscriber = redisPublisher.duplicate();
const redisState = redisPublisher.duplicate();

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
      service: "realtime-traditional",
      level,
      event,
      data,
      at: new Date().toISOString(),
    }),
  });
}

async function roomPresence(roomId: string) {
  return redisState.sCard(`presence:${roomId}`);
}

io.on("connection", (socket) => {
  socket.on("room.join", async ({ roomId, userId }) => {
    const span = tracer.startSpan("realtime.room_join");
    socket.join(roomId);
    await redisState.sAdd(`presence:${roomId}`, userId);
    const count = await roomPresence(roomId);
    io.to(roomId).emit("presence.updated", { roomId, count });
    await sendCentralLog("info", "realtime.room_join", { roomId, userId, count });
    span.end();
  });
});

app.post("/rooms/:roomId/score", async (req, res) => {
  const span = tracer.startSpan("realtime.update_score");
  const message = {
    roomId: req.params.roomId,
    playerId: req.body.playerId,
    score: req.body.score,
    updatedAt: new Date().toISOString(),
  };
  await redisPublisher.publish(`rooms:${req.params.roomId}:updates`, JSON.stringify(message));
  await sendCentralLog("info", "realtime.update_score", message);
  span.end();
  res.status(202).json({ accepted: true, roomId: req.params.roomId });
});

async function bootRealtime() {
  await redisPublisher.connect();
  await redisSubscriber.connect();
  await redisState.connect();
  await redisSubscriber.pSubscribe("rooms:*:updates", async (raw, channel) => {
    const roomId = channel.split(":")[1];
    io.to(roomId).emit("room.updated", JSON.parse(raw));
    await sendCentralLog("info", "realtime.broadcast_update", { roomId });
  });
}

void bootRealtime();
server.listen(3003);
