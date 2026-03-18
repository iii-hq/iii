import express from "express";
import { createServer } from "http";
import { Server as ColyseusServer, Room, Client } from "colyseus";
import { WebSocketTransport } from "@colyseus/ws-transport";
import { createClient } from "redis";
import pino from "pino";
import { NodeSDK } from "@opentelemetry/sdk-node";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-http";
import { trace } from "@opentelemetry/api";

const telemetry = new NodeSDK({
  traceExporter: new OTLPTraceExporter({
    url: process.env.OTEL_EXPORTER_OTLP_TRACES_ENDPOINT,
  }),
  serviceName: "multiplayer-traditional",
});
void telemetry.start();

const logger = pino({ name: "multiplayer-traditional", level: process.env.LOG_LEVEL ?? "info" });
const tracer = trace.getTracer("multiplayer-traditional");
const redis = createClient({ url: process.env.REDIS_URL || "redis://localhost:6379" });
const subscriber = redis.duplicate();
const app = express();
app.use(express.json());
const server = createServer(app);

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
      service: "multiplayer-traditional",
      level,
      event,
      data,
      at: new Date().toISOString(),
    }),
  });
}

class MatchRoom extends Room<{ players: Record<string, { score: number }> }> {
  async onCreate() {
    this.setState({ players: {} });
    this.onMessage("score", async (client, payload: { score: number }) => {
      const span = tracer.startSpan("multiplayer.score_update");
      const roomId = this.roomId;
      const playerId = client.sessionId;
      await redis.hSet(`room:${roomId}:scores`, playerId, String(payload.score));
      this.state.players[playerId] = { score: payload.score };
      this.broadcast("room.updated", { roomId, playerId, score: payload.score });
      await redis.publish("rooms.updated", JSON.stringify({ roomId, playerId, score: payload.score }));
      await sendCentralLog("info", "multiplayer.score_update", { roomId, playerId, score: payload.score });
      span.end();
    });
  }

  async onJoin(client: Client) {
    const span = tracer.startSpan("multiplayer.player_join");
    this.state.players[client.sessionId] = { score: 0 };
    await redis.sAdd(`room:${this.roomId}:players`, client.sessionId);
    const playerCount = await redis.sCard(`room:${this.roomId}:players`);
    this.broadcast("presence.updated", { roomId: this.roomId, count: playerCount });
    await sendCentralLog("info", "multiplayer.player_join", {
      roomId: this.roomId,
      playerId: client.sessionId,
      count: playerCount,
    });
    span.end();
  }

  async onLeave(client: Client) {
    await redis.sRem(`room:${this.roomId}:players`, client.sessionId);
    const playerCount = await redis.sCard(`room:${this.roomId}:players`);
    this.broadcast("presence.updated", { roomId: this.roomId, count: playerCount });
    await sendCentralLog("info", "multiplayer.player_leave", {
      roomId: this.roomId,
      playerId: client.sessionId,
      count: playerCount,
    });
  }
}

const gameServer = new ColyseusServer({
  transport: new WebSocketTransport({ server }),
});
gameServer.define("match", MatchRoom);

app.post("/rooms/:roomId/score", async (req, res) => {
  const roomId = req.params.roomId;
  const playerId = String(req.body.playerId);
  const score = Number(req.body.score);
  await redis.hSet(`room:${roomId}:scores`, playerId, String(score));
  await redis.publish("rooms.updated", JSON.stringify({ roomId, playerId, score }));
  await sendCentralLog("info", "multiplayer.score_update", { roomId, playerId, score });
  res.status(202).json({ accepted: true, roomId });
});

app.get("/rooms/:roomId/state", async (req, res) => {
  const scores = await redis.hGetAll(`room:${req.params.roomId}:scores`);
  const count = await redis.sCard(`room:${req.params.roomId}:players`);
  res.json({ roomId: req.params.roomId, players: count, scores });
});

async function bootMultiplayer() {
  await redis.connect();
  await subscriber.connect();
  await subscriber.subscribe("rooms.updated", async (raw) => {
    const evt = JSON.parse(raw) as { roomId: string; playerId: string; score: number };
    const room = gameServer.rooms.get(evt.roomId);
    if (room) {
      room.broadcast("room.updated", evt);
    }
    await sendCentralLog("info", "multiplayer.broadcast_update", evt);
  });
}

void bootMultiplayer();
server.listen(3010);
