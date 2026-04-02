"""Test triple trigger (event + API + cron)."""

from typing import Any

from motia import ApiRequest, ApiResponse, FlowContext, cron, http, logger, queue


def is_business_hours(input: Any, ctx: FlowContext[Any]) -> bool:
    """Check if current time is business hours."""
    _ = input
    _ = ctx
    from datetime import datetime

    now = datetime.now()
    return 9 <= now.hour < 17


config = {
    "name": "TripleTrigger",
    "description": "Test triple trigger (event + API + cron)",
    "triggers": [
        queue("test.triple"),
        http("POST", "/test/triple", condition=is_business_hours),
        cron("0 2 * * * *"),
    ],
    "enqueues": ["test.triple.processed"],
}


def handler(input_data: Any, ctx: FlowContext[Any]) -> Any:
    """Dispatch triple trigger handlers."""

    def _event_handler(input: Any) -> None:
        logger.info("Triple trigger fired (queue)", {"data": input, "topic": ctx.trigger.topic})

    def _api_handler(request: ApiRequest[Any]) -> ApiResponse[Any]:
        logger.info("Triple trigger fired (api)", {"path": ctx.trigger.path, "method": ctx.trigger.method})
        return ApiResponse(status=200, body={"message": "Triple trigger via API"})

    def _cron_handler() -> None:
        logger.info("Triple trigger fired (cron)", {"expression": ctx.trigger.expression})

    return ctx.match(
        {
            "queue": _event_handler,
            "http": _api_handler,
            "cron": _cron_handler,
        },
    )
