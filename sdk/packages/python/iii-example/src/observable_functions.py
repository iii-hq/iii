"""Examples of observable functions and observers.

Shows three patterns:
1. An event source with args: a webhook function whose observers name the path
   they receive, set up by ``on_observer_added`` and matched by key
2. A source that sets something up per observer: a schedule per ``at``
3. Observers of the built-in sources (cron, state, pubsub)
"""

from typing import Any

from iii import IIIClient, Observer, ObserveRejection
from pydantic import BaseModel, Field

# ── Webhook source ───────────────────────────────────────────────────────


class WebhookArgs(BaseModel):
    """What an observer of ``example::on-webhook`` asks for."""

    path: str = Field(description="URL path whose webhooks reach the observer")
    methods: list[str] | None = Field(default=None, description="HTTP methods to accept (all when absent)")


class WebhookEvent(BaseModel):
    """One incoming webhook: the function's input and what its observers observe."""

    path: str = Field(description="URL path the webhook arrived on")
    method: str = Field(description="HTTP method of the incoming webhook")
    headers: dict[str, str] = Field(default_factory=dict, description="Request headers")
    body: Any = Field(default=None, description="Request body")


# ── Schedule source ──────────────────────────────────────────────────────


class ScheduleArgs(BaseModel):
    """What an observer of ``example::on-schedule`` asks for."""

    at: str = Field(description="ISO 8601 datetime for when to fire")
    timezone: str | None = Field(default=None, description="Timezone (defaults to UTC)")
    repeat_daily: bool | None = Field(default=None, description="Repeat at the same time daily")


# ── Setup ────────────────────────────────────────────────────────────────


def setup(iii: IIIClient) -> None:
    # ── 1. Webhook: hooks by id, then the registration ──────────────────

    @iii.on_observer_added("example::on-webhook")
    def accept_route(obs: Observer) -> None:
        if not obs.args["path"].startswith("/"):
            raise ObserveRejection("INVALID_PATH", f"webhook paths start with '/': {obs.args['path']!r}")
        print(f"[webhook] {obs.target} receives {obs.args['path']}")

    @iii.on_observer_removed("example::on-webhook")
    def drop_route(obs: Observer) -> None:
        print(f"[webhook] {obs.target} no longer receives {obs.args['path']}")

    # each webhook reaches the observers of its path
    iii.on_observer_match(
        "example::on-webhook",
        observer=lambda obs: obs.args["path"],
        call=lambda call: call.input["path"],
    )

    # an event source returns the event it is called with
    iii.register_function(
        "example::on-webhook",
        lambda event: event,
        description="One call per incoming webhook. Observe it with args { path, methods? }.",
        request_format=WebhookEvent.model_json_schema(),
        response_format=WebhookEvent.model_json_schema(),
        args_format=WebhookArgs,
        private=True,
    )

    async def handle_webhook(envelope: dict[str, Any]) -> dict[str, Any]:
        return {"processed": True, "method": envelope["input"]["method"]}

    iii.register_function("example::webhook_handler", handle_webhook)
    iii.observe(
        {"function_id": "example::on-webhook", "args": {"path": "/hooks/my-service", "methods": ["POST", "PUT"]}},
        "example::webhook_handler",
    )

    # ── 2. Schedule: a job per observer, removed with it ────────────────

    jobs: dict[str, ScheduleArgs] = {}

    @iii.on_observer_added("example::on-schedule")
    def start_job(obs: Observer) -> None:
        jobs[obs.id] = ScheduleArgs(**obs.args)
        print(f"[schedule] {obs.target} at {obs.args['at']}")

    @iii.on_observer_removed("example::on-schedule")
    def stop_job(obs: Observer) -> None:
        jobs.pop(obs.id, None)

    iii.register_function(
        "example::on-schedule",
        lambda fired: fired,
        description="One call per scheduled time. Observe it with args { at, timezone?, repeat_daily? }.",
        args_format=ScheduleArgs,
        private=True,
    )

    async def handle_schedule(envelope: dict[str, Any]) -> dict[str, Any]:
        return {"sent": True, "scheduled_at": envelope["input"]["scheduled_at"]}

    iii.register_function("example::send_report", handle_schedule)
    iii.observe(
        {
            "function_id": "example::on-schedule",
            "args": {"at": "2026-03-25T09:00:00Z", "timezone": "America/Sao_Paulo", "repeat_daily": True},
        },
        "example::send_report",
    )

    # ── 3. Built-in sources ─────────────────────────────────────────────

    async def handle_cleanup(envelope: dict[str, Any]) -> dict[str, Any]:
        return {"cleaned": True, "scheduled_time": envelope["input"].get("scheduled_time")}

    iii.register_function("example::scheduled_cleanup", handle_cleanup)
    iii.observe({"function_id": "cron::on-tick", "args": {"expression": "0 * * * * *"}}, "example::scheduled_cleanup")

    async def handle_user_updated(change: dict[str, Any]) -> dict[str, Any]:
        return {"processed": True, "key": change.get("key")}

    iii.register_function("example::on_user_updated", handle_user_updated)
    iii.observe(
        {"function_id": "state::on-changed", "filters": {"input.scope": {"$eq": "users"}}, "deliver": "input"},
        "example::on_user_updated",
    )

    async def handle_order(published: dict[str, Any]) -> dict[str, Any]:
        return {"processed": True, "order": published["data"]}

    iii.register_function("example::on_order_created", handle_order)
    iii.observe(
        {"function_id": "pubsub::publish", "filters": {"input.topic": {"$eq": "orders.created"}}, "deliver": "input"},
        "example::on_order_created",
    )
