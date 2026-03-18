import express from "express";
import amqp, { Channel, Connection, ConsumeMessage } from "amqplib";
import pino from "pino";
import { NodeSDK } from "@opentelemetry/sdk-node";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-http";
import { trace } from "@opentelemetry/api";

const telemetry = new NodeSDK({
  traceExporter: new OTLPTraceExporter({
    url: process.env.OTEL_EXPORTER_OTLP_TRACES_ENDPOINT,
  }),
  serviceName: "events-traditional",
});
void telemetry.start();

const logger = pino({ name: "events-traditional", level: process.env.LOG_LEVEL ?? "info" });
const tracer = trace.getTracer("events-traditional");
const app = express();
app.use(express.json());

let channel: Channel;
const processed = new Set<string>();

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
      service: "events-traditional",
      level,
      event,
      data,
      at: new Date().toISOString(),
    }),
  });
}

async function handleProjection(message: ConsumeMessage | null) {
  if (!message) return;
  const span = tracer.startSpan("events.consume_order_created");
  const event = JSON.parse(message.content.toString()) as {
    eventId: string;
    orderId: string;
    customerId: string;
    total: number;
  };
  if (processed.has(event.eventId)) {
    await sendCentralLog("warn", "events.consume_order_created.duplicate", { eventId: event.eventId });
    channel.ack(message);
    span.end();
    return;
  }
  processed.add(event.eventId);
  await sendCentralLog("info", "events.consume_order_created.applied", {
    eventId: event.eventId,
    orderId: event.orderId,
  });
  channel.ack(message);
  span.end();
}

async function bootBroker() {
  const connection: Connection = await amqp.connect(process.env.AMQP_URL || "amqp://localhost");
  channel = await connection.createChannel();
  await channel.assertExchange("orders.events", "topic", { durable: true });
  await channel.assertQueue("inventory.projection", { durable: true });
  await channel.bindQueue("inventory.projection", "orders.events", "order.created");
  await channel.consume("inventory.projection", handleProjection, { noAck: false });
}

app.post("/events/order-created", async (req, res) => {
  const span = tracer.startSpan("events.publish_order_created");
  const payload = {
    eventId: req.body.eventId ?? `evt-${Date.now()}`,
    orderId: req.body.orderId,
    customerId: req.body.customerId,
    total: req.body.total,
  };
  channel.publish("orders.events", "order.created", Buffer.from(JSON.stringify(payload)), {
    persistent: true,
  });
  await sendCentralLog("info", "events.publish_order_created.published", {
    eventId: payload.eventId,
    orderId: payload.orderId,
  });
  span.end();
  res.status(202).json({ accepted: true, eventId: payload.eventId });
});

app.get("/events/processed", async (_req, res) => {
  await sendCentralLog("info", "events.processed.snapshot", { count: processed.size });
  res.json({ processedCount: processed.size });
});

void bootBroker();
app.listen(3002);
