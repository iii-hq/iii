"""
Pattern: HTTP-Invoked Functions

Registers external HTTP endpoints as iii functions so the engine
calls them when triggered — no client-side HTTP code needed.
Combines with cron, state, and queue triggers for reactive integrations.

How-to references:
  - HTTP-invoked functions: https://iii.dev/docs/how-to/use-functions-and-triggers#http-invoked-functions
  - Engine config:         https://iii.dev/docs/how-to/configure-engine
  - State management:      https://iii.dev/docs/how-to/manage-state
  - Cron:                  https://iii.dev/docs/how-to/schedule-cron-task
  - Queues:                https://iii.dev/docs/how-to/use-queues

Prerequisites:
  - HttpFunctionsModule enabled in iii engine config
  - Env vars: SLACK_WEBHOOK_TOKEN, STRIPE_API_KEY, ORDER_WEBHOOK_SECRET
"""

import asyncio
import os
from datetime import datetime, timezone

from iii import InitOptions, Logger, TriggerAction, register_worker

iii = register_worker(
    address=os.environ.get("III_ENGINE_URL", "ws://localhost:49134"),
    options=InitOptions(worker_name="http-invoked-integrations"),
)

# ---
# Data-driven registration for immutable legacy endpoints
# ---
legacy_base_url = os.environ.get("LEGACY_API_URL", "https://legacy.internal.example.com")
legacy_endpoints = [
    {"path": "/webhook", "id": "legacy::webhook"},
    {"path": "/orders", "id": "legacy::orders"},
]

for ep in legacy_endpoints:
    iii.register_function(
        ep["id"],
        {
            "url": f"{legacy_base_url}{ep['path']}",
            "method": "POST",
            "timeout_ms": 8000,
        },
    )

# ---
# HTTP-invoked function: Slack webhook (bearer auth)
# ---
iii.register_function(
    "integrations::slack-notify",
    {
        "url": "https://hooks.slack.example.com/services/incoming",
        "method": "POST",
        "timeout_ms": 5000,
        "headers": {"Content-Type": "application/json"},
        "auth": {
            "type": "bearer",
            "token_key": "SLACK_WEBHOOK_TOKEN",
        },
    },
)

# ---
# HTTP-invoked function: Stripe charges (api_key auth)
# ---
iii.register_function(
    "integrations::stripe-charge",
    {
        "url": "https://api.stripe.example.com/v1/charges",
        "method": "POST",
        "timeout_ms": 10000,
        "headers": {"Content-Type": "application/x-www-form-urlencoded"},
        "auth": {
            "type": "api_key",
            "header_name": "Authorization",
            "value_key": "STRIPE_API_KEY",
        },
    },
)

# ---
# HTTP-invoked function: Analytics endpoint (no auth)
# ---
iii.register_function(
    "integrations::analytics-track",
    {
        "url": "https://analytics.internal.example.com/events",
        "method": "POST",
        "timeout_ms": 3000,
    },
)

# ---
# HTTP-invoked function: Order status webhook (hmac auth)
# ---
iii.register_function(
    "integrations::order-webhook",
    {
        "url": "https://fulfillment.partner.example.com/webhooks/orders",
        "method": "POST",
        "timeout_ms": 5000,
        "auth": {
            "type": "hmac",
            "secret_key": "ORDER_WEBHOOK_SECRET",
        },
    },
)

# ---
# Handler-based function that triggers HTTP-invoked functions
# ---


async def orders_process(data):
    logger = Logger()

    await iii.trigger_async({
        "function_id": "state::set",
        "payload": {"scope": "orders", "key": data["orderId"], "value": {**data, "status": "processing"}},
    })

    charge_result = await iii.trigger_async({
        "function_id": "integrations::stripe-charge",
        "payload": {"amount": data["amount"], "currency": "usd", "source": data["paymentToken"]},
    })

    logger.info("Payment charged", {"orderId": data["orderId"], "chargeId": charge_result["id"]})

    await iii.trigger_async({
        "function_id": "state::set",
        "payload": {"scope": "orders", "key": data["orderId"], "value": {**data, "status": "charged"}},
    })

    iii.trigger({
        "function_id": "integrations::slack-notify",
        "payload": {"text": f"Order {data['orderId']} charged ${data['amount']}"},
        "action": TriggerAction.Void(),
    })

    iii.trigger({
        "function_id": "integrations::analytics-track",
        "payload": {"event": "order.charged", "properties": {"orderId": data["orderId"], "amount": data["amount"]}},
        "action": TriggerAction.Void(),
    })

    return {"orderId": data["orderId"], "chargeId": charge_result["id"], "status": "charged"}


iii.register_function("orders::process", orders_process)

# ---
# Trigger: state change -> notify fulfillment partner via HTTP-invoked function
# ---
iii.register_trigger({
    "type": "state",
    "function_id": "integrations::order-webhook",
    "config": {"scope": "orders", "key": "status"},
})

# ---
# Trigger: scheduled analytics ping every hour
# ---


async def hourly_heartbeat(data):
    logger = Logger()
    worker_count = await iii.trigger_async({"function_id": "engine::workers::list", "payload": {}})

    await iii.trigger_async({
        "function_id": "integrations::analytics-track",
        "payload": {
            "event": "system.heartbeat",
            "properties": {"workers": len(worker_count), "timestamp": datetime.now(timezone.utc).isoformat()},
        },
    })

    logger.info("Hourly heartbeat sent")


iii.register_function("integrations::hourly-heartbeat", hourly_heartbeat)

iii.register_trigger({
    "type": "cron",
    "function_id": "integrations::hourly-heartbeat",
    "config": {"expression": "0 0 * * * * *"},
})

# ---
# Trigger: enqueue Stripe charges for reliable delivery with retries
# ---


async def orders_enqueue_charge(data):
    result = await iii.trigger_async({
        "function_id": "integrations::stripe-charge",
        "payload": {"amount": data["amount"], "currency": "usd", "source": data["paymentToken"]},
        "action": TriggerAction.Enqueue({"queue": "payments"}),
    })

    return {"messageReceiptId": result["messageReceiptId"]}


iii.register_function("orders::enqueue-charge", orders_enqueue_charge)


async def main():
    while True:
        await asyncio.sleep(60)


if __name__ == "__main__":
    asyncio.run(main())
