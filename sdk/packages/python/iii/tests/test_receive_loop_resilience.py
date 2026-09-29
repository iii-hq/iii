"""iii-hq/iii#2180 regressions: the receive loop must never leave a zombie worker.

* A normal close (1000, 1001, or a close frame without a code, 1005: e.g. a
  reverse proxy reload, or the engine rejecting a session) ends ``async for``
  over the socket quietly, because websockets swallows ``ConnectionClosedOK``.
  The cleanup and reconnect lived only in ``except ConnectionClosed``, so the
  worker kept reporting ``connected`` and never reconnected.
* Any other exception raised while handling one frame (malformed JSON, a
  non-object frame, a result for an invocation whose caller already gave up)
  ended the loop with the socket still open: the worker stayed ``connected``
  but never read another message.

Most tests run a real ``III`` client against an in-process websockets server
that plays the engine; fixtures tear both down even when a test fails.
"""

import asyncio
import json
import threading
import time
from collections.abc import Awaitable, Callable, Iterator
from types import SimpleNamespace
from typing import Any

import pytest
import websockets
from iii_helpers.observability import ReconnectionConfig
from websockets.frames import Close

import iii.iii as iii_module
from iii.iii import III, _PendingInvocation
from iii.iii_constants import InitOptions

Frames = list[dict[str, Any]]
OnFirst = Callable[[Any, Frames], Awaitable[None]]


class _FakeEngine:
    """Records every JSON frame per connection; ``on_first`` scripts what the
    engine does to the first connection. ``stop`` closes the server and joins
    its thread."""

    def __init__(self, on_first: OnFirst | None = None) -> None:
        self.connections: list[Frames] = []
        self.close_codes: dict[int, int | None] = {}
        self.port = 0
        self._on_first = on_first
        self._changed = threading.Condition()
        self._loop: asyncio.AbstractEventLoop | None = None
        self._stop: asyncio.Event | None = None
        started = threading.Event()
        self._thread = threading.Thread(target=self._run, args=(started,), daemon=True)
        self._thread.start()
        assert started.wait(5), "fake engine did not start"

    def _run(self, started: threading.Event) -> None:
        async def main() -> None:
            self._loop = asyncio.get_running_loop()
            self._stop = asyncio.Event()
            server = await websockets.serve(self._handler, "127.0.0.1", 0)
            self.port = next(iter(server.sockets)).getsockname()[1]
            started.set()
            await self._stop.wait()
            server.close()
            try:
                await asyncio.wait_for(server.wait_closed(), 5)
            except asyncio.TimeoutError:
                pass  # asyncio.run cancels any handler still running

        asyncio.run(main())

    def stop(self) -> None:
        if self._thread.is_alive() and self._loop is not None and self._stop is not None:
            self._loop.call_soon_threadsafe(self._stop.set)
        self._thread.join(10)
        assert not self._thread.is_alive(), "fake engine thread did not stop"

    def _notify(self) -> None:
        with self._changed:
            self._changed.notify_all()

    def wait_for(self, predicate: Callable[[], bool], timeout: float = 10.0) -> bool:
        with self._changed:
            return self._changed.wait_for(predicate, timeout)

    async def _read(self, ws: Any, frames: Frames) -> None:
        try:
            async for raw in ws:
                frames.append(json.loads(raw))
                self._notify()
        except websockets.ConnectionClosed:
            pass

    async def _handler(self, ws: Any) -> None:
        idx = len(self.connections)
        frames: Frames = []
        self.connections.append(frames)
        self._notify()
        reader = asyncio.ensure_future(self._read(ws, frames))
        if idx == 0 and self._on_first is not None:
            await self._on_first(ws, frames)
        await reader
        self.close_codes[idx] = ws.close_code
        self._notify()


def _registered(frames: Frames, function_id: str) -> bool:
    return any(
        f.get("type") == "registerfunction" and function_id in (f.get("id"), f.get("function_id")) for f in frames
    )


def _answered(frames: Frames, invocation_id: str) -> bool:
    return any(f.get("type") == "invocationresult" and f.get("invocation_id") == invocation_id for f in frames)


async def _until(predicate: Callable[[], Any], timeout: float = 10.0) -> Any:
    deadline = time.monotonic() + timeout
    while not (value := predicate()):
        if time.monotonic() > deadline:
            raise TimeoutError("fake engine script timed out")
        await asyncio.sleep(0.01)
    return value


def _probe(invocation_id: str = "probe-1") -> str:
    return json.dumps(
        {
            "type": "invokefunction",
            "invocation_id": invocation_id,
            "function_id": "test::echo",
            "data": {"ping": 1},
        }
    )


@pytest.fixture
def quiet_otel(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr("iii_helpers.observability.telemetry.init_otel", lambda **kwargs: None)
    monkeypatch.setattr("iii_helpers.observability.telemetry.attach_event_loop", lambda loop: None)


@pytest.fixture
def start_engine() -> Iterator[Callable[..., _FakeEngine]]:
    engines: list[_FakeEngine] = []

    def start(on_first: OnFirst | None = None) -> _FakeEngine:
        engine = _FakeEngine(on_first)
        engines.append(engine)
        return engine

    yield start
    for engine in engines:
        engine.stop()


@pytest.fixture
def connect(quiet_otel: None, start_engine: Callable[..., _FakeEngine]) -> Iterator[Callable[[_FakeEngine], III]]:
    # Depends on start_engine so clients are shut down before engines stop.
    clients: list[III] = []

    def connect_to(engine: _FakeEngine) -> III:
        client = III(
            f"ws://127.0.0.1:{engine.port}",
            InitOptions(
                worker_name="receive-loop-test",
                enable_metrics_reporting=False,
                reconnection_config=ReconnectionConfig(initial_delay_ms=50, max_delay_ms=100, jitter_factor=0.0),
            ),
        )
        clients.append(client)
        client._wait_until_connected()
        client.register_function("test::echo", lambda data: data)
        # The client can report `connected` before the fake engine's handler
        # (another thread) has recorded the connection; wait for it so tests
        # can index engine.connections[0] safely.
        assert engine.wait_for(lambda: bool(engine.connections) and _registered(engine.connections[0], "test::echo")), (
            "the fake engine never saw the registration"
        )
        return client

    yield connect_to
    for client in clients:
        if client._thread.is_alive():  # shutdown() must not run twice
            client.shutdown()


@pytest.mark.parametrize("close", ["1000", "1001", "1005", "abort"])
def test_reconnects_and_reregisters_after_engine_close(
    close: str, start_engine: Callable[..., _FakeEngine], connect: Callable[[_FakeEngine], III]
) -> None:
    async def drop(ws: Any, frames: Frames) -> None:
        await _until(lambda: _registered(frames, "test::echo"))
        if close == "abort":
            ws.transport.abort()  # no close frame: surfaces as ConnectionClosedError
        elif close == "1005":
            await ws.close(code=None)  # empty close frame, like the engine's Close(None)
        else:
            await ws.close(code=int(close), reason="engine restart")

    engine = start_engine(drop)
    client = connect(engine)
    assert engine.wait_for(lambda: len(engine.connections) >= 2 and _registered(engine.connections[1], "test::echo")), (
        f"no reconnect after close={close} (state={client._connection_state!r})"
    )
    assert client._connected_event.wait(5)
    assert client._connection_state == "connected"


def test_bad_frames_do_not_stop_the_receive_loop(
    start_engine: Callable[..., _FakeEngine], connect: Callable[[_FakeEngine], III]
) -> None:
    async def garbage_then_probe(ws: Any, frames: Frames) -> None:
        await _until(lambda: _registered(frames, "test::echo"))
        await ws.send("not json")
        await ws.send("[]")
        await ws.send(
            json.dumps({"type": "triggerregistrationresult", "id": "t1", "trigger_type": "x", "error": "boom"})
        )
        await ws.send(_probe())

    engine = start_engine(garbage_then_probe)
    connect(engine)
    assert engine.wait_for(lambda: _answered(engine.connections[0], "probe-1")), (
        "the worker stopped reading after a bad frame"
    )
    assert len(engine.connections) == 1, "a bad frame must not drop the connection"


def test_late_result_after_caller_gave_up_is_dropped(
    start_engine: Callable[..., _FakeEngine], connect: Callable[[_FakeEngine], III]
) -> None:
    async def answer_late_then_probe(ws: Any, frames: Frames) -> None:
        invocation_id = await _until(
            lambda: next(
                (
                    f["invocation_id"]
                    for f in frames
                    if f.get("type") == "invokefunction" and f.get("function_id") == "test::slow"
                ),
                None,
            )
        )
        await asyncio.sleep(0.3)
        await ws.send(json.dumps({"type": "invocationresult", "invocation_id": invocation_id, "result": {}}))
        await ws.send(_probe())

    engine = start_engine(answer_late_then_probe)
    client = connect(engine)

    async def impatient() -> Any:
        # Shorter than the invocation timeout: cancels the SDK's own await.
        return await asyncio.wait_for(client.trigger_async({"function_id": "test::slow", "payload": {}}), 0.05)

    with pytest.raises(asyncio.TimeoutError):
        client._run_on_loop(impatient())
    assert not any(p.function_id == "test::slow" for p in client._pending.values()), (
        "a cancelled await must release its pending entry"
    )
    assert engine.wait_for(lambda: _answered(engine.connections[0], "probe-1")), (
        "the worker stopped reading after a late result"
    )


@pytest.mark.parametrize("failure", ["cancelled", "raises"])
def test_pending_entry_released_when_send_does_not_complete(failure: str) -> None:
    async def scenario() -> None:
        # Bare client: only what trigger_async touches before and during the send.
        client = III.__new__(III)
        client._options = InitOptions()
        client._fatal_error = None
        client._pending = {}
        client._loop = asyncio.get_running_loop()
        client._inject_traceparent = lambda: None
        client._inject_baggage = lambda: None
        entered = asyncio.Event()

        async def send(message: Any) -> None:
            entered.set()
            if failure == "raises":
                raise RuntimeError("send failed")
            await asyncio.Future()  # e.g. write backpressure that never clears

        client._send = send
        task = asyncio.create_task(client.trigger_async({"function_id": "test::slow", "payload": {}}))
        await entered.wait()
        if failure == "cancelled":
            assert len(client._pending) == 1
            task.cancel()
            with pytest.raises(asyncio.CancelledError):
                await task
        else:
            with pytest.raises(RuntimeError, match="send failed"):
                await task
        assert client._pending == {}

    asyncio.run(scenario())


def test_shutdown_closes_the_socket_and_does_not_reconnect(
    start_engine: Callable[..., _FakeEngine], connect: Callable[[_FakeEngine], III]
) -> None:
    engine = start_engine()
    client = connect(engine)
    assert engine.wait_for(lambda: _registered(engine.connections[0], "test::echo"))

    client.shutdown()

    # Cancelling the receive loop must leave `_ws` for shutdown to close.
    assert engine.wait_for(lambda: 0 in engine.close_codes), "shutdown left the socket open"
    assert engine.close_codes[0] == 1000
    time.sleep(0.3)
    assert len(engine.connections) == 1, "shutdown must not reconnect"


@pytest.mark.parametrize("outcome", ["result", "error"])
def test_response_for_an_already_settled_future_is_dropped(outcome: str) -> None:
    async def scenario() -> None:
        client = III.__new__(III)
        future: asyncio.Future[Any] = asyncio.get_running_loop().create_future()
        future.cancel()  # the caller stopped waiting; its finally has not run yet
        client._pending = {"inv-1": _PendingInvocation(future=future, function_id="test::slow")}
        if outcome == "result":
            client._handle_result("inv-1", {"ok": True}, None)
        else:
            client._handle_result("inv-1", None, {"code": "boom", "message": "late error"})
        assert client._pending == {}

    asyncio.run(scenario())


class _DeadWs:
    """Handshake done, then closed by the peer before the replay finished."""

    state = SimpleNamespace(name="CLOSING")

    async def send(self, payload: str) -> None:
        raise websockets.ConnectionClosedOK(Close(1001, "going away"), Close(1001, "going away"), True)

    async def close(self) -> None:
        self.state = SimpleNamespace(name="CLOSED")

    def __aiter__(self) -> "_DeadWs":
        return self

    async def __anext__(self) -> Any:
        raise StopAsyncIteration


class _LiveWs:
    def __init__(self) -> None:
        self.state = SimpleNamespace(name="OPEN")
        self.sent: Frames = []
        self._closed = asyncio.Event()

    async def send(self, payload: str) -> None:
        self.sent.append(json.loads(payload))

    async def close(self) -> None:
        self.state = SimpleNamespace(name="CLOSED")
        self._closed.set()

    def __aiter__(self) -> "_LiveWs":
        return self

    async def __anext__(self) -> Any:
        await self._closed.wait()
        raise StopAsyncIteration


def _wait(predicate: Callable[[], bool], timeout: float = 5.0) -> bool:
    deadline = time.monotonic() + timeout
    while not predicate():
        if time.monotonic() > deadline:
            return False
        time.sleep(0.01)
    return True


def _start_with_sockets(
    monkeypatch: pytest.MonkeyPatch, dead_count: int | None, max_retries: int = -1
) -> tuple[III, list[Any]]:
    """Client whose first ``dead_count`` sockets die between the handshake and
    the receive loop (every socket when None); later sockets stay open."""
    sockets: list[Any] = []
    release = threading.Event()

    async def fake_connect(url: str, **kwargs: Any) -> Any:
        while not release.is_set():
            await asyncio.sleep(0.01)
        ws = _DeadWs() if dead_count is None or len(sockets) < dead_count else _LiveWs()
        sockets.append(ws)
        return ws

    monkeypatch.setattr(iii_module.websockets, "connect", fake_connect)
    client = III(
        "ws://fake",
        InitOptions(
            worker_name="receive-loop-test",
            enable_metrics_reporting=False,
            reconnection_config=ReconnectionConfig(
                initial_delay_ms=10, max_delay_ms=20, jitter_factor=0.0, max_retries=max_retries
            ),
        ),
    )
    try:
        client.register_function("test::echo", lambda data: data)
        # Registered before the first socket exists, so _on_connected replays it
        # into the queue (the socket is not OPEN) and the flush then raises.
        assert _wait(lambda: "test::echo" in client._functions), "function not registered before connecting"
    except BaseException:
        client.shutdown()
        raise
    release.set()
    return client, sockets


@pytest.mark.parametrize("dead_count", [1, 2])
def test_socket_that_dies_during_the_registration_replay_is_dropped(
    dead_count: int, monkeypatch: pytest.MonkeyPatch, quiet_otel: None
) -> None:
    # A socket that dies between the handshake and the receive loop makes the
    # queue flush in _on_connected raise. Keeping it in `_ws` would stop the
    # reconnect loop with no receiver (the #2180 zombie by another path).
    # dead_count=1 fails on the initial connect; dead_count=2 also fails inside
    # _reconnect_loop, where _schedule_reconnect is a no-op and only `_ws = None`
    # keeps the loop going.
    client, sockets = _start_with_sockets(monkeypatch, dead_count)
    live = dead_count
    try:
        assert _wait(
            lambda: len(sockets) > live and client._ws is sockets[live] and client._connection_state == "connected"
        ), f"no reconnect after the socket died (sockets={len(sockets)}, state={client._connection_state!r})"
        assert _wait(lambda: _registered(sockets[live].sent, "test::echo")), "not re-registered on the new socket"
        assert _wait(lambda: client._reconnect_attempt == 0), "backoff not reset after a successful setup"
    finally:
        client.shutdown()


def test_sockets_that_die_during_setup_count_against_max_retries(
    monkeypatch: pytest.MonkeyPatch, quiet_otel: None
) -> None:
    client, sockets = _start_with_sockets(monkeypatch, dead_count=None, max_retries=1)
    try:
        assert _wait(lambda: client._connection_state == "failed"), (
            f"never gave up (sockets={len(sockets)}, state={client._connection_state!r})"
        )
        assert len(sockets) == 2  # the initial connect plus one retry
    finally:
        client.shutdown()
