"""Test multi-trigger registration."""

from typing import Any

from motia import (
    ApiResponse,
    ApiTrigger,
    CronTrigger,
    EventTrigger,
    FlowContext,
    StepConfig,
    TriggerInput,
    step_wrapper,
)

single_event_config = StepConfig(
    name="SingleEventTrigger",
    description="Test single event trigger",
    triggers=[
        EventTrigger(subscribes=["test.event"]),
    ],
    emits=["test.processed"],
)


async def single_event_handler(input: TriggerInput[Any], ctx: FlowContext[Any]) -> None:
    """Handle single event trigger."""
    ctx.logger.info("Single event trigger fired", {"data": input.data})


step_wrapper(single_event_config, __file__, single_event_handler)


single_api_config = StepConfig(
    name="SingleApiTrigger",
    description="Test single API trigger",
    triggers=[
        ApiTrigger(path="/test/single", method="GET"),
    ],
    emits=[],
)


async def single_api_handler(input: TriggerInput[Any], ctx: FlowContext[Any]) -> ApiResponse[Any]:
    """Handle single API trigger."""
    ctx.logger.info("Single API trigger fired")
    return ApiResponse(status=200, body={"message": "Single API trigger works"})


step_wrapper(single_api_config, __file__, single_api_handler)


single_cron_config = StepConfig(
    name="SingleCronTrigger",
    description="Test single cron trigger",
    triggers=[
        CronTrigger(expression="*/5 * * * *"),
    ],
    emits=[],
)


async def single_cron_handler(input: TriggerInput[Any], ctx: FlowContext[Any]) -> None:
    """Handle single cron trigger."""
    ctx.logger.info("Single cron trigger fired")


step_wrapper(single_cron_config, __file__, single_cron_handler)


dual_trigger_config = StepConfig(
    name="DualTrigger",
    description="Test dual trigger (event + API)",
    triggers=[
        EventTrigger(subscribes=["test.dual"]),
        ApiTrigger(path="/test/dual", method="POST"),
    ],
    emits=["test.dual.processed"],
)


async def dual_trigger_handler(input: TriggerInput[Any], ctx: FlowContext[Any]) -> Any:
    """Handle dual trigger."""
    ctx.logger.info("Dual trigger fired", {"data": input.data, "trigger": input.trigger.type})
    if input.trigger.type == "api":
        return ApiResponse(status=200, body={"message": "Dual trigger via API"})
    return None


step_wrapper(dual_trigger_config, __file__, dual_trigger_handler)


async def is_business_hours(
    input: TriggerInput[Any], ctx: FlowContext[Any], trigger: dict[str, Any]
) -> bool:
    """Check if current time is business hours."""
    from datetime import datetime

    now = datetime.now()
    return 9 <= now.hour < 17


triple_trigger_config = StepConfig(
    name="TripleTrigger",
    description="Test triple trigger (event + API + cron)",
    triggers=[
        EventTrigger(subscribes=["test.triple"]),
        ApiTrigger(path="/test/triple", method="POST", condition=is_business_hours),
        CronTrigger(expression="0 */2 * * *"),
    ],
    emits=["test.triple.processed"],
)


async def triple_trigger_handler(input: TriggerInput[Any], ctx: FlowContext[Any]) -> Any:
    """Handle triple trigger."""
    ctx.logger.info("Triple trigger fired", {"data": input.data, "trigger": input.trigger.type})
    if input.trigger.type == "api":
        return ApiResponse(status=200, body={"message": "Triple trigger via API"})
    return None


step_wrapper(triple_trigger_config, __file__, triple_trigger_handler)


multiple_events_config = StepConfig(
    name="MultipleEvents",
    description="Test multiple event triggers",
    triggers=[
        EventTrigger(subscribes=["test.event.1"]),
        EventTrigger(subscribes=["test.event.2"]),
        EventTrigger(subscribes=["test.event.3"]),
    ],
    emits=["test.events.processed"],
)


async def multiple_events_handler(input: TriggerInput[Any], ctx: FlowContext[Any]) -> None:
    """Handle multiple event triggers."""
    ctx.logger.info("Multiple events trigger fired", {"data": input.data, "topic": input.trigger.topic})


step_wrapper(multiple_events_config, __file__, multiple_events_handler)


multiple_apis_config = StepConfig(
    name="MultipleApis",
    description="Test multiple API triggers",
    triggers=[
        ApiTrigger(path="/test/api/1", method="GET"),
        ApiTrigger(path="/test/api/2", method="POST"),
        ApiTrigger(path="/test/api/3", method="PUT"),
    ],
    emits=[],
)


async def multiple_apis_handler(input: TriggerInput[Any], ctx: FlowContext[Any]) -> ApiResponse[Any]:
    """Handle multiple API triggers."""
    ctx.logger.info("Multiple APIs trigger fired", {"path": input.trigger.path, "method": input.trigger.method})
    return ApiResponse(status=200, body={"message": "Multiple APIs trigger works"})


step_wrapper(multiple_apis_config, __file__, multiple_apis_handler)
