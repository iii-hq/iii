"""Test trigger conditions."""

from typing import Any

from motia import (
    ApiRequest,
    ApiResponse,
    ApiTrigger,
    CronTrigger,
    EventTrigger,
    FlowContext,
    StepConfig,
    step_wrapper,
)


def is_high_value(input: Any, ctx: FlowContext[Any]) -> bool:
    """Check if order value is high."""
    data = input or {}
    amount = data.get("amount", 0) if isinstance(data, dict) else 0
    return amount > 1000


def is_verified_user(input: Any, ctx: FlowContext[Any]) -> bool:
    """Check if user is verified."""
    if ctx.trigger.type == "api" and isinstance(input, ApiRequest):
        body = input.body or {}
        user = body.get("user", {}) if isinstance(body, dict) else {}
        return user.get("verified", False)
    data = input or {}
    user = data.get("user", {}) if isinstance(data, dict) else {}
    return user.get("verified", False)


def is_domestic(input: Any, ctx: FlowContext[Any]) -> bool:
    """Check if order is domestic."""
    data = input or {}
    country = data.get("country") if isinstance(data, dict) else None
    return country in ["US", "CA"]


single_condition_config = StepConfig(
    name="SingleConditionTest",
    description="Test single condition on event trigger",
    triggers=[
        EventTrigger(
            subscribes=["order.created"],
            condition=is_high_value,
        ),
    ],
    emits=["order.processed"],
)


async def single_condition_handler(input: Any, ctx: FlowContext[Any]) -> None:
    """Handle orders that pass single condition."""
    ctx.logger.info("Processing high-value order", {"data": input})


step_wrapper(single_condition_config, __file__, single_condition_handler)


def all_premium_checks(input: Any, ctx: FlowContext[Any]) -> bool:
    """Combined check for premium orders."""
    return is_high_value(input, ctx) and is_verified_user(input, ctx) and is_domestic(input, ctx)


multiple_conditions_config = StepConfig(
    name="MultipleConditionsTest",
    description="Test multiple conditions (AND logic)",
    triggers=[
        EventTrigger(
            subscribes=["order.created"],
            condition=all_premium_checks,
        ),
    ],
    emits=["premium.order.processed"],
)


async def multiple_conditions_handler(input: Any, ctx: FlowContext[Any]) -> None:
    """Handle orders that pass all conditions."""
    ctx.logger.info("Processing premium order", {"data": input})


step_wrapper(multiple_conditions_config, __file__, multiple_conditions_handler)


def api_premium_check(input: Any, ctx: FlowContext[Any]) -> bool:
    """Combined check for API premium orders."""
    return is_high_value(input, ctx) and is_verified_user(input, ctx)


api_with_conditions_config = StepConfig(
    name="ApiWithConditions",
    description="Test conditions on API trigger",
    triggers=[
        ApiTrigger(
            path="/orders/premium",
            method="POST",
            condition=api_premium_check,
        ),
    ],
    emits=[],
)


async def api_with_conditions_handler(request: ApiRequest[Any], ctx: FlowContext[Any]) -> ApiResponse[Any]:
    """Handle API requests that pass conditions."""
    ctx.logger.info("Processing premium order via API")
    return ApiResponse(
        status=200, body={"message": "Premium order processed", "data": request.body}
    )


step_wrapper(api_with_conditions_config, __file__, api_with_conditions_handler)


async def is_business_hours(input: Any, ctx: FlowContext[Any]) -> bool:
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
            condition=is_business_hours,
        ),
    ],
    emits=[],
)


async def cron_with_condition_handler(input: None, ctx: FlowContext[Any]) -> None:
    """Handle cron that fires only during business hours."""
    ctx.logger.info("Running business hours task")


step_wrapper(cron_with_condition_config, __file__, cron_with_condition_handler)


mixed_triggers_with_conditions_config = StepConfig(
    name="MixedTriggersWithConditions",
    description="Test multiple triggers each with different conditions",
    triggers=[
        EventTrigger(
            subscribes=["order.created"],
            condition=is_high_value,
        ),
        ApiTrigger(
            path="/orders/manual",
            method="POST",
            condition=is_verified_user,
        ),
    ],
    emits=["order.processed"],
)


async def mixed_triggers_handler(input: Any, ctx: FlowContext[Any]) -> Any:
    """Handle orders from multiple triggers with different conditions."""
    ctx.logger.info("Processing order", {"data": input, "trigger": ctx.trigger.type})
    
    if ctx.trigger.type == "api":
        return ApiResponse(status=200, body={"message": "Order processed via API"})
    
    return None


step_wrapper(mixed_triggers_with_conditions_config, __file__, mixed_triggers_handler)
