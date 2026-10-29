"""A fake engine for the observe unit tests: an in-process websockets server
that records every frame the SDK sends and lets a test push engine frames.

It answers the SDK's ``engine::workers::register`` call with
``workerregistered`` (with the ``features`` of an observe v2 engine, of a 0.25
engine, none for a pre-0.25 engine, or not at all), the way the engine does
once a connection said who it is.
"""

from __future__ import annotations

import asyncio
import json
import threading
import time
import uuid
from collections.abc import Callable, Iterator
from typing import Any

import pytest
import websockets
from iii_helpers.observability import ReconnectionConfig

from iii.iii import III
from iii.iii_constants import InitOptions

Frame = dict[str, Any]

ENGINE_FEATURES = ["observe.v1", "observe.v2", "local-dispatch.v1"]
#: A 0.25 engine: observe, without observe.v2.
V1_FEATURES = ["observe.v1", "local-dispatch.v1"]


class FakeEngine:
    def __init__(self, features: list[str] | str | None = None, worker_id: str = "worker-1") -> None:
        #: ``ENGINE_FEATURES`` (default), ``V1_FEATURES``, ``"legacy"`` (no features: a
        #: pre-0.25 engine), or ``"silent"``.
        self.features: list[str] | str = ENGINE_FEATURES if features is None else features
        self.worker_id = worker_id
        self.connections: list[list[Frame]] = []
        self.sockets: list[Any] = []
        #: Barrier frames seen (``settle``); never in ``frames()``.
        self.barriers: set[str] = set()
        #: Called for every frame (on the engine thread); may return frames to send back.
        self.on_frame: Callable[[Frame], list[Frame] | None] | None = None
        self.port = 0
        self._changed = threading.Condition()
        self._loop: asyncio.AbstractEventLoop | None = None
        self._stop: asyncio.Event | None = None
        started = threading.Event()
        self._thread = threading.Thread(target=self._run, args=(started,), daemon=True)
        self._thread.start()
        assert started.wait(5), "fake engine did not start"

    @property
    def url(self) -> str:
        return f"ws://127.0.0.1:{self.port}"

    def _run(self, started: threading.Event) -> None:
        async def main() -> None:
            self._loop = asyncio.get_running_loop()
            self._stop = asyncio.Event()
            server = await websockets.serve(self._handler, "127.0.0.1", 0, max_size=None)
            self.port = next(iter(server.sockets)).getsockname()[1]
            started.set()
            await self._stop.wait()
            server.close()
            try:
                await asyncio.wait_for(server.wait_closed(), 5)
            except asyncio.TimeoutError:
                pass

        asyncio.run(main())

    def stop(self) -> None:
        if self._thread.is_alive() and self._loop is not None and self._stop is not None:
            self._loop.call_soon_threadsafe(self._stop.set)
        self._thread.join(10)

    async def _handler(self, ws: Any) -> None:
        frames: list[Frame] = []
        with self._changed:
            self.connections.append(frames)
            self.sockets.append(ws)
            self._changed.notify_all()
        try:
            async for raw in ws:
                frame = json.loads(raw)
                with self._changed:
                    if _is_barrier(frame):
                        self.barriers.add(_barrier_id(frame))
                    else:
                        frames.append(frame)
                    self._changed.notify_all()
                replies: list[Frame] = []
                if (
                    frame.get("type") == "invokefunction"
                    and frame.get("function_id") == "engine::workers::register"
                    and self.features != "silent"
                ):
                    registered: Frame = {
                        "type": "workerregistered",
                        "worker_id": self.worker_id,
                        "reattach_token": "token-1",
                    }
                    if self.features != "legacy":
                        registered["features"] = self.features
                    replies.append(registered)
                if self.on_frame is not None:
                    replies.extend(self.on_frame(frame) or [])
                for reply in replies:
                    await ws.send(json.dumps(reply))
        except websockets.ConnectionClosed:
            pass

    # ── test API ───────────────────────────────────────────────────────────

    def deliver(self, frame: Frame, connection: int = -1) -> None:
        """Sends one engine → worker frame on a connection (the last by default)."""
        assert self._loop is not None
        ws = self.sockets[connection]
        asyncio.run_coroutine_threadsafe(ws.send(json.dumps(frame)), self._loop).result(5)

    def drop(self, connection: int = -1) -> None:
        """Drops a connection the way a network failure does; the SDK reconnects."""
        assert self._loop is not None
        ws = self.sockets[connection]
        self._loop.call_soon_threadsafe(ws.transport.abort)

    def frames(self, connection: int | None = None) -> list[Frame]:
        with self._changed:
            if connection is None:
                return [f for frames in self.connections for f in frames]
            return list(self.connections[connection])

    def frames_of(self, type_: str, connection: int | None = None) -> list[Frame]:
        return [f for f in self.frames(connection) if f.get("type") == type_]

    def clear(self) -> None:
        with self._changed:
            for frames in self.connections:
                frames.clear()

    def wait_for(self, predicate: Callable[[], Any], timeout: float = 5.0, what: str = "a condition") -> Any:
        deadline = time.monotonic() + timeout
        while True:
            with self._changed:
                value = predicate()
                if value:
                    return value
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise AssertionError(f"timed out after {timeout}s waiting for {what}")
                self._changed.wait(min(remaining, 0.05))


_BARRIER_FN = "__barrier__::settle"


def _is_barrier(frame: Frame) -> bool:
    if frame.get("type") == "barrier":
        return True
    return frame.get("type") == "invocationresult" and frame.get("function_id") == _BARRIER_FN


def _barrier_id(frame: Frame) -> str:
    return str(frame.get("id") or frame.get("invocation_id"))


def settle(client: III, engine: FakeEngine) -> None:
    """A barrier, for asserting that something did *not* happen: every frame
    the engine delivered before was handled by the client, its observe hooks
    have run, buffered reports were flushed, and every frame the client sent
    since has reached the engine."""
    marker = uuid.uuid4().hex
    # engine → client: the client answers frames in order, so this answer comes after the earlier ones
    engine.deliver(
        {"type": "invokefunction", "invocation_id": f"in-{marker}", "function_id": _BARRIER_FN, "data": None}
    )
    engine.wait_for(lambda: f"in-{marker}" in engine.barriers, what="the client to read up to the barrier")

    async def drain() -> None:
        while True:  # hook queues may queue more work (removals)
            tails = [
                fn.observe._tail
                for fn in list(client._functions.values())
                if fn.observe is not None and fn.observe._tail is not None and not fn.observe._tail.done()
            ]
            if not tails:
                break
            await asyncio.wait(tails)
        client._reporter.flush()
        client._send_frame(json.dumps({"type": "barrier", "id": f"out-{marker}"}))

    client._run_on_loop(drain())
    # client → engine: frames arrive in order, so everything sent before the marker is recorded
    engine.wait_for(lambda: f"out-{marker}" in engine.barriers, what="the client's frames to arrive")


def loop_barrier(client: III) -> None:
    """Everything already handed to the client's SDK loop (``call_soon``,
    scheduled coroutines) has run."""

    async def noop() -> None:
        await asyncio.sleep(0)

    client._run_on_loop(noop())


def wait_until(predicate: Callable[[], Any], timeout: float = 5.0, what: str = "a condition") -> Any:
    deadline = time.monotonic() + timeout
    while True:
        value = predicate()
        if value:
            return value
        if time.monotonic() > deadline:
            raise AssertionError(f"timed out after {timeout}s waiting for {what}")
        time.sleep(0.005)


def function_registered(
    function_id: str,
    observers: list[Frame] | None = None,
    *,
    revision: int = 0,
    local_dispatch: bool = True,
    namespace: str = "default",
) -> Frame:
    return {
        "type": "functionregistered",
        "id": function_id,
        "namespace": namespace,
        "revision": revision,
        "local_dispatch": local_dispatch,
        "observers": observers or [],
    }


def observer_info(
    observer_id: str,
    target: str,
    *,
    args: Frame | None = None,
    filters: Frame | None = None,
    capture: list[str] | None = None,
    on: str = "success",
) -> Frame:
    return {
        "id": observer_id,
        "args": args or {},
        "filters": filters or {},
        "target": target,
        "capture": capture if capture is not None else ["input", "output", "caller"],
        "on": on,
    }


def observer_added(function_id: str, revision: int, observer: Frame, namespace: str = "default") -> Frame:
    return {
        "type": "observeradded",
        "function_id": function_id,
        "namespace": namespace,
        "revision": revision,
        "observer": observer,
    }


def observer_removed(function_id: str, revision: int, observer_id: str, namespace: str = "default") -> Frame:
    return {
        "type": "observerremoved",
        "function_id": function_id,
        "namespace": namespace,
        "revision": revision,
        "id": observer_id,
    }


def invoke(function_id: str, data: Any, invocation_id: str | None = None, **fields: Any) -> Frame:
    """A routed call as the engine forwards it (an ``invocation_id`` unless ``None`` is asked for)."""
    frame: Frame = {"type": "invokefunction", "function_id": function_id, "data": data, **fields}
    if invocation_id is not None:
        frame["invocation_id"] = invocation_id
    return frame


def answers(engine: FakeEngine, invocation_id: str) -> list[Frame]:
    """The ``invocationresult`` frames the SDK sent for one invocation."""
    return [f for f in engine.frames_of("invocationresult") if f.get("invocation_id") == invocation_id]


def rejected(function_id: str, code: str = "FUNCTION_NAMESPACE_CONFLICT", owner: str = "other") -> Frame:
    return {
        "type": "registrationrejected",
        "code": code,
        "namespace": "default",
        "function_id": function_id,
        "owner_worker_id": owner,
    }


def reported_items(engine: FakeEngine) -> list[Frame]:
    """The items of every ``invocationsobserved`` frame sent so far."""
    return [item for frame in engine.frames_of("invocationsobserved") for item in frame["items"]]


def acks(engine: FakeEngine) -> list[Frame]:
    return engine.frames_of("observerack")


@pytest.fixture
def quiet_otel(monkeypatch: pytest.MonkeyPatch) -> None:
    """No telemetry connection: the fake engine sees the SDK's main socket only."""
    monkeypatch.setattr("iii_helpers.observability.telemetry.init_otel", lambda **kwargs: None)
    monkeypatch.setattr("iii_helpers.observability.telemetry.attach_event_loop", lambda loop: None)


@pytest.fixture
def engine_factory() -> Iterator[Callable[..., FakeEngine]]:
    engines: list[FakeEngine] = []

    def start(features: list[str] | str | None = None) -> FakeEngine:
        engine = FakeEngine(features)
        engines.append(engine)
        return engine

    yield start
    for engine in engines:
        engine.stop()


@pytest.fixture
def engine(engine_factory: Callable[..., FakeEngine]) -> FakeEngine:
    return engine_factory()


@pytest.fixture
def connect(quiet_otel: None, engine_factory: Callable[..., FakeEngine]) -> Iterator[Callable[..., III]]:
    """Connects an ``III`` client to a fake engine and waits for ``workerregistered``."""
    clients: list[III] = []

    def connect_to(engine: FakeEngine, wait_registered: bool = True, **options: Any) -> III:
        opts = {
            "worker_name": "observe-test",
            "enable_metrics_reporting": False,
            "reconnection_config": ReconnectionConfig(initial_delay_ms=20, max_delay_ms=50, jitter_factor=0.0),
            **options,
        }
        accepted = len(engine.sockets)
        client = III(engine.url, InitOptions(**opts))
        clients.append(client)
        client._wait_until_connected()
        # The client can see its handshake complete before the engine's handler has filed the socket; deliver()
        # needs it filed.
        wait_until(lambda: len(engine.sockets) > accepted, what="the engine to accept the connection")
        if wait_registered:
            wait_until(lambda: client._registered_on_connection, what="workerregistered")
        return client

    yield connect_to
    for client in clients:
        if client._thread.is_alive():
            try:
                client.shutdown()
            except Exception:
                pass


def on_loop(client: III, fn: Callable[[], Any]) -> Any:
    """Runs ``fn`` on the client's SDK loop and returns its result."""

    async def run() -> Any:
        return fn()

    return client._run_on_loop(run())
