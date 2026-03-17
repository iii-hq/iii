"""Greetings summary step for API and cron."""

from typing import Any

from motia import ApiRequest, ApiResponse, FlowContext, Stream, cron, http, logger

GREETINGS_GROUP_ID = "default"
greetings_stream: Stream[dict[str, Any]] = Stream("greetings")


config = {
    "name": "GreetingsSummary",
    "description": "Summarize greetings via API or every 5 seconds",
    "triggers": [
        http("GET", "/greetings/summary"),
        cron("*/5 * * * * *"),
    ],
    "enqueues": [],
}


def handler(input_data: Any, ctx: FlowContext[Any]) -> Any:
    """Dispatch to the API or cron handler based on trigger type."""

    def _summary_api_handler(request: ApiRequest[Any]) -> ApiResponse[dict[str, Any]]:
        greetings = greetings_stream.get_group(GREETINGS_GROUP_ID)
        logger.info("Greetings summary requested", {"count": len(greetings)})
        return ApiResponse(status=200, body={"count": len(greetings), "greetings": greetings})

    def _summary_cron_handler() -> None:
        greetings = greetings_stream.get_group(GREETINGS_GROUP_ID)
        logger.info("Greetings summary (cron)", {"count": len(greetings)})

    return ctx.match(
        {
            "http": _summary_api_handler,
            "cron": _summary_cron_handler,
        },
    )
