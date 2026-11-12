"""Call-native streams: :class:`InputStream` and :class:`OutputStream` are values.

Put a stream in a call payload or a handler result, and the SDK carries it:
it creates one pipe per stream value (``engine::channels::create``), sends the
pipe's ref where the value was, and pumps the data. What a worker receives,
in a handler's input and in a call's result, has every ref turned back into a
stream. The JSON on the wire is the ref ``{channel_id, access_key,
direction}``, the same one older SDKs send and read as channel readers and
writers.

Streams are async. Every operation runs where the stream's socket (or local
source or sink) lives, so a stream works from any event loop or thread: sync
code (a sync handler, or after ``iii.call()``) runs one with
``asyncio.run(stream.read_all())``.
"""

from __future__ import annotations

import asyncio
import codecs
import contextvars
import inspect
import io
import json
import logging
import os
import threading
import uuid
from collections import deque
from collections.abc import AsyncIterable, AsyncIterator, Awaitable, Callable, Coroutine, Iterable
from concurrent.futures import Future
from dataclasses import dataclass, field
from typing import IO, Any, Literal, TypeVar, Union, cast

from pydantic import BaseModel, ConfigDict

from . import _wire_json
from ._channels import MAX_FRAME_SIZE, ChannelReader, ChannelWriter
from .errors import StreamAbortedError, StreamClosedError, StreamInUseError
from .iii_types import ChannelItem, StreamChannelRef

log = logging.getLogger("iii.streams")

T = TypeVar("T")

#: One frame of a stream: ``bytes`` is a binary frame, ``str`` a text frame (a message).
Frame = Union[bytes, str]

#: The pieces a stream source may yield.
Chunk = Union[bytes, bytearray, memoryview, str]

_DIRECTIONS = ("read", "write")


def _is_ref(value: Any) -> bool:
    return (
        isinstance(value, dict)
        and isinstance(value.get("channel_id"), str)
        and isinstance(value.get("access_key"), str)
        and value.get("direction") in _DIRECTIONS
    )


def _ref_of(value: dict[str, Any]) -> StreamChannelRef:
    return StreamChannelRef(
        channel_id=value["channel_id"], access_key=value["access_key"], direction=value["direction"]
    )


def _wire_ref(ref: StreamChannelRef) -> dict[str, Any]:
    """The JSON a ref travels as: exactly ``{channel_id, access_key, direction}``."""
    return {"channel_id": ref.channel_id, "access_key": ref.access_key, "direction": ref.direction}


def _reason(error: BaseException) -> str:
    return str(error) or type(error).__name__


@dataclass(frozen=True)
class _Binding:
    """Where a received stream's socket opens: the engine, the handshake headers, and the SDK loop."""

    engine_ws_base: str
    headers: dict[str, str] | None
    loop: asyncio.AbstractEventLoop


def _running_loop() -> asyncio.AbstractEventLoop | None:
    try:
        return asyncio.get_running_loop()
    except RuntimeError:
        return None


async def _on_home(home: asyncio.AbstractEventLoop | None, factory: Callable[[], Awaitable[T]]) -> T:
    """Runs ``factory()`` on ``home``, the loop the stream's socket or source lives on,
    awaiting it from the current loop. Here when ``home`` is this loop, unset, or stopped."""
    running = asyncio.get_running_loop()
    if home is None or home is running or home.is_closed() or not home.is_running():
        return await factory()

    async def run() -> T:
        return await factory()

    return await asyncio.wrap_future(asyncio.run_coroutine_threadsafe(run(), home))


class _Workers:
    """Threads the SDK owns for blocking work off the event loop (file I/O, sync
    iterators). Not ``concurrent.futures`` (the loop's default executor,
    ``asyncio.to_thread``): its pools take no new work once the main thread
    ended, and a worker process often lives on after its main thread returned
    (the SDK's loop thread keeps it alive). A thread is started when every
    thread is busy (up to ``MAX``) and ends after ``IDLE_S`` without work."""

    MAX = 32
    IDLE_S = 30.0

    def __init__(self) -> None:
        self._jobs: deque[Callable[[], None]] = deque()
        self._cond = threading.Condition()
        self._threads = 0
        self._idle = 0

    def submit(self, job: Callable[[], None]) -> None:
        with self._cond:
            self._jobs.append(job)
            if len(self._jobs) > self._idle and self._threads < self.MAX:
                self._threads += 1
                threading.Thread(target=self._work, name="iii-stream-io", daemon=True).start()
            self._cond.notify()

    def _work(self) -> None:
        while True:
            with self._cond:
                while not self._jobs:
                    self._idle += 1
                    woke = self._cond.wait(self.IDLE_S)
                    self._idle -= 1
                    if not woke and not self._jobs:
                        self._threads -= 1
                        return
                job = self._jobs.popleft()
            job()


_WORKERS = _Workers()


async def _offload(fn: Callable[..., T], *args: Any) -> T:
    """``fn(*args)`` on an SDK worker thread (in a copy of the current context),
    awaited from the running loop."""
    loop = asyncio.get_running_loop()
    future: asyncio.Future[T] = loop.create_future()
    context = contextvars.copy_context()

    def settle(ok: bool, outcome: Any) -> None:
        if future.done():
            return
        if ok:
            future.set_result(outcome)
        else:
            future.set_exception(outcome)

    def job() -> None:
        outcome: Any
        try:
            outcome, ok = context.run(fn, *args), True
        except BaseException as error:
            outcome, ok = error, False
        try:
            loop.call_soon_threadsafe(settle, ok, outcome)
        except RuntimeError:
            pass  # the loop is closed: nobody waits for it

    _WORKERS.submit(job)
    return await future


_detached: set[asyncio.Future[Any]] = set()


def _spawn(coro: Coroutine[Any, Any, Any]) -> asyncio.Future[Any]:
    """Runs a coroutine nobody awaits on the running loop, keeping it alive; its error is logged."""
    task = asyncio.ensure_future(coro)
    _detached.add(task)
    task.add_done_callback(_detached_done)
    return task


def _detached_done(task: asyncio.Future[Any]) -> None:
    _detached.discard(task)
    if task.cancelled():
        return
    error = task.exception()
    if error is not None:
        log.debug("Stream task failed: %r", error)


def _to_bytes(item: Any, where: str) -> bytes:
    if isinstance(item, bytes):
        return item
    if isinstance(item, (bytearray, memoryview)):
        return bytes(item)
    if isinstance(item, str):
        return item.encode("utf-8")
    raise TypeError(f"{where}: a stream source yields bytes or str, got {type(item).__name__}")


# ---------------------------------------------------------------------------
# Sources: where an InputStream's frames come from
# ---------------------------------------------------------------------------


class _Source:
    async def next(self) -> Frame | None:
        raise NotImplementedError

    async def close(self, *, open_first: bool = False) -> None:
        return None


class _RefSource(_Source):
    """The read end of a pipe, on the SDK loop."""

    def __init__(self, ref: StreamChannelRef, binding: _Binding) -> None:
        self._reader = ChannelReader(binding.engine_ws_base, ref, binding.headers)
        self._home = binding.loop

    async def next(self) -> Frame | None:
        return await _on_home(self._home, self._reader.recv)

    async def close(self, *, open_first: bool = False) -> None:
        await _on_home(self._home, lambda: self._reader.close(open_first=open_first))


class _BytesSource(_Source):
    def __init__(self, data: bytes) -> None:
        self._data: bytes | None = data

    async def next(self) -> Frame | None:
        data, self._data = self._data, None
        return data or None

    async def close(self, *, open_first: bool = False) -> None:
        self._data = None


class _IterSource(_Source):
    """A sync or async iterable of bytes or str (str is UTF-8 bytes). An async one is
    read on the loop it was wrapped on, while that loop runs. A sync one (but a list
    or a tuple) is advanced on a worker thread, one item at a time: a slow or
    blocking ``next()`` does not hold up the event loop."""

    def __init__(self, iterable: Any, home: asyncio.AbstractEventLoop | None) -> None:
        self._closed = False
        self._aiter: Any = None
        self._iter: Any = None
        #: Held while a sync ``next()`` runs: a generator cannot be closed while it runs.
        self._lock = threading.Lock()
        self._inline = False
        if isinstance(iterable, AsyncIterable) or hasattr(iterable, "__aiter__"):
            self._aiter = iterable.__aiter__()
            self._home = home
        else:
            # In memory: nothing to wait for, no thread needed.
            self._inline = isinstance(iterable, (list, tuple))
            self._iter = iter(iterable)
            self._home = None

    @property
    def home(self) -> asyncio.AbstractEventLoop | None:
        """The loop an async iterable is read on (``None`` for a sync one)."""
        return self._home

    async def next(self) -> Frame | None:
        if self._aiter is not None:
            return await _on_home(self._home, self._next_async)
        if self._inline:
            return self._next_sync()
        return await _offload(self._next_sync)

    def _next_sync(self) -> Frame | None:
        with self._lock:
            if self._closed:
                return None
            try:
                item = next(self._iter)
            except StopIteration:
                return None
        return _to_bytes(item, "InputStream.from_iter")

    def _close_sync(self) -> None:
        with self._lock:
            close = getattr(self._iter, "close", None)
            if callable(close):
                close()

    async def _next_async(self) -> Frame | None:
        if self._closed:
            return None
        try:
            item = await self._aiter.__anext__()
        except StopAsyncIteration:
            return None
        return _to_bytes(item, "InputStream.from_iter")

    async def close(self, *, open_first: bool = False) -> None:
        if self._closed:
            return
        self._closed = True
        if self._aiter is not None and hasattr(self._aiter, "aclose"):
            aclose = self._aiter.aclose
            try:
                await _on_home(self._home, aclose)
            except RuntimeError:
                pass  # running in another task right now: it stops at its next item
        elif self._iter is not None and hasattr(self._iter, "close"):
            if self._lock.acquire(blocking=False):
                try:
                    self._iter.close()
                finally:
                    self._lock.release()
            else:
                # A next() runs on a worker thread: close the iterator there once it returns.
                _spawn(_offload(self._close_sync))


def _open_for_reading(path: str) -> IO[bytes]:
    return open(path, "rb")


def _open_for_writing(path: str) -> IO[bytes]:
    return open(path, "wb")


class _FileSource(_Source):
    """A binary file, read in chunks on an SDK worker thread. A file opened from a path is
    closed at the end; a file object passed in stays open (its owner closes it)."""

    def __init__(self, file: IO[bytes] | None, path: str | None, chunk_size: int) -> None:
        self._file = file
        self._path = path
        self._owned = file is None
        self._chunk = chunk_size
        self._done = False

    async def next(self) -> Frame | None:
        if self._done:
            return None
        if self._file is None:
            path = self._path
            assert path is not None
            self._file = await _offload(_open_for_reading, path)
        data = await _offload(self._file.read, self._chunk)
        if not data:
            await self.close()
            return None
        return bytes(data)

    async def close(self, *, open_first: bool = False) -> None:
        self._done = True
        if self._owned and self._file is not None and not self._file.closed:
            self._file.close()


#: Frames a tee branch may hold for its reader before the other branch waits for it.
TEE_WINDOW_FRAMES = 16


class _Tee:
    """Two readers of one stream. A frame one branch read is held for the other until
    it reads it; a branch holds at most ``TEE_WINDOW_FRAMES`` frames, and the faster
    reader waits for the slower one past that. Safe across threads and event loops:
    the state is guarded by a lock, waits are thread-safe futures."""

    def __init__(self, parent: InputStream) -> None:
        self.parent = parent
        self.queues: tuple[deque[Frame], deque[Frame]] = (deque(), deque())
        self.alive = [True, True]
        #: ``(error,)`` once the parent ended (``error`` is ``None`` for a clean end).
        self.end: tuple[BaseException | None] | None = None
        self.pulling = False
        self.waiters: list[Future[None]] = []
        self.lock = threading.Lock()

    def _notify(self) -> None:
        waiters, self.waiters = self.waiters, []
        for waiter in waiters:
            if not waiter.done():
                waiter.set_result(None)

    async def next(self, branch: int) -> Frame | None:
        other = 1 - branch
        while True:
            with self.lock:
                queue = self.queues[branch]
                if queue:
                    frame = queue.popleft()
                    self._notify()
                    return frame
                if self.end is not None:
                    if self.end[0] is not None:
                        raise self.end[0]
                    return None
                pull = not self.pulling and not (self.alive[other] and len(self.queues[other]) >= TEE_WINDOW_FRAMES)
                if pull:
                    self.pulling = True
                else:
                    waiter: Future[None] = Future()
                    self.waiters.append(waiter)
            if not pull:
                await asyncio.wrap_future(waiter)
                continue
            try:
                pulled = await self.parent._next()
            except asyncio.CancelledError:
                with self.lock:
                    self.pulling = False
                    self._notify()
                raise
            except BaseException as error:
                with self.lock:
                    self.pulling = False
                    self.end = (error,)
                    self._notify()
                raise
            with self.lock:
                self.pulling = False
                if pulled is None:
                    self.end = (None,)
                else:
                    for index in (0, 1):
                        if self.alive[index]:
                            self.queues[index].append(pulled)
                self._notify()

    async def close(self, branch: int) -> None:
        with self.lock:
            self.alive[branch] = False
            self.queues[branch].clear()
            self._notify()
            both = not any(self.alive)
        if both:
            await self.parent._release()


class _TeeBranch(_Source):
    def __init__(self, tee: _Tee, branch: int) -> None:
        self._tee = tee
        self._branch = branch

    async def next(self) -> Frame | None:
        return await self._tee.next(self._branch)

    async def close(self, *, open_first: bool = False) -> None:
        await self._tee.close(self._branch)


# ---------------------------------------------------------------------------
# InputStream
# ---------------------------------------------------------------------------


class InputStream:
    """A stream to read: bytes in binary frames, messages in text frames.

    Received in a handler's input or in a call's result wherever the sender put
    a stream (a ref with ``"direction": "read"``). Build one to send with
    :meth:`from_file`, :meth:`from_iter` or :meth:`from_bytes`; an open binary
    file and an async generator in a payload are streamed as well.

    A stream has one reader: a second iteration, :meth:`read_all`,
    :meth:`text`, :meth:`lines`, :meth:`messages` or :meth:`frames` raises
    :class:`RuntimeError`; call :meth:`tee` before reading for two copies.
    A received stream opens its socket on the first read. When the writer
    aborts the stream, the read raises :class:`~iii.StreamAbortedError` once
    the data sent before the abort has been read.

    Examples:
        >>> async def process(payload: dict) -> dict:
        ...     total = 0
        ...     async for chunk in payload["body"]:
        ...         total += len(chunk)
        ...     return {"bytes": total}
    """

    def __init__(
        self,
        _source: _Source | None = None,
        *,
        _ref: StreamChannelRef | None = None,
        _binding: _Binding | None = None,
    ) -> None:
        if _source is None and (_ref is None or _binding is None):
            raise TypeError(
                "InputStream is not built directly: use InputStream.from_file(), from_iter() or from_bytes(), "
                "or iii.pipe()"
            )
        self._source = _source
        self._ref = _ref
        self._binding = _binding
        self._id = _ref.channel_id if _ref is not None else f"local:{uuid.uuid4().hex}"
        #: A reader took the stream (iteration, read_all, tee, ...).
        self._claimed = False
        #: A frame was asked for.
        self._started = False
        #: The end (clean or not) was reached, or the stream was cancelled.
        self._finished = False
        self._cancelled = False
        #: Passed on to another call (by ref, or through a pump).
        self._transferred = False
        #: The stream this one is a tee branch of.
        self._parent: InputStream | None = None

    # -- construction --------------------------------------------------------

    @classmethod
    def from_bytes(cls, data: bytes | bytearray | memoryview | str) -> InputStream:
        """A stream of ``data`` (``str`` is sent as UTF-8 bytes)."""
        return cls(_BytesSource(_to_bytes(data, "InputStream.from_bytes")))

    @classmethod
    def from_file(cls, file: str | os.PathLike[str] | IO[bytes], *, chunk_size: int = MAX_FRAME_SIZE) -> InputStream:
        """A stream of a file's bytes, read in ``chunk_size`` pieces off the event loop.

        A path is opened on the first read and closed at the end. An open
        binary file object is read from its position and left open.

        Raises:
            FileNotFoundError: the path names no file.
        """
        if isinstance(file, (str, os.PathLike)):
            path = os.fspath(file)
            if not os.path.isfile(path):
                raise FileNotFoundError(f"InputStream.from_file: no such file: {path!r}")
            return cls(_FileSource(None, path, chunk_size))
        if not callable(getattr(file, "read", None)):
            raise TypeError(f"InputStream.from_file takes a path or a binary file object, got {type(file).__name__}")
        return cls(_FileSource(file, None, chunk_size))

    @classmethod
    def from_iter(cls, iterable: Iterable[Chunk] | AsyncIterable[Chunk]) -> InputStream:
        """A stream of what ``iterable`` yields: ``bytes`` (``str`` is sent as UTF-8 bytes).

        An async iterable (an async generator) is read on the event loop it was
        wrapped on while that loop runs, so it may await what lives there. A
        sync iterable (but a list or a tuple) is advanced on an SDK thread, one
        item at a time, so a blocking ``next()`` does not hold up the loop.
        Any other item fails the stream: its reader gets
        :class:`~iii.StreamAbortedError`.
        """
        return cls(_IterSource(iterable, _running_loop()))

    @classmethod
    def _from_ref(cls, ref: StreamChannelRef, binding: _Binding) -> InputStream:
        return cls(_ref=ref, _binding=binding)

    # -- identity ------------------------------------------------------------

    @property
    def id(self) -> str:
        """The channel id of a received stream; ``local:<hex>`` for one built in this process."""
        return self._id

    def __repr__(self) -> str:
        return f"InputStream(id={self._id!r})"

    def __copy__(self) -> InputStream:
        return self

    def __deepcopy__(self, memo: dict[int, Any]) -> InputStream:
        # A payload copy (local dispatch) keeps the stream itself: it has one reader.
        return self

    def __iii_json__(self) -> dict[str, Any]:
        """What a filter or an observer report sees: the stream's :class:`StreamInfo`."""
        return {"stream": True, "direction": "read", "id": self._id}

    # -- reading -------------------------------------------------------------

    def _claim(self) -> None:
        if self._transferred:
            raise StreamInUseError("this InputStream was passed on to another call; it cannot be read here")
        if self._claimed:
            raise StreamInUseError(
                "this InputStream is already being read: a stream has one reader (call tee() before reading "
                "for two copies)"
            )
        self._claimed = True

    def _get_source(self) -> _Source:
        if self._source is None:
            assert self._ref is not None and self._binding is not None
            self._source = _RefSource(self._ref, self._binding)
        return self._source

    async def _next(self) -> Frame | None:
        """The next frame, ``None`` at the end; tracks how far the stream was read."""
        if self._finished:
            return None
        self._started = True
        try:
            frame = await self._get_source().next()
        except asyncio.CancelledError:
            raise
        except BaseException:
            self._finished = True
            raise
        if frame is None:
            self._finished = True
        return frame

    async def _release(self) -> None:
        """Stop reading (cancel, the handler guard, a pump whose reader left)."""
        if self._finished and self._source is None:
            return
        self._finished = True
        if self._source is not None:
            try:
                await self._source.close()
            except Exception as error:
                log.debug("Closing %r failed: %s", self, error)

    def __aiter__(self) -> AsyncIterator[bytes]:
        """The bytes of the binary frames, in order (text frames are skipped)."""
        self._claim()
        return self._binary()

    async def _binary(self) -> AsyncIterator[bytes]:
        while (frame := await self._next()) is not None:
            if isinstance(frame, bytes):
                yield frame

    async def read_all(self) -> bytes:
        """Every byte of the stream, once it ended (text frames are skipped)."""
        self._claim()
        return b"".join([chunk async for chunk in self._binary()])

    async def text(self, encoding: str = "utf-8", errors: str = "strict") -> str:
        """The stream's bytes, decoded."""
        return (await self.read_all()).decode(encoding, errors)

    def lines(self, encoding: str = "utf-8") -> AsyncIterator[str]:
        """The stream's text, one line at a time, without the line ending (``\\n``
        or ``\\r\\n``); a last line with no newline is yielded too."""
        self._claim()
        return self._lines(encoding)

    async def _lines(self, encoding: str) -> AsyncIterator[str]:
        decoder = codecs.getincrementaldecoder(encoding)()
        pending = ""
        async for chunk in self._binary():
            pending += decoder.decode(chunk)
            start = 0
            while (end := pending.find("\n", start)) != -1:
                line = pending[start:end]
                yield line[:-1] if line.endswith("\r") else line
                start = end + 1
            pending = pending[start:]
        pending += decoder.decode(b"", final=True)
        if pending:
            yield pending[:-1] if pending.endswith("\r") else pending

    def messages(self) -> AsyncIterator[Any]:
        """The messages (text frames), each parsed as JSON; a text frame that is not
        JSON is yielded as the string it is. Binary frames are skipped."""
        self._claim()
        return self._messages()

    async def _messages(self) -> AsyncIterator[Any]:
        while (frame := await self._next()) is not None:
            if isinstance(frame, str):
                try:
                    yield json.loads(frame)
                except ValueError:
                    yield frame

    def frames(self) -> AsyncIterator[ChannelItem]:
        """Every frame in order: :class:`~iii.protocol.ChannelItem` with ``binary``
        (bytes) or ``text`` (a message, as sent)."""
        self._claim()
        return self._frames()

    async def _frames(self) -> AsyncIterator[ChannelItem]:
        while (frame := await self._next()) is not None:
            yield ChannelItem.binary_item(frame) if isinstance(frame, bytes) else ChannelItem.text_item(frame)

    def tee(self) -> tuple[InputStream, InputStream]:
        """Two streams that each read every frame of this one (in-process copies).
        Call it before reading; this stream is then read through the copies.

        Read both copies at the same time (``asyncio.gather``, two tasks, or two
        threads): a frame one copy read is held for the other, at most 16 frames
        per copy, and past that the faster reader waits for the slower one. Reading
        one copy to the end before starting the other stalls. Cancel a copy you
        stop reading, so the other is not held back."""
        self._claim()
        tee = _Tee(self)
        first, second = InputStream(_TeeBranch(tee, 0)), InputStream(_TeeBranch(tee, 1))
        first._parent = second._parent = self
        return first, second

    async def cancel(self) -> None:
        """Stop reading: the writer learns the reader left. A read in progress ends,
        and later reads find the end of the stream."""
        if self._cancelled:
            return
        self._cancelled = True
        if self._finished:
            return
        self._finished = True
        if self._source is None and self._ref is not None and self._binding is not None:
            # Never read: open the socket to close it, so the writer stops waiting for a reader.
            self._source = _RefSource(self._ref, self._binding)
            await self._source.close(open_first=True)
        elif self._source is not None:
            await self._source.close()

    # -- forwarding ------------------------------------------------------------

    def _forwardable(self) -> bool:
        """Received, never read: it travels on as its ref, and no byte passes through this worker."""
        return self._ref is not None and self._source is None and not self._claimed and not self._finished

    def _mark_transferred(self) -> None:
        self._transferred = True
        if self._parent is not None:
            self._parent._transferred = True


# ---------------------------------------------------------------------------
# Sinks: where a locally built OutputStream's bytes go
# ---------------------------------------------------------------------------


class _Sink:
    home: asyncio.AbstractEventLoop | None = None

    async def write(self, data: bytes) -> None:
        raise NotImplementedError

    async def message(self, text: str) -> None:
        # Files and writers take bytes: a message has nowhere to go.
        log.debug("Message to a byte sink dropped: %.80s", text)

    async def close(self) -> None:
        return None

    async def abort(self, reason: str) -> None:
        return None


class _FileSink(_Sink):
    """A file opened (created or truncated) on the first write, or on close for an empty stream."""

    def __init__(self, path: str) -> None:
        self._path = path
        self._file: IO[bytes] | None = None

    async def _open(self) -> IO[bytes]:
        if self._file is None:
            self._file = await _offload(_open_for_writing, self._path)
        return self._file

    async def write(self, data: bytes) -> None:
        file = await self._open()
        await _offload(file.write, data)

    async def close(self) -> None:
        file = await self._open()
        await _offload(file.close)

    async def abort(self, reason: str) -> None:
        # What was written stays; the call that carried the stream rejects.
        if self._file is not None and not self._file.closed:
            self._file.close()


class _WriterSink(_Sink):
    """Anything with ``write``: a binary file object, an ``asyncio.StreamWriter``, ... It is
    flushed (or drained) at the end and left open: its owner closes it."""

    def __init__(self, target: Any, home: asyncio.AbstractEventLoop | None) -> None:
        self._target = target
        self._blocking = isinstance(target, io.IOBase)
        # A file object is written on a worker thread from any loop; another writer
        # (an asyncio.StreamWriter, say) is used on the loop it was passed on.
        self.home = None if self._blocking else home

    async def _call(self, method: Callable[..., Any], *args: Any) -> None:
        if self._blocking:
            await _offload(method, *args)
            return
        result = method(*args)
        if inspect.isawaitable(result):
            await result

    async def write(self, data: bytes) -> None:
        await self._call(self._target.write, data)
        drain = getattr(self._target, "drain", None)
        if callable(drain):
            await self._call(drain)

    async def close(self) -> None:
        flush = getattr(self._target, "flush", None)
        if callable(flush):
            await self._call(flush)


# ---------------------------------------------------------------------------
# OutputStream
# ---------------------------------------------------------------------------


class OutputStream:
    """A stream to write: :meth:`write` sends bytes, :meth:`send` sends a message.

    Received in a handler's input or in a call's result wherever the sender put
    a sink (a ref with ``"direction": "write"``). Build one to pass with
    :meth:`to_file` or :meth:`to`: the call resolves once what the function
    wrote reached it.

    In a handler, a received ``OutputStream`` that was written to is closed
    when the handler returns and aborted when it raises. One the handler never
    used is closed when it returns, unless the invocation is an observer
    activation (an HTTP route target, say) that returned a value other than
    ``None``: then it is left untouched. Call :meth:`keep_open` to keep writing
    after the handler returns. ``async
    with out:`` closes it at the end of the block, or aborts it on an error.

    Examples:
        >>> async def render_pdf(payload: dict) -> None:
        ...     out: OutputStream = payload["out"]
        ...     async for page in render_pages(payload["report_id"]):
        ...         await out.write(page)
    """

    def __init__(
        self,
        _sink: _Sink | None = None,
        *,
        _ref: StreamChannelRef | None = None,
        _binding: _Binding | None = None,
    ) -> None:
        if _sink is None and (_ref is None or _binding is None):
            raise TypeError(
                "OutputStream is not built directly: use OutputStream.to_file() or OutputStream.to(), or iii.pipe()"
            )
        self._sink = _sink
        self._ref = _ref
        self._binding = _binding
        self._writer: ChannelWriter | None = None
        self._home = _binding.loop if _binding is not None else (_sink.home if _sink is not None else None)
        self._id = _ref.channel_id if _ref is not None else f"local:{uuid.uuid4().hex}"
        self._used = False
        self._closed = False
        self._aborted: str | None = None
        self._keep_open = False
        self._transferred = False
        #: Settles when the stream closed (result) or aborted (StreamAbortedError); thread-safe.
        self._done: Future[None] = Future()

    # -- construction --------------------------------------------------------

    @classmethod
    def to_file(cls, path: str | os.PathLike[str]) -> OutputStream:
        """A sink that writes the stream's bytes to ``path`` (created or truncated on the
        first write; an empty stream leaves an empty file). Messages are not written.
        On an abort the file keeps what was written, and the call rejects."""
        return cls(_FileSink(os.fspath(path)))

    @classmethod
    def to(cls, writer: Any) -> OutputStream:
        """A sink that writes the stream's bytes to ``writer``: a binary file object, an
        ``asyncio.StreamWriter``, or anything with ``write(bytes)`` (sync or async).
        It is flushed or drained at the end and never closed: its owner closes it.
        Messages are not written. A writer that is not a file object (an
        ``asyncio`` writer, say) is used on the loop it was passed on."""
        if not callable(getattr(writer, "write", None)):
            raise TypeError(f"OutputStream.to takes an object with write(), got {type(writer).__name__}")
        return cls(_WriterSink(writer, _running_loop()))

    @classmethod
    def _from_ref(cls, ref: StreamChannelRef, binding: _Binding) -> OutputStream:
        return cls(_ref=ref, _binding=binding)

    # -- identity ------------------------------------------------------------

    @property
    def id(self) -> str:
        """The channel id of a received stream; ``local:<hex>`` for one built in this process."""
        return self._id

    def __repr__(self) -> str:
        return f"OutputStream(id={self._id!r})"

    def __copy__(self) -> OutputStream:
        return self

    def __deepcopy__(self, memo: dict[int, Any]) -> OutputStream:
        return self

    def __iii_json__(self) -> dict[str, Any]:
        """What a filter or an observer report sees: the stream's :class:`StreamInfo`."""
        return {"stream": True, "direction": "write", "id": self._id}

    # -- writing -------------------------------------------------------------

    def _check_writable(self) -> None:
        if self._transferred:
            raise StreamInUseError("this OutputStream was passed on to another call; it cannot be written here")
        if self._aborted is not None:
            raise StreamClosedError(f"this OutputStream was aborted ({self._aborted})")
        if self._closed:
            raise StreamClosedError("this OutputStream is closed")

    def _channel(self) -> ChannelWriter:
        if self._writer is None:
            assert self._ref is not None and self._binding is not None
            self._writer = ChannelWriter(self._binding.engine_ws_base, self._ref, self._binding.headers)
        return self._writer

    async def _put(self, data: bytes) -> None:
        sink = self._sink
        try:
            if sink is not None:
                await _on_home(sink.home, lambda: sink.write(data))
            else:
                await _on_home(self._home, lambda: self._channel().write(data))
        except asyncio.CancelledError:
            raise
        except Exception as error:
            raise self._write_failed(error) from error

    async def _put_text(self, text: str) -> None:
        sink = self._sink
        try:
            if sink is not None:
                await _on_home(sink.home, lambda: sink.message(text))
            else:
                await _on_home(self._home, lambda: self._channel().send_text(text))
        except asyncio.CancelledError:
            raise
        except Exception as error:
            raise self._write_failed(error) from error

    def _write_failed(self, error: Exception) -> Exception:
        import websockets

        if isinstance(error, websockets.ConnectionClosed):
            return StreamAbortedError("the reader closed the stream")
        return error

    async def write(self, data: bytes | bytearray | memoryview | str) -> None:
        """Send bytes (``str`` is sent as UTF-8 bytes) in binary frames; returns once
        they were accepted (backpressure).

        Raises:
            RuntimeError: the stream is closed, aborted or was passed on.
            StreamAbortedError: the reader left.
        """
        self._check_writable()
        self._used = True
        await self._put(_to_bytes(data, "OutputStream.write"))

    async def send(self, message: Any) -> None:
        """Send ``message`` as JSON in one text frame (a file or writer sink drops it).

        Raises:
            RuntimeError: the stream is closed, aborted or was passed on.
            StreamAbortedError: the reader left.
        """
        self._check_writable()
        self._used = True
        await self._put_text(_wire_json.dumps(message))

    async def _finish(self) -> None:
        if self._closed or self._aborted is not None:
            return
        self._closed = True
        sink = self._sink
        try:
            if sink is not None:
                await _on_home(sink.home, sink.close)
            else:
                await _on_home(self._home, lambda: self._channel().close())
        except BaseException as error:
            if not self._done.done():
                self._done.set_exception(error)
            raise
        if not self._done.done():
            self._done.set_result(None)

    async def _fail(self, reason: str) -> None:
        if self._closed or self._aborted is not None:
            return
        self._aborted = reason
        if not self._done.done():
            self._done.set_exception(StreamAbortedError(reason))
        sink = self._sink
        try:
            if sink is not None:
                await _on_home(sink.home, lambda: sink.abort(reason))
            else:
                await _on_home(self._home, lambda: self._channel().abort(reason))
        except Exception as error:
            log.debug("Aborting %r failed: %s", self, error)

    async def close(self) -> None:
        """End the stream: the reader sees a clean end. A stream never written is
        opened and closed (the reader sees an empty stream). Idempotent; a no-op
        once aborted or passed on."""
        if self._transferred:
            return
        await self._finish()

    async def abort(self, reason: str | BaseException | None = None) -> None:
        """End the stream as failed: the reader gets
        :class:`~iii.StreamAbortedError` instead of a stream that looks complete.
        A stream never written is opened first, so the reader always learns of
        the failure. Idempotent; a no-op once closed or passed on."""
        if self._transferred:
            return
        text = _reason(reason) if isinstance(reason, BaseException) else (reason or "aborted")
        await self._fail(text)

    def keep_open(self) -> OutputStream:
        """Keep the stream open after the handler that received it returns: the
        handler's code then closes or aborts it."""
        self._keep_open = True
        return self

    async def __aenter__(self) -> OutputStream:
        self._check_writable()
        self._used = True
        return self

    async def __aexit__(self, exc_type: Any, exc: BaseException | None, tb: Any) -> None:
        if exc is not None:
            await self.abort(exc)
        else:
            await self.close()

    # -- forwarding and completion ------------------------------------------

    def _forwardable(self) -> bool:
        """Received, never used: it travels on as its ref."""
        return (
            self._ref is not None
            and self._writer is None
            and not self._used
            and not self._closed
            and self._aborted is None
        )

    def _untouched(self) -> bool:
        return not (self._used or self._closed or self._aborted is not None or self._keep_open or self._transferred)

    async def _wait(self) -> None:
        """Until the stream closed; raises :class:`StreamAbortedError` if it aborted."""
        await asyncio.wrap_future(self._done)


class StreamInfo(BaseModel):
    """What an observer sees where the observed call carried a stream: that a stream
    was passed, its direction and its channel id, and no bytes.

    An observer's target receives it in the ``input`` and ``output`` of the
    :class:`~iii.Observed` payload iii delivers. The access key still travels on
    the wire (a 0.26 engine sends the ref); this SDK drops it.
    """

    model_config = ConfigDict(frozen=True)

    stream: Literal[True] = True
    direction: Literal["read", "write"]
    id: str


@dataclass(frozen=True)
class Pipe:
    """Both ends of a pipe made with ``iii.pipe()``: pass ``input`` to the function
    that reads, write ``output`` here (or the other way round)."""

    input: InputStream
    output: OutputStream


# ---------------------------------------------------------------------------
# The walks
# ---------------------------------------------------------------------------


def _is_binary_reader(value: Any) -> bool:
    if not isinstance(value, (io.BufferedIOBase, io.RawIOBase)):
        return False
    try:
        return not value.closed and value.readable()
    except ValueError:
        return False


_ATOMS = (str, int, float, bool, bytes, type(None))


def _reject_unstreamable(value: Any) -> None:
    """A value that looks like a stream but is not one: a clear error, not the JSON
    encoder's (which names an internal class)."""
    if isinstance(value, io.TextIOBase):
        raise TypeError(
            "a text-mode file cannot be sent: only bytes are streamed. Open the file in binary mode "
            "(open(path, 'rb')) or pass InputStream.from_file(path)"
        )
    if inspect.isgenerator(value):
        raise TypeError(
            "a sync generator cannot be sent: streams are InputStream values, async generators and open "
            "binary files. Wrap it with InputStream.from_iter(...) to stream what it yields (bytes or str), "
            "or pass list(...) for a JSON array"
        )


def _check_forwardable_input(value: InputStream) -> None:
    """A stream read to the end has nothing left to pass on: the same error as reading it again."""
    if value._claimed and value._finished and not value._cancelled:
        raise StreamInUseError(
            f"{value!r} was already read to the end: a stream has one reader (call tee() before reading for two copies)"
        )


@dataclass
class StreamScan:
    """What :func:`wrap_sources` found: stream values, and refs built by hand."""

    streams: bool = False
    refs: bool = False


def wrap_sources(value: Any, scan: StreamScan, *, here: bool = True) -> Any:
    """``value`` with its raw sources (async generators, readable binary files) as
    :class:`InputStream`; containers are rebuilt only along the path to one. An
    async generator is read on the running loop when ``here``, else wherever it
    is read (a sync caller blocks its loop while the call runs)."""
    if isinstance(value, _ATOMS):
        return value
    if isinstance(value, (InputStream, OutputStream)):
        scan.streams = True
        return value
    if isinstance(value, dict):
        if _is_ref(value):
            scan.refs = True
            return value
        out: dict[Any, Any] | None = None
        for key, member in value.items():
            wrapped = wrap_sources(member, scan, here=here)
            if wrapped is not member:
                if out is None:
                    out = dict(value)
                out[key] = wrapped
        return value if out is None else out
    if isinstance(value, (list, tuple)):
        items: list[Any] | None = None
        for index, member in enumerate(value):
            wrapped = wrap_sources(member, scan, here=here)
            if wrapped is not member:
                if items is None:
                    items = list(value)
                items[index] = wrapped
        if items is None:
            return value
        return items if isinstance(value, list) else tuple(items)
    if inspect.isasyncgen(value):
        scan.streams = True
        return InputStream(_IterSource(value, _running_loop() if here else None))
    if _is_binary_reader(value):
        scan.streams = True
        return InputStream.from_file(value)
    _reject_unstreamable(value)
    return value


def collect_streams(value: Any, found: list[InputStream | OutputStream]) -> list[InputStream | OutputStream]:
    """Every stream value in ``value``, in order."""
    if isinstance(value, (InputStream, OutputStream)):
        found.append(value)
    elif isinstance(value, dict):
        for member in value.values():
            collect_streams(member, found)
    elif isinstance(value, (list, tuple)):
        for member in value:
            collect_streams(member, found)
    return found


def materialize(value: Any, binding: _Binding, tracked: list[InputStream | OutputStream] | None) -> Any:
    """``value`` with its refs as streams bound to ``binding`` (added to ``tracked``).
    Containers are rebuilt only along the path to a ref: without refs it is
    ``value`` itself."""
    if isinstance(value, dict):
        if _is_ref(value):
            ref = _ref_of(value)
            stream: InputStream | OutputStream = (
                InputStream._from_ref(ref, binding) if ref.direction == "read" else OutputStream._from_ref(ref, binding)
            )
            if tracked is not None:
                tracked.append(stream)
            return stream
        out: dict[Any, Any] | None = None
        for key, member in value.items():
            resolved = materialize(member, binding, tracked)
            if resolved is not member:
                if out is None:
                    out = dict(value)
                out[key] = resolved
        return value if out is None else out
    if isinstance(value, (list, tuple)):
        items = [materialize(member, binding, tracked) for member in value]
        if all(new is old for new, old in zip(items, value)):
            return value
        return items if isinstance(value, list) else tuple(items)
    return value


def _is_stream_info(value: dict[Any, Any]) -> bool:
    return (
        value.get("stream") is True
        and value.get("direction") in _DIRECTIONS
        and isinstance(value.get("id"), str)
        and len(value) == 3
    )


def to_stream_info(value: Any) -> Any:
    """``value`` with its refs (and stream descriptions) as :class:`StreamInfo`."""
    if isinstance(value, dict):
        if _is_ref(value):
            return StreamInfo(direction=value["direction"], id=value["channel_id"])
        if _is_stream_info(value):
            return StreamInfo(direction=value["direction"], id=value["id"])
        out: dict[Any, Any] | None = None
        for key, member in value.items():
            info = to_stream_info(member)
            if info is not member:
                if out is None:
                    out = dict(value)
                out[key] = info
        return value if out is None else out
    if isinstance(value, (list, tuple)):
        items = [to_stream_info(member) for member in value]
        if all(new is old for new, old in zip(items, value)):
            return value
        return items if isinstance(value, list) else tuple(items)
    return value


def is_observed_envelope(data: Any, activation: dict[str, Any] | None) -> bool:
    """An engine-built ``Observed`` payload: the activation's ids are in it. A call
    through an observer gets its ``activation_id`` from the engine after the
    caller built the payload, so it never matches."""
    if not activation or not isinstance(data, dict):
        return False
    activation_id = activation.get("activation_id")
    return (
        isinstance(activation_id, str)
        and data.get("activation_id") == activation_id
        and data.get("observer_id") == activation.get("observer_id")
    )


def materialize_observed(
    data: dict[str, Any], binding: _Binding, tracked: list[InputStream | OutputStream] | None
) -> dict[str, Any]:
    """An ``Observed`` payload: refs under ``input`` and ``output`` as
    :class:`StreamInfo` (the observer gets no live end), the rest as streams."""
    out: dict[str, Any] = {}
    for key, member in data.items():
        out[key] = to_stream_info(member) if key in ("input", "output") else materialize(member, binding, tracked)
    return out


# -- outbound ------------------------------------------------------------------

PipeFactory = Callable[[], Awaitable[tuple[StreamChannelRef, StreamChannelRef]]]
"""Creates one pipe: ``(writer ref, reader ref)``."""


@dataclass
class _Need:
    kind: Literal["source", "sink"]
    stream: InputStream | OutputStream
    container: Any
    key: Any


@dataclass
class Outbound:
    """A value about to be sent: stream values replaced by refs, the pipes it
    still needs, and the pumps that carry the data once it is sent."""

    value: Any
    needs: list[_Need] = field(default_factory=list)
    marked: list[InputStream | OutputStream] = field(default_factory=list)
    sources: list[tuple[InputStream, ChannelWriter]] = field(default_factory=list)
    sinks: list[tuple[ChannelReader, OutputStream]] = field(default_factory=list)
    sink_tasks: list[asyncio.Future[None]] = field(default_factory=list)
    source_tasks: list[asyncio.Future[None]] = field(default_factory=list)

    @property
    def has_sinks(self) -> bool:
        return bool(self.sinks)

    async def create_pipes(self, create_pipe: PipeFactory, binding: _Binding) -> None:
        """One pipe per stream that is not forwarded, in parallel; their refs go where
        the streams were. A failure restores the streams and raises."""
        if not self.needs:
            return
        results = await asyncio.gather(*(create_pipe() for _ in self.needs), return_exceptions=True)
        for result in results:
            if isinstance(result, BaseException):
                self.restore()
                raise result
        for need, result in zip(self.needs, results):
            assert not isinstance(result, BaseException)
            writer_ref, reader_ref = result
            if need.kind == "source":
                ref = reader_ref
                self.sources.append(
                    (
                        need.stream,  # type: ignore[arg-type]
                        ChannelWriter(binding.engine_ws_base, writer_ref, binding.headers),
                    )
                )
            else:
                ref = writer_ref
                self.sinks.append(
                    (
                        ChannelReader(binding.engine_ws_base, reader_ref, binding.headers),
                        need.stream,  # type: ignore[arg-type]
                    )
                )
            if need.container is None:
                self.value = _wire_ref(ref)
            else:
                need.container[need.key] = _wire_ref(ref)
        self.needs = []

    def restore(self) -> None:
        """Nothing was sent: the streams are as they were."""
        for stream in self.marked:
            stream._transferred = False
            if isinstance(stream, InputStream) and stream._parent is not None:
                stream._parent._transferred = False
        self.marked = []

    def start(self) -> None:
        """Starts the pumps on the running loop (the SDK loop)."""
        for stream, writer in self.sources:
            self.source_tasks.append(_spawn(pump_source(stream, writer)))
        for reader, sink in self.sinks:
            self.sink_tasks.append(_spawn(pump_sink(reader, sink)))

    async def wait_sinks(self, result: Any = None, timeout: float | None = None) -> bool:
        """Until every sink finished (R4), for at most ``timeout`` seconds: ``False``
        when it expired first (the caller then aborts the open sinks:
        :meth:`abort_sinks`). Raises what the first failed pump raised.

        Not when the call's ``result`` (as it arrived) hands the caller a stream to
        write (a write ref): the caller can only write it once the call returned, and
        a sink may depend on it (its own ref handed back, or a sink the handler wrote
        to and returned, bridged under a new ref). Waiting would never end; the sinks
        finish in the background as the caller writes."""
        if not self.sink_tasks or has_write_ref(result):
            return True
        _, pending = await asyncio.wait(self.sink_tasks, timeout=timeout)
        if pending:
            return False
        for task in self.sink_tasks:
            if not task.cancelled() and task.exception() is not None:
                raise cast(BaseException, task.exception())
        return True

    async def abort_sinks(self, reason: str) -> None:
        """The sinks still open: their pumps stopped, their pipe readers closed (the
        writer learns the reader left), the sinks aborted with ``reason``."""
        for task, (reader, sink) in zip(self.sink_tasks, self.sinks):
            if task.done():
                continue
            task.cancel()
            _spawn(reader.close())
            await sink._fail(reason)

    async def cancel(self, reason: str) -> None:
        """The call failed (or was never sent): stop the pumps, abort the sinks (done
        when this returns) and the streams this call was to carry (in the background)."""
        for task in self.sink_tasks + self.source_tasks:
            task.cancel()
        for stream, writer in self.sources:
            _spawn(_abort_source(stream, writer, reason))
        for reader, _ in self.sinks:
            _spawn(reader.close())
        await asyncio.gather(*(sink._fail(reason) for _, sink in self.sinks), return_exceptions=True)


async def _abort_source(stream: InputStream, writer: ChannelWriter, reason: str) -> None:
    await stream._release()
    await writer.abort(reason)


def plan_outbound(value: Any) -> Outbound:
    """Replaces the stream values of ``value`` (a call payload or a handler result):
    a received stream never used goes on as its ref; any other needs a pipe. Raw
    sources are wrapped first. The value is rebuilt along the path to a stream."""
    plan = Outbound(value=None)
    try:
        planned = _plan(value, plan)
    except BaseException:
        plan.restore()
        raise
    if isinstance(planned, _Placeholder):
        planned.need.container = None
    plan.value = planned
    return plan


class _Placeholder:
    """Where a pipe's ref goes once the pipe exists."""

    __slots__ = ("need",)

    def __init__(self, need: _Need) -> None:
        self.need = need


def _placed(member: Any, container: Any, key: Any) -> Any:
    if isinstance(member, _Placeholder):
        member.need.container, member.need.key = container, key
    return member


def _plan(value: Any, plan: Outbound) -> Any:
    if isinstance(value, _ATOMS):
        return value
    if inspect.isasyncgen(value):
        value = InputStream.from_iter(value)
    elif _is_binary_reader(value):
        value = InputStream.from_file(value)
    if isinstance(value, InputStream):
        if value._transferred:
            raise StreamInUseError(f"{value!r} was already passed on to another call")
        _check_forwardable_input(value)
        forward = value._forwardable()
        value._mark_transferred()
        plan.marked.append(value)
        if forward:
            assert value._ref is not None
            return _wire_ref(value._ref)
        need = _Need("source", value, None, None)
        plan.needs.append(need)
        return _Placeholder(need)
    if isinstance(value, OutputStream):
        if value._transferred:
            raise StreamInUseError(f"{value!r} was already passed on to another call")
        if value._closed or value._aborted is not None:
            raise StreamClosedError(f"{value!r} is closed; it cannot be passed on")
        forward = value._forwardable()
        value._transferred = True
        plan.marked.append(value)
        if forward:
            assert value._ref is not None
            return _wire_ref(value._ref)
        need = _Need("sink", value, None, None)
        plan.needs.append(need)
        return _Placeholder(need)
    if isinstance(value, dict):
        if _is_ref(value):
            return value
        out: dict[Any, Any] | None = None
        for key, member in value.items():
            planned = _plan(member, plan)
            if planned is not member:
                if out is None:
                    out = dict(value)
                out[key] = _placed(planned, out, key)
        return value if out is None else out
    if isinstance(value, (list, tuple)):
        items: list[Any] | None = None
        for index, member in enumerate(value):
            planned = _plan(member, plan)
            if planned is not member:
                if items is None:
                    items = list(value)
                items[index] = _placed(planned, items, index)
        # A tuple goes out as a JSON array: the list we own holds the refs.
        return value if items is None else items
    _reject_unstreamable(value)
    return value


# -- pumps -----------------------------------------------------------------------


async def pump_source(stream: InputStream, writer: ChannelWriter) -> None:
    """``stream`` into a pipe's writer: closed at the end, aborted when the source
    fails (R1). Stops, and releases the source, when the reader leaves."""
    while True:
        try:
            frame = await stream._next()
        except asyncio.CancelledError:
            raise
        except BaseException as error:
            log.debug("Stream source failed: %r", error)
            await writer.abort(_reason(error))
            return
        if frame is None:
            break
        try:
            if isinstance(frame, bytes):
                await writer.write(frame)
            else:
                await writer.send_text(frame)
        except asyncio.CancelledError:
            raise
        except Exception as error:
            # The reader left (or the socket failed): nobody wants the rest.
            log.debug("Stream pump stopped: %r", error)
            await stream._release()
            return
    await writer.close()


async def pump_sink(reader: ChannelReader, sink: OutputStream) -> None:
    """A pipe's reader into ``sink``: closed at the end, aborted when the stream
    aborts (raises :class:`StreamAbortedError`) or the sink fails (raises its error)."""
    try:
        while (frame := await reader.recv()) is not None:
            if isinstance(frame, bytes):
                await sink._put(frame)
            else:
                await sink._put_text(frame)
    except StreamAbortedError as error:
        await sink._fail(error.reason or error.message)
        raise
    except asyncio.CancelledError:
        raise
    except BaseException as error:
        await reader.close()
        await sink._fail(_reason(error))
        raise
    await sink._finish()


# -- handler guard ------------------------------------------------------------------


def guard(
    tracked: list[InputStream | OutputStream], ok: bool, result: Any, reason: str, *, activated: bool = False
) -> None:
    """What a settled handler leaves behind (R2, R3), started and not awaited:

    - an ``OutputStream`` it used and did not finish: closed on success, aborted on failure;
    - one it never used: on success, closed, except in an observer activation
      (``activated``: the invocation carries ``activation``) with a result other than
      ``None``, which leaves it untouched (the HTTP worker reads an untouched response
      and a result as a buffered response; closing it first would send an empty body);
      on failure, left untouched (the caller learns from the error);
    - an ``InputStream`` it started reading and did not finish: cancelled.

    Streams passed on, kept open or already finished are skipped. A plain call
    (``activated`` false) thus always ends a sink it carried, and the call resolves (R4).
    """
    for stream in tracked:
        if isinstance(stream, OutputStream):
            if stream._transferred or stream._keep_open or stream._closed or stream._aborted is not None:
                continue
            if stream._used:
                _spawn(stream._finish() if ok else stream._fail(reason))
            elif ok and (not activated or result is None):
                _spawn(stream._finish())
        elif stream._started and not stream._finished and not stream._transferred:
            _spawn(stream._release())


async def settle_local_sinks(
    sinks: list[OutputStream],
    ok: bool,
    reason: str,
    result: Any = None,
    timeout: float | None = None,
    timeout_reason: str = "timed out",
) -> bool:
    """A local call's sinks once its handler settled (R4): on success, an untouched
    sink is closed (in-process, nothing else can still write it) and the call
    waits for every sink to finish, for at most ``timeout`` seconds; on failure
    they are aborted. ``False`` when the wait expired: the sinks still open were
    aborted with ``timeout_reason``.

    A sink the handler handed back in ``result`` is the caller's to write: it is
    neither closed nor waited for. When ``result`` holds any ``OutputStream``, the
    call waits for no sink (one may depend on what the caller writes; see
    :meth:`Outbound.wait_sinks`)."""
    if not ok:
        for sink in sinks:
            await sink._fail(reason)
        return True
    returned = collect_streams(result, []) if result is not None else []
    handed_back = {id(stream) for stream in returned}
    for sink in sinks:
        if sink._untouched() and id(sink) not in handed_back:
            await sink._finish()
    if not sinks or any(isinstance(stream, OutputStream) for stream in returned):
        return True
    waits = [asyncio.ensure_future(sink._wait()) for sink in sinks]
    _, pending = await asyncio.wait(waits, timeout=timeout)
    if pending:
        for sink in sinks:
            if not sink._done.done():
                await sink._fail(timeout_reason)
        await asyncio.gather(*waits, return_exceptions=True)
        return False
    for wait in waits:
        if wait.exception() is not None:
            raise cast(BaseException, wait.exception())
    return True


class _RestSource(_Source):
    """What is left of a stream a local handler read part of and handed back."""

    def __init__(self, stream: InputStream) -> None:
        self._stream = stream

    async def next(self) -> Frame | None:
        return await self._stream._next()

    async def close(self, *, open_first: bool = False) -> None:
        await self._stream._release()


def hand_back(value: Any) -> Any:
    """A local handler's result as its caller gets it, held to the rules the
    outbound walk applies to a remote handler's result: an ``InputStream`` the
    handler read part of becomes a new stream of the rest (the caller is its one
    reader; the handler's guard leaves the original alone), and one read to the
    end, already passed on, or there twice raises :class:`~iii.StreamInUseError`.
    Containers are rebuilt only along the path to a replaced stream."""
    return _hand_back(value, set())


def _hand_back(value: Any, seen: set[int]) -> Any:
    if isinstance(value, InputStream):
        if id(value) in seen or value._transferred:
            raise StreamInUseError(f"{value!r} was already passed on to another call")
        seen.add(id(value))
        _check_forwardable_input(value)
        if not value._claimed:
            return value
        rest = InputStream(_RestSource(value))
        value._mark_transferred()
        return rest
    if isinstance(value, dict):
        out: dict[Any, Any] | None = None
        for key, member in value.items():
            new = _hand_back(member, seen)
            if new is not member:
                if out is None:
                    out = dict(value)
                out[key] = new
        return value if out is None else out
    if isinstance(value, (list, tuple)):
        items = [_hand_back(member, seen) for member in value]
        if all(new is old for new, old in zip(items, value)):
            return value
        return items if isinstance(value, list) else tuple(items)
    return value


def has_write_ref(value: Any) -> bool:
    """``value`` (JSON as it arrived) holds a ref with ``"direction": "write"``."""
    if isinstance(value, dict):
        if _is_ref(value):
            return bool(value["direction"] == "write")
        return any(has_write_ref(member) for member in value.values())
    if isinstance(value, list):
        return any(has_write_ref(member) for member in value)
    return False


def check_local_streams(value: Any) -> None:
    """The streams of a local call's payload, held to the rules the outbound walk
    applies on the remote path: each passed once, none already passed on to another
    call, none read to the end."""
    seen: set[int] = set()
    for stream in collect_streams(value, []):
        if id(stream) in seen or stream._transferred:
            raise StreamInUseError(f"{stream!r} was already passed on to another call")
        seen.add(id(stream))
        if isinstance(stream, InputStream):
            _check_forwardable_input(stream)


def bound_to(value: Any, loop: asyncio.AbstractEventLoop) -> list[InputStream | OutputStream]:
    """The streams of ``value`` that run on ``loop``: an ``InputStream.from_iter`` of an
    async iterable, or an ``OutputStream.to`` of a writer that is not a file object,
    made while ``loop`` ran (and the tee copies of such a stream)."""
    return [stream for stream in collect_streams(value, []) if _home_of(stream) is loop]


def _home_of(stream: InputStream | OutputStream) -> asyncio.AbstractEventLoop | None:
    if isinstance(stream, OutputStream):
        return stream._sink.home if stream._sink is not None else None
    source = stream._source
    if isinstance(source, _IterSource):
        return source.home
    if isinstance(source, _TeeBranch):
        return _home_of(source._tee.parent)
    return None


def local_sinks(value: Any) -> list[OutputStream]:
    """The sinks built in this process (``to_file`` / ``to``) in a local call's payload."""
    return [s for s in collect_streams(value, []) if isinstance(s, OutputStream) and s._sink is not None]
