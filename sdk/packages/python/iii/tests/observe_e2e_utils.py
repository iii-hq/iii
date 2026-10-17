"""Helpers for the observe tests that run against a real engine (``III_URL``)."""

from __future__ import annotations

import asyncio
import os
import time
import uuid
from collections.abc import Callable, Iterator
from dataclasses import dataclass
from typing import Any

import pytest
from iii_helpers.observability import ReconnectionConfig

from iii import InitOptions, InvocationContext
from iii.iii import III

ENGINE_WS_URL = os.environ.get("III_URL", "ws://localhost:49199")

#: The workers of the running test (``settle_engine`` drains them all).
_WORKERS: list[III] = []


@pytest.fixture
def worker() -> Iterator[Callable[..., III]]:
    """``worker(role)``: a worker of its own, shut down after the test."""
    workers: list[III] = []

    def make(role: str, **options: Any) -> III:
        client = III(
            ENGINE_WS_URL,
            InitOptions(
                worker_name=f"py-{role}-{uuid.uuid4().hex[:8]}",
                reconnection_config=ReconnectionConfig(
                    max_retries=20, initial_delay_ms=100, max_delay_ms=500, jitter_factor=0.0
                ),
                **options,
            ),
        )
        workers.append(client)
        _WORKERS.append(client)
        client._wait_until_connected()
        wait_for(lambda: client._registered_on_connection, "workerregistered")
        return client

    yield make
    for client in workers:
        _WORKERS.remove(client)
        try:
            client.shutdown()
        except Exception:
            pass


def flush_client(client: III) -> None:
    """Client barrier: buffered reports are flushed, and the frames the
    observe machinery handed to the socket (reports, acks, observe frames:
    ``_send_frame``) have been written. Calls made with ``trigger`` are
    written before ``trigger`` returns (or by their own task, for ``Void``)."""

    async def drain() -> None:
        client._reporter.flush()
        sends = [t for t in client._frame_sends if not t.done()]
        if sends:
            await asyncio.wait(sends)

    client._run_on_loop(drain())


def settle_engine(rounds: int = 2) -> None:
    """Engine round-trip barrier, for asserting that something did *not*
    happen: every worker's frames are written and the engine has read them
    (a builtin call on each connection), and each worker has received what
    the engine sent it before (a call to itself through the engine, on its
    connection, answered after the frames that came earlier)."""
    for _ in range(rounds):
        for client in list(_WORKERS):
            if client.get_connection_state() != "connected":
                continue
            flush_client(client)
            client.trigger({"function_id": "engine::functions::list", "payload": {}, "dispatch": "remote"})
            probe = getattr(client, "_e2e_probe", None)
            if probe is None:
                probe = f"py-probe-{uuid.uuid4().hex[:8]}::settle"
                client.register_function(probe, lambda data: None)
                client._e2e_probe = probe  # type: ignore[attr-defined]
            client.trigger({"function_id": probe, "payload": {}, "dispatch": "remote"})


@pytest.fixture
def fid() -> Callable[[str], str]:
    """``fid("app::fn")``: a function id of this test's own (``app-<hex>::fn``).

    The engine keeps the observers of a worker that left for its grace period
    (30 s), and pushes them to the next owner of the same function: ids shared
    across tests (or runs) would see each other's observers.
    """
    suffix = uuid.uuid4().hex[:8]

    def make(function_id: str) -> str:
        head, sep, tail = function_id.partition("::")
        return f"{head}-{suffix}{sep}{tail}"

    return make


def wait_for(get: Callable[[], Any], what: str, timeout: float = 5.0) -> Any:
    deadline = time.monotonic() + timeout
    while True:
        value = get()
        if value:
            return value
        if time.monotonic() > deadline:
            raise AssertionError(f"timed out after {timeout}s waiting for {what}")
        time.sleep(0.01)


@dataclass
class Activation:
    data: Any
    metadata: Any
    inv: InvocationContext | None


def collector(client: III, function_id: str) -> list[Activation]:
    """Registers a target that records every activation it receives."""
    got: list[Activation] = []

    def target(data: Any, metadata: Any = None) -> None:
        got.append(Activation(data, metadata, client.current_invocation()))

    client.register_function(function_id, target)
    return got


def exactly(got: list[Any], n: int, what: str, timeout: float = 5.0) -> None:
    """Waits until ``got`` holds ``n`` items, then past an engine barrier, so
    an extra one would have arrived too."""
    wait_for(lambda: len(got) >= n, what, timeout)
    settle_engine()
    assert len(got) == n, f"{what}: expected {n}, got {len(got)}"


def local_dispatch_ready(client: III, function_id: str, timeout: float = 5.0) -> None:
    """The engine confirmed the function for local dispatch (``functionregistered``, M3)."""
    wait_for(
        lambda: (
            getattr(client._functions.get(function_id), "observe", None) is not None
            and client._functions[function_id].observe.local_dispatch
        ),
        f"functionregistered {{local_dispatch: true}} for {function_id}",
        timeout,
    )


class FrameCounter:
    """Counts every frame the worker writes to its socket (at the write, on the SDK loop)."""

    def __init__(self, client: III) -> None:
        self.types: list[str] = []
        ws = client._ws
        assert ws is not None
        real = ws.send

        async def send(data: Any, *args: Any, **kwargs: Any) -> None:
            text = data if isinstance(data, str) else bytes(data).decode()
            self.types.append(str(_frame_type(text)))
            await real(data, *args, **kwargs)

        ws.send = send  # type: ignore[method-assign]

    @property
    def count(self) -> int:
        return len(self.types)


def _frame_type(text: str) -> Any:
    import json

    try:
        return json.loads(text).get("type")
    except Exception:
        return None


def drop_connection(client: III) -> None:
    """Drops the worker's socket the way a network failure does; the SDK reconnects."""

    async def abort() -> None:
        if client._ws is not None:
            client._ws.transport.abort()

    client._run_on_loop(abort())


def stable(envelope: dict[str, Any]) -> dict[str, Any]:
    """The envelope without the fields that differ between two runs of the same invocation."""
    out = {
        k: v for k, v in envelope.items() if k not in ("activation_id", "invocation_id", "started_at_ms", "duration_ms")
    }
    if isinstance(out.get("error"), dict):
        out["error"] = {k: v for k, v in out["error"].items() if k != "stacktrace"}
    if isinstance(out.get("caller"), dict):
        out["caller"] = {k: v for k, v in out["caller"].items() if k != "invocation_id"}
    return out
