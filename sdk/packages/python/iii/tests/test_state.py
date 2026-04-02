"""Integration tests for state operations."""

import asyncio
import time

import pytest
import pytest_asyncio

from iii.iii import III

SCOPE = "test-scope-py"


def _unique_key(prefix: str) -> str:
    return f"{prefix}-{int(time.time() * 1000)}"


async def _poll(check, *, retries: int = 100, delay: float = 0.1):
    """Retry an assertion up to `retries` times with `delay` between attempts."""
    for attempt in range(retries):
        try:
            check()
            return
        except (AssertionError, Exception):
            if attempt == retries - 1:
                raise
            await asyncio.sleep(delay)


@pytest_asyncio.fixture(autouse=True)
async def cleanup_state(iii_client: III):
    yield


@pytest.mark.asyncio
async def test_state_set_new_item(iii_client: III):
    """Setting a new state item returns old_value=null and new_value."""
    key = _unique_key("set-new")
    test_data = {"name": "Test Item", "value": 42}

    result = iii_client.trigger(
        {
            "function_id": "state::set",
            "payload": {"scope": SCOPE, "key": key, "value": test_data},
        }
    )

    assert result is not None
    assert result == {"old_value": None, "new_value": test_data}


@pytest.mark.asyncio
async def test_state_set_overwrite(iii_client: III):
    """Overwriting a state item returns the old and new values."""
    key = _unique_key("set-overwrite")
    initial_data = {"value": 1}
    updated_data = {"value": 2, "updated": True}

    iii_client.trigger(
        {
            "function_id": "state::set",
            "payload": {"scope": SCOPE, "key": key, "value": initial_data},
        }
    )

    result = iii_client.trigger(
        {
            "function_id": "state::set",
            "payload": {"scope": SCOPE, "key": key, "value": updated_data},
        }
    )

    assert result["old_value"] == initial_data
    assert result["new_value"] == updated_data


@pytest.mark.asyncio
async def test_state_get_existing_item(iii_client: III):
    """Getting an existing state item returns its data."""
    key = _unique_key("get-existing")
    data = {"name": "Test", "value": 100}

    iii_client.trigger(
        {
            "function_id": "state::set",
            "payload": {"scope": SCOPE, "key": key, "value": data},
        }
    )

    result = iii_client.trigger(
        {
            "function_id": "state::get",
            "payload": {"scope": SCOPE, "key": key},
        }
    )

    assert result == data


@pytest.mark.asyncio
async def test_state_get_non_existent_item(iii_client: III):
    """Getting a non-existent state item returns None."""
    result = iii_client.trigger(
        {
            "function_id": "state::get",
            "payload": {"scope": SCOPE, "key": "non-existent-item"},
        }
    )

    assert result is None


@pytest.mark.asyncio
async def test_state_delete_existing_item(iii_client: III):
    """Deleting an existing state item removes it."""
    key = _unique_key("delete-existing")

    iii_client.trigger(
        {
            "function_id": "state::set",
            "payload": {"scope": SCOPE, "key": key, "value": {"test": True}},
        }
    )
    iii_client.trigger(
        {
            "function_id": "state::delete",
            "payload": {"scope": SCOPE, "key": key},
        }
    )

    result = iii_client.trigger(
        {
            "function_id": "state::get",
            "payload": {"scope": SCOPE, "key": key},
        }
    )
    assert result is None


@pytest.mark.asyncio
async def test_state_delete_non_existent_item(iii_client: III):
    """Deleting a non-existent state item does not raise an error."""
    iii_client.trigger(
        {
            "function_id": "state::delete",
            "payload": {"scope": SCOPE, "key": "non-existent"},
        }
    )


@pytest.mark.asyncio
async def test_state_list_all_items_in_scope(iii_client: III):
    """Listing state items in a scope returns all items set."""
    scope = f"state-py-{int(time.time() * 1000)}"
    items = [
        {"id": "state-item1", "value": 1},
        {"id": "state-item2", "value": 2},
        {"id": "state-item3", "value": 3},
    ]

    for item in items:
        iii_client.trigger(
            {
                "function_id": "state::set",
                "payload": {"scope": scope, "key": item["id"], "value": item},
            }
        )

    result = iii_client.trigger(
        {
            "function_id": "state::list",
            "payload": {"scope": scope},
        }
    )

    assert isinstance(result, list)
    assert len(result) >= len(items)
    sorted_result = sorted(result, key=lambda x: x["id"])
    sorted_items = sorted(items, key=lambda x: x["id"])
    assert sorted_result == sorted_items


@pytest.mark.asyncio
async def test_reactive_state(iii_client: III):
    """Registering a state trigger fires the handler when state is updated."""
    reactive_key = _unique_key("reactive")

    data = {"name": "Test", "value": 100}
    updated_data = {"name": "New Test Data", "value": 200}
    reactive_result: dict = {"called": False, "data": None}

    iii_client.trigger(
        {
            "function_id": "state::set",
            "payload": {"scope": SCOPE, "key": reactive_key, "value": data},
        }
    )

    async def state_updated_handler(event):
        if event.get("type") == "state" and event.get("event_type") == "state:updated":
            reactive_result["data"] = event.get("new_value")
            reactive_result["called"] = True
        return {}

    fn = iii_client.register_function({"id": "test.state.py.updated"}, state_updated_handler)
    trigger = iii_client.register_trigger({
        "type": "state",
        "function_id": fn.id,
        "config": {"scope": SCOPE, "key": reactive_key},
    })

    try:
        # Poll: re-trigger state::set each attempt until the handler fires.
        # Trigger registration is async so the first few sets may not be observed.
        for attempt in range(100):
            iii_client.trigger(
                {
                    "function_id": "state::set",
                    "payload": {"scope": SCOPE, "key": reactive_key, "value": updated_data},
                }
            )
            if reactive_result["called"]:
                break
            await asyncio.sleep(0.1)

        assert reactive_result["called"] is True
        assert reactive_result["data"] == updated_data
    finally:
        trigger.unregister()
        fn.unregister()
