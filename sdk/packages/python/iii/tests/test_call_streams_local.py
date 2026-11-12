"""Call-native streams without a real engine: local (in-process) dispatch, the Enqueue rule,
pipe-creation failures, and StreamInfo in an engine-built ``Observed`` payload (fake engine)."""

from __future__ import annotations

import asyncio
import io
import os
import tempfile
import threading
from typing import Any

import pytest
from observe_fake_engine import FakeEngine, answers, function_registered, invoke, settle, wait_until

from iii import (
    CallAction,
    InputStream,
    InvocationError,
    OutputStream,
    StreamAbortedError,
    StreamInfo,
)
from iii.iii import III

_READ_REF = {"channel_id": "ch-read", "access_key": "secret-r", "direction": "read"}
_WRITE_REF = {"channel_id": "ch-write", "access_key": "secret-w", "direction": "write"}


def _confirm(client: III, engine: FakeEngine, function_id: str) -> None:
    engine.deliver(function_registered(function_id, []))
    wait_until(lambda: client._functions[function_id].observe.confirmed, what=f"{function_id} confirmed")


def _invokes(engine: FakeEngine, function_id: str) -> list[dict[str, Any]]:
    return [f for f in engine.frames_of("invokefunction") if f.get("function_id") == function_id]


# ── local dispatch ─────────────────────────────────────────────────────────────


async def _numbers(n: int) -> Any:
    for i in range(n):
        yield f"{i}\n".encode()


def test_a_local_call_hands_payload_streams_to_the_handler_as_input_streams(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    seen: list[Any] = []

    async def count(payload: dict[str, Any]) -> dict[str, Any]:
        seen.append(payload["body"])
        return {"bytes": len(await payload["body"].read_all())}

    client.register_function("files::count", count)
    _confirm(client, engine, "files::count")

    async def run() -> list[Any]:
        return [
            await client.call_async({"function_id": "files::count", "payload": {"body": body}})
            for body in (InputStream.from_bytes(b"abc"), _numbers(3), io.BytesIO(b"file"), InputStream.from_iter(["é"]))
        ]

    assert asyncio.run(run()) == [{"bytes": 3}, {"bytes": 6}, {"bytes": 4}, {"bytes": 2}]
    assert len(seen) == 4 and all(isinstance(body, InputStream) for body in seen)
    # Nothing went to the engine: no pipe, no invocation.
    settle(client, engine)
    assert not _invokes(engine, "engine::channels::create") and not _invokes(engine, "files::count")


def test_a_local_call_returns_streams_of_the_result_as_input_streams(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    async def export(payload: dict[str, Any]) -> dict[str, Any]:
        return {"id": payload["id"], "csv": _numbers(4)}

    client.register_function("reports::export", export)
    _confirm(client, engine, "reports::export")

    async def run() -> list[str]:
        result = await client.call_async({"function_id": "reports::export", "payload": {"id": "r"}})
        assert result["id"] == "r" and isinstance(result["csv"], InputStream)
        return [line async for line in result["csv"].lines()]

    assert asyncio.run(run()) == ["0", "1", "2", "3"]


def test_a_local_sink_is_written_before_the_call_resolves(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    async def render(payload: dict[str, Any]) -> dict[str, Any]:
        out: OutputStream = payload["out"]
        for page in range(3):
            await out.write(f"page{page};")
        return {"pages": 3}

    client.register_function("reports::render", render)
    _confirm(client, engine, "reports::render")
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "out.txt")
        result = asyncio.run(
            client.call_async({"function_id": "reports::render", "payload": {"out": OutputStream.to_file(path)}})
        )
        assert result == {"pages": 3}
        with open(path) as f:
            assert f.read() == "page0;page1;page2;"


def test_an_untouched_local_sink_is_closed_when_the_handler_returns(engine: FakeEngine, connect: Any) -> None:
    # In-process nothing else can write it: the call must not wait for it forever.
    client = connect(engine)
    client.register_function("reports::noop", lambda payload: {"ok": True})
    _confirm(client, engine, "reports::noop")
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "empty.txt")
        assert client.call({"function_id": "reports::noop", "payload": {"out": OutputStream.to_file(path)}}) == {
            "ok": True
        }
        assert os.path.getsize(path) == 0


def test_a_local_handler_that_raises_aborts_the_sink_and_the_call_rejects(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    async def render(payload: dict[str, Any]) -> None:
        await payload["out"].write(b"half")
        raise RuntimeError("render failed")

    client.register_function("reports::broken", render)
    _confirm(client, engine, "reports::broken")
    with tempfile.TemporaryDirectory() as tmp:
        sink = OutputStream.to_file(os.path.join(tmp, "out.bin"))
        with pytest.raises(InvocationError, match="render failed"):
            asyncio.run(client.call_async({"function_id": "reports::broken", "payload": {"out": sink}}))
        assert sink._aborted == "render failed"


def test_a_local_sink_aborted_after_the_handler_returned_rejects_the_call(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    async def render(payload: dict[str, Any]) -> dict[str, bool]:
        out: OutputStream = payload["out"].keep_open()

        async def later() -> None:
            await out.write(b"late")
            await out.abort("disk on the other side is full")

        asyncio.get_running_loop().create_task(later())
        return {"ok": True}

    client.register_function("reports::later", render)
    _confirm(client, engine, "reports::later")
    with tempfile.TemporaryDirectory() as tmp:
        with pytest.raises(StreamAbortedError) as error:
            asyncio.run(
                client.call_async(
                    {"function_id": "reports::later", "payload": {"out": OutputStream.to_file(os.path.join(tmp, "f"))}}
                )
            )
        assert (error.value.code, error.value.reason) == ("STREAM_ABORTED", "disk on the other side is full")
        assert error.value.function_id == "reports::later"


def test_a_local_sync_handler_reads_its_stream_with_asyncio_run(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    def count(payload: dict[str, Any]) -> int:
        assert threading.current_thread() is not client._thread
        return len(asyncio.run(payload["body"].read_all()))

    client.register_function("files::count-sync", count)
    _confirm(client, engine, "files::count-sync")
    assert client.call({"function_id": "files::count-sync", "payload": {"body": _numbers(10)}}) == 20


def test_a_local_handler_partially_reading_its_input_cancels_it(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    stopped = threading.Event()

    async def endless() -> Any:
        try:
            while True:
                yield b"z"
                await asyncio.sleep(0)
        finally:
            stopped.set()

    async def head(payload: dict[str, Any]) -> bytes:
        async for chunk in payload["body"]:
            return chunk
        return b""

    client.register_function("files::head", head)
    _confirm(client, engine, "files::head")

    async def run() -> bytes:
        result: bytes = await client.call_async({"function_id": "files::head", "payload": {"body": endless()}})
        for _ in range(100):
            if stopped.is_set():
                break
            await asyncio.sleep(0.01)
        return result

    assert asyncio.run(run()) == b"z"
    assert stopped.is_set()


def test_local_reports_carry_streams_as_stream_info(engine: FakeEngine, connect: Any) -> None:
    from observe_fake_engine import observer_info, reported_items

    client = connect(engine)

    async def count(payload: dict[str, Any]) -> int:
        return len(await payload["body"].read_all())

    client.register_function("files::observed", count)
    engine.deliver(function_registered("files::observed", [observer_info("o", "t", capture=["input"])]))
    wait_until(lambda: client._functions["files::observed"].observe.confirmed, what="confirmed")
    body = InputStream.from_bytes(b"abcd")
    assert asyncio.run(client.call_async({"function_id": "files::observed", "payload": {"body": body}})) == 4
    settle(client, engine)
    [item] = engine.wait_for(lambda: [i for i in reported_items(engine) if i["function_id"] == "files::observed"])
    assert item["input"] == {"body": {"stream": True, "direction": "read", "id": body.id}}


# ── what is refused before anything is sent ───────────────────────────────────────


@pytest.mark.parametrize(
    "payload",
    [
        pytest.param({"body": InputStream.from_bytes(b"a")}, id="input-stream"),
        pytest.param({"out": OutputStream.to(open(os.devnull, "wb"))}, id="output-stream"),
        pytest.param({"items": [_READ_REF]}, id="raw-ref"),
    ],
)
def test_enqueue_with_streams_is_rejected_before_sending(engine: FakeEngine, connect: Any, payload: Any) -> None:
    client = connect(engine)
    with pytest.raises(InvocationError) as error:
        client.call({"function_id": "jobs::run", "payload": payload, "action": CallAction.Enqueue(queue="jobs")})
    assert error.value.code == "STREAM_NOT_ALLOWED"
    assert "cannot be queued" in error.value.message
    settle(client, engine)
    assert not _invokes(engine, "jobs::run") and not _invokes(engine, "engine::channels::create")


def test_a_pipe_that_cannot_be_made_rejects_the_call_before_it_is_sent(engine: FakeEngine, connect: Any) -> None:
    def refuse(frame: dict[str, Any]) -> list[dict[str, Any]] | None:
        if frame.get("type") == "invokefunction" and frame.get("function_id") == "engine::channels::create":
            return [
                {
                    "type": "invocationresult",
                    "invocation_id": frame["invocation_id"],
                    "function_id": "engine::channels::create",
                    "error": {"code": "FORBIDDEN", "message": "no channels for you"},
                }
            ]
        return None

    engine.on_frame = refuse
    client = connect(engine)
    body = InputStream.from_bytes(b"data")
    with pytest.raises(InvocationError) as error:
        client.call({"function_id": "files::remote", "payload": {"body": body}})
    assert error.value.code == "FORBIDDEN"
    settle(client, engine)
    assert len(_invokes(engine, "engine::channels::create")) == 1 and not _invokes(engine, "files::remote")
    # Nothing was sent: the stream can still be used.
    assert not body._transferred and asyncio.run(body.read_all()) == b"data"


# ── refs a handler receives ─────────────────────────────────────────────────────────


def test_an_observed_payload_carries_stream_info_and_other_fields_live_streams(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    seen: list[Any] = []

    def audit(observed: dict[str, Any]) -> None:
        seen.append(observed)

    client.register_function("audit::uploads", audit)
    observed = {
        "observer_id": "o1",
        "activation_id": "a1",
        "function_id": "files::process",
        "input": {"filename": "r.csv", "body": _READ_REF, "parts": [_WRITE_REF]},
        "output": {"csv": _READ_REF},
        "metadata": {"reply": _WRITE_REF},
    }
    engine.deliver(invoke("audit::uploads", observed, "inv-1", activation={"observer_id": "o1", "activation_id": "a1"}))
    engine.wait_for(lambda: answers(engine, "inv-1"), what="the answer")
    [envelope] = seen
    assert envelope["input"]["body"] == StreamInfo(direction="read", id="ch-read")
    assert envelope["input"]["parts"] == [StreamInfo(direction="write", id="ch-write")]
    assert envelope["output"]["csv"].model_dump() == {"stream": True, "direction": "read", "id": "ch-read"}
    assert isinstance(envelope["metadata"]["reply"], OutputStream)  # not under input/output: a live end
    assert envelope["input"]["filename"] == "r.csv"


def test_a_call_through_an_observer_gets_live_streams(engine: FakeEngine, connect: Any) -> None:
    # The caller built the payload before the engine chose the activation id: it never matches.
    client = connect(engine)
    seen: list[Any] = []
    client.register_function("audit::direct", lambda data: seen.append(data))
    payload = {"observer_id": "o1", "input": {"body": _READ_REF}}
    engine.deliver(invoke("audit::direct", payload, "inv-2", activation={"observer_id": "o1", "activation_id": "a9"}))
    engine.wait_for(lambda: answers(engine, "inv-2"), what="the answer")
    assert isinstance(seen[0]["input"]["body"], InputStream)


def test_a_handler_that_returns_an_untouched_received_output_stream_forwards_its_ref(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)
    client.register_function("files::hand-back", lambda data: {"out": data["out"]})
    engine.deliver(invoke("files::hand-back", {"out": _WRITE_REF}, "inv-3"))
    [answer] = engine.wait_for(lambda: answers(engine, "inv-3"), what="the answer")
    assert answer["result"] == {"out": _WRITE_REF}


def test_a_stream_already_passed_on_cannot_be_sent_again(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    sent: list[Any] = []

    def capture(frame: dict[str, Any]) -> list[dict[str, Any]] | None:
        if frame.get("type") == "invokefunction" and frame.get("function_id") == "files::store":
            sent.append(frame["data"])
            return [
                {
                    "type": "invocationresult",
                    "invocation_id": frame["invocation_id"],
                    "function_id": "files::store",
                    "result": None,
                }
            ]
        return None

    engine.on_frame = capture

    async def forward(data: dict[str, Any]) -> None:
        body = data["body"]
        await client.call_async({"function_id": "files::store", "payload": {"body": body}})
        with pytest.raises(RuntimeError, match="already passed on"):
            await client.call_async({"function_id": "files::store", "payload": {"body": body}})
        with pytest.raises(RuntimeError, match="passed on"):
            await body.read_all()

    client.register_function("files::forward", forward)
    engine.deliver(invoke("files::forward", {"body": _READ_REF}, "inv-4"))
    [answer] = engine.wait_for(lambda: answers(engine, "inv-4"), what="the answer")
    assert "error" not in answer, answer
    # The unread stream went on as its ref, exactly: no bytes through this worker.
    assert sent == [{"body": _READ_REF}]


# ── the guard of a remote invocation: plain call or observer activation ──────────────────────


@pytest.mark.parametrize("observed", [False, True], ids=["direct", "routed-with-observers"])
def test_the_guard_knows_an_observer_activation_from_a_plain_call(
    engine: FakeEngine, connect: Any, monkeypatch: pytest.MonkeyPatch, observed: bool
) -> None:
    # A plain call closes an output its handler never used, whatever the result; an observer
    # activation (the frame carries `activation`) with a non-None result leaves it untouched.
    from observe_fake_engine import observer_info

    from iii import streams

    settled: list[tuple[str, bool, Any, bool]] = []

    def record(tracked: list[Any], ok: bool, result: Any, reason: str, *, activated: bool = False) -> None:
        for stream in tracked:
            settled.append((stream._ref.channel_id, ok, result, activated))

    monkeypatch.setattr(streams, "guard", record)
    client = connect(engine)
    client.register_function("route::target", lambda data: {"status_code": 200, "body": "buffered"})
    observers = [observer_info("o9", "audit::t", capture=["input"])] if observed else []
    engine.deliver(function_registered("route::target", observers))
    wait_until(lambda: client._functions["route::target"].observe.confirmed, what="confirmed")

    engine.deliver(invoke("route::target", {"out": {**_WRITE_REF, "channel_id": "plain"}}, "inv-plain"))
    engine.deliver(
        invoke(
            "route::target",
            {"out": {**_WRITE_REF, "channel_id": "activated"}},
            "inv-act",
            activation={"observer_id": "o1", "activation_id": "a1"},
        )
    )
    engine.deliver(invoke("route::target", {"out": {**_WRITE_REF, "channel_id": "unanswered"}}, None))
    engine.wait_for(lambda: answers(engine, "inv-plain") and answers(engine, "inv-act"), what="the answers")
    wait_until(lambda: len(settled) == 3, what="the three guards")
    result = {"status_code": 200, "body": "buffered"}
    assert sorted(settled) == [
        ("activated", True, result, True),
        ("plain", True, result, False),
        ("unanswered", True, result, False),
    ]


# ── adversarial findings ────────────────────────────────────────────────────────────────


def test_a_local_sink_handed_back_is_not_waited_for_and_the_caller_writes_it(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    async def echo(payload: dict[str, Any]) -> dict[str, Any]:
        if payload.get("head"):
            await payload["out"].write(b"head|")
        return {"out": payload["out"]}

    client.register_function("files::echo-out", echo)
    _confirm(client, engine, "files::echo-out")
    with tempfile.TemporaryDirectory() as tmp:
        for head in (False, True):
            path = os.path.join(tmp, f"echo-{head}")

            async def run() -> None:
                payload = {"out": OutputStream.to_file(path), "head": head}
                result = await asyncio.wait_for(
                    client.call_async({"function_id": "files::echo-out", "payload": payload}), 5
                )
                await result["out"].write(b"from the caller")
                await result["out"].close()

            asyncio.run(run())
            with open(path, "rb") as f:
                assert f.read() == (b"head|" if head else b"") + b"from the caller"


def test_a_sync_call_from_a_running_loop_refuses_a_stream_bound_to_that_loop(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    async def write(payload: dict[str, Any]) -> int:
        if "out" in payload:
            await payload["out"].write(b"data")
            return 4
        return len(await payload["body"].read_all())

    client.register_function("files::rw", write)
    _confirm(client, engine, "files::rw")

    class AsyncWriter:
        async def write(self, data: bytes) -> None:
            pass

    async def numbers() -> Any:
        yield b"1"

    async def main() -> list[str]:
        refused = []
        for payload in ({"body": InputStream.from_iter(numbers())}, {"out": OutputStream.to(AsyncWriter())}):
            with pytest.raises(RuntimeError, match="call_async") as error:
                client.call({"function_id": "files::rw", "payload": payload, "timeout_ms": 2000})
            refused.append(str(error.value))
        # Not bound to this loop: a raw async generator, a file object.
        assert client.call({"function_id": "files::rw", "payload": {"body": numbers()}}) == 1
        buffer = io.BytesIO()
        assert client.call({"function_id": "files::rw", "payload": {"out": OutputStream.to(buffer)}}) == 4
        assert buffer.getvalue() == b"data"
        return refused

    refused = asyncio.run(asyncio.wait_for(main(), 10))
    assert len(refused) == 2
    settle(client, engine)
    assert not _invokes(engine, "engine::channels::create")


def test_local_dispatch_refuses_a_stream_twice_or_already_passed_on(engine: FakeEngine, connect: Any) -> None:
    from iii import StreamInUseError
    from iii.streams import plan_outbound

    client = connect(engine)
    calls: list[Any] = []
    client.register_function("files::take", lambda payload: calls.append(payload))
    _confirm(client, engine, "files::take")
    twice = InputStream.from_bytes(b"x")
    sent = InputStream.from_bytes(b"y")
    plan_outbound({"body": sent})
    whole = InputStream.from_bytes(b"z")
    asyncio.run(whole.read_all())
    for payload, match in (
        ({"a": twice, "b": twice}, "already passed on"),
        ([twice, twice], "already passed on"),
        ({"body": sent}, "already passed on"),
        ({"body": whole}, "read to the end"),
    ):
        with pytest.raises(StreamInUseError, match=match) as error:
            client.call({"function_id": "files::take", "payload": payload})
        assert error.value.code == "STREAM_IN_USE"
    assert calls == []


def test_a_fully_read_stream_is_not_forwarded_to_a_remote_function(engine: FakeEngine, connect: Any) -> None:
    from iii import StreamInUseError

    client = connect(engine)
    whole = InputStream.from_bytes(b"whole")
    asyncio.run(whole.read_all())
    with pytest.raises(StreamInUseError, match="read to the end"):
        client.call({"function_id": "elsewhere::count", "payload": {"body": whole}})
    settle(client, engine)
    assert not _invokes(engine, "engine::channels::create") and not _invokes(engine, "elsewhere::count")


def test_sync_generators_and_text_files_get_a_clear_error(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    def gen() -> Any:
        yield b"x"

    client.register_function("files::sink-any", lambda payload: None)
    client.register_function("files::returns-gen", lambda payload: {"g": gen()})
    _confirm(client, engine, "files::sink-any")
    _confirm(client, engine, "files::returns-gen")
    with open(__file__, encoding="utf-8") as text:
        for function_id in ("files::sink-any", "elsewhere::sink"):  # local and remote
            with pytest.raises(TypeError, match="InputStream.from_iter"):
                client.call({"function_id": function_id, "payload": {"body": gen()}})
            with pytest.raises(TypeError, match="binary mode"):
                client.call({"function_id": function_id, "payload": {"body": text}})
    with pytest.raises(InvocationError, match="sync generator cannot be sent"):
        client.call({"function_id": "files::returns-gen", "payload": {}})
    # A remote handler that returns one: the caller gets the same message.
    engine.deliver(invoke("files::returns-gen", {}, "inv-gen"))
    [answer] = engine.wait_for(lambda: answers(engine, "inv-gen"), what="the answer")
    assert "sync generator cannot be sent" in answer["error"]["message"]
    assert "SerializationIterator" not in answer["error"]["message"]


def test_a_local_handler_returning_its_partly_read_input_hands_back_the_rest(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)

    async def skip_header(payload: dict[str, Any]) -> dict[str, Any]:
        header = b""
        async for chunk in payload["body"]:
            header = chunk
            break
        return {"header": header.decode(), "rest": payload["body"]}

    def skip_header_sync(payload: dict[str, Any]) -> dict[str, Any]:
        async def first() -> bytes:
            async for chunk in payload["body"]:
                return chunk
            return b""

        return {"header": asyncio.run(first()).decode(), "rest": payload["body"]}

    client.register_function("files::skip-header", skip_header)
    client.register_function("files::skip-header-sync", skip_header_sync)
    _confirm(client, engine, "files::skip-header")
    _confirm(client, engine, "files::skip-header-sync")

    async def run(function_id: str) -> tuple[str, bytes]:
        body = InputStream.from_iter([b"H|", b"one|", b"two"])
        result = await client.call_async({"function_id": function_id, "payload": {"body": body}})
        await asyncio.sleep(0.05)  # the guard has run
        return result["header"], await result["rest"].read_all()

    assert asyncio.run(run("files::skip-header")) == ("H|", b"one|two")
    assert asyncio.run(run("files::skip-header-sync")) == ("H|", b"one|two")


# ── the call's timeout bounds the wait for its sinks (R4) ─────────────────────────────────


@pytest.mark.parametrize("sync_handler", [False, True], ids=["async-handler", "sync-handler"])
@pytest.mark.parametrize("sync_call", [False, True], ids=["call_async", "call"])
def test_a_local_sink_never_closed_times_the_call_out_and_is_aborted(
    engine: FakeEngine, connect: Any, sync_handler: bool, sync_call: bool
) -> None:
    import time

    client = connect(engine)
    kept: list[OutputStream] = []

    async def keep(payload: dict[str, Any]) -> dict[str, bool]:
        out = payload["out"].keep_open()
        await out.write(b"partial")
        kept.append(out)
        return {"accepted": True}

    def keep_sync(payload: dict[str, Any]) -> dict[str, bool]:
        return asyncio.run(keep(payload))

    function_id = "files::keep-sync" if sync_handler else "files::keep"
    client.register_function(function_id, keep_sync if sync_handler else keep)
    _confirm(client, engine, function_id)
    with tempfile.TemporaryDirectory() as tmp:
        sink = OutputStream.to_file(os.path.join(tmp, "never-closed"))
        request = {"function_id": function_id, "payload": {"out": sink}, "timeout_ms": 1500}
        t0 = time.monotonic()
        with pytest.raises(InvocationError) as error:
            if sync_call:
                client.call(request)
            else:
                asyncio.run(client.call_async(request))
        elapsed = time.monotonic() - t0
    assert error.value.code == "TIMEOUT" and error.value.message == "invocation timed out after 1500ms"
    assert 1.3 < elapsed < 3.0, elapsed
    assert sink._aborted == "invocation timed out after 1500ms"
    with pytest.raises(InvocationError, match="STREAM_CLOSED"):
        asyncio.run(kept[0].write(b"late"))
