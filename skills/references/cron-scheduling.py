"""
Pattern: Cron Scheduling
Comparable to: node-cron, APScheduler, crontab

Schedules recurring tasks using 7-field cron expressions:
  second  minute  hour  day  month  weekday  year

Cron handlers should be fast — enqueue heavy work to a queue.

How-to references:
  - Cron scheduling: https://iii.dev/docs/how-to/schedule-cron-task
"""

import asyncio
import os
import time
from datetime import datetime, timezone

from iii import InitOptions, Logger, TriggerAction, register_worker

iii = register_worker(
    address=os.environ.get("III_ENGINE_URL", "ws://localhost:49134"),
    options=InitOptions(worker_name="cron-scheduling"),
)

# ---
# Hourly cleanup — runs at the top of every hour
# Cron: 0 0 * * * * *  (second=0, minute=0, every hour)
# ---


async def hourly_cleanup(data):
    logger = Logger()
    logger.info("Hourly cleanup started")

    expired_items = await iii.trigger_async({
        "function_id": "state::list",
        "payload": {"scope": "sessions"},
    })

    now = int(time.time() * 1000)
    cleaned = 0

    for session in expired_items or []:
        last_active_ms = int(datetime.fromisoformat(session["last_active"]).timestamp() * 1000)
        age = now - last_active_ms
        if age > 3600000:
            iii.trigger({
                "function_id": "cleanup::process-expired",
                "payload": {"sessionId": session["id"]},
                "action": TriggerAction.Enqueue({"queue": "cleanup"}),
            })
            cleaned += 1

    logger.info("Hourly cleanup enqueued", {"cleaned": cleaned})
    return {"cleaned": cleaned}


iii.register_function("cron::hourly-cleanup", hourly_cleanup)

iii.register_trigger({
    "type": "cron",
    "function_id": "cron::hourly-cleanup",
    "config": {"expression": "0 0 * * * * *"},
})

# ---
# Daily report — runs at midnight every day
# Cron: 0 0 0 * * * *  (second=0, minute=0, hour=0, every day)
# ---


async def daily_report(data):
    logger = Logger()
    logger.info("Daily report generation started")

    metrics = await iii.trigger_async({
        "function_id": "state::get",
        "payload": {"scope": "daily-metrics", "key": "today"},
    })

    result = await iii.trigger_async({
        "function_id": "reports::generate",
        "payload": {
            "type": "daily-summary",
            "date": datetime.now(timezone.utc).isoformat().split("T")[0],
            "metrics": metrics or {"signups": 0, "orders": 0, "revenue": 0},
        },
        "action": TriggerAction.Enqueue({"queue": "reports"}),
    })

    logger.info("Daily report enqueued", {"messageReceiptId": result["messageReceiptId"]})

    await iii.trigger_async({
        "function_id": "state::set",
        "payload": {
            "scope": "daily-metrics",
            "key": "today",
            "value": {"signups": 0, "orders": 0, "revenue": 0, "reset_at": datetime.now(timezone.utc).isoformat()},
        },
    })

    return {"status": "enqueued"}


iii.register_function("cron::daily-report", daily_report)

iii.register_trigger({
    "type": "cron",
    "function_id": "cron::daily-report",
    "config": {"expression": "0 0 0 * * * *"},
})

# ---
# Health check — runs every 5 minutes
# Cron: 0 */5 * * * * *  (second=0, every 5th minute)
# ---


async def health_check(data):
    logger = Logger()
    timestamp = datetime.now(timezone.utc).isoformat()

    status = await iii.trigger_async({
        "function_id": "state::get",
        "payload": {"scope": "system", "key": "health"},
    })

    healthy = status.get("healthy", True) if isinstance(status, dict) else True

    await iii.trigger_async({
        "function_id": "state::set",
        "payload": {
            "scope": "system",
            "key": "health",
            "value": {"healthy": healthy, "checked_at": timestamp},
        },
    })

    if not healthy:
        logger.warn("Health check failed", {"timestamp": timestamp})

        iii.trigger({
            "function_id": "alerts::send",
            "payload": {"type": "health-check-failed", "timestamp": timestamp},
            "action": TriggerAction.Enqueue({"queue": "alerts"}),
        })

    return {"healthy": healthy, "checked_at": timestamp}


iii.register_function("cron::health-check", health_check)

iii.register_trigger({
    "type": "cron",
    "function_id": "cron::health-check",
    "config": {"expression": "0 */5 * * * * *"},
})

# ---
# Worker for enqueued cleanup tasks
# ---


async def process_expired(data):
    logger = Logger()

    await iii.trigger_async({
        "function_id": "state::delete",
        "payload": {"scope": "sessions", "key": data["sessionId"]},
    })

    logger.info("Expired session cleaned up", {"sessionId": data["sessionId"]})
    return {"deleted": data["sessionId"]}


iii.register_function("cleanup::process-expired", process_expired)


async def main():
    while True:
        await asyncio.sleep(60)


if __name__ == "__main__":
    asyncio.run(main())
