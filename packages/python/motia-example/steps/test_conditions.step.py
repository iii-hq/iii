"""Test trigger conditions."""

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


def is_high_value(input: TriggerInput[Any], ctx: FlowContext[Any], trigger: dict[str, Any]) -> bool:
    """Check if order value is high."""
    data = input.data or {}
    amount = data.get("amount", 0) if isinstance(data, dict) else 0
    return amount > 1000


def is_verified_user(input: TriggerInput[Any], ctx: FlowContext[Any], trigger: dict[str, Any]) -> bool:
    """Check if user is verified."""
    data = input.data or {}
    user = data.get("user", {}) if isinstance(data, dict) else {}
    return user.get("verified", False)


def is_domestic(input: TriggerInput[Any], ctx: FlowContext[Any], trigger: dict[str, Any]) -> bool:
    """Check if order is domestic."""
    data = input.data or {}
    country = data.get("country") if isinstance(data, dict) else None
    return country in ["US", "CA"]


single_condition_config = StepConfig(
    name="SingleConditionTest",
    description="Test single condition on event trigger",
    triggers=[
        EventTrigger(
            subscribes=["order.created"],
            conditions=[is_high_value],
        ),
    ],
    emits=["order.processed"],
)


async def single_condition_handler(input: TriggerInput[Any], ctx: FlowContext[Any]) -> None:
    """Handle orders that pass single condition."""
    ctx.logger.info("Processing high-value order", {"data": input.data})


step_wrapper(single_condition_config, __file__, single_condition_handler)


multiple_conditions_config = StepConfig(
    name="MultipleConditionsTest",
    description="Test multiple conditions (AND logic)",
    triggers=[
        EventTrigger(
            subscribes=["order.created"],
            conditions=[is_high_value, is_verified_user, is_domestic],
        ),
    ],
    emits=["premium.order.processed"],
)


async def multiple_conditions_handler(input: TriggerInput[Any], ctx: FlowContext[Any]) -> None:
    """Handle orders that pass all conditions."""
    ctx.logger.info("Processing premium order", {"data": input.data})


step_wrapper(multiple_conditions_config, __file__, multiple_conditions_handler)


api_with_conditions_config = StepConfig(
    name="ApiWithConditions",
    description="Test conditions on API trigger",
    triggers=[
        ApiTrigger(
            path="/orders/premium",
            method="POST",
            conditions=[is_high_value, is_verified_user],
        ),
    ],
    emits=[],
)


async def api_with_conditions_handler(
    input: TriggerInput[Any], ctx: FlowContext[Any]
) -> ApiResponse[Any]:
    """Handle API requests that pass conditions."""
    ctx.logger.info("Processing premium order via API")
    return ApiResponse(
        status=200, body={"message": "Premium order processed", "data": input.data}
    )


step_wrapper(api_with_conditions_config, __file__, api_with_conditions_handler)


async def is_business_hours(
    input: TriggerInput[Any], ctx: FlowContext[Any], trigger: dict[str, Any]
) -> bool:
    """Check if current time is business hours."""
    from datetime import datetime

    now = datetime.now()
    return 9 <= now.hour < 17


cron_with_condition_config = StepConfig(
    name="CronWithCondition",
    description="Test condition on cron trigger",
    triggers=[
        CronTrigger(
            expression="*/5 * * * *",
            conditions=[is_business_hours],
        ),
    ],
    emits=[],
)


async def cron_with_condition_handler(input: TriggerInput[Any], ctx: FlowContext[Any]) -> None:
    """Handle cron that fires only during business hours."""
    ctx.logger.info("Running business hours task")


step_wrapper(cron_with_condition_config, __file__, cron_with_condition_handler)


mixed_triggers_with_conditions_config = StepConfig(
    name="MixedTriggersWithConditions",
    description="Test multiple triggers each with different conditions",
    triggers=[
        EventTrigger(
            subscribes=["order.created"],
            conditions=[is_high_value],
        ),
        ApiTrigger(
            path="/orders/manual",
            method="POST",
            conditions=[is_verified_user],
        ),
    ],
    emits=["order.processed"],
)


async def mixed_triggers_handler(input: TriggerInput[Any], ctx: FlowContext[Any]) -> Any:
    """Handle orders from multiple triggers with different conditions."""
    ctx.logger.info("Processing order", {"data": input.data, "trigger": input.trigger.type})
    
    if input.trigger.type == "api":
        return ApiResponse(status=200, body={"message": "Order processed via API"})
    
    return None


step_wrapper(mixed_triggers_with_conditions_config, __file__, mixed_triggers_handler)
