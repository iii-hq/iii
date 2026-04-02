import express from "express";
import { createClient } from "redis";
import pino from "pino";
import { NodeSDK } from "@opentelemetry/sdk-node";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-http";
import { trace } from "@opentelemetry/api";

const telemetry = new NodeSDK({
  traceExporter: new OTLPTraceExporter({
    url: process.env.OTEL_EXPORTER_OTLP_TRACES_ENDPOINT,
  }),
  serviceName: "state-traditional",
});
void telemetry.start();

const logger = pino({
  name: "state-traditional",
  level: process.env.LOG_LEVEL ?? "info",
});
const tracer = trace.getTracer("state-traditional");

const app = express();
app.use(express.json());
const redis = createClient({
  url: process.env.REDIS_URL || "redis://localhost:6379",
});

async function sendCentralLog(event: string, data: Record<string, unknown>) {
  logger.info({ event, ...data });
  await fetch(`${process.env.OBSERVABILITY_URL}/logs`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ event, data }),
  });
}

app.post("/state/carts/:cartId/items", async (req, res) => {
  const span = tracer.startSpan("state.cart_add_item");
  const key = `cart:${req.params.cartId}`;
  const cartRaw = await redis.get(key);
  const cart = cartRaw ? JSON.parse(cartRaw) : { id: req.params.cartId, items: [] };
  cart.items.push({ sku: req.body.sku, qty: req.body.qty });
  await redis.set(key, JSON.stringify(cart));
  await sendCentralLog("state.cart_add_item", { cartId: req.params.cartId });
  span.end();
  res.status(201).json(cart);
});

app.get("/state/carts/:cartId", async (req, res) => {
  const cartRaw = await redis.get(`cart:${req.params.cartId}`);
  if (!cartRaw) {
    await sendCentralLog("state.cart_get.not_found", { cartId: req.params.cartId });
    res.status(404).json({ error: "Cart not found" });
    return;
  }
  res.json(JSON.parse(cartRaw));
});

void redis.connect();
app.listen(3004);
