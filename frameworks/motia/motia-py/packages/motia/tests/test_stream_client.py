import asyncio
import json
import sys
from types import SimpleNamespace
from typing import Any
from unittest.mock import AsyncMock, MagicMock

import pytest

from motia.stream_client import (
    JoinMessage,
    StreamClient,
    StreamGroupSubscription,
    StreamItemSubscription,
    StreamSubscription,
)


class FakeTask:
    def __init__(self) -> None:
        self.cancelled = False

    def cancel(self) -> None:
        self.cancelled = True


class FakeWebSocket:
    def __init__(self, received: list[str] | None = None) -> None:
        self.sent: list[str] = []
        self.received = received or []
        self.closed = False

    async def send(self, payload: str) -> None:
        self.sent.append(payload)

    async def recv(self) -> str:
        if not self.received:
            raise RuntimeError("no messages")
        return self.received.pop(0)

    async def close(self) -> None:
        self.closed = True


def test_stream_subscription_manages_state_events_and_close_callbacks() -> None:
    subscription = StreamSubscription(JoinMessage("todos", "group-1", "sub-1"), {"id": "initial"})
    changes: list[Any] = []
    events: list[Any] = []
    closed: list[str] = []

    subscription.add_change_listener(changes.append)
    subscription.on_event("ping", events.append)
    subscription.on_close(lambda: closed.append("closed"))

    subscription._set_state({"id": "updated"})
    subscription._on_event_received({"type": "ping", "data": {"ok": True}})
    subscription.off_event("ping", events.append)
    subscription.remove_change_listener(changes.append)
    subscription.close()

    assert subscription.get_state() == {"id": "updated"}
    assert changes == [{"id": "updated"}]
    assert events == [{"ok": True}]
    assert closed == ["closed"]

    with pytest.raises(NotImplementedError):
        subscription.listener({})


def test_stream_item_subscription_updates_state_for_supported_events() -> None:
    subscription = StreamItemSubscription(JoinMessage("todos", "group-1", "sub-1", "item-1"))
    received_events: list[Any] = []
    subscription.on_event("ping", received_events.append)

    subscription.listener({"timestamp": 1, "event": {"type": "sync", "data": {"id": "item-1", "value": 1}}})
    subscription.listener({"timestamp": 1, "event": {"type": "update", "data": {"id": "item-1", "value": 99}}})
    subscription.listener({"timestamp": 2, "event": {"type": "update", "data": {"id": "item-1", "value": 2}}})
    subscription.listener({"timestamp": 3, "event": {"type": "event", "event": {"type": "ping", "data": "pong"}}})
    subscription.listener({"timestamp": 4, "event": {"type": "delete"}})

    assert subscription.get_state() is None
    assert received_events == ["pong"]


def test_stream_group_subscription_handles_sync_create_update_delete_and_event() -> None:
    subscription = StreamGroupSubscription(JoinMessage("todos", "group-1", "sub-1"), sort_key="name")
    received_events: list[Any] = []
    subscription.on_event("ping", received_events.append)

    subscription.listener(
        {
            "timestamp": 1,
            "event": {
                "type": "sync",
                "data": [
                    {"id": "2", "name": "bravo"},
                    {"id": "1", "name": "alpha"},
                ],
            },
        }
    )
    subscription.listener({"timestamp": 0, "event": {"type": "sync", "data": []}})
    subscription.listener({"timestamp": 2, "event": {"type": "create", "data": {"id": "3", "name": "charlie"}}})
    subscription.listener({"timestamp": 3, "event": {"type": "update", "data": {"id": "2", "name": "zulu"}}})
    subscription.listener({"timestamp": 3, "event": {"type": "update", "data": {"id": "2", "name": "ignored"}}})
    subscription.listener({"timestamp": 4, "event": {"type": "delete", "data": {"id": "1"}}})
    subscription.listener({"timestamp": 5, "event": {"type": "event", "event": {"type": "ping", "data": "pong"}}})
    subscription.listener({"timestamp": 6, "event": {"type": "update", "data": None}})
    subscription.listener({"timestamp": 7, "event": {"type": "delete", "data": None}})

    assert subscription.get_state() == [
        {"id": "3", "name": "charlie"},
        {"id": "2", "name": "zulu"},
    ]
    assert received_events == ["pong"]


@pytest.mark.asyncio
async def test_stream_client_connect_send_receive_disconnect_and_rejoin(monkeypatch: pytest.MonkeyPatch) -> None:
    ws = FakeWebSocket([json.dumps({"hello": "world"})])
    task = FakeTask()
    client = StreamClient("ws://localhost:1234")
    client._rejoin_all = AsyncMock()

    def fake_create_task(coro: Any) -> FakeTask:
        coro.close()
        return task

    monkeypatch.setattr(asyncio, "create_task", fake_create_task)
    monkeypatch.setitem(sys.modules, "websockets", SimpleNamespace(connect=AsyncMock(return_value=ws)))

    await client.connect()
    await client._send({"type": "join"})
    received = await client._receive()
    await client.disconnect()

    assert client._rejoin_all.await_count == 1
    assert task.cancelled is True
    assert ws.closed is True
    assert json.loads(ws.sent[0]) == {"type": "join"}
    assert received == {"hello": "world"}


@pytest.mark.asyncio
async def test_stream_client_connect_requires_websockets(monkeypatch: pytest.MonkeyPatch) -> None:
    client = StreamClient("ws://localhost:1234")
    real_import = __import__

    def fake_import(name: str, *args: object, **kwargs: object):
        if name == "websockets":
            raise ImportError("missing")
        return real_import(name, *args, **kwargs)

    monkeypatch.setattr("builtins.__import__", fake_import)

    with pytest.raises(ImportError, match="websockets package is required"):
        await client.connect()


@pytest.mark.asyncio
async def test_stream_client_send_and_receive_require_connection() -> None:
    client = StreamClient("ws://localhost:1234")

    with pytest.raises(RuntimeError, match="Not connected"):
        await client._send({"type": "join"})

    with pytest.raises(RuntimeError, match="Not connected"):
        await client._receive()


@pytest.mark.asyncio
async def test_stream_client_receive_loop_reconnects_after_error(monkeypatch: pytest.MonkeyPatch) -> None:
    client = StreamClient("ws://localhost:1234")
    messages = [
        {"streamName": "todos", "groupId": "group-1", "event": {"type": "sync", "data": []}},
        RuntimeError("boom"),
    ]
    dispatched: list[dict[str, Any]] = []

    async def fake_receive() -> dict[str, Any]:
        result = messages.pop(0)
        if isinstance(result, Exception):
            raise result
        return result

    monkeypatch.setattr(client, "_receive", fake_receive)
    monkeypatch.setattr(client, "_dispatch", dispatched.append)
    client._reconnect = AsyncMock()

    await client._receive_loop()

    assert dispatched == [{"streamName": "todos", "groupId": "group-1", "event": {"type": "sync", "data": []}}]
    assert client._reconnect.await_count == 1


@pytest.mark.asyncio
async def test_stream_client_reconnect_retries_until_connect_succeeds(monkeypatch: pytest.MonkeyPatch) -> None:
    client = StreamClient("ws://localhost:1234", reconnect_delay=0.01)
    attempts = {"count": 0}

    async def fake_connect() -> None:
        attempts["count"] += 1
        if attempts["count"] < 3:
            raise RuntimeError("retry")

    client.disconnect = AsyncMock()
    monkeypatch.setattr(client, "connect", fake_connect)
    monkeypatch.setattr("asyncio.sleep", AsyncMock())

    await client._reconnect()

    assert client.disconnect.await_count == 1
    assert attempts["count"] == 3


def test_stream_client_dispatch_forwards_to_item_and_group_subscriptions() -> None:
    client = StreamClient("ws://localhost:1234")

    class Listener:
        def __init__(self) -> None:
            self.listener = MagicMock()

    item_listener = Listener()
    group_listener = Listener()
    sync_group_listener = Listener()

    item_room = client._room_name({"streamName": "todos", "groupId": "group-1", "id": "item-1"})
    group_room = client._room_name({"streamName": "todos", "groupId": "group-1"})
    client._listeners = {
        item_room: {item_listener},
        group_room: {group_listener, sync_group_listener},
    }

    message = {
        "streamName": "todos",
        "groupId": "group-1",
        "id": "item-1",
        "event": {"type": "update", "data": {"id": "item-1"}},
    }
    client._dispatch(message)

    assert item_listener.listener.call_count == 1
    assert group_listener.listener.call_count == 1

    sync_message = {
        "streamName": "todos",
        "groupId": "group-1",
        "id": "item-1",
        "event": {"type": "sync", "data": {"id": "item-1"}},
    }
    client._dispatch(sync_message)

    assert sync_group_listener.listener.call_count == 1


@pytest.mark.asyncio
async def test_stream_client_rejoin_join_leave_and_subscribe_lifecycle(monkeypatch: pytest.MonkeyPatch) -> None:
    ws = FakeWebSocket()
    client = StreamClient("ws://localhost:1234")
    client._ws = ws
    scheduled: list[tuple[str, Any]] = []

    def fake_schedule(coro: Any, action: str) -> None:
        scheduled.append((action, coro))
        coro.close()

    monkeypatch.setattr(client, "_schedule", fake_schedule)

    item_subscription = StreamItemSubscription(JoinMessage("todos", "group-1", "sub-1", "item-1"))
    group_subscription = StreamGroupSubscription(JoinMessage("todos", "group-1", "sub-2"))
    client._listeners = {
        client._room_name({"streamName": "todos", "groupId": "group-1", "id": "item-1"}): {item_subscription},
        client._room_name({"streamName": "todos", "groupId": "group-1"}): {group_subscription},
    }
    client._join = AsyncMock()

    await client._rejoin_all()
    await StreamClient("ws://localhost:1234")._join(item_subscription)
    await StreamClient("ws://localhost:1234")._leave(item_subscription)
    await client._join(item_subscription)

    client._subscribe(item_subscription)
    room = client._room_name({"streamName": "todos", "groupId": "group-1", "id": "item-1"})
    assert item_subscription in client._listeners[room]
    item_subscription.close()
    assert item_subscription not in client._listeners.get(room, set())
    assert [action for action, _ in scheduled] == ["StreamClient.join", "StreamClient subscription close"]


@pytest.mark.asyncio
async def test_stream_client_subscribe_helpers_and_close(monkeypatch: pytest.MonkeyPatch) -> None:
    client = StreamClient("ws://localhost:1234")
    captured: list[tuple[str, str, str | None]] = []

    def fake_subscribe(subscription: Any) -> None:
        captured.append((subscription.sub.stream_name, subscription.sub.group_id, subscription.sub.id))

    monkeypatch.setattr(client, "_subscribe", fake_subscribe)
    item_subscription = client.subscribe_item("todos", "group-1", "item-1")
    group_subscription = client.subscribe_group("todos", "group-1", sort_key="name")

    assert item_subscription.sub.id == "item-1"
    assert group_subscription.get_state() == []
    assert captured == [("todos", "group-1", "item-1"), ("todos", "group-1", None)]

    scheduled: list[tuple[str, Any]] = []

    def fake_schedule(coro: Any, action: str) -> None:
        scheduled.append((action, coro))
        coro.close()

    monkeypatch.setattr(client, "_schedule", fake_schedule)
    closeable = MagicMock()
    client._listeners = {"todos:group:group-1": {closeable}}
    client.close()

    closeable.close.assert_called_once()
    assert client._listeners == {}
    assert scheduled[0][0] == "StreamClient.disconnect"


def test_stream_client_room_name_and_loop_errors() -> None:
    client = StreamClient("ws://localhost:1234")

    assert client._room_name({"streamName": "todos", "groupId": "group-1"}) == "todos:group:group-1"
    assert client._room_name({"streamName": "todos", "groupId": "group-1", "id": "item-1"}) == (
        "todos:group:group-1:item:item-1"
    )

    with pytest.raises(RuntimeError, match="requires a running event loop"):
        client._get_running_loop("StreamClient.subscribe_*")
