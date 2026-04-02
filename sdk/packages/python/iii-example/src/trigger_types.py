"""Examples demonstrating trigger type configuration formats and listing.

Shows three patterns:
1. Custom trigger type with Pydantic models — returns a typed handle
2. Built-in trigger types (cron, state, subscribe) with dict config
3. Listing all available trigger types with their schemas
"""

from typing import Any

from iii import IIIClient, RegisterTriggerTypeInput, TriggerConfig, TriggerHandler
from pydantic import BaseModel, Field

# ── Webhook trigger type ─────────────────────────────────────────────────


class WebhookTriggerConfig(BaseModel):
    """Configuration for registering a webhook trigger."""

    url: str = Field(description="URL path for incoming webhooks")
    secret: str | None = Field(default=None, description="HMAC secret for signature verification")
    methods: list[str] | None = Field(default=None, description="HTTP methods to accept")


class WebhookCallRequest(BaseModel):
    """Payload received by functions when a webhook fires."""

    method: str = Field(description="HTTP method of the incoming webhook")
    headers: dict[str, str] = Field(default_factory=dict, description="Request headers")
    body: Any = Field(default=None, description="Request body")
    signature_verified: bool = Field(default=False, description="Whether HMAC was verified")


class WebhookHandler(TriggerHandler):
    async def register_trigger(self, config: TriggerConfig) -> None:
        print(f"[webhook] Registered trigger {config.id} -> {config.function_id}")

    async def unregister_trigger(self, config: TriggerConfig) -> None:
        print(f"[webhook] Unregistered trigger {config.id}")


# ── Schedule trigger type ────────────────────────────────────────────────


class ScheduleTriggerConfig(BaseModel):
    """Configuration for registering a schedule trigger."""

    at: str = Field(description="ISO 8601 datetime for when to fire")
    timezone: str | None = Field(default=None, description="Timezone (defaults to UTC)")
    repeat_daily: bool | None = Field(default=None, description="Repeat at the same time daily")


class ScheduleCallRequest(BaseModel):
    """Payload received by functions when a schedule fires."""

    scheduled_at: str = Field(description="The scheduled datetime that triggered this")
    fired_at: str = Field(description="Actual firing time")
    fire_count: int = Field(description="How many times this schedule has fired")


class ScheduleHandler(TriggerHandler):
    async def register_trigger(self, config: TriggerConfig) -> None:
        print(f"[schedule] Registered: {config.config}")

    async def unregister_trigger(self, config: TriggerConfig) -> None:
        pass


# ── Setup ────────────────────────────────────────────────────────────────


def setup(iii: IIIClient) -> None:
    # ── 1. Webhook: typed handle with register_function + register_trigger ──

    webhook = iii.register_trigger_type(
        RegisterTriggerTypeInput(
            id="webhook",
            description="Incoming webhook trigger",
            trigger_request_format=WebhookTriggerConfig,
            call_request_format=WebhookCallRequest,
        ),
        WebhookHandler(),
    )

    # register_function on the handle: handler receives WebhookCallRequest
    async def handle_webhook(data: WebhookCallRequest) -> dict:
        return {"processed": True, "method": data.method}

    webhook.register_function("example::webhook_handler", handle_webhook)

    # register_trigger on the handle: config is WebhookTriggerConfig
    webhook.register_trigger(
        "example::webhook_handler",
        WebhookTriggerConfig(
            url="/hooks/my-service",
            secret="my-secret-key",
            methods=["POST", "PUT"],
        ),
    )

    # ── 2. Schedule: same pattern ────────────────────────────────────

    schedule = iii.register_trigger_type(
        {
            "id": "schedule",
            "description": "One-time or daily scheduled trigger",
            "trigger_request_format": ScheduleTriggerConfig,
            "call_request_format": ScheduleCallRequest,
        },
        ScheduleHandler(),
    )

    async def handle_schedule(data: ScheduleCallRequest) -> dict:
        return {"sent": True, "scheduled_at": data.scheduled_at}

    schedule.register_function("example::send_report", handle_schedule)

    schedule.register_trigger(
        "example::send_report",
        ScheduleTriggerConfig(
            at="2026-03-25T09:00:00Z",
            timezone="America/Sao_Paulo",
            repeat_daily=True,
        ),
    )

    # ── 3. Built-in trigger types ────────────────────────────────────

    async def handle_cleanup(data: dict) -> dict:
        return {"cleaned": True, "job_id": data.get("job_id")}

    iii.register_function("example::scheduled_cleanup", handle_cleanup)
    iii.register_trigger(
        {
            "type": "cron",
            "function_id": "example::scheduled_cleanup",
            "config": {"expression": "0 * * * * *"},
        }
    )

    async def handle_user_updated(data: dict) -> dict:
        return {"processed": True, "event": data.get("event_type")}

    iii.register_function("example::on_user_updated", handle_user_updated)
    iii.register_trigger(
        {
            "type": "state",
            "function_id": "example::on_user_updated",
            "config": {"scope": "users"},
        }
    )

    async def handle_order(data: dict) -> dict:
        return {"processed": True, "order": data}

    iii.register_function("example::on_order_created", handle_order)
    iii.register_trigger(
        {
            "type": "subscribe",
            "function_id": "example::on_order_created",
            "config": {"topic": "orders.created"},
        }
    )


# ── List trigger types ───────────────────────────────────────────────────


def list_trigger_types_example(iii: IIIClient) -> None:
    """List all trigger types and print their schemas."""
    print("\n--- Listing all trigger types ---")

    trigger_types = iii.list_trigger_types()

    print(f"Found {len(trigger_types)} trigger types:\n")
    for tt in trigger_types:
        print(f"  [{tt.id}] {tt.description}")
        if tt.trigger_request_format:
            print(f"    trigger_request_format: {tt.trigger_request_format}")
        if tt.call_request_format:
            print(f"    call_request_format: {tt.call_request_format}")
        print()
