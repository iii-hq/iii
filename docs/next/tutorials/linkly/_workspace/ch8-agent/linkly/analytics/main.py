import os
from datetime import datetime, timezone

from iii import register_worker, InitOptions, Logger

worker = register_worker(
    os.environ.get("III_URL", "ws://localhost:49134"),
    InitOptions(worker_name="analytics"),
)
logger = Logger()

DB = "analytics"


def ensure_schema() -> None:
    """The analytics worker owns its own table, in its own database."""
    worker.trigger(
        {
            "function_id": "database::execute",
            "payload": {
                "db": DB,
                "sql": "CREATE TABLE IF NOT EXISTS daily_link_counts (day TEXT PRIMARY KEY, count INTEGER NOT NULL)",
            },
        }
    )


def on_link_created(data: dict) -> dict:
    """Runs whenever link publishes `link.created`. Counts links per day."""
    day = datetime.now(timezone.utc).strftime("%Y-%m-%d")
    worker.trigger(
        {
            "function_id": "database::execute",
            "payload": {
                "db": DB,
                "sql": "INSERT INTO daily_link_counts (day, count) VALUES (?, 1) "
                "ON CONFLICT(day) DO UPDATE SET count = count + 1",
                "params": [day],
            },
        }
    )
    logger.info(f"counted new link {data.get('code')} for {day}")
    return {"ok": True}


ensure_schema()

worker.register_function("analytics::on_link_created", on_link_created)
worker.register_trigger(
    {
        "type": "subscribe",
        "function_id": "analytics::on_link_created",
        "config": {"topic": "link.created"},
    }
)

print("Analytics worker started")
