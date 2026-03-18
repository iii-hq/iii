import express from "express";
import axios from "axios";
import pRetry from "p-retry";
import pino from "pino";
import { NodeSDK } from "@opentelemetry/sdk-node";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-http";
import { trace } from "@opentelemetry/api";

const telemetry = new NodeSDK({
  traceExporter: new OTLPTraceExporter({
    url: process.env.OTEL_EXPORTER_OTLP_TRACES_ENDPOINT,
  }),
  serviceName: "remote-traditional",
});
void telemetry.start();

const logger = pino({
  name: "remote-traditional",
  level: process.env.LOG_LEVEL ?? "info",
});
const tracer = trace.getTracer("remote-traditional");
const app = express();
app.use(express.json());

async function sendCentralLog(event: string, data: Record<string, unknown>) {
  logger.info({ event, ...data });
  await fetch(`${process.env.OBSERVABILITY_URL}/logs`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ event, data }),
  });
}

app.post("/remote/invoices", async (req, res) => {
  const span = tracer.startSpan("remote.create_invoice");
  const invoice = await pRetry(
    async () => {
      const response = await axios.post(
        `${process.env.BILLING_API_URL}/invoices`,
        {
          customerId: req.body.customerId,
          amount: req.body.amount,
        },
        {
          timeout: 5000,
          headers: {
            authorization: `Bearer ${process.env.BILLING_API_TOKEN}`,
          },
        },
      );
      return response.data;
    },
    {
      retries: 2,
      onFailedAttempt: async (error) => {
        await sendCentralLog("remote.create_invoice.retry", {
          attempt: error.attemptNumber,
          retriesLeft: error.retriesLeft,
        });
      },
    },
  );
  // ...map remote response shape...
  await sendCentralLog("remote.create_invoice.completed", {
    invoiceId: invoice.id,
  });
  span.end();
  res.json(invoice);
});

app.listen(3013);
