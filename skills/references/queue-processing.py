"""
Pattern: Queue Processing
Comparable to: BullMQ, Celery, SQS

Enqueue work for durable, retryable async processing.
Standard queues process concurrently; FIFO queues preserve order.

Retry / backoff is configured in iii-config.yaml under queue_configs:
  queue_configs:
    - name: payment
      max_retries: 3
      backoff_ms: 1000
      backoff_multiplier: 2
    - name: email
      fifo: true
      max_retries: 5
      backoff_ms: 500

How-to references:
  - Queues: https://iii.dev/docs/how-to/use-queues
"""

import asyncio
import os
import time
from datetime import datetime, timezone

from iii import InitOptions, Logger, TriggerAction, register_worker

iii = register_worker(
    address=os.environ.get("III_ENGINE_URL", "ws://localhost:49134"),
    options=InitOptions(worker_name="queue-processing"),
)

# ---
# Enqueue work — standard queue (concurrent processing)
# ---


async def payments_submit(data):
    logger = Logger()

    try:
        result = await iii.trigger_async({
            "function_id": "payments::process",
            "payload": {
                "orderId": data["orderId"],
                "amount": data["amount"],
                "currency": data.get("currency", "usd"),
                "method": data["paymentMethod"],
            },
            "action": TriggerAction.Enqueue({"queue": "payment"}),
        })

        logger.info("Payment enqueued", {
            "orderId": data["orderId"],
            "messageReceiptId": result["messageReceiptId"],
        })

        return {"status": "queued", "messageReceiptId": result["messageReceiptId"]}
    except Exception as err:
        logger.error("Failed to enqueue payment", {"orderId": data["orderId"], "error": str(err)})
        raise


iii.register_function("payments::submit", payments_submit)

# ---
# Process payment — handler that runs from the queue
# ---


async def payments_process(data):
    logger = Logger()
    logger.info("Processing payment", {"orderId": data["orderId"], "amount": data["amount"]})

    charge_id = f"ch-{int(time.time() * 1000)}"

    await iii.trigger_async({
        "function_id": "state::set",
        "payload": {
            "scope": "payments",
            "key": data["orderId"],
            "value": {
                "orderId": data["orderId"],
                "chargeId": charge_id,
                "amount": data["amount"],
                "currency": data["currency"],
                "status": "captured",
                "processed_at": datetime.now(timezone.utc).isoformat(),
            },
        },
    })

    iii.trigger({
        "function_id": "notifications::send",
        "payload": {"type": "payment_captured", "orderId": data["orderId"], "chargeId": charge_id},
        "action": TriggerAction.Void(),
    })

    logger.info("Payment captured", {"orderId": data["orderId"], "chargeId": charge_id})
    return {"chargeId": charge_id, "status": "captured"}


iii.register_function("payments::process", payments_process)

# ---
# Enqueue work — FIFO queue (ordered processing)
# FIFO queues guarantee messages are processed in the order they arrive.
# Configure fifo: true in iii-config.yaml queue_configs.
# ---


async def emails_enqueue(data):
    logger = Logger()

    result = await iii.trigger_async({
        "function_id": "emails::send",
        "payload": {
            "to": data["to"],
            "subject": data["subject"],
            "body": data["body"],
            "template": data.get("template"),
        },
        "action": TriggerAction.Enqueue({"queue": "email"}),
    })

    logger.info("Email enqueued (FIFO)", {
        "to": data["to"],
        "messageReceiptId": result["messageReceiptId"],
    })

    return {"status": "queued", "messageReceiptId": result["messageReceiptId"]}


iii.register_function("emails::enqueue", emails_enqueue)

# ---
# Process email — FIFO handler preserves send order
# ---


async def emails_send(data):
    logger = Logger()
    logger.info("Sending email", {"to": data["to"], "subject": data["subject"]})

    message_id = f"msg-{int(time.time() * 1000)}"

    await iii.trigger_async({
        "function_id": "state::set",
        "payload": {
            "scope": "email-log",
            "key": message_id,
            "value": {
                "messageId": message_id,
                "to": data["to"],
                "subject": data["subject"],
                "status": "sent",
                "sent_at": datetime.now(timezone.utc).isoformat(),
            },
        },
    })

    logger.info("Email sent", {"messageId": message_id, "to": data["to"]})
    return {"messageId": message_id, "status": "sent"}


iii.register_function("emails::send", emails_send)

# ---
# Receipt capture — checking enqueue acknowledgement
# ---


async def orders_place(data):
    logger = Logger()

    payment_receipt = await iii.trigger_async({
        "function_id": "payments::process",
        "payload": {"orderId": data["orderId"], "amount": data["total"], "currency": "usd", "method": data["method"]},
        "action": TriggerAction.Enqueue({"queue": "payment"}),
    })

    email_receipt = await iii.trigger_async({
        "function_id": "emails::send",
        "payload": {"to": data["email"], "subject": "Order confirmed", "body": f"Order {data['orderId']}"},
        "action": TriggerAction.Enqueue({"queue": "email"}),
    })

    logger.info("Order placed", {
        "orderId": data["orderId"],
        "paymentReceipt": payment_receipt["messageReceiptId"],
        "emailReceipt": email_receipt["messageReceiptId"],
    })

    await iii.trigger_async({
        "function_id": "state::set",
        "payload": {
            "scope": "orders",
            "key": data["orderId"],
            "value": {
                "orderId": data["orderId"],
                "status": "pending",
                "paymentReceiptId": payment_receipt["messageReceiptId"],
                "emailReceiptId": email_receipt["messageReceiptId"],
            },
        },
    })

    return {
        "orderId": data["orderId"],
        "paymentReceiptId": payment_receipt["messageReceiptId"],
        "emailReceiptId": email_receipt["messageReceiptId"],
    }


iii.register_function("orders::place", orders_place)

# ---
# HTTP trigger to accept orders
# ---
iii.register_trigger({
    "type": "http",
    "function_id": "orders::place",
    "config": {"api_path": "/orders", "http_method": "POST"},
})


async def main():
    while True:
        await asyncio.sleep(60)


if __name__ == "__main__":
    asyncio.run(main())
