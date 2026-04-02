from motia.guards import (
    get_api_triggers,
    get_cron_triggers,
    get_queue_triggers,
    get_state_triggers,
    get_stream_triggers,
    is_api_step,
    is_api_trigger,
    is_cron_step,
    is_cron_trigger,
    is_queue_step,
    is_queue_trigger,
    is_state_step,
    is_state_trigger,
    is_stream_step,
    is_stream_trigger,
)
from motia.triggers import cron, http, queue, stream
from motia.types import Step, StepConfig, state


def test_trigger_guards_match_trigger_types() -> None:
    api_trigger = http("GET", "/items")
    queue_trigger = queue("items.created")
    cron_trigger = cron("* * * * *")
    state_trigger = state()
    stream_trigger = stream("items")

    assert is_api_trigger(api_trigger) is True
    assert is_queue_trigger(queue_trigger) is True
    assert is_cron_trigger(cron_trigger) is True
    assert is_state_trigger(state_trigger) is True
    assert is_stream_trigger(stream_trigger) is True


def test_step_guards_and_trigger_getters_return_expected_values() -> None:
    config = StepConfig(
        name="all-triggers",
        triggers=[
            http("GET", "/items"),
            queue("items.created"),
            cron("* * * * *"),
            state(),
            stream("items"),
        ],
    )
    step = Step(file_path="steps/example_step.py", config=config)

    assert is_api_step(step) is True
    assert is_queue_step(step) is True
    assert is_cron_step(step) is True
    assert is_state_step(step) is True
    assert is_stream_step(step) is True

    assert len(get_api_triggers(step)) == 1
    assert len(get_queue_triggers(step)) == 1
    assert len(get_cron_triggers(step)) == 1
    assert len(get_state_triggers(step)) == 1
    assert len(get_stream_triggers(step)) == 1

