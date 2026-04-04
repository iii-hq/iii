"""WebSocket-based stream client for consuming Motia streams."""

from __future__ import annotations

import asyncio
import json
import logging
from dataclasses import dataclass
from typing import Any, Callable, Coroutine

log = logging.getLogger("motia.stream_client")


@dataclass(frozen=True)
class JoinMessage:
    stream_name: str
    group_id: str
    subscription_id: str
    id: str | None = None


class StreamSubscription:
    """Base stream subscription with state and event listeners."""

    def __init__(self, sub: JoinMessage, state: Any) -> None:
        self.sub = sub
        self._state = state
        self._change_listeners: set[Callable[[Any], None]] = set()
        self._event_listeners: dict[str, list[Callable[[Any], None]]] = {}
        self._close_listeners: set[Callable[[], None]] = set()

    def listener(self, message: dict[str, Any]) -> None:  # pragma: no cover - implemented by subclasses
        raise NotImplementedError

    def add_change_listener(self, listener: Callable[[Any], None]) -> None:
        self._change_listeners.add(listener)

    def remove_change_listener(self, listener: Callable[[Any], None]) -> None:
        self._change_listeners.discard(listener)

    def on_event(self, event_type: str, listener: Callable[[Any], None]) -> None:
        self._event_listeners.setdefault(event_type, []).append(listener)

    def off_event(self, event_type: str, listener: Callable[[Any], None]) -> None:
        listeners = self._event_listeners.get(event_type, [])
        self._event_listeners[event_type] = [fn for fn in listeners if fn != listener]

    def on_close(self, listener: Callable[[], None]) -> None:
        self._close_listeners.add(listener)

    def close(self) -> None:
        for listener in list(self._close_listeners):
            listener()
        self._close_listeners.clear()

    def get_state(self) -> Any:
        return self._state

    def _set_state(self, state: Any) -> None:
        self._state = state
        for listener in list(self._change_listeners):
            listener(state)

    def _on_event_received(self, event: dict[str, Any]) -> None:
        event_type = event.get("type")
        if not event_type:
            return
        for listener in self._event_listeners.get(event_type, []):
            listener(event.get("data"))


class StreamItemSubscription(StreamSubscription):
    """Subscription for a single item."""

    def __init__(self, sub: JoinMessage) -> None:
        super().__init__(sub, None)
        self._last_timestamp = 0

    def listener(self, message: dict[str, Any]) -> None:
        timestamp = message.get("timestamp", 0)
        if timestamp <= self._last_timestamp:
            return
        self._last_timestamp = timestamp

        event = message.get("event", {})
        event_type = event.get("type")
        if event_type in {"sync", "create", "update"}:
            self._set_state(event.get("data"))
        elif event_type == "delete":
            self._set_state(None)
        elif event_type == "event":
            self._on_event_received(event.get("event", {}))


class StreamGroupSubscription(StreamSubscription):
    """Subscription for a group of items."""

    def __init__(self, sub: JoinMessage, sort_key: str | None = None) -> None:
        super().__init__(sub, [])
        self._last_timestamp = 0
        self._last_timestamp_map: dict[str, int] = {}
        self._sort_key = sort_key

    def _sort(self, state: list[dict[str, Any]]) -> list[dict[str, Any]]:
        if not self._sort_key:
            return state
        sort_key = self._sort_key
        return sorted(state, key=lambda item: str(item.get(sort_key, "")))

    def _set_state(self, state: Any) -> None:
        super()._set_state(self._sort(state))

    def listener(self, message: dict[str, Any]) -> None:
        timestamp = message.get("timestamp", 0)
        event = message.get("event", {})
        event_type = event.get("type")

        if event_type == "sync":
            if timestamp < self._last_timestamp:
                return
            self._last_timestamp = timestamp
            self._last_timestamp_map = {}
            self._set_state(event.get("data", []))
            return

        if event_type == "create":
            state = self.get_state()
            item = event.get("data")
            if item and not any(existing.get("id") == item.get("id") for existing in state):
                self._set_state(state + [item])
            return

        if event_type == "update":
            item = event.get("data")
            if not item:
                return
            item_id = item.get("id")
            current_ts = self._last_timestamp_map.get(item_id)
            if current_ts and current_ts >= timestamp:
                return
            self._last_timestamp = timestamp
            self._last_timestamp_map[item_id] = timestamp
            state = self.get_state()
            self._set_state([item if existing.get("id") == item_id else existing for existing in state])
            return

        if event_type == "delete":
            item = event.get("data")
            if not item:
                return
            item_id = item.get("id")
            self._last_timestamp = timestamp
            self._last_timestamp_map[item_id] = timestamp
            state = self.get_state()
            self._set_state([existing for existing in state if existing.get("id") != item_id])
            return

        if event_type == "event":
            self._on_event_received(event.get("event", {}))


class StreamClient:
    """Client for subscribing to Motia stream updates."""

    def __init__(self, url: str, reconnect_delay: float = 2.0) -> None:
        self.url = url
        self._ws: Any = None
        self._receive_task: asyncio.Task[None] | None = None
        self._listeners: dict[str, set[StreamSubscription]] = {}
        self._reconnect_delay = reconnect_delay
        self._connecting: asyncio.Lock = asyncio.Lock()

    def _get_running_loop(self, action: str) -> asyncio.AbstractEventLoop:
        try:
            return asyncio.get_running_loop()
        except RuntimeError as exc:
            raise RuntimeError(
                f"{action} requires a running event loop. "
                "Call it from within an async function and await "
                "client.connect()/client.disconnect() as needed."
            ) from exc

    def _schedule(self, coro: Coroutine[Any, Any, Any], action: str) -> None:
        loop = self._get_running_loop(action)
        loop.create_task(coro)

    async def connect(self) -> None:
        """Connect to the stream server."""
        async with self._connecting:
            if self._ws is not None:
                return

            try:
                import websockets

                self._ws = await websockets.connect(self.url)
                log.info("Connected to stream server: %s", self.url)
                self._receive_task = asyncio.create_task(self._receive_loop())
                await self._rejoin_all()
            except ImportError as exc:
                raise ImportError(
                    "websockets package is required for StreamClient. Install it with: pip install websockets"
                ) from exc

    async def disconnect(self) -> None:
        """Disconnect from the stream server."""
        if self._receive_task:
            self._receive_task.cancel()
            self._receive_task = None

        if self._ws:
            await self._ws.close()
            self._ws = None
            log.info("Disconnected from stream server")

    async def _send(self, message: dict[str, Any]) -> None:
        if not self._ws:
            raise RuntimeError("Not connected. Call connect() first.")
        await self._ws.send(json.dumps(message))

    async def _receive(self) -> dict[str, Any]:
        if not self._ws:
            raise RuntimeError("Not connected. Call connect() first.")
        data = await self._ws.recv()
        result: dict[str, Any] = json.loads(data)
        return result

    async def _receive_loop(self) -> None:
        while True:
            try:
                message = await self._receive()
                self._dispatch(message)
            except Exception as exc:
                log.debug("Stream receive loop ended: %s", exc)
                await self._reconnect()
                return

    async def _reconnect(self) -> None:
        await self.disconnect()
        while True:
            await asyncio.sleep(self._reconnect_delay)
            try:
                await self.connect()
                return
            except Exception as exc:
                log.debug("Reconnect failed: %s", exc)

    def _dispatch(self, message: dict[str, Any]) -> None:
        room = self._room_name(message)
        for listener in list(self._listeners.get(room, set())):
            listener.listener(message)

        if message.get("id") and message.get("event", {}).get("type") != "sync":
            group_room = self._room_name(
                {
                    "streamName": message.get("streamName"),
                    "groupId": message.get("groupId"),
                }
            )
            for listener in list(self._listeners.get(group_room, set())):
                listener.listener(message)

    async def _rejoin_all(self) -> None:
        for subscriptions in self._listeners.values():
            for subscription in subscriptions:
                await self._join(subscription)

    async def _join(self, subscription: StreamSubscription) -> None:
        if not self._ws:
            return
        await self._send(
            {
                "type": "join",
                "data": {
                    "streamName": subscription.sub.stream_name,
                    "groupId": subscription.sub.group_id,
                    "id": subscription.sub.id,
                    "subscriptionId": subscription.sub.subscription_id,
                },
            }
        )

    async def _leave(self, subscription: StreamSubscription) -> None:
        if not self._ws:
            return
        await self._send(
            {
                "type": "leave",
                "data": {
                    "streamName": subscription.sub.stream_name,
                    "groupId": subscription.sub.group_id,
                    "id": subscription.sub.id,
                    "subscriptionId": subscription.sub.subscription_id,
                },
            }
        )

    def _subscribe(self, subscription: StreamSubscription) -> None:
        self._get_running_loop("StreamClient.subscribe_*")
        room = self._room_name(
            {
                "streamName": subscription.sub.stream_name,
                "groupId": subscription.sub.group_id,
                "id": subscription.sub.id,
            }
        )
        self._listeners.setdefault(room, set()).add(subscription)

        if self._ws is None:
            self._schedule(self.connect(), "StreamClient.connect")

        def _on_close() -> None:
            listeners = self._listeners.get(room)
            if listeners is not None:
                listeners.discard(subscription)
                if not listeners:
                    self._listeners.pop(room, None)
            self._schedule(self._leave(subscription), "StreamClient subscription close")

        subscription.on_close(_on_close)
        self._schedule(self._join(subscription), "StreamClient.join")

    def subscribe_item(self, stream_name: str, group_id: str, item_id: str) -> StreamItemSubscription:
        """Subscribe to an item in a stream."""
        sub = JoinMessage(stream_name=stream_name, group_id=group_id, id=item_id, subscription_id=_uuid())
        subscription = StreamItemSubscription(sub)
        self._subscribe(subscription)
        return subscription

    def subscribe_group(self, stream_name: str, group_id: str, sort_key: str | None = None) -> StreamGroupSubscription:
        """Subscribe to a group in a stream."""
        sub = JoinMessage(stream_name=stream_name, group_id=group_id, id=None, subscription_id=_uuid())
        subscription = StreamGroupSubscription(sub, sort_key)
        self._subscribe(subscription)
        return subscription

    def close(self) -> None:
        """Close the client and all subscriptions."""
        self._get_running_loop("StreamClient.close")
        for subscriptions in list(self._listeners.values()):
            for subscription in list(subscriptions):
                subscription.close()
        self._listeners = {}
        self._schedule(self.disconnect(), "StreamClient.disconnect")

    @staticmethod
    def _room_name(message: dict[str, Any]) -> str:
        stream_name = message.get("streamName")
        group_id = message.get("groupId")
        item_id = message.get("id")
        if item_id:
            return f"{stream_name}:group:{group_id}:item:{item_id}"
        return f"{stream_name}:group:{group_id}"


def _uuid() -> str:
    import uuid

    return str(uuid.uuid4())
