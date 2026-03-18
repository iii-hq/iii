import express from "express";
import pino from "pino";
import { NodeSDK } from "@opentelemetry/sdk-node";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-http";
import { trace } from "@opentelemetry/api";

const telemetry = new NodeSDK({
  traceExporter: new OTLPTraceExporter({
    url: process.env.OTEL_EXPORTER_OTLP_TRACES_ENDPOINT,
  }),
  serviceName: "logging-traditional",
});
void telemetry.start();

const logger = pino({
  name: "logging-traditional",
  level: process.env.LOG_LEVEL ?? "info",
});
const tracer = trace.getTracer("logging-traditional");
const app = express();
app.use(express.json());

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
      service: "logging-traditional",
      level,
      event,
      data,
      at: new Date().toISOString(),
    }),
  });
}

app.post("/checkout", async (req, res) => {
  const rootSpan = tracer.startSpan("checkout.request");
  const requestId = req.header("x-request-id") || `req-${Date.now()}`;
  await sendCentralLog("info", "checkout.request.received", {
    requestId,
    cartId: req.body.cartId,
  });

  const pricingSpan = tracer.startSpan("checkout.calculate_totals");
  const subtotal = Number(req.body.subtotal ?? 0);
  const tax = subtotal * 0.1;
  const total = subtotal + tax;
  pricingSpan.setAttribute("checkout.total", total);
  pricingSpan.end();
  await sendCentralLog("info", "checkout.calculate_totals.done", {
    requestId,
    subtotal,
    tax,
    total,
  });

  const paymentSpan = tracer.startSpan("checkout.charge_payment");
  const approved = req.body.cardToken === "tok_valid";
  paymentSpan.setAttribute("checkout.payment_approved", approved);
  paymentSpan.end();

  if (!approved) {
    await sendCentralLog("warn", "checkout.charge_payment.declined", {
      requestId,
    });
    rootSpan.end();
    res.status(402).json({ error: "Payment declined" });
    return;
  }

  await sendCentralLog("info", "checkout.completed", {
    requestId,
    total,
  });
  rootSpan.end();
  res.json({ requestId, total, status: "paid" });
});

app.listen(3006);
