"""How a stream ends on the wire: end of stream vs abort, the writer's clean close and abort frames,
and InputStream / OutputStream over a channel socket (a local WebSocket server stands in for the engine)."""

import asyncio
import gc
import io
import json
import socket
import threading
from contextlib import asynccontextmanager
from typing import Any, AsyncIterator, Awaitable, Callable

import pytest
import websockets

from iii import InputStream, OutputStream, StreamAbortedError, StreamResponse
from iii._channels import (
    ABORT_CLOSE_CODE,
    MAX_CLOSE_REASON_BYTES,
    MAX_FRAME_SIZE,
    WRITER_CLOSE_TIMEOUT_S,
    ChannelReader,
    ChannelWriter,
    truncate_reason,
)
from iii.iii_types import StreamChannelRef
from iii.protocol import ChannelItem
from iii.streams import _Binding


@asynccontextmanager
async def _serve(handler: Callable[[Any], Awaitable[None]], **options: Any) -> AsyncIterator[str]:
    async with websockets.serve(handler, "127.0.0.1", 0, **options) as server:
        yield f"ws://127.0.0.1:{next(iter(server.sockets)).getsockname()[1]}"


def _ref(direction: str) -> StreamChannelRef:
    return StreamChannelRef(channel_id="c", access_key="k", direction=direction)  # type: ignore[arg-type]


def _reader(url: str) -> ChannelReader:
    return ChannelReader(url, _ref("read"))


def _writer(url: str) -> ChannelWriter:
    return ChannelWriter(url, _ref("write"))


def _binding(url: str, loop: asyncio.AbstractEventLoop | None = None) -> _Binding:
    return _Binding(url, None, loop or asyncio.get_running_loop())


def _input(url: str, loop: asyncio.AbstractEventLoop | None = None) -> InputStream:
    return InputStream._from_ref(_ref("read"), _binding(url, loop))


def _output(url: str, loop: asyncio.AbstractEventLoop | None = None) -> OutputStream:
    return OutputStream._from_ref(_ref("write"), _binding(url, loop))


async def _read(reader: ChannelReader) -> tuple[list[Any], BaseException | None]:
    frames: list[Any] = []
    try:
        while (frame := await reader.recv()) is not None:
            frames.append(frame)
    except Exception as e:
        return frames, e
    return frames, None


def _sends_then(end: Callable[[Any], Awaitable[None]], *frames: Any) -> Callable[[Any], Awaitable[None]]:
    async def handler(ws: Any) -> None:
        for frame in frames or (b"a", b"b"):
            await ws.send(frame)
        await end(ws)
        await ws.wait_closed()

    return handler


def _clean(ws: Any) -> Awaitable[None]:
    return ws.close(code=None)


# ── the read end ───────────────────────────────────────────────────────────────


@pytest.mark.asyncio
@pytest.mark.parametrize(
    "end",
    [
        pytest.param(lambda ws: ws.close(code=None), id="no-code-1005"),
        pytest.param(lambda ws: ws.close(1000, "done"), id="1000"),
    ],
)
async def test_clean_close_is_end_of_stream(end: Callable[[Any], Awaitable[None]]) -> None:
    async with _serve(_sends_then(end)) as url:
        assert await _reader(url).read_all() == b"ab"
        assert await _input(url).read_all() == b"ab"


async def _abort_transport(ws: Any) -> None:
    ws.transport.abort()


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("end", "code", "reason"),
    [
        pytest.param(
            lambda ws: ws.close(1011, "channel writer aborted: closed with code 1011"),
            1011,
            "channel writer aborted: closed with code 1011",
            id="1011",
        ),
        pytest.param(lambda ws: ws.close(1011), 1011, "", id="1011-no-reason"),
        # websockets counts 1001 as a clean close; the channel contract does not.
        pytest.param(lambda ws: ws.close(1001, "going away"), 1001, "going away", id="1001"),
        pytest.param(_abort_transport, 1006, "no close frame received or sent", id="no-close-frame-1006"),
    ],
)
async def test_unclean_end_raises_stream_aborted_after_the_data(
    end: Callable[[Any], Awaitable[None]], code: int, reason: str
) -> None:
    async with _serve(_sends_then(end)) as url:
        reader = _reader(url)
        frames, error = await _read(reader)
        assert frames == [b"a", b"b"]
        assert isinstance(error, StreamAbortedError)
        assert (error.code, error.close_code, error.reason) == ("STREAM_ABORTED", code, reason)
        assert error.message == f"stream aborted (close {code}{': ' + reason if reason else ''})"
        # A second read reports the abort again, not a clean end.
        with pytest.raises(StreamAbortedError):
            await reader.read_all()

        chunks: list[bytes] = []
        with pytest.raises(StreamAbortedError):
            async for chunk in _input(url):
                chunks.append(chunk)
        assert chunks == [b"a", b"b"]


@pytest.mark.asyncio
async def test_local_close_during_a_read_is_not_an_error() -> None:
    drop = asyncio.Event()

    async def handler(ws: Any) -> None:
        # Like the engine, never answer the reader's close: drop the socket instead.
        ws.transport.pause_reading()
        await ws.send(b"a")
        await drop.wait()
        ws.transport.abort()

    async with _serve(handler) as url:
        stream = _input(url)
        first = asyncio.Event()

        async def consume() -> tuple[list[bytes], BaseException | None]:
            chunks: list[bytes] = []
            try:
                async for chunk in stream:
                    chunks.append(chunk)
                    first.set()
            except Exception as e:
                return chunks, e
            return chunks, None

        reading = asyncio.create_task(consume())
        await asyncio.wait_for(first.wait(), 5)
        cancelling = asyncio.create_task(stream.cancel())
        await asyncio.sleep(0.05)
        drop.set()
        assert await asyncio.wait_for(reading, 5) == ([b"a"], None)
        await asyncio.wait_for(cancelling, 5)


# ── the write end ───────────────────────────────────────────────────────────────


class _Sink:
    """Server end of a writer: records the frames and the close code and reason it received."""

    def __init__(self) -> None:
        self.frames: list[Any] = []
        self.close: asyncio.Future[tuple[int | None, str | None]] = asyncio.get_running_loop().create_future()
        self.connections = 0

    async def __call__(self, ws: Any) -> None:
        self.connections += 1
        try:
            async for message in ws:
                self.frames.append(message)
        except websockets.ConnectionClosed:
            pass
        if not self.close.done():
            self.close.set_result((ws.close_code, ws.close_reason))

    async def closed_with(self) -> int | None:
        return (await asyncio.wait_for(self.close, 5))[0]

    async def reason(self) -> str | None:
        return (await asyncio.wait_for(self.close, 5))[1]


@pytest.mark.asyncio
async def test_writer_close_sends_1000_after_every_frame() -> None:
    sink = _Sink()
    data = bytes(range(256)) * (3 * MAX_FRAME_SIZE // 256)
    async with _serve(sink) as url:
        writer = _writer(url)
        await writer.write(b"data")
        await writer.send_text("msg")
        await writer.write(data)
        await writer.close()
        assert await sink.closed_with() == 1000
        assert sink.frames[:2] == [b"data", "msg"]
        assert all(len(f) <= MAX_FRAME_SIZE for f in sink.frames[2:]) and b"".join(sink.frames[2:]) == data


@pytest.mark.asyncio
async def test_writer_close_on_a_never_opened_writer_opens_it_and_ends_cleanly() -> None:
    sink = _Sink()
    async with _serve(sink) as url:
        await _writer(url).close()
        assert await sink.closed_with() == 1000
        assert sink.frames == [] and sink.connections == 1


@pytest.mark.asyncio
async def test_writer_abort_sends_the_abort_code_and_a_truncated_reason() -> None:
    sink = _Sink()
    async with _serve(sink) as url:
        writer = _writer(url)
        await writer.write(b"partial")
        await writer.abort("é" * 200)
        await writer.abort("again")  # idempotent
        assert await sink.closed_with() == ABORT_CLOSE_CODE
        reason = await sink.reason()
        assert reason == truncate_reason("é" * 200) and len(reason.encode()) <= MAX_CLOSE_REASON_BYTES
        assert sink.frames == [b"partial"]


@pytest.mark.asyncio
async def test_writer_abort_on_a_never_opened_writer_opens_it_first() -> None:
    # R7: the reader always learns of the failure.
    sink = _Sink()
    async with _serve(sink) as url:
        await _writer(url).abort("source failed")
        assert await sink.closed_with() == ABORT_CLOSE_CODE
        assert await sink.reason() == "source failed"


@pytest.mark.asyncio
async def test_writer_whose_write_failed_refuses_later_writes() -> None:
    # A later write must not reconnect and end cleanly: the stream already lost what the failed write carried.
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        url = f"ws://127.0.0.1:{s.getsockname()[1]}"
    writer = _writer(url)
    with pytest.raises(OSError) as first:
        await writer.write(b"lost")
    with pytest.raises(ConnectionError, match="an earlier write did not finish") as retry:
        await writer.write(b"retry")
    assert retry.value.__cause__ is first.value


@pytest.mark.asyncio
async def test_writer_close_outlasts_a_stalled_reader() -> None:
    # websockets' 10 s default drops the queued close frame while the engine is not reading the socket.
    sink = _Sink()
    async with _serve(sink) as url:
        writer = _writer(url)
        await writer.write(b"data")
        assert writer._ws is not None
        assert writer._ws.close_timeout == WRITER_CLOSE_TIMEOUT_S >= 5 * 60
        await writer.close()
        assert await sink.closed_with() == 1000


@pytest.mark.asyncio
async def test_channel_sockets_send_no_keepalive_pings() -> None:
    # A slow consumer delays the pong behind queued data; a ping timeout would cut a healthy stream.
    sink = _Sink()
    async with _serve(sink) as url:
        writer = _writer(url)
        await writer.write(b"data")
        assert writer._ws is not None and writer._ws.ping_interval is None
        await writer.close()
    async with _serve(_sends_then(_clean)) as url:
        reader = _reader(url)
        await reader.read_all()
        assert reader._ws is not None and reader._ws.ping_interval is None


@pytest.mark.asyncio
async def test_writer_dropped_without_close_ends_its_socket_with_no_close_frame() -> None:
    # A writer that goes away mid stream must reach the reader as an abort, not leave it waiting.
    sink = _Sink()
    async with _serve(sink) as url:
        out = _output(url)
        await out.write(b"a")
        del out
        gc.collect()
        assert await sink.closed_with() == 1006
        assert sink.frames == [b"a"]


# ── the handler guard (R2, revised v2) ─────────────────────────────────────────────────


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("ok", "result", "activated", "closed"),
    [
        pytest.param(True, {"status_code": 200}, False, True, id="plain-call-result-closes"),
        pytest.param(True, None, False, True, id="plain-call-none-closes"),
        pytest.param(True, None, True, True, id="activation-none-closes"),
        pytest.param(True, {"status_code": 200}, True, False, id="activation-result-leaves-it"),
        pytest.param(True, 0, True, False, id="activation-falsy-result-leaves-it"),
        pytest.param(False, None, False, False, id="plain-call-failure-leaves-it"),
        pytest.param(False, None, True, False, id="activation-failure-leaves-it"),
    ],
)
async def test_the_guard_settles_an_output_the_handler_never_used(
    ok: bool, result: Any, activated: bool, closed: bool
) -> None:
    from iii import streams

    sink = _Sink()
    async with _serve(sink) as url:
        out = _output(url)
        streams.guard([out], ok, result, "" if ok else "boom", activated=activated)
        if closed:
            assert await sink.closed_with() == 1000 and sink.frames == []
        else:
            await asyncio.sleep(0.2)
            assert sink.connections == 0, "an untouched stream left as it is opens no socket"
            assert not out._closed and out._aborted is None


@pytest.mark.asyncio
@pytest.mark.parametrize("activated", [False, True])
@pytest.mark.parametrize(("ok", "code"), [(True, 1000), (False, ABORT_CLOSE_CODE)])
async def test_the_guard_closes_or_aborts_an_output_the_handler_used(activated: bool, ok: bool, code: int) -> None:
    from iii import streams

    sink = _Sink()
    async with _serve(sink) as url:
        out = _output(url)
        await out.write(b"part")
        streams.guard([out], ok, {"x": 1} if ok else None, "" if ok else "handler failed", activated=activated)
        assert await sink.closed_with() == code and sink.frames == [b"part"]
        if not ok:
            assert await sink.reason() == "handler failed"


@pytest.mark.asyncio
async def test_the_guard_skips_kept_open_and_passed_on_outputs() -> None:
    from iii import streams

    sink = _Sink()
    async with _serve(sink) as url:
        kept, passed = _output(url).keep_open(), _output(url)
        passed._transferred = True  # forwarded as its ref
        streams.guard([kept, passed], True, None, "")
        await asyncio.sleep(0.2)
        assert sink.connections == 0


# ── OutputStream over a channel ────────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_output_stream_writes_sends_and_closes() -> None:
    sink = _Sink()
    async with _serve(sink) as url:
        out = _output(url)
        await out.write(b"bytes")
        await out.write("text as utf-8: é")
        await out.send({"type": "progress", "n": 1, "name": "relatório"})
        await out.close()
        await out.close()  # idempotent
        assert await sink.closed_with() == 1000
        assert sink.frames == [b"bytes", "text as utf-8: é".encode(), '{"type":"progress","n":1,"name":"relatório"}']
        with pytest.raises(RuntimeError, match="closed"):
            await out.write(b"late")
        await out.abort("too late")  # a no-op once closed
        await out._wait()


@pytest.mark.asyncio
async def test_output_stream_abort_reaches_the_reader_and_later_writes_fail() -> None:
    sink = _Sink()
    async with _serve(sink) as url:
        out = _output(url)
        await out.write(b"partial")
        await out.abort(ValueError("render failed"))
        assert await sink.closed_with() == ABORT_CLOSE_CODE
        assert await sink.reason() == "render failed"
        with pytest.raises(RuntimeError, match="aborted"):
            await out.send({"late": True})
        await out.close()  # a no-op once aborted
        with pytest.raises(StreamAbortedError):
            await out._wait()


@pytest.mark.asyncio
async def test_output_stream_context_manager_closes_or_aborts() -> None:
    sink = _Sink()
    async with _serve(sink) as url:
        async with _output(url) as out:
            await out.write(b"ok")
        assert await sink.closed_with() == 1000
    sink = _Sink()
    async with _serve(sink) as url:
        with pytest.raises(KeyError):
            async with _output(url) as out:
                await out.write(b"half")
                raise KeyError("missing")
        assert await sink.closed_with() == ABORT_CLOSE_CODE
        assert sink.frames == [b"half"]


@pytest.mark.asyncio
async def test_output_stream_write_after_the_reader_left_raises_stream_aborted() -> None:
    async def leave(ws: Any) -> None:
        await ws.recv()
        await ws.close(code=None)  # the engine closes the writer when its reader leaves

    async with _serve(leave) as url:
        out = _output(url)
        await out.write(b"first")
        with pytest.raises(StreamAbortedError, match="reader closed the stream"):
            for _ in range(100):
                await out.write(b"more")
                await asyncio.sleep(0.01)


@pytest.mark.asyncio
async def test_stream_response_control_messages_are_compact_utf8_json() -> None:
    # The HTTP worker parses these exact control messages from the response stream.
    sink = _Sink()
    async with _serve(sink) as url:
        response = StreamResponse(_output(url))
        await response.status(201)
        await response.headers({"content-disposition": 'attachment; filename="relatório.pdf"'})
        await response.write(b"%PDF")
        await response.close()
        assert await sink.closed_with() == 1000
        assert sink.frames == [
            '{"type":"set_status","status_code":201}',
            '{"type":"set_headers","headers":{"content-disposition":"attachment; filename=\\"relatório.pdf\\""}}',
            b"%PDF",
        ]


@pytest.mark.asyncio
async def test_output_stream_works_from_a_thread_through_asyncio_run() -> None:
    # A sync handler runs on a thread with no loop: each asyncio.run() runs on the stream's home loop.
    sink = _Sink()
    async with _serve(sink) as url:
        out = _output(url)

        def handler() -> None:
            asyncio.run(out.write(b"a"))
            asyncio.run(out.send({"step": 2}))
            asyncio.run(out.close())

        await asyncio.to_thread(handler)
        assert await sink.closed_with() == 1000
        assert sink.frames == [b"a", '{"step":2}']


# ── InputStream over a channel ─────────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_input_stream_frames_messages_bytes_and_lines() -> None:
    frames = (b"line one\r\nline ", '{"n":1}', b"two\nlast", "not json", b"")
    async with _serve(_sends_then(_clean, *frames)) as url:
        assert [(item.binary if item.is_binary else item.text) async for item in _input(url).frames()] == list(frames)
        assert [m async for m in _input(url).messages()] == [{"n": 1}, "not json"]
        assert await _input(url).read_all() == b"line one\r\nline two\nlast"
        assert await _input(url).text() == "line one\r\nline two\nlast"
        assert [line async for line in _input(url).lines()] == ["line one", "line two", "last"]


@pytest.mark.asyncio
async def test_lines_decode_utf8_split_across_frames() -> None:
    word = "relatório\n".encode()
    async with _serve(_sends_then(_clean, word[:5], word[5:], b"end")) as url:
        assert [line async for line in _input(url).lines()] == ["relatório", "end"]


@pytest.mark.asyncio
async def test_input_stream_has_one_reader() -> None:
    async with _serve(_sends_then(_clean)) as url:
        stream = _input(url)
        assert await stream.read_all() == b"ab"
        for read in (stream.read_all, stream.text):
            with pytest.raises(RuntimeError, match="one reader"):
                await read()
        for start in (stream.__aiter__, stream.lines, stream.messages, stream.frames, stream.tee):
            with pytest.raises(RuntimeError, match="one reader"):
                start()


@pytest.mark.asyncio
async def test_tee_gives_two_streams_of_every_frame() -> None:
    async with _serve(_sends_then(_clean, b"a", "m", b"b")) as url:
        first, second = _input(url).tee()
        assert await first.read_all() == b"ab"
        assert [item async for item in second.frames()] == [
            ChannelItem.binary_item(b"a"),
            ChannelItem.text_item("m"),
            ChannelItem.binary_item(b"b"),
        ]


@pytest.mark.asyncio
async def test_cancel_of_a_never_read_stream_opens_and_closes_it() -> None:
    sink = _Sink()
    async with _serve(sink) as url:
        stream = _input(url)
        await stream.cancel()
        assert await sink.closed_with() == 1000 and sink.connections == 1
        assert [chunk async for chunk in stream] == []  # a cancelled stream reads as ended


@pytest.mark.asyncio
async def test_input_stream_read_from_another_loop_runs_on_its_home_loop() -> None:
    # A sync call() returns streams bound to the SDK loop; user code reads them with asyncio.run in a thread.
    other = asyncio.new_event_loop()
    thread = threading.Thread(target=other.run_forever, daemon=True)
    thread.start()
    try:
        async with _serve(_sends_then(_clean, b"x" * 10, b"y")) as url:
            stream = _input(url, other)
            data = await asyncio.to_thread(lambda: asyncio.run(stream.read_all()))
            assert data == b"x" * 10 + b"y"
    finally:
        other.call_soon_threadsafe(other.stop)
        thread.join()
        other.close()


def test_streams_are_not_built_directly() -> None:
    with pytest.raises(TypeError, match="from_file"):
        InputStream()
    with pytest.raises(TypeError, match="to_file"):
        OutputStream()


def test_streams_serialize_to_their_stream_info_for_filters_and_reports() -> None:
    from iii.filter._value import to_json_value
    from iii.reporter import encode_json

    body = InputStream.from_bytes(b"x")
    out = OutputStream.to(open("/dev/null", "wb"))
    value = {"body": body, "out": out}
    assert to_json_value(value) == {
        "body": {"stream": True, "direction": "read", "id": body.id},
        "out": {"stream": True, "direction": "write", "id": out.id},
    }
    assert json.loads(encode_json(value))["body"]["id"].startswith("local:")


# ── sources off the loop, the bounded tee ──────────────────────────────────────────────


@pytest.mark.asyncio
async def test_a_sync_iterable_is_advanced_off_the_event_loop() -> None:
    # A blocking next() must not stall the loop (every other call of the worker runs there).
    import time

    def slow() -> Any:
        for index in range(3):
            time.sleep(0.15)
            yield f"{index};"

    ticks = 0

    async def ticker() -> None:
        nonlocal ticks
        while True:
            await asyncio.sleep(0.01)
            ticks += 1

    ticking = asyncio.ensure_future(ticker())
    try:
        assert await InputStream.from_iter(slow()).read_all() == b"0;1;2;"
    finally:
        ticking.cancel()
    assert ticks >= 20, f"the loop ran {ticks} ticks in 0.45 s of blocking next()"


@pytest.mark.asyncio
async def test_cancelling_a_sync_iterable_while_its_next_runs_does_not_wait_for_it() -> None:
    import time

    started, closed = threading.Event(), threading.Event()

    def blocking() -> Any:
        try:
            yield b"first"
            started.set()
            time.sleep(0.5)
            yield b"never read"
        finally:
            closed.set()

    stream = InputStream.from_iter(blocking())
    iterator = stream.__aiter__()
    assert await iterator.__anext__() == b"first"
    pending = asyncio.ensure_future(iterator.__anext__())
    await asyncio.to_thread(started.wait, 5)
    t0 = asyncio.get_running_loop().time()
    await stream.cancel()
    assert asyncio.get_running_loop().time() - t0 < 0.3, "cancel waited for the blocked next()"
    await asyncio.wait_for(pending, 5)
    assert await asyncio.to_thread(closed.wait, 5), "the generator was closed once its next() returned"


@pytest.mark.asyncio
async def test_a_list_source_keeps_its_order_and_types() -> None:
    assert [f async for f in InputStream.from_iter([b"a", "b", bytearray(b"c")]).frames()] == [
        ChannelItem.binary_item(b"a"),
        ChannelItem.binary_item(b"b"),
        ChannelItem.binary_item(b"c"),
    ]


@pytest.mark.asyncio
async def test_tee_holds_at_most_its_window_for_a_lagging_copy() -> None:
    from iii.streams import TEE_WINDOW_FRAMES

    pulled = 0

    def frames() -> Any:
        nonlocal pulled
        for index in range(200):
            pulled += 1
            yield index.to_bytes(2, "big")

    first, second = InputStream.from_iter(frames()).tee()
    fast = first.__aiter__()
    got = [await fast.__anext__() for _ in range(TEE_WINDOW_FRAMES)]
    # The lagging copy holds a full window: the fast one waits instead of buffering more.
    waiting = asyncio.ensure_future(fast.__anext__())
    done, _ = await asyncio.wait({waiting}, timeout=0.3)
    assert not done, "the fast copy read past the window of the lagging one"
    assert pulled <= TEE_WINDOW_FRAMES + 1

    async def rest_of_first() -> list[bytes]:
        return [*got, await waiting] + [chunk async for chunk in fast]

    a, b = await asyncio.gather(rest_of_first(), second.read_all())
    assert b"".join(a) == b == b"".join(i.to_bytes(2, "big") for i in range(200))


@pytest.mark.asyncio
async def test_a_cancelled_tee_copy_does_not_hold_the_other_back() -> None:
    first, second = InputStream.from_iter([bytes([i]) for i in range(100)]).tee()
    await second.cancel()
    assert await asyncio.wait_for(first.read_all(), 5) == bytes(range(100))


def test_tee_copies_read_from_two_threads_each_with_its_own_loop() -> None:
    first, second = InputStream.from_iter(bytes([i % 256]) * 1024 for i in range(500)).tee()
    results: dict[str, Any] = {}

    def read(name: str, stream: InputStream) -> None:
        async def run() -> int:
            total = 0
            async for chunk in stream:
                total += len(chunk)
                await asyncio.sleep(0)
            return total

        try:
            results[name] = asyncio.run(asyncio.wait_for(run(), 20))
        except BaseException as error:
            results[name] = error

    threads = [threading.Thread(target=read, args=(n, s)) for n, s in (("a", first), ("b", second))]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join(30)
    assert results == {"a": 500 * 1024, "b": 500 * 1024}


@pytest.mark.asyncio
async def test_tee_copies_both_get_the_source_error() -> None:
    def broken() -> Any:
        yield b"ok"
        raise ValueError("disk read failed")

    first, second = InputStream.from_iter(broken()).tee()
    results = await asyncio.gather(first.read_all(), second.read_all(), return_exceptions=True)
    assert [type(r) for r in results] == [ValueError, ValueError]


# ── what cannot be sent, what was read already ─────────────────────────────────────────


@pytest.mark.parametrize(
    ("make", "match"),
    [
        pytest.param(lambda: (b for b in [b"x"]), "sync generator.*InputStream.from_iter", id="sync-generator"),
        pytest.param(lambda: open(__file__, encoding="utf-8"), "text-mode file.*'rb'", id="text-file"),
    ],
)
def test_values_that_are_not_streams_get_a_clear_error(make: Callable[[], Any], match: str) -> None:
    from iii.streams import StreamScan, plan_outbound, wrap_sources

    value = make()
    try:
        with pytest.raises(TypeError, match=match):
            wrap_sources({"body": value}, StreamScan())
        with pytest.raises(TypeError, match=match):
            plan_outbound({"result": [value]})
    finally:
        close = getattr(value, "close", None)
        if callable(close):
            close()


@pytest.mark.asyncio
async def test_a_stream_read_to_the_end_cannot_be_passed_on() -> None:
    from iii.streams import plan_outbound

    whole = InputStream.from_bytes(b"whole")
    await whole.read_all()
    with pytest.raises(RuntimeError, match="already read to the end: a stream has one reader"):
        plan_outbound({"body": whole})
    assert not whole._transferred

    # Partly read: the rest is passed on.
    partial = InputStream.from_iter([b"head", b"rest"])
    async for _ in partial:
        break
    plan = plan_outbound({"body": partial})
    assert plan.needs and partial._transferred
    plan.restore()


@pytest.mark.asyncio
async def test_a_writer_sink_on_a_file_object_is_not_bound_to_a_loop() -> None:
    from iii.streams import bound_to

    class AsyncWriter:
        async def write(self, data: bytes) -> None:
            pass

    on_file = OutputStream.to(io.BytesIO())
    on_async = OutputStream.to(AsyncWriter())
    agen_source = InputStream.from_iter(_agen())
    loop = asyncio.get_running_loop()
    assert bound_to({"a": on_file, "b": on_async, "c": agen_source, "d": InputStream.from_iter([b"x"])}, loop) == [
        on_async,
        agen_source,
    ]
    tee_copy, _ = InputStream.from_iter(_agen()).tee()
    assert bound_to([tee_copy], loop) == [tee_copy]


async def _agen() -> Any:
    yield b"x"


# ── error codes shared by the SDKs ──────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_writing_a_closed_or_aborted_output_is_stream_closed() -> None:
    from iii import InvocationError, StreamClosedError

    for end in ("close", "abort"):
        out = OutputStream.to(io.BytesIO())
        await (out.close() if end == "close" else out.abort("stop"))
        for attempt in (out.write(b"late"), out.send({"late": True})):
            with pytest.raises(StreamClosedError) as error:
                await attempt
            assert error.value.code == "STREAM_CLOSED"
            assert isinstance(error.value, InvocationError) and isinstance(error.value, RuntimeError)


@pytest.mark.asyncio
async def test_a_stream_used_twice_is_stream_in_use() -> None:
    from iii import StreamInUseError
    from iii.streams import plan_outbound

    async def in_use(make: Callable[[], Any]) -> str:
        with pytest.raises(StreamInUseError) as error:
            result = make()
            if asyncio.iscoroutine(result):
                await result
        assert error.value.code == "STREAM_IN_USE" and isinstance(error.value, RuntimeError)
        return error.value.message

    read = InputStream.from_bytes(b"x")
    await read.read_all()
    assert "one reader" in await in_use(read.read_all)  # a second reader
    assert "read to the end" in await in_use(lambda: plan_outbound({"body": read}))

    twice = InputStream.from_bytes(b"x")
    assert "already passed on" in await in_use(lambda: plan_outbound({"a": twice, "b": twice}))

    sent = InputStream.from_bytes(b"x")
    plan_outbound({"body": sent})  # passed on to a call
    assert "already passed on" in await in_use(lambda: plan_outbound({"body": sent}))
    assert "passed on" in await in_use(sent.read_all)

    out = OutputStream.to(io.BytesIO())
    plan_outbound({"out": out})
    assert "passed on" in await in_use(lambda: out.write(b"x"))


# ── a local handler's result ────────────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_a_partly_read_stream_handed_back_locally_is_the_rest_for_a_new_reader() -> None:
    from iii import StreamInUseError
    from iii.streams import guard, hand_back

    body = InputStream.from_iter([b"head|", b"middle|", b"tail"])
    async for _ in body:
        break
    result = hand_back({"rest": body})
    rest = result["rest"]
    assert rest is not body and body._transferred
    guard([body], True, result, "")  # the handler's guard leaves the original alone
    assert await rest.read_all() == b"middle|tail"

    unread = InputStream.from_bytes(b"all")
    assert hand_back({"same": unread})["same"] is unread
    whole = InputStream.from_bytes(b"x")
    await whole.read_all()
    with pytest.raises(StreamInUseError):
        hand_back({"whole": whole})


# ── a worker whose main thread returned ─────────────────────────────────────────────────


def test_file_and_iterator_streams_work_after_the_main_thread_returned(tmp_path: Any) -> None:
    # A worker script often ends its main thread right after registering; the SDK's loop
    # thread keeps the process alive. concurrent.futures takes no new work from then on.
    import subprocess
    import sys
    import textwrap

    script = textwrap.dedent(
        """
        import asyncio, io, sys, threading, time
        from iii import InputStream, OutputStream

        path = sys.argv[1]

        def later():
            time.sleep(0.3)  # the main thread has returned by now

            async def go():
                try:
                    await asyncio.get_running_loop().run_in_executor(None, time.time)
                    print("executor: accepted", flush=True)
                except RuntimeError:
                    print("executor: refused", flush=True)
                out = OutputStream.to_file(path)
                await out.write(b"written|")
                await out.close()
                data = await InputStream.from_file(path).read_all()

                def gen():
                    yield b"sync-"
                    yield b"iter"

                it = await InputStream.from_iter(gen()).read_all()
                buffer = io.BytesIO()
                sink = OutputStream.to(buffer)
                await sink.write(b"to-file-object")
                await sink.close()
                print((data + it + b"|" + buffer.getvalue()).decode(), flush=True)

            asyncio.run(go())

        threading.Thread(target=later).start()  # not a daemon: it keeps the process alive
        """
    )
    done = subprocess.run(
        [sys.executable, "-c", script, str(tmp_path / "out.bin")], capture_output=True, text=True, timeout=60
    )
    assert done.returncode == 0, done.stderr
    lines = done.stdout.splitlines()
    assert lines[0] == "executor: refused", "this Python does not reproduce the condition"
    assert lines[1] == "written|sync-iter|to-file-object"
