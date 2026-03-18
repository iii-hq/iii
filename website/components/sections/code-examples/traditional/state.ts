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

const logger = pino({ name: "state-traditional", level: process.env.LOG_LEVEL ?? "info" });
const tracer = trace.getTracer("state-traditional");

const app = express();
app.use(express.json());
const redis = createClient({ url: process.env.REDIS_URL || "redis://localhost:6379" });
const subscriber = redis.duplicate();

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
      service: "state-traditional",
      level,
      event,
      data,
      at: new Date().toISOString(),
    }),
  });
}

app.post("/state/carts/:cartId/items", async (req, res) => {
  const span = tracer.startSpan("state.cart_add_item");
  const key = `cart:${req.params.cartId}`;
  const cartRaw = await redis.get(key);
  const cart = cartRaw ? JSON.parse(cartRaw) : { id: req.params.cartId, items: [], checkedOut: false };
  cart.items.push({ sku: req.body.sku, qty: req.body.qty });
  await redis.set(key, JSON.stringify(cart));
  await redis.publish("state.carts.changed", JSON.stringify({ cartId: req.params.cartId, cart }));
  await sendCentralLog("info", "state.cart_add_item", { cartId: req.params.cartId, sku: req.body.sku });
  span.end();
  res.status(201).json(cart);
});

app.get("/state/carts/:cartId", async (req, res) => {
  const cartRaw = await redis.get(`cart:${req.params.cartId}`);
  if (!cartRaw) {
    await sendCentralLog("warn", "state.cart_get.not_found", { cartId: req.params.cartId });
    res.status(404).json({ error: "Cart not found" });
    return;
  }
  const cart = JSON.parse(cartRaw);
  await sendCentralLog("info", "state.cart_get.found", { cartId: req.params.cartId, itemCount: cart.items.length });
  res.json(cart);
});

app.post("/state/carts/:cartId/checkout", async (req, res) => {
  const key = `cart:${req.params.cartId}`;
  const cartRaw = await redis.get(key);
  if (!cartRaw) {
    await sendCentralLog("warn", "state.checkout.not_found", { cartId: req.params.cartId });
    res.status(404).json({ error: "Cart not found" });
    return;
  }
  const cart = JSON.parse(cartRaw);
  cart.checkedOut = true;
  await redis.set(key, JSON.stringify(cart));
  await redis.publish("state.carts.changed", JSON.stringify({ cartId: req.params.cartId, cart }));
  await sendCentralLog("info", "state.checkout.completed", { cartId: req.params.cartId });
  res.json(cart);
});

async function bootStateReactions() {
  await redis.connect();
  await subscriber.connect();
  await subscriber.subscribe("state.carts.changed", async (raw) => {
    const event = JSON.parse(raw) as { cartId: string; cart: { checkedOut: boolean } };
    if (event.cart.checkedOut) {
      await redis.incr("metrics:checkedout_carts");
      await sendCentralLog("info", "state.metrics.checkedout_incremented", { cartId: event.cartId });
    }
  });
}

void bootStateReactions();
app.listen(3004);
