"""How a channel ends on the wire: reader end-of-stream vs abort, and the writer's clean close frame."""

import asyncio
import contextlib
import gc
import socket
import threading
from contextlib import asynccontextmanager
from typing import Any, AsyncIterator, Awaitable, Callable

import pytest
import websockets
from websockets.protocol import State

from iii.channels import MAX_FRAME_SIZE, WRITER_CLOSE_TIMEOUT_S, ChannelReader, ChannelWriter
from iii.iii_types import StreamChannelRef


@asynccontextmanager
async def _serve(handler: Callable[[Any], Awaitable[None]], **options: Any) -> AsyncIterator[str]:
    async with websockets.serve(handler, "127.0.0.1", 0, **options) as server:
        yield f"ws://127.0.0.1:{next(iter(server.sockets)).getsockname()[1]}"


def _reader(url: str) -> ChannelReader:
    return ChannelReader(url, StreamChannelRef(channel_id="c", access_key="k", direction="read"))


def _writer(url: str) -> ChannelWriter:
    return ChannelWriter(url, StreamChannelRef(channel_id="c", access_key="k", direction="write"))


async def _read(reader: ChannelReader) -> tuple[list[bytes], BaseException | None]:
    chunks: list[bytes] = []
    try:
        async for chunk in reader:
            chunks.append(chunk)
    except Exception as e:
        return chunks, e
    return chunks, None


def _sends_then(end: Callable[[Any], Awaitable[None]]) -> Callable[[Any], Awaitable[None]]:
    async def handler(ws: Any) -> None:
        await ws.send(b"a")
        await ws.send(b"b")
        await end(ws)
        await ws.wait_closed()

    return handler


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


async def _abort_transport(ws: Any) -> None:
    ws.transport.abort()


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("end", "message"),
    [
        pytest.param(
            lambda ws: ws.close(1011, "channel writer aborted: connection lost before close"),
            "channel aborted (close 1011: channel writer aborted: connection lost before close)",
            id="1011",
        ),
        pytest.param(lambda ws: ws.close(1011), "channel aborted (close 1011)", id="1011-no-reason"),
        # websockets counts 1001 as a clean close; the channel contract does not.
        pytest.param(lambda ws: ws.close(1001, "going away"), "channel aborted (close 1001: going away)", id="1001"),
        pytest.param(
            _abort_transport, "channel aborted (close 1006: no close frame received or sent)", id="no-close-frame-1006"
        ),
    ],
)
async def test_unclean_end_raises_after_the_data(end: Callable[[Any], Awaitable[None]], message: str) -> None:
    async with _serve(_sends_then(end)) as url:
        reader = _reader(url)
        chunks, error = await _read(reader)
        assert chunks == [b"a", b"b"]
        assert isinstance(error, ConnectionError)
        assert str(error) == message
        # A second read reports the abort again, not a clean end.
        with pytest.raises(ConnectionError):
            await reader.read_all()


@pytest.mark.asyncio
async def test_local_close_during_iteration_is_not_an_error() -> None:
    drop = asyncio.Event()

    async def handler(ws: Any) -> None:
        # Like the engine, never answer the reader's close: drop the socket instead.
        ws.transport.pause_reading()
        await ws.send(b"a")
        await drop.wait()
        ws.transport.abort()

    async with _serve(handler) as url:
        reader = _reader(url)
        first = asyncio.Event()

        async def consume() -> tuple[list[bytes], BaseException | None]:
            chunks: list[bytes] = []
            try:
                async for chunk in reader:
                    chunks.append(chunk)
                    first.set()
            except Exception as e:
                return chunks, e
            return chunks, None

        reading = asyncio.create_task(consume())
        await asyncio.wait_for(first.wait(), 5)
        closing = asyncio.create_task(reader.close_async())
        await asyncio.sleep(0.05)
        drop.set()
        assert await asyncio.wait_for(reading, 5) == ([b"a"], None)
        await asyncio.wait_for(closing, 5)


class _Sink:
    """Server end of a writer: records the frames and the close code it received."""

    def __init__(self) -> None:
        self.frames: list[Any] = []
        self.close_code: asyncio.Future[int | None] = asyncio.get_running_loop().create_future()

    async def __call__(self, ws: Any) -> None:
        try:
            async for message in ws:
                self.frames.append(message)
        except websockets.ConnectionClosed:
            pass
        self.close_code.set_result(ws.close_code)

    async def closed_with(self) -> int | None:
        return await asyncio.wait_for(self.close_code, 5)


@pytest.mark.asyncio
async def test_sync_close_from_the_owning_loop_sends_1000() -> None:
    sink = _Sink()
    async with _serve(sink) as url:
        writer = _writer(url)
        await writer.write(b"data")
        writer.close()
        assert await sink.closed_with() == 1000
        assert sink.frames == [b"data"]


@pytest.mark.asyncio
async def test_sync_end_from_the_owning_loop_writes_then_sends_1000() -> None:
    sink = _Sink()
    async with _serve(sink) as url:
        writer = _writer(url)
        await writer.write(b"data")
        writer.stream.end(b"tail")
        assert await sink.closed_with() == 1000
        assert sink.frames == [b"data", b"tail"]


async def _slow_handshake(connection: Any, request: Any) -> None:
    # Keeps the first operation connecting for longer than the close's flush delay.
    await asyncio.sleep(0.1)


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("first", "frames"),
    [
        pytest.param(lambda w: w.stream.write(b"a"), [b"a"], id="write"),
        pytest.param(lambda w: w.send_message("m"), ["m"], id="send_message"),
    ],
)
@pytest.mark.parametrize(
    "end",
    [pytest.param(lambda w: w.stream.end(), id="end"), pytest.param(lambda w: w.close(), id="close")],
)
async def test_sync_close_queued_while_the_first_write_connects_still_sends_1000(
    first: Callable[[ChannelWriter], None], frames: list[Any], end: Callable[[ChannelWriter], None]
) -> None:
    sink = _Sink()
    async with _serve(sink, process_request=_slow_handshake) as url:
        writer = _writer(url)
        first(writer)
        end(writer)
        assert await sink.closed_with() == 1000
        assert sink.frames == frames


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("queued", "frames"),
    [
        pytest.param(lambda w: w.stream.write(b"b"), [b"a", b"b"], id="write"),
        pytest.param(lambda w: w.stream.end(b"tail"), [b"a", b"tail"], id="end"),
    ],
)
async def test_sync_call_queued_before_an_awaited_close_goes_out_first(
    queued: Callable[[ChannelWriter], None], frames: list[bytes]
) -> None:
    # e.g. an HTTP handler: await status() connects, then stream.write(body), then await close_async().
    sink = _Sink()
    async with _serve(sink) as url:
        writer = _writer(url)
        await writer.write(b"a")
        queued(writer)
        await writer.close_async()
        assert await sink.closed_with() == 1000
        assert sink.frames == frames


@pytest.mark.asyncio
async def test_sync_close_waits_for_every_frame_of_a_queued_write() -> None:
    # Under backpressure a multi-frame write is still sending when the close runs; a clean close must not cut it.
    sink = _Sink()
    data = bytes(range(256)) * (3 * MAX_FRAME_SIZE // 256)
    async with _serve(sink) as url:
        writer = _writer(url)
        await writer.write(b"")
        assert writer._ws is not None
        send = writer._ws.send

        async def slow_send(message: Any) -> None:
            await asyncio.sleep(0.02)
            await send(message)

        writer._ws.send = slow_send  # type: ignore[method-assign]
        writer.stream.write(data)
        writer.close()
        assert await sink.closed_with() == 1000
        assert b"".join(sink.frames[1:]) == data


@pytest.mark.asyncio
@pytest.mark.parametrize(
    "end",
    [pytest.param(lambda w: w.close(), id="close"), pytest.param(lambda w: w.stream.end(b"tail"), id="end")],
)
async def test_sync_close_as_the_last_statement_of_asyncio_run_sends_1000(
    end: Callable[[ChannelWriter], None],
) -> None:
    # asyncio.run cancels the queued close at loop teardown; it must still finish.
    sink = _Sink()
    async with _serve(sink) as url:

        async def main() -> None:
            writer = _writer(url)
            await writer.write(b"data")
            end(writer)

        await asyncio.to_thread(asyncio.run, main())
        assert await sink.closed_with() == 1000
        assert sink.frames[0] == b"data"


@pytest.mark.asyncio
@pytest.mark.parametrize(
    "end",
    [pytest.param(lambda w: w.close(), id="close"), pytest.param(lambda w: w.stream.end(), id="end")],
)
async def test_write_cut_by_asyncio_run_teardown_aborts_instead_of_closing_cleanly(
    end: Callable[[ChannelWriter], None],
) -> None:
    # asyncio.run cancels a queued multi-frame write mid stream; the queued close must not then end it as complete.
    sink = _Sink()
    data = bytes(range(256)) * (3 * MAX_FRAME_SIZE // 256)
    async with _serve(sink) as url:

        async def main() -> None:
            writer = _writer(url)
            await writer.write(b"")
            assert writer._ws is not None
            send = writer._ws.send

            async def slow_send(message: Any) -> None:
                await asyncio.sleep(0.02)
                await send(message)

            writer._ws.send = slow_send  # type: ignore[method-assign]
            writer.stream.write(data)
            end(writer)

        await asyncio.to_thread(asyncio.run, main())
        assert await sink.closed_with() == 1006
        assert b"".join(sink.frames[1:]) != data


@pytest.mark.asyncio
async def test_message_cut_by_asyncio_run_teardown_aborts_instead_of_closing_cleanly() -> None:
    sink = _Sink()
    async with _serve(sink) as url:

        async def main() -> None:
            writer = _writer(url)
            await writer.write(b"")
            assert writer._ws is not None
            send = writer._ws.send

            async def slow_send(message: Any) -> None:
                await asyncio.sleep(0.02)
                await send(message)

            writer._ws.send = slow_send  # type: ignore[method-assign]
            writer.send_message("one")
            writer.send_message("two")
            writer.close()

        await asyncio.to_thread(asyncio.run, main())
        assert await sink.closed_with() == 1006


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
async def test_writer_built_on_another_loop_runs_on_the_callers_loop() -> None:
    # Sync create_channel() builds the writer on the SDK loop; async user code then uses it on its own.
    sink = _Sink()
    other = asyncio.new_event_loop()
    thread = threading.Thread(target=other.run_forever, daemon=True)
    thread.start()
    try:
        async with _serve(sink) as url:

            async def build() -> ChannelWriter:
                return _writer(url)

            writer = await asyncio.wrap_future(asyncio.run_coroutine_threadsafe(build(), other))
            writer.stream.write(b"a")
            await asyncio.sleep(0)
            await writer.write(b"b")
            await writer.close_async()
            assert await sink.closed_with() == 1000
            assert sink.frames == [b"a", b"b"]
    finally:
        other.call_soon_threadsafe(other.stop)
        thread.join()
        other.close()


@pytest.mark.asyncio
async def test_sync_handler_does_not_wait_on_the_loop_that_built_the_writer() -> None:
    # Local sync call() from a coroutine: the writer is built on the caller's loop, which then blocks on the handler.
    sink = _Sink()
    async with _serve(sink) as url:

        async def caller() -> bool:
            writer = _writer(url)
            handler = threading.Thread(target=writer.stream.end, args=(b"x",))
            handler.start()
            handler.join(5)
            return not handler.is_alive()

        assert await asyncio.to_thread(asyncio.run, caller())
        assert await sink.closed_with() == 1000
        assert sink.frames == [b"x"]


@pytest.mark.asyncio
async def test_sync_only_writer_built_off_any_loop_writes_and_ends() -> None:
    sink = _Sink()
    async with _serve(sink) as url:

        def handler() -> State | None:
            writer = _writer(url)
            writer.stream.write(b"a")
            writer.stream.end(b"b")
            # A sync caller returns once the close is done: a script may exit right after end().
            return writer._ws.state if writer._ws else None

        assert await asyncio.to_thread(handler) is State.CLOSED
        assert await sink.closed_with() == 1000
        assert sink.frames == [b"a", b"b"]


@pytest.mark.asyncio
async def test_sync_writer_dropped_without_close_ends_its_socket_with_no_close_frame() -> None:
    # A sync handler that raises mid stream drops its writer: the reader must get an abort, not wait forever.
    sink = _Sink()
    async with _serve(sink) as url:

        def handler() -> None:
            _writer(url).stream.write(b"a")

        await asyncio.to_thread(handler)
        gc.collect()
        assert await sink.closed_with() == 1006
        assert sink.frames == [b"a"]


async def _drop_after_first_frame(ws: Any) -> None:
    await ws.recv()
    ws.transport.abort()


async def _until(condition: Callable[[], bool]) -> None:
    for _ in range(500):
        if condition():
            return
        await asyncio.sleep(0.01)
    raise AssertionError("condition not met within 5 s")


@pytest.mark.asyncio
async def test_sync_close_from_a_coroutine_runs_on_the_loop_that_owns_the_socket() -> None:
    sink = _Sink()
    async with _serve(sink) as url:
        writer = _writer(url)
        # A sync handler's write connects on the SDK's channel loop; async code then closes the writer.
        await asyncio.to_thread(writer.stream.write, b"a")
        writer.close()
        assert await sink.closed_with() == 1000
        # On this loop the close would fail after its frame and leave the writer marked connected.
        await _until(lambda: not writer._connected)
        assert sink.frames == [b"a"]


@pytest.mark.asyncio
async def test_failed_sync_call_posted_to_the_socket_loop_is_logged(caplog: pytest.LogCaptureFixture) -> None:
    async with _serve(_drop_after_first_frame) as url:
        writer = _writer(url)
        await asyncio.to_thread(writer.stream.write, b"a")
        assert writer._ws is not None
        ws = writer._ws
        await _until(lambda: ws.state is State.CLOSED)
        # Posted to the loop that owns the socket, and nothing waits for it.
        writer.stream.write(b"b")
        await _until(lambda: any(r.name == "iii.channels" for r in caplog.records))
    assert [r.getMessage() for r in caplog.records if r.name == "iii.channels"] == ["Channel writer call failed"]


@pytest.mark.asyncio
async def test_sync_close_after_the_socket_loop_stopped_returns() -> None:
    # A stopped loop runs nothing, so the close must not wait on it.
    other = asyncio.new_event_loop()
    thread = threading.Thread(target=other.run_forever, daemon=True)
    thread.start()
    async with _serve(_drop_after_first_frame) as url:
        writer = _writer(url)
        await asyncio.wrap_future(asyncio.run_coroutine_threadsafe(writer.write(b"a"), other))
        other.call_soon_threadsafe(other.stop)
        thread.join()

        def close() -> None:
            with contextlib.suppress(Exception):  # the socket belongs to the stopped loop, so it may fail
                writer.close()

        closer = threading.Thread(target=close, daemon=True)
        closer.start()
        await asyncio.to_thread(closer.join, 3)
        assert not closer.is_alive()
    other.close()


@pytest.mark.asyncio
async def test_writer_close_outlasts_a_stalled_reader() -> None:
    # websockets' 10 s default drops the queued close frame while the engine is not reading the socket.
    sink = _Sink()
    async with _serve(sink) as url:
        writer = _writer(url)
        await writer.write(b"data")
        assert writer._ws is not None
        assert writer._ws.close_timeout == WRITER_CLOSE_TIMEOUT_S >= 5 * 60
        await writer.close_async()
        assert await sink.closed_with() == 1000


@pytest.mark.asyncio
async def test_channel_sockets_send_no_keepalive_pings() -> None:
    # A slow consumer delays the pong behind queued data; a ping timeout would cut a healthy stream.
    sink = _Sink()
    async with _serve(sink) as url:
        writer = _writer(url)
        await writer.write(b"data")
        assert writer._ws is not None and writer._ws.ping_interval is None
        await writer.close_async()
    async with _serve(_sends_then(lambda ws: ws.close())) as url:
        reader = _reader(url)
        await reader.read_all()
        assert reader._ws is not None and reader._ws.ping_interval is None


@pytest.mark.asyncio
async def test_sync_close_from_another_thread_runs_on_the_owning_loop() -> None:
    sink = _Sink()
    async with _serve(sink) as url:
        writer = _writer(url)
        await writer.write(b"data")
        # A sync handler runs in a thread with no loop while the socket belongs to the SDK loop.
        await asyncio.to_thread(writer.close)
        assert await sink.closed_with() == 1000
        assert sink.frames == [b"data"]


@pytest.mark.asyncio
async def test_sync_handler_writes_and_ends_a_writer_the_sdk_loop_built() -> None:
    sink = _Sink()
    async with _serve(sink) as url:
        # The SDK builds the writer on its loop, then hands it to a sync handler's thread.
        writer = _writer(url)

        def handler() -> None:
            writer.stream.write(b"a")
            writer.send_message("progress")
            writer.stream.end(b"b")

        await asyncio.to_thread(handler)
        assert await sink.closed_with() == 1000
        assert sink.frames == [b"a", "progress", b"b"]
