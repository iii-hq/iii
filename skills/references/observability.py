"""
Pattern: Observability
Comparable to: Datadog, Grafana, Honeycomb, OpenTelemetry SDK

iii has built-in OpenTelemetry support for traces, metrics, and logs.
The Python SDK provides get_context() for trace correlation and Logger
for structured logging. with_span and get_meter are JS SDK features
that do not exist in the Python SDK — this file uses what is actually
available.

How-to references:
  - Telemetry & observability: https://iii.dev/docs/advanced/telemetry
"""

import asyncio
import os
import signal
import time

from iii import InitOptions, Logger, TriggerAction, register_worker

# ---
# 1. SDK initialization with OpenTelemetry config
# ---
iii = register_worker(
    address=os.environ.get("III_ENGINE_URL", "ws://localhost:49134"),
    options=InitOptions(
        worker_name="observability",
        otel={
            "enabled": True,
            "service_name": "my-service",
            "service_version": "1.2.0",
            "metrics_enabled": True,
        },
    ),
)

# ---
# 2. Structured logging with trace correlation
# Logger automatically attaches trace/span IDs when otel is enabled.
# Use get_context() to read the current trace ID for manual correlation.
# ---


async def orders_process(data):
    logger = Logger()

    ctx = iii.get_context()
    logger.info("Processing order", {"orderId": data["order_id"], "traceId": ctx.get("trace_id")})

    items = data.get("items") or []
    if not items:
        raise Exception("Empty cart")

    item_count = len(items)
    logger.info("Validated order", {"orderId": data["order_id"], "itemCount": item_count})

    total = sum(item["price"] * item["qty"] for item in items)
    logger.info("Calculated total", {"orderId": data["order_id"], "total": total})

    await iii.trigger_async({
        "function_id": "state::set",
        "payload": {
            "scope": "orders",
            "key": data["order_id"],
            "value": {"_key": data["order_id"], "total": total, "status": "confirmed"},
        },
    })

    logger.info("Order persisted", {"orderId": data["order_id"], "total": total})
    return {"order_id": data["order_id"], "total": total, "validated": True}


iii.register_function("orders::process", orders_process)

iii.register_trigger({
    "type": "http",
    "function_id": "orders::process",
    "config": {"api_path": "/orders/process", "http_method": "POST"},
})

# ---
# 3. Metrics via structured logging
# The Python SDK does not expose get_meter(). Record metrics as structured
# log entries — the OTEL log exporter forwards them to your collector.
# ---


async def orders_with_metrics(data):
    logger = Logger()
    start = time.time()

    result = {"order_id": data["order_id"], "status": "complete"}

    elapsed_ms = (time.time() - start) * 1000
    logger.info("metric.orders.processed", {
        "status": "success",
        "region": data.get("region", "us-east-1"),
        "latency_ms": elapsed_ms,
        "endpoint": "/orders",
    })

    return result


iii.register_function("orders::with-metrics", orders_with_metrics)

# ---
# 4. Trace context propagation
# Use get_context() to read the current trace ID for correlation with
# external services.
# ---


async def call_external(data):
    logger = Logger()

    ctx = iii.get_context()
    trace_id = ctx.get("trace_id")
    logger.info("Current trace", {"traceId": trace_id})

    logger.info("Trace context available for propagation", {
        "traceId": trace_id,
        "userId": data.get("user_id"),
    })

    return {"traceId": trace_id, "propagated": True}


iii.register_function("orders::call-external", call_external)

# ---
# 5. Structured logging levels with trace correlation
# ---


async def log_demo(data):
    logger = Logger()

    logger.info("Processing request", {"requestId": data.get("id")})
    logger.warn("Slow query detected", {"query": data.get("query"), "duration_ms": 1200})
    logger.error("Unexpected state", {"expected": "active", "actual": data.get("status")})

    return {"logged": True}


iii.register_function("debug::log-demo", log_demo)

# ---
# 6. Clean shutdown — flush pending telemetry on process exit
# ---


def _shutdown(signum, frame):
    asyncio.get_event_loop().run_until_complete(iii.shutdown_otel())
    raise SystemExit(0)


signal.signal(signal.SIGTERM, _shutdown)
signal.signal(signal.SIGINT, _shutdown)


async def main():
    while True:
        await asyncio.sleep(60)


if __name__ == "__main__":
    asyncio.run(main())
