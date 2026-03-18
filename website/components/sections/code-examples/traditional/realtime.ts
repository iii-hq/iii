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

const logger = pino({
  name: "realtime-traditional",
  level: process.env.LOG_LEVEL ?? "info",
});
const tracer = trace.getTracer("realtime-traditional");

const app = express();
app.use(express.json());
const server = createServer(app);
const io = new Server(server, {
  cors: { origin: "*" },
});
const redisPublisher = createClient({
  url: process.env.REDIS_URL || "redis://localhost:6379",
});
const redisSubscriber = redisPublisher.duplicate();
const streamName = "room-score-stream";

async function sendCentralLog(event: string, data: Record<string, unknown>) {
  logger.info({ event, ...data });
  await fetch(`${process.env.OBSERVABILITY_URL}/logs`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ event, data }),
  });
}

app.post("/rooms/:roomId/score", async (req, res) => {
  const span = tracer.startSpan("stream.publish_room_score");
  const score = {
    roomId: req.params.roomId,
    playerId: req.body.playerId,
    score: Number(req.body.score),
    at: new Date().toISOString(),
  };
  await redisPublisher.xAdd(streamName, "*", {
    roomId: score.roomId,
    playerId: String(score.playerId),
    score: String(score.score),
    at: score.at,
  });
  await sendCentralLog("stream.publish_room_score", score);
  span.end();
  res.status(202).json({
    accepted: true,
    roomId: req.params.roomId,
  });
});

async function bootRealtime() {
  await redisPublisher.connect();
  await redisSubscriber.connect();
  let lastId = "$";
  while (true) {
    const streams = await redisSubscriber.xRead(
      [{ key: streamName, id: lastId }],
      { BLOCK: 0, COUNT: 10 },
    );
    if (!streams) continue;
    for (const stream of streams) {
      for (const message of stream.messages) {
        lastId = message.id;
        const payload = {
          roomId: String(message.message.roomId),
          playerId: String(message.message.playerId),
          score: Number(message.message.score),
          at: String(message.message.at),
        };
        io.emit("room.stream.update", payload);
        await sendCentralLog("stream.consume_room_score", {
          eventId: message.id,
          roomId: payload.roomId,
        });
      }
    }
  }
}

void bootRealtime();
server.listen(3003);
