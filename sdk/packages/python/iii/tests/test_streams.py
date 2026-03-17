"""Integration tests for stream operations."""

import asyncio
import builtins
import time
from typing import Any

import pytest

from iii.iii import III
from iii.stream import IStream
from iii.stream import StreamDeleteInput, StreamDeleteResult, StreamGetInput, StreamListGroupsInput, StreamListInput, StreamSetInput, StreamSetResult, StreamUpdateInput, StreamUpdateResult

_list = list 

STREAM_NAME = "test-stream-py"
GROUP_ID = "test-group"
ITEM_ID = "test-item"


@pytest.fixture(autouse=True)
def cleanup_stream(iii_client: III):
    iii_client.trigger({
        "function_id": "stream::delete",
        "payload": {"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": ITEM_ID},
    })
    yield
    iii_client.trigger({
        "function_id": "stream::delete",
        "payload": {"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": ITEM_ID},
    })


@pytest.mark.asyncio
async def test_stream_set_new_item(iii_client: III):
    """Setting a new stream item returns old_value=null and new_value."""
    test_data = {"name": "Test Item", "value": 42}

    result = iii_client.trigger({
        "function_id": "stream::set",
        "payload": {
            "stream_name": STREAM_NAME,
            "group_id": GROUP_ID,
            "item_id": ITEM_ID,
            "data": test_data,
        },
    })

    assert result is not None
    assert result == {"old_value": None, "new_value": test_data}


@pytest.mark.asyncio
async def test_stream_set_overwrite(iii_client: III):
    """Overwriting a stream item returns the old and new values."""
    initial_data = {"value": 1}
    updated_data = {"value": 2, "updated": True}

    iii_client.trigger({
        "function_id": "stream::set",
        "payload": {"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": ITEM_ID, "data": initial_data},
    })

    result = iii_client.trigger({
        "function_id": "stream::set",
        "payload": {"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": ITEM_ID, "data": updated_data},
    })

    assert result["old_value"] == initial_data
    assert result["new_value"] == updated_data


@pytest.mark.asyncio
async def test_stream_get_existing_item(iii_client: III):
    """Getting an existing stream item returns its data."""
    test_data = {"name": "Test", "value": 100}

    iii_client.trigger({
        "function_id": "stream::set",
        "payload": {"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": ITEM_ID, "data": test_data},
    })

    result = iii_client.trigger({
        "function_id": "stream::get",
        "payload": {"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": ITEM_ID},
    })

    assert result == test_data


@pytest.mark.asyncio
async def test_stream_get_non_existent_item(iii_client: III):
    """Getting a non-existent stream item returns None."""
    result = iii_client.trigger({
        "function_id": "stream::get",
        "payload": {"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": "non-existent-item"},
    })

    assert result is None


@pytest.mark.asyncio
async def test_stream_delete_existing_item(iii_client: III):
    """Deleting an existing stream item removes it."""
    iii_client.trigger({
        "function_id": "stream::set",
        "payload": {"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": ITEM_ID, "data": {"test": True}},
    })
    iii_client.trigger({
        "function_id": "stream::delete",
        "payload": {"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": ITEM_ID},
    })

    result = iii_client.trigger({
        "function_id": "stream::get",
        "payload": {"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": ITEM_ID},
    })
    assert result is None


@pytest.mark.asyncio
async def test_stream_delete_non_existent_item(iii_client: III):
    """Deleting a non-existent stream item does not raise an error."""
    iii_client.trigger({
        "function_id": "stream::delete",
        "payload": {"stream_name": STREAM_NAME, "group_id": GROUP_ID, "item_id": "non-existent"},
    })


@pytest.mark.asyncio
async def test_stream_list_items_in_group(iii_client: III):
    """Listing stream items in a group returns all items set."""
    group_id = f"stream-py-{int(time.time())}"
    items = [
        {"id": "stream-item1", "value": 1},
        {"id": "stream-item2", "value": 2},
        {"id": "stream-item3", "value": 3},
    ]

    for item in items:
        iii_client.trigger({
            "function_id": "stream::set",
            "payload": {"stream_name": STREAM_NAME, "group_id": group_id, "item_id": item["id"], "data": item},
        })

    result = iii_client.trigger({
        "function_id": "stream::list",
        "payload": {"stream_name": STREAM_NAME, "group_id": group_id},
    })

    assert isinstance(result, list)
    assert len(result) >= len(items)
    sorted_result = sorted(result, key=lambda x: x["id"])
    sorted_items = sorted(items, key=lambda x: x["id"])
    assert sorted_result == sorted_items


@pytest.mark.asyncio
async def test_stream_custom_operations(iii_client: III):
    """Custom stream registered via create_stream routes operations to the implementation."""
    stream_name = f"test-stream-custom-py-{int(time.time())}"
    state: dict[str, Any] = {}

    class InMemoryStream(IStream):
        async def get(self, input: StreamGetInput) -> Any:
            return state.get(f"{input.group_id}::{input.item_id}")

        async def set(self, input: StreamSetInput) -> StreamSetResult:
            key = f"{input.group_id}::{input.item_id}"
            old_value = state.get(key)
            state[key] = input.data
            return StreamSetResult(old_value=old_value, new_value=input.data)

        async def delete(self, input: StreamDeleteInput) -> StreamDeleteResult:
            old = state.pop(f"{input.group_id}::{input.item_id}", None)
            return StreamDeleteResult(old_value=old)

        async def list(self, input: StreamListInput) -> list:
            prefix = f"{input.group_id}::"
            return [v for k, v in state.items() if k.startswith(prefix)]

        async def list_groups(self, input: StreamListGroupsInput) -> _list[str]:
            return list(state.keys())

        async def update(self, input: StreamUpdateInput) -> StreamUpdateResult | None:
            raise NotImplementedError

    iii_client.create_stream(stream_name, InMemoryStream())
    await asyncio.sleep(1.0)

    test_data = {"name": "Test", "value": 100}
    get_args = {"stream_name": stream_name, "group_id": GROUP_ID, "item_id": ITEM_ID}

    iii_client.trigger({"function_id": "stream::set", "payload": {**get_args, "data": test_data}})

    assert state.get(f"{GROUP_ID}::{ITEM_ID}") == test_data

    result = iii_client.trigger({"function_id": "stream::get", "payload": get_args})
    assert result == test_data

    iii_client.trigger({"function_id": "stream::delete", "payload": get_args})
    result_after_delete = iii_client.trigger({"function_id": "stream::get", "payload": get_args})
    assert result_after_delete is None
