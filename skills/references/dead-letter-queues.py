"""
Pattern: Dead Letter Queues
Comparable to: SQS DLQ, RabbitMQ dead-letter exchanges, BullMQ failed jobs

When a queued function exhausts its retry budget (configured via
queue_configs.max_retries and backoff_ms in iii.config.yaml) the message
moves to the queue's dead-letter queue (DLQ). Messages in the DLQ can be
inspected and redriven back to the source queue via the SDK or CLI.

How-to references:
  - Dead letter queues: https://iii.dev/docs/how-to/dead-letter-queues
"""

import asyncio
import os
import random

from iii import InitOptions, Logger, TriggerAction, register_worker

iii = register_worker(
    address=os.environ.get("III_ENGINE_URL", "ws://localhost:49134"),
    options=InitOptions(worker_name="dead-letter-queues"),
)

# ---
# Queue configuration reference (iii.config.yaml)
#
#   queue_configs:
#     payment:
#       max_retries: 3        # after 3 failures the message goes to DLQ
#       backoff_ms: 1000      # exponential backoff base
#     email:
#       max_retries: 5
#       backoff_ms: 2000
# ---

# ---
# 1. Function that processes payments — may fail and exhaust retries
# After max_retries failures the message lands in the "payment" DLQ.
# ---


async def payments_charge(data):
    logger = Logger()
    logger.info("Attempting payment charge", {"orderId": data["order_id"]})

    gateway_up = random.random() > 0.7
    if not gateway_up:
        raise Exception("Payment gateway timeout — will be retried")

    logger.info("Payment succeeded", {"orderId": data["order_id"]})
    return {"charged": True, "order_id": data["order_id"]}


iii.register_function("payments::charge", payments_charge)

iii.register_trigger({
    "type": "durable:subscriber",
    "function_id": "payments::charge",
    "config": {"topic": "payment"},
})

# ---
# 2. Enqueue a payment to demonstrate the retry / DLQ flow
# ---


async def submit_payment(data):
    logger = Logger()

    order_id = data.get("order_id") if isinstance(data, dict) else None
    amount = data.get("amount") if isinstance(data, dict) else None
    if not order_id or amount is None:
        return {"status_code": 400, "body": {"error": "order_id and amount required"}}

    receipt = await iii.trigger_async({
        "function_id": "payments::charge",
        "payload": {"order_id": order_id, "amount": amount},
        "action": TriggerAction.Enqueue({"queue": "payment"}),
    })

    logger.info("Payment enqueued", {"receiptId": receipt["messageReceiptId"]})
    return receipt


iii.register_function("orders::submit-payment", submit_payment)

iii.register_trigger({
    "type": "http",
    "function_id": "orders::submit-payment",
    "config": {"api_path": "/orders/pay", "http_method": "POST"},
})

# ---
# 3. Redrive DLQ messages back to the source queue via SDK
# Calls the built-in iii::queue::redrive function. Returns the queue name
# and the count of redriven messages.
# ---


async def redrive_payments(data):
    logger = Logger()

    result = await iii.trigger_async({
        "function_id": "iii::queue::redrive",
        "payload": {"queue": "payment"},
    })

    logger.info("Redrive complete", {"queue": result["queue"], "redriven": result["redriven"]})
    return result


iii.register_function("admin::redrive-payments", redrive_payments)

iii.register_trigger({
    "type": "http",
    "function_id": "admin::redrive-payments",
    "config": {"api_path": "/admin/redrive/payments", "http_method": "POST"},
})

# ---
# CLI alternative for redrive (run from terminal):
#   iii trigger --function-id='iii::queue::redrive' --payload='{"queue": "payment"}'
#   iii trigger --function-id='iii::queue::redrive' --payload='{"queue": "payment"}' --timeout-ms=60000
# ---

# ---
# 4. DLQ inspection pattern — check how many messages are stuck
# ---


async def dlq_status(data):
    logger = Logger()

    queues = ["payment", "email"]
    statuses = []

    for queue in queues:
        info = await iii.trigger_async({
            "function_id": "iii::queue::status",
            "payload": {"queue": queue},
        })

        logger.info("Queue status", {"queue": queue, "dlq_count": info["dlq_count"], "pending": info["pending"]})
        statuses.append({"queue": queue, "dlq_count": info["dlq_count"], "pending": info["pending"]})

    return {"queues": statuses}


iii.register_function("admin::dlq-status", dlq_status)

iii.register_trigger({
    "type": "http",
    "function_id": "admin::dlq-status",
    "config": {"api_path": "/admin/dlq/status", "http_method": "GET"},
})

# ---
# 5. Targeted redrive — redrive a single queue from a cron schedule
# Useful for automatically retrying failed messages every hour.
# ---


async def auto_redrive(data):
    logger = Logger()

    result = await iii.trigger_async({
        "function_id": "iii::queue::redrive",
        "payload": {"queue": "payment"},
    })

    if result["redriven"] > 0:
        logger.info("Auto-redrive recovered messages", {"redriven": result["redriven"]})

    return result


iii.register_function("admin::auto-redrive", auto_redrive)

iii.register_trigger({
    "type": "cron",
    "function_id": "admin::auto-redrive",
    "config": {"expression": "0 0 * * * * *"},
})


async def main():
    while True:
        await asyncio.sleep(60)


if __name__ == "__main__":
    asyncio.run(main())
