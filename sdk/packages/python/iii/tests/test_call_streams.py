"""Call-native streams between workers, through a real engine (``III_URL``).

Two workers each: a caller and a host, so every call goes through the engine
and every stream through a pipe (or travels on as its ref)."""

from __future__ import annotations

import asyncio
import json
import os
import tempfile
import threading
from collections.abc import Callable
from typing import Any

import pytest
import websockets
from observe_e2e_utils import wait_for

from iii import (
    CallAction,
    InputStream,
    InvocationError,
    OutputStream,
    StreamAbortedError,
    StreamInfo,
)
from iii._channels import build_channel_url
from iii.iii import III

TIMEOUT_S = 20


def _run(coro: Any) -> Any:
    return asyncio.run(asyncio.wait_for(coro, TIMEOUT_S))


async def _lines(n: int) -> Any:
    for i in range(n):
        yield f"row {i}\n".encode()


@pytest.fixture
def pair(worker: Callable[..., III], fid: Callable[[str], str]) -> tuple[III, III, Callable[[str], str]]:
    return worker("streams-caller"), worker("streams-host"), fid


def _register(host: III, function_id: str, handler: Any) -> None:
    host.register_function(function_id, handler)
    # The engine must know the function before the caller's first call.
    wait_for(
        lambda: (
            getattr(host._functions.get(function_id), "observe", None) is not None
            and host._functions[function_id].observe.confirmed
        ),
        f"{function_id} registered",
        10,
    )


# ── send, return, sink ──────────────────────────────────────────────────────────


def test_streams_in_the_payload_reach_the_handler(pair: Any) -> None:
    caller, host, fid = pair

    async def count(payload: dict[str, Any]) -> dict[str, Any]:
        body: InputStream = payload["body"]
        assert isinstance(body, InputStream)
        total = 0
        async for chunk in body:
            total += len(chunk)
        return {"bytes": total}

    _register(host, fid("files::count"), count)
    data = os.urandom(300 * 1024)
    with tempfile.NamedTemporaryFile(delete=False) as f:
        f.write(data)

    async def run() -> list[int]:
        sizes = []
        try:
            with open(f.name, "rb") as fh:
                for body in (
                    InputStream.from_bytes(data),
                    InputStream.from_file(f.name),
                    fh,  # an open binary file
                    _lines(500),  # an async generator
                    InputStream.from_iter(["a", b"b"]),
                ):
                    result = await caller.call_async({"function_id": fid("files::count"), "payload": {"body": body}})
                    sizes.append(result["bytes"])
        finally:
            os.unlink(f.name)
        return sizes

    expected_lines = sum(len(f"row {i}\n") for i in range(500))
    assert _run(run()) == [len(data), len(data), len(data), expected_lines, 2]


def test_a_stream_in_the_result_reaches_the_caller(pair: Any) -> None:
    caller, host, fid = pair

    async def export(payload: dict[str, Any]) -> dict[str, Any]:
        return {"report_id": payload["report_id"], "csv": _lines(payload["rows"])}

    _register(host, fid("reports::export"), export)

    async def run() -> list[str]:
        result = await caller.call_async(
            {"function_id": fid("reports::export"), "payload": {"report_id": "r", "rows": 3}}
        )
        assert result["report_id"] == "r" and isinstance(result["csv"], InputStream)
        return [line async for line in result["csv"].lines()]

    assert _run(run()) == ["row 0", "row 1", "row 2"]


def test_a_sink_is_written_before_the_call_resolves(pair: Any) -> None:
    caller, host, fid = pair

    async def render(payload: dict[str, Any]) -> None:
        out: OutputStream = payload["out"]
        for page in range(5):
            await out.write(f"page {page}\n")
        # Returning closes `out`.

    _register(host, fid("reports::render"), render)
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "r.txt")
        assert (
            _run(
                caller.call_async(
                    {"function_id": fid("reports::render"), "payload": {"out": OutputStream.to_file(path)}}
                )
            )
            is None
        )
        with open(path) as f:
            assert f.read() == "".join(f"page {page}\n" for page in range(5))


def test_a_handler_that_raises_rejects_the_call_and_aborts_the_sink(pair: Any) -> None:
    caller, host, fid = pair

    async def render(payload: dict[str, Any]) -> None:
        await payload["out"].write(b"half a page")
        raise RuntimeError("renderer crashed")

    _register(host, fid("reports::broken"), render)
    with tempfile.TemporaryDirectory() as tmp:
        sink = OutputStream.to_file(os.path.join(tmp, "r.txt"))
        with pytest.raises(InvocationError, match="renderer crashed"):
            _run(caller.call_async({"function_id": fid("reports::broken"), "payload": {"out": sink}}))
        assert sink._aborted is not None


def test_a_sink_aborted_after_the_handler_returned_rejects_with_stream_aborted(pair: Any) -> None:
    caller, host, fid = pair

    async def render(payload: dict[str, Any]) -> dict[str, bool]:
        out: OutputStream = payload["out"].keep_open()

        async def later() -> None:
            await out.write(b"late page")
            await out.abort("ran out of pages")

        asyncio.get_running_loop().create_task(later())
        return {"accepted": True}

    _register(host, fid("reports::later"), render)
    with tempfile.TemporaryDirectory() as tmp:
        with pytest.raises(StreamAbortedError) as error:
            _run(
                caller.call_async(
                    {
                        "function_id": fid("reports::later"),
                        "payload": {"out": OutputStream.to_file(os.path.join(tmp, "f"))},
                    }
                )
            )
        assert error.value.code == "STREAM_ABORTED" and error.value.function_id == fid("reports::later")
        # A 0.26 engine relays the abort, not the writer's reason.
        assert error.value.close_code == 1011


# ── forwarding ────────────────────────────────────────────────────────────────────


def test_an_unread_input_stream_is_forwarded_as_its_ref(pair: Any, worker: Callable[..., III]) -> None:
    caller, host, fid = pair
    relay = worker("streams-relay")
    sockets: list[Any] = []

    async def count(payload: dict[str, Any]) -> int:
        return len(await payload["body"].read_all())

    async def forward(payload: dict[str, Any]) -> int:
        body: InputStream = payload["body"]
        result: int = await relay.call_async({"function_id": fid("files::count"), "payload": {"body": body}})
        sockets.append(body._source)  # never opened here: no byte passed through this worker
        return result

    _register(host, fid("files::count"), count)
    _register(relay, fid("files::forward"), forward)
    data = os.urandom(200_000)
    assert _run(
        caller.call_async({"function_id": fid("files::forward"), "payload": {"body": InputStream.from_bytes(data)}})
    ) == len(data)
    assert sockets == [None]


def test_a_partially_read_input_stream_forwards_the_rest(pair: Any, worker: Callable[..., III]) -> None:
    caller, host, fid = pair
    relay = worker("streams-relay")

    async def collect(payload: dict[str, Any]) -> str:
        return (await payload["body"].read_all()).decode()

    async def skip_header(payload: dict[str, Any]) -> dict[str, Any]:
        body: InputStream = payload["body"]
        header = b""
        async for chunk in body:
            header = chunk
            break
        rest = await relay.call_async({"function_id": fid("files::collect"), "payload": {"body": body}})
        return {"header": header.decode(), "rest": rest}

    _register(host, fid("files::collect"), collect)
    _register(relay, fid("files::skip-header"), skip_header)
    result = _run(caller.call_async({"function_id": fid("files::skip-header"), "payload": {"body": _lines(4)}}))
    assert result == {"header": "row 0\n", "rest": "row 1\nrow 2\nrow 3\n"}


# ── aborts ──────────────────────────────────────────────────────────────────────────


def test_a_source_that_raises_aborts_the_stream_for_the_reader(pair: Any) -> None:
    caller, host, fid = pair
    seen: list[BaseException] = []

    async def count(payload: dict[str, Any]) -> int:
        chunks = []
        try:
            async for chunk in payload["body"]:
                chunks.append(chunk)
        except StreamAbortedError as error:
            seen.append(error)
            return -len(chunks)
        return len(chunks)

    _register(host, fid("files::count"), count)

    async def broken() -> Any:
        yield b"first"
        yield b"second"
        raise ValueError("disk read failed")

    # The data sent before the abort is read first.
    assert _run(caller.call_async({"function_id": fid("files::count"), "payload": {"body": broken()}})) == -2
    assert seen[0].code == "STREAM_ABORTED"  # type: ignore[attr-defined]


def test_a_handler_that_raises_aborts_its_output_for_the_reader(pair: Any) -> None:
    caller, host, fid = pair

    async def fail(payload: dict[str, Any]) -> None:
        await payload["out"].write(b"partial")
        raise RuntimeError("handler failed")

    _register(host, fid("files::fail"), fail)

    async def run() -> tuple[list[bytes], BaseException]:
        pipe = await caller.pipe_async()
        chunks: list[bytes] = []

        async def read() -> None:
            async for chunk in pipe.input:
                chunks.append(chunk)

        reading = asyncio.ensure_future(read())
        with pytest.raises(InvocationError, match="handler failed"):
            await caller.call_async({"function_id": fid("files::fail"), "payload": {"out": pipe.output}})
        with pytest.raises(StreamAbortedError) as error:
            await reading
        return chunks, error.value

    chunks, error = _run(run())
    assert chunks == [b"partial"] and error.close_code == 1011  # type: ignore[attr-defined]


# ── the handler guard ──────────────────────────────────────────────────────────────


def test_the_guard_of_a_plain_call_closes_used_and_untouched_outputs(pair: Any) -> None:
    caller, host, fid = pair

    async def write_then_return(payload: dict[str, Any]) -> dict[str, bool]:
        await payload["out"].write(b"body")
        return {"streamed": True}

    _register(host, fid("files::streamed"), write_then_return)
    _register(host, fid("files::result"), lambda payload: {"status_code": 200, "body": "buffered"})
    _register(host, fid("files::empty"), lambda payload: None)

    async def run() -> None:
        used = await caller.pipe_async()
        await caller.call_async({"function_id": fid("files::streamed"), "payload": {"out": used.output}})
        assert await asyncio.wait_for(used.input.read_all(), 5) == b"body"

        # Never used, a non-null result, a plain call (no activation): closed all the same.
        untouched = await caller.pipe_async()
        result = await caller.call_async({"function_id": fid("files::result"), "payload": {"out": untouched.output}})
        assert result == {"status_code": 200, "body": "buffered"}
        assert await asyncio.wait_for(untouched.input.read_all(), 5) == b""

        empty = await caller.pipe_async()
        assert await caller.call_async({"function_id": fid("files::empty"), "payload": {"out": empty.output}}) is None
        assert await asyncio.wait_for(empty.input.read_all(), 5) == b""

    _run(run())


def test_a_sink_the_handler_never_used_ends_and_the_call_resolves(pair: Any) -> None:
    caller, host, fid = pair
    _register(host, fid("files::ignore"), lambda payload: {"accepted": True})
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "never-written")
        sink = OutputStream.to_file(path)
        # Before the v2 guard this waited for the engine to sweep the unattached pipe (minutes).
        result = asyncio.run(
            asyncio.wait_for(caller.call_async({"function_id": fid("files::ignore"), "payload": {"out": sink}}), 5)
        )
        assert result == {"accepted": True}
        assert sink._closed and os.path.getsize(path) == 0


def test_a_handler_that_raises_leaves_an_untouched_output_to_the_caller(pair: Any) -> None:
    caller, host, fid = pair

    def fail(payload: dict[str, Any]) -> None:
        raise RuntimeError("nothing written")

    _register(host, fid("files::fail-untouched"), fail)

    async def run() -> None:
        pipe = await caller.pipe_async()
        with pytest.raises(InvocationError, match="nothing written"):
            await caller.call_async({"function_id": fid("files::fail-untouched"), "payload": {"out": pipe.output}})
        reading = asyncio.ensure_future(pipe.input.read_all())
        done, _ = await asyncio.wait({reading}, timeout=0.5)
        assert not done, "the handler's guard must not end an output it never used when it raised"
        reading.cancel()

    _run(run())


def test_an_observer_activation_leaves_an_untouched_output_with_a_result_open(
    worker: Callable[..., III], fid: Callable[[str], str]
) -> None:
    # The router plays the HTTP worker: its deliver hook calls each route target through the
    # observer (the engine's frame then carries `activation`) with a response pipe, then reads it.
    router, host, client = worker("streams-router"), worker("streams-route-host"), worker("streams-client")
    source, target = fid("fake-http::on-request"), fid("routes::target")
    activations: list[Any] = []

    @router.on_observation_deliver(source)
    async def deliver(call: Any, observers: Any) -> dict[str, Any]:
        [obs] = observers
        pipe = await router.pipe_async()
        result = await router.call_async({"observer": obs, "payload": {**call.input, "response": pipe.output}})
        reading = asyncio.ensure_future(pipe.input.read_all())
        done, _ = await asyncio.wait({reading}, timeout=1)
        if not done:
            reading.cancel()
        return {"result": result, "body": reading.result().decode() if done else "still open"}

    router.register_function(source, lambda request: request)

    async def route(request: dict[str, Any]) -> Any:
        invocation = host.current_invocation()
        activations.append(invocation.activation if invocation is not None else None)
        response: OutputStream = request["response"]
        if request["mode"] == "streamed":
            await response.write(b"streamed body")
            return None
        if request["mode"] == "buffered":
            return {"status_code": 200, "body": "buffered"}
        return None  # no body

    host.register_function(target, route)
    handle = host.observe({"function_id": source}, target)
    handle.ready(timeout=10)
    wait_for(lambda: router.observers(source), "the observer on its host")

    def request(mode: str) -> Any:
        return client.call({"function_id": source, "payload": {"mode": mode}, "timeout_ms": TIMEOUT_S * 1000})

    assert request("buffered") == {"result": {"status_code": 200, "body": "buffered"}, "body": "still open"}
    assert request("empty") == {"result": None, "body": ""}
    assert request("streamed") == {"result": None, "body": "streamed body"}
    assert len(activations) == 3 and all(a is not None and a.get("observer_id") for a in activations)
    handle.dispose()


def test_keep_open_lets_the_handler_write_after_it_returned(pair: Any) -> None:
    caller, host, fid = pair

    async def background(payload: dict[str, Any]) -> None:
        out = payload["out"].keep_open()
        await out.write(b"now ")

        async def later() -> None:
            await asyncio.sleep(0.2)
            await out.write(b"later")
            await out.close()

        asyncio.get_running_loop().create_task(later())

    _register(host, fid("files::background"), background)

    async def run() -> bytes:
        pipe = await caller.pipe_async()
        await caller.call_async({"function_id": fid("files::background"), "payload": {"out": pipe.output}})
        return await asyncio.wait_for(pipe.input.read_all(), 5)

    assert _run(run()) == b"now later"


def test_the_guard_cancels_a_partially_read_input(pair: Any) -> None:
    caller, host, fid = pair
    stopped = threading.Event()

    async def endless() -> Any:
        try:
            while True:
                yield b"x" * 1024
                await asyncio.sleep(0)
        finally:
            stopped.set()

    async def head(payload: dict[str, Any]) -> int:
        async for chunk in payload["body"]:
            return len(chunk)
        return 0

    _register(host, fid("files::head"), head)
    assert _run(caller.call_async({"function_id": fid("files::head"), "payload": {"body": endless()}})) == 1024
    # The engine closed the writer once the handler's reader left: the source stopped.
    assert stopped.wait(10)


# ── rules for calls ────────────────────────────────────────────────────────────────


def test_enqueue_rejects_streams_and_void_carries_them(pair: Any) -> None:
    caller, host, fid = pair
    got: list[bytes] = []
    done = threading.Event()

    async def sink(payload: dict[str, Any]) -> None:
        got.append(await payload["body"].read_all())
        done.set()

    _register(host, fid("files::sink"), sink)
    with pytest.raises(InvocationError) as error:
        caller.call(
            {
                "function_id": fid("files::sink"),
                "payload": {"body": InputStream.from_bytes(b"queued")},
                "action": CallAction.Enqueue(queue="default"),
            }
        )
    assert error.value.code == "STREAM_NOT_ALLOWED"
    caller.call({"function_id": fid("files::sink"), "payload": {"body": _lines(2)}, "action": CallAction.Void()})
    assert done.wait(10) and got == [b"row 0\nrow 1\n"]


def test_a_sync_call_returns_streams_readable_with_asyncio_run(pair: Any) -> None:
    caller, host, fid = pair

    async def export(payload: dict[str, Any]) -> dict[str, Any]:
        return {"csv": _lines(3)}

    _register(host, fid("reports::export"), export)
    result = caller.call({"function_id": fid("reports::export"), "payload": {}})
    assert asyncio.run(result["csv"].read_all()) == b"row 0\nrow 1\nrow 2\n"


def test_a_sync_handler_writes_its_output_with_asyncio_run(pair: Any) -> None:
    caller, host, fid = pair

    def render(payload: dict[str, Any]) -> None:
        out: OutputStream = payload["out"]
        asyncio.run(out.write(b"from "))
        asyncio.run(out.write(b"a thread"))

    _register(host, fid("reports::sync"), render)
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "out")
        caller.call({"function_id": fid("reports::sync"), "payload": {"out": OutputStream.to_file(path)}})
        with open(path, "rb") as f:
            assert f.read() == b"from a thread"


def test_pipe_messages_and_bytes_keep_their_order(pair: Any) -> None:
    caller, host, fid = pair

    async def progress(payload: dict[str, Any]) -> None:
        out: OutputStream = payload["out"]
        await out.send({"type": "progress", "pct": 50})
        await out.write(b"chunk")
        await out.send({"type": "done"})

    _register(host, fid("jobs::progress"), progress)

    async def run() -> list[Any]:
        pipe = await caller.pipe_async()
        await caller.call_async({"function_id": fid("jobs::progress"), "payload": {"out": pipe.output}})
        return [item.binary if item.is_binary else item.text async for item in pipe.input.frames()]

    assert _run(run()) == ['{"type":"progress","pct":50}', b"chunk", '{"type":"done"}']


# ── observers ─────────────────────────────────────────────────────────────────────────


def test_an_observer_of_a_streamed_call_sees_stream_info(pair: Any) -> None:
    caller, host, fid = pair
    seen: list[Any] = []
    arrived = threading.Event()

    async def count(payload: dict[str, Any]) -> dict[str, Any]:
        return {"bytes": len(await payload["body"].read_all()), "copy": _lines(1)}

    def audit(observed: dict[str, Any]) -> None:
        seen.append(observed)
        arrived.set()

    _register(host, fid("files::count"), count)
    caller.register_function(fid("audit::uploads"), audit)
    handle = caller.observe({"function_id": fid("files::count"), "capture": ["input", "output"]}, fid("audit::uploads"))
    handle.ready(timeout=10)

    async def run() -> Any:
        result = await caller.call_async(
            {
                "function_id": fid("files::count"),
                "payload": {"filename": "a.csv", "body": InputStream.from_bytes(b"abc")},
            }
        )
        assert await result["copy"].read_all() == b"row 0\n"
        return result["bytes"]

    assert _run(run()) == 3
    assert arrived.wait(10)
    observed = seen[0]
    assert observed["input"]["filename"] == "a.csv"
    body, copy = observed["input"]["body"], observed["output"]["copy"]
    assert isinstance(body, StreamInfo) and (body.stream, body.direction) == (True, "read")
    assert isinstance(copy, StreamInfo) and copy.direction == "read"
    handle.dispose()


# ── interop with a raw ref (what a 0.26 SDK sends and expects) ────────────────────────────


def test_a_ref_built_by_hand_is_read_as_an_input_stream(pair: Any) -> None:
    caller, host, fid = pair

    async def count(payload: dict[str, Any]) -> int:
        assert isinstance(payload["body"], InputStream)
        return len(await payload["body"].read_all())

    _register(host, fid("files::count"), count)

    async def run() -> int:
        writer_ref, reader_ref = await caller._on_sdk_loop(caller._create_pipe())
        ws = await websockets.connect(
            build_channel_url(caller._address, writer_ref.channel_id, writer_ref.access_key, "write")
        )
        await ws.send(b"written by an old SDK")
        await ws.close()
        raw_ref = {"channel_id": reader_ref.channel_id, "access_key": reader_ref.access_key, "direction": "read"}
        result: int = await caller.call_async({"function_id": fid("files::count"), "payload": {"body": raw_ref}})
        return result

    assert _run(run()) == len(b"written by an old SDK")


def test_a_returned_stream_travels_as_a_plain_ref_an_old_sdk_can_read(pair: Any) -> None:
    caller, host, fid = pair

    async def export(payload: dict[str, Any]) -> dict[str, Any]:
        return {"csv": _lines(2)}

    _register(host, fid("reports::export"), export)

    async def run() -> bytes:
        # What an old SDK gets: the raw JSON result.
        raw = await caller._on_sdk_loop(
            caller._invoke_remote(fid("reports::export"), {}, None, None, None, TIMEOUT_S * 1000)
        )
        ref = raw["csv"]
        assert set(ref) == {"channel_id", "access_key", "direction"} and ref["direction"] == "read"
        ws = await websockets.connect(build_channel_url(caller._address, ref["channel_id"], ref["access_key"], "read"))
        chunks = []
        try:
            while True:
                chunks.append(await ws.recv())
        except websockets.ConnectionClosed as closed:
            assert closed.rcvd is None or closed.rcvd.code in (1000, 1005)
        return b"".join(chunks)

    assert _run(run()) == b"row 0\nrow 1\n"


def test_a_handler_reads_its_input_and_streams_progress_and_a_result_back(pair: Any) -> None:
    caller, host, fid = pair

    async def words(payload: dict[str, Any]) -> dict[str, int]:
        out: OutputStream = payload["out"]
        chunks = []
        async for chunk in payload["text"]:
            chunks.append(chunk)
            await out.send({"type": "progress", "chunks": len(chunks)})
        text = b"".join(chunks).decode()
        await out.write(" ".join(reversed(text.split())))
        return {"words": len(text.split())}

    _register(host, fid("text::words"), words)

    async def source() -> Any:
        for part in (b"the quick ", b"brown fox ", b"jumps"):
            yield part

    async def run() -> tuple[Any, list[Any]]:
        pipe = await caller.pipe_async()
        frames: list[Any] = []

        async def read() -> None:
            async for item in pipe.input.frames():
                frames.append(item.text if item.is_text else item.binary)

        reading = asyncio.ensure_future(read())
        result = await caller.call_async(
            {"function_id": fid("text::words"), "payload": {"text": source(), "out": pipe.output}}
        )
        await asyncio.wait_for(reading, 5)
        return result, frames

    result, frames = _run(run())
    assert result == {"words": 5}
    *progress, reversed_text = frames
    assert reversed_text == b"jumps fox brown quick the"
    assert [json.loads(p)["type"] for p in progress] == ["progress"] * len(progress) and progress


def test_an_http_handler_reads_the_request_body_and_streams_its_response(pair: Any) -> None:
    # The caller plays the HTTP worker: it sends raw refs and reads the response channel as frames.
    from iii_helpers.http import http

    from iii import StreamRequest, StreamResponse

    caller, host, fid = pair

    @http
    async def upload(req: StreamRequest, response: StreamResponse) -> None:
        body = await req.request_body.read_all()
        await response.status(201)
        await response.headers({"content-type": "text/plain"})
        await response.write(f"got {len(body)} bytes: ".encode())
        await response.write(body.upper())

    _register(host, fid("http::upload"), upload)

    async def run() -> list[Any]:
        body_w, body_r = await caller._on_sdk_loop(caller._create_pipe())
        resp_w, resp_r = await caller._on_sdk_loop(caller._create_pipe())
        url = caller._address

        async def send_body() -> None:
            ws = await websockets.connect(build_channel_url(url, body_w.channel_id, body_w.access_key, "write"))
            await ws.send(b"hello ")
            await ws.send(b"world")
            await ws.close()

        async def read_response() -> list[Any]:
            ws = await websockets.connect(build_channel_url(url, resp_r.channel_id, resp_r.access_key, "read"))
            frames = []
            try:
                while True:
                    frames.append(await ws.recv())
            except websockets.ConnectionClosed as closed:
                assert closed.rcvd is None or closed.rcvd.code in (1000, 1005)
            return frames

        reading = asyncio.ensure_future(read_response())
        sending = asyncio.ensure_future(send_body())
        result = await caller.call_async(
            {
                "function_id": fid("http::upload"),
                "payload": {
                    "path_params": {},
                    "query_params": {},
                    "body": None,
                    "headers": {},
                    "method": "POST",
                    "request_body": {
                        "channel_id": body_r.channel_id,
                        "access_key": body_r.access_key,
                        "direction": "read",
                    },
                    "response": {
                        "channel_id": resp_w.channel_id,
                        "access_key": resp_w.access_key,
                        "direction": "write",
                    },
                },
            }
        )
        assert result is None
        await sending
        return await asyncio.wait_for(reading, 5)

    assert _run(run()) == [
        '{"type":"set_status","status_code":201}',
        '{"type":"set_headers","headers":{"content-type":"text/plain"}}',
        b"got 11 bytes: ",
        b"HELLO WORLD",
    ]


# ── adversarial findings ────────────────────────────────────────────────────────────────


def test_a_sink_handed_back_by_the_handler_does_not_hold_the_call(pair: Any) -> None:
    # The caller can only write the returned stream once the call resolved: waiting for the
    # sink first (R4) would never end. Also when the handler wrote to it first (bridged back).
    caller, host, fid = pair

    async def echo(payload: dict[str, Any]) -> dict[str, Any]:
        if payload.get("head"):
            await payload["out"].write(b"head|")
        return {"out": payload["out"]}

    _register(host, fid("files::echo-out"), echo)
    with tempfile.TemporaryDirectory() as tmp:
        for head in (False, True):
            path = os.path.join(tmp, f"echo-{head}")
            sink = OutputStream.to_file(path)

            async def run() -> None:
                payload = {"out": sink, "head": head}
                result = await asyncio.wait_for(
                    caller.call_async({"function_id": fid("files::echo-out"), "payload": payload}), 5
                )
                assert isinstance(result["out"], OutputStream)
                await result["out"].write(b"from the caller")
                await result["out"].close()
                await asyncio.wait_for(sink._wait(), 5)  # the sink finishes in the background

            _run(run())
            with open(path, "rb") as f:
                assert f.read() == (b"head|" if head else b"") + b"from the caller"


def test_a_partly_read_input_returned_by_the_handler_is_its_rest(pair: Any) -> None:
    caller, host, fid = pair

    async def skip_header(payload: dict[str, Any]) -> dict[str, Any]:
        header = b""
        async for chunk in payload["body"]:
            header = chunk
            break
        return {"header": header.decode(), "rest": payload["body"]}

    _register(host, fid("files::skip-header"), skip_header)

    async def run() -> tuple[str, bytes]:
        result = await caller.call_async({"function_id": fid("files::skip-header"), "payload": {"body": _lines(4)}})
        return result["header"], await asyncio.wait_for(result["rest"].read_all(), 5)

    assert _run(run()) == ("row 0\n", b"row 1\nrow 2\nrow 3\n")


def test_a_worker_whose_main_thread_returned_still_streams_files(
    worker: Callable[..., III], fid: Callable[[str], str]
) -> None:
    # Many worker scripts end their main thread right after registering; the SDK's loop thread
    # keeps the process alive, and concurrent.futures takes no new work from then on.
    import subprocess
    import sys
    import textwrap
    import time

    from observe_e2e_utils import ENGINE_WS_URL

    caller = worker("streams-mainexit-caller")
    function_id = fid("files::mainexit")
    script = textwrap.dedent(
        """
        import sys
        from iii import InitOptions, InputStream, OutputStream, register_worker

        iii = register_worker(sys.argv[1], InitOptions(worker_name="py-streams-mainexit"))

        async def handle(payload):
            out = OutputStream.to_file(payload["path"])
            await out.write(b"written|")
            await out.close()
            def gen():
                yield b"sync-iter"
            return {"file": InputStream.from_file(payload["path"]), "iter": InputStream.from_iter(gen())}

        iii.register_function(sys.argv[2], handle)
        """
    )  # the main thread returns here
    # A host name, as in CI: resolving it uses the loop's default executor, which a plain
    # ThreadPoolExecutor refuses once the main thread ended. An IP address skips that lookup.
    url = ENGINE_WS_URL.replace("127.0.0.1", "localhost")
    process = subprocess.Popen([sys.executable, "-c", script, url, function_id])
    try:
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "out.bin")

            async def run() -> tuple[bytes, bytes]:
                deadline = time.monotonic() + 15
                while True:
                    try:
                        result = await caller.call_async(
                            {"function_id": function_id, "payload": {"path": path}, "timeout_ms": 5000}
                        )
                        break
                    except InvocationError as error:
                        if "not found" not in error.message.lower() or time.monotonic() > deadline:
                            raise
                        await asyncio.sleep(0.2)
                return await result["file"].read_all(), await result["iter"].read_all()

            assert asyncio.run(asyncio.wait_for(run(), 30)) == (b"written|", b"sync-iter")
    finally:
        process.kill()
        process.wait(10)


@pytest.mark.parametrize("sync_call", [False, True], ids=["call_async", "call"])
def test_a_sink_the_handler_never_closes_times_the_call_out_and_is_aborted(pair: Any, sync_call: bool) -> None:
    import time

    caller, host, fid = pair
    late: list[BaseException] = []

    async def keep(payload: dict[str, Any]) -> dict[str, bool]:
        out = payload["out"].keep_open()
        await out.write(b"partial")

        async def later() -> None:
            await asyncio.sleep(3)
            try:
                await out.write(b"late")
            except BaseException as error:
                late.append(error)

        asyncio.get_running_loop().create_task(later())
        return {"accepted": True}

    _register(host, fid("files::keep"), keep)
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "never-closed")
        sink = OutputStream.to_file(path)
        request = {"function_id": fid("files::keep"), "payload": {"out": sink}, "timeout_ms": 1500}
        t0 = time.monotonic()
        with pytest.raises(InvocationError) as error:
            if sync_call:
                caller.call(request)
            else:
                asyncio.run(caller.call_async(request))
        elapsed = time.monotonic() - t0
        assert error.value.code == "TIMEOUT" and error.value.message == "invocation timed out after 1500ms"
        assert error.value.invocation_id
        assert 1.3 < elapsed < 3.0, elapsed
        assert sink._aborted == "invocation timed out after 1500ms"
        with open(path, "rb") as f:
            assert f.read() == b"partial"
    # The caller closed its end: the handler's later write learns the reader left.
    wait_for(lambda: late, "the handler's late write", 6)
    assert isinstance(late[0], StreamAbortedError)
