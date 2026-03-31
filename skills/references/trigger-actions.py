"""
Pattern: Trigger Actions (Invocation Modes)
Comparable to: Synchronous calls, async queues, fire-and-forget messaging

Every iii.trigger() call can specify an invocation mode via the `action`
parameter. There are exactly three modes:
  1. Synchronous (default) — blocks until the target returns a result.
  2. Fire-and-forget (TriggerAction.Void()) — returns None immediately.
  3. Enqueue (TriggerAction.Enqueue({ queue })) — durably enqueues and
     returns { messageReceiptId }.

This file shows each mode in isolation and then combines all three in a
realistic checkout workflow.

How-to references:
  - Trigger actions: https://iii.dev/docs/how-to/trigger-actions
"""

import asyncio
import os
import time

from iii import InitOptions, Logger, TriggerAction, register_worker

iii = register_worker(
    address=os.environ.get("III_ENGINE_URL", "ws://localhost:49134"),
    options=InitOptions(worker_name="trigger-actions"),
)

# ---
# Helper functions used by the examples below
# ---


async def validate_cart(data):
    logger = Logger()
    logger.info("Validating cart", {"cartId": data.get("cart_id")})

    items = data.get("items") or []
    if not items:
        return {"valid": False, "reason": "Cart is empty"}

    total = sum(i["price"] * i["qty"] for i in items)
    return {"valid": True, "cart_id": data["cart_id"], "total": total}


iii.register_function("checkout::validate-cart", validate_cart)


async def charge_payment(data):
    logger = Logger()
    logger.info("Charging payment", {"cart_id": data["cart_id"], "total": data["total"]})
    return {"charged": True, "transaction_id": f"txn_{int(time.time() * 1000)}"}


iii.register_function("checkout::charge-payment", charge_payment)


async def send_confirmation(data):
    logger = Logger()
    logger.info("Sending order confirmation email", {"email": data["email"]})
    return {"sent": True}


iii.register_function("checkout::send-confirmation", send_confirmation)

# ---
# Mode 1 — Synchronous (default)
# Blocks until the target function returns. The result is the function's
# return value. Use this when the caller needs the result to continue.
# ---


async def sync_call(data):
    logger = Logger()

    result = await iii.trigger_async({
        "function_id": "checkout::validate-cart",
        "payload": {"cart_id": data["cart_id"], "items": data["items"]},
    })

    logger.info("Sync result received", {"valid": result["valid"], "total": result.get("total")})
    return result


iii.register_function("examples::sync-call", sync_call)

# ---
# Mode 2 — Fire-and-forget (TriggerAction.Void())
# Returns None immediately. The target function runs asynchronously and its
# return value is discarded. Use for side-effects like logging, notifications,
# or analytics where the caller does not need to wait.
# ---


async def void_call(data):
    logger = Logger()

    iii.trigger({
        "function_id": "checkout::send-confirmation",
        "payload": {"email": data["email"], "order_id": data["order_id"]},
        "action": TriggerAction.Void(),
    })

    logger.info("Confirmation dispatched (fire-and-forget)")
    return {"dispatched": True}


iii.register_function("examples::void-call", void_call)

# ---
# Mode 3 — Enqueue (TriggerAction.Enqueue({ queue }))
# Durably enqueues the payload onto a named queue. Returns immediately with
# { messageReceiptId }. The target function processes the message when a
# worker picks it up. Use for work that must survive crashes and be retried.
# ---


async def enqueue_call(data):
    logger = Logger()

    receipt = await iii.trigger_async({
        "function_id": "checkout::charge-payment",
        "payload": {"cart_id": data["cart_id"], "total": data["total"]},
        "action": TriggerAction.Enqueue({"queue": "payments"}),
    })

    logger.info("Payment enqueued", {"messageReceiptId": receipt["messageReceiptId"]})
    return receipt


iii.register_function("examples::enqueue-call", enqueue_call)

# ---
# Realistic workflow — Checkout combining all three modes
#   1. Validate cart  (sync)    — need the result to decide whether to proceed
#   2. Charge payment (enqueue) — durable, retryable, must not be lost
#   3. Send email     (void)    — best-effort notification, don't block
# ---


async def checkout_process(data):
    logger = Logger()

    validation = await iii.trigger_async({
        "function_id": "checkout::validate-cart",
        "payload": {"cart_id": data["cart_id"], "items": data["items"]},
    })

    if not validation["valid"]:
        return {"error": validation["reason"]}

    receipt = await iii.trigger_async({
        "function_id": "checkout::charge-payment",
        "payload": {"cart_id": data["cart_id"], "total": validation["total"]},
        "action": TriggerAction.Enqueue({"queue": "payments"}),
    })

    logger.info("Payment queued", {"receiptId": receipt["messageReceiptId"]})

    iii.trigger({
        "function_id": "checkout::send-confirmation",
        "payload": {"email": data["email"], "order_id": data["cart_id"]},
        "action": TriggerAction.Void(),
    })

    return {
        "status": "accepted",
        "cart_id": data["cart_id"],
        "total": validation["total"],
        "payment_receipt": receipt["messageReceiptId"],
    }


iii.register_function("checkout::process", checkout_process)

iii.register_trigger({
    "type": "http",
    "function_id": "checkout::process",
    "config": {"api_path": "/checkout", "http_method": "POST"},
})


async def main():
    while True:
        await asyncio.sleep(60)


if __name__ == "__main__":
    asyncio.run(main())
