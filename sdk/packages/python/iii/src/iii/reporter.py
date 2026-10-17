"""The reporter (sdk.md §4.4, CONTRACT D12): one bounded buffer for matched
local invocations of auto functions and ``obs.emit`` events of owner
functions, flushed as ``invocationsobserved`` batches; plus the periodic
``localinvocationstats``.

It never blocks a call: when the buffer is full the newest item is dropped
and counted (``BUFFER_FULL``), and an item too large on its own is dropped and
counted (``TOO_LARGE``). Items stay buffered while disconnected and are
flushed once the engine confirmed their function on the new connection.

Items are pushed from any thread (handlers run on the SDK loop, on the
caller's loop or on their own thread); the buffer is guarded by a lock, and
frames are only sent from the SDK loop.
"""

from __future__ import annotations

import json
import logging
import math
import re
import threading
import time
from collections.abc import Callable, Iterable
from dataclasses import dataclass, field, replace
from typing import Any, Literal, Protocol

from .filter._value import ABSENT, to_json_value
from .iii_types import (
    DroppedReportCount,
    LocalInvocationCount,
    LocalInvocationStatsMessage,
    MessageType,
)

log = logging.getLogger("iii.reporter")

PARTS: tuple[str, ...] = ("input", "output", "error", "caller", "metadata")

FunctionStatus = Literal["ready", "wait", "gone"]


@dataclass(frozen=True)
class ReporterLimits:
    #: Flush at this many items.
    batch_items: int = 256
    #: Flush this long after the first buffered item.
    linger_ms: float = 5.0
    #: Bounded buffer: items.
    max_items: int = 10_000
    #: Bounded buffer: bytes; an item larger than this is ``TOO_LARGE``.
    max_bytes: int = 8 * 1024 * 1024
    #: One frame's limit (the engine's).
    max_frame_bytes: int = 16 * 1024 * 1024
    #: ``localinvocationstats`` cadence.
    stats_interval_ms: float = 10_000.0


class ReporterHost(Protocol):
    def online(self) -> bool:
        """Connected to an engine that advertised ``observe.v1``."""

    def status(self, function_id: str) -> FunctionStatus:
        """``ready``: the engine confirmed the function on the current
        connection; ``wait``: not yet; ``gone``: no longer registered here."""

    def send_frame(self, data: str, on_failure: Callable[[BaseException], None] | None = None) -> bool:
        """Sends one JSON frame (called on the SDK loop); ``False`` when the
        socket is not open. ``on_failure`` runs if the write fails later."""

    def unsent_bytes(self) -> int:
        """Bytes handed to the socket but not written yet (its send queue): they
        count against the same bound, so a peer that stops reading cannot grow
        memory without limit."""

    def call_soon(self, callback: Callable[[], None]) -> None:
        """Runs ``callback`` on the SDK loop (thread-safe)."""

    def call_later(self, delay_s: float, callback: Callable[[], None]) -> None:
        """Runs ``callback`` on the SDK loop after ``delay_s`` (thread-safe)."""


@dataclass
class ReportInit:
    """One observed invocation or emitted event, before encoding."""

    function_id: str
    invocation_id: str
    matched: list[str]
    status: Literal["success", "error"]
    #: The parts an item may carry; a part is sent only when a matched observer captures it.
    values: dict[str, Any]
    started_at_ms: float
    duration_ms: float
    observe_depth: int = 0
    namespace: str | None = None
    traceparent: str | None = None
    baggage: str | None = None


#: An integer literal no double can hold has at least 309 digits.
_LONG_DIGITS = re.compile(r"\d{309,}")
#: A surrogate pair spelled as two code points (``chr(0xD83D) + chr(0xDE00)``).
_SURROGATE_PAIRS = re.compile("[\ud800-\udbff][\udc00-\udfff]")
#: Any surrogate left after joining the pairs is a lone one.
_SURROGATES = re.compile("[\ud800-\udfff]")


def _join_pair(match: re.Match[str]) -> str:
    pair = match.group()
    return chr(0x10000 + ((ord(pair[0]) - 0xD800) << 10) + (ord(pair[1]) - 0xDC00))


def encode_json(value: Any) -> str:
    """JSON text of a host value, compact, as the filters read it: a value
    JSON cannot encode, and a number no double can hold (which the engine's
    decoder rejects, failing the whole frame), is ``null``; a lone surrogate in
    a string or key is U+FFFD. Raises ``TypeError`` / ``ValueError`` when even
    that fails."""
    try:
        text = json.dumps(value, separators=(",", ":"), ensure_ascii=False, allow_nan=False)
    except (TypeError, ValueError):
        text = None
    if text is None or (len(text) >= 309 and _LONG_DIGITS.search(text) is not None):
        # Slow path, rare: what the evaluator sees (to_json_value), written exactly.
        mapped = to_json_value(value)
        text = json.dumps(
            None if mapped is ABSENT else mapped, separators=(",", ":"), ensure_ascii=False, allow_nan=False
        )
    # A lone surrogate (from `surrogateescape`) is neither UTF-8 nor something the
    # engine's decoder accepts as `\u` escape: one would fail the whole frame. A
    # pair spelled as two code points is the character it stands for (as in JS).
    if _SURROGATES.search(text) is None:
        return text
    return _SURROGATES.sub("\ufffd", _SURROGATE_PAIRS.sub(_join_pair, text))


def _number(n: float) -> str:
    if isinstance(n, float) and not math.isfinite(n):
        return "0"
    if isinstance(n, float) and n.is_integer() and abs(n) < 2**53:
        return str(int(n))
    return json.dumps(n)


def _byte_length(s: str) -> int:
    return len(s.encode("utf-8", "surrogatepass"))


class _ReportItem:
    __slots__ = ("bytes", "captured", "context", "event", "init", "json", "parts")

    def __init__(self, init: ReportInit, event: Any = ABSENT, context: Any = None) -> None:
        self.init = init
        self.parts: dict[str, str] = {}
        self.captured: set[str] = set()
        self.json = ""
        self.bytes = 0
        #: ``obs.emit``: the event value (held, so its identity cannot be reused while buffered).
        self.event = event
        #: ``obs.emit``: the invocation context it was emitted from.
        self.context = context

    def capture(self, captures: Iterable[str]) -> None:
        """Adds the parts ``captures`` asks for that are not in the item yet."""
        for c in captures:
            if c in self.captured:
                continue
            self.captured.add(c)
            if c not in PARTS or c not in self.init.values:
                continue
            self.parts[c] = encode_json(self.init.values[c])

    def render(self) -> None:
        """Assembles the wire JSON from the pre-encoded parts."""
        i = self.init
        out = [f'{{"function_id":{json.dumps(i.function_id)}']
        if i.namespace is not None:
            out.append(f',"namespace":{json.dumps(i.namespace)}')
        out.append(f',"invocation_id":{json.dumps(i.invocation_id)},"matched":{json.dumps(i.matched)}')
        out.append(f',"status":{json.dumps(i.status)}')
        for part in PARTS:
            encoded = self.parts.get(part)
            if encoded is not None:
                out.append(f',"{part}":{encoded}')
        out.append(f',"started_at_ms":{_number(int(i.started_at_ms))},"duration_ms":{_number(i.duration_ms)}')
        out.append(f',"observe_depth":{_number(i.observe_depth)}')
        if i.traceparent is not None:
            out.append(f',"traceparent":{json.dumps(i.traceparent)}')
        if i.baggage is not None:
            out.append(f',"baggage":{json.dumps(i.baggage)}')
        out.append("}")
        self.json = "".join(out)
        self.bytes = _byte_length(self.json)


_FRAME_HEAD = f'{{"type":"{MessageType.INVOCATIONS_OBSERVED.value}","items":['
_FRAME_TAIL = "]}"
_FRAME_OVERHEAD = _byte_length(_FRAME_HEAD) + _byte_length(_FRAME_TAIL)


@dataclass
class _FunctionStats:
    count: int = 0
    errors: int = 0
    duration_ms_sum: float = 0.0


@dataclass
class _Counters:
    stats: dict[str, _FunctionStats] = field(default_factory=dict)
    drops: dict[str, dict[str, int]] = field(default_factory=dict)


class Reporter:
    def __init__(self, host: ReporterHost, limits: ReporterLimits | None = None) -> None:
        self._host = host
        self.limits = limits or ReporterLimits()
        self._lock = threading.Lock()
        self._items: list[_ReportItem] = []
        self._bytes = 0
        #: Emitted events still in the buffer, by event identity: one event to N observers is one item.
        self._open_emits: dict[int, list[_ReportItem]] = {}
        self._counters = _Counters()
        self._window_start = time.monotonic()
        self._linger_scheduled = False
        self._unencodable: set[str] = set()
        self._running = False

    # ── lifecycle ──────────────────────────────────────────────────────────

    def start(self) -> None:
        """Starts the ``localinvocationstats`` timer (call on the SDK loop)."""
        if self._running:
            return
        self._running = True
        self._window_start = time.monotonic()
        self._host.call_later(self.limits.stats_interval_ms / 1000.0, self._stats_tick)

    def stop(self) -> None:
        self._running = False

    def _stats_tick(self) -> None:
        if not self._running:
            return
        try:
            self.send_stats()
        except Exception:
            log.exception("Failed to send localinvocationstats")
        self._host.call_later(self.limits.stats_interval_ms / 1000.0, self._stats_tick)

    @property
    def size(self) -> int:
        """Buffered items (for tests and diagnostics)."""
        return len(self._items)

    def with_limits(self, **changes: Any) -> None:
        """Replaces some limits (tests)."""
        self.limits = replace(self.limits, **changes)

    # ── items ──────────────────────────────────────────────────────────────

    def push(self, init: ReportInit, captures: Iterable[str]) -> bool:
        """A matched local invocation of an auto function."""
        item = _ReportItem(init)
        if not self._build(item, captures):
            return False
        return self._enqueue(item)

    def push_emit(self, init: ReportInit, observer_id: str, event: Any, context: Any, captures: Iterable[str]) -> bool:
        """An ``obs.emit`` event. The same event value emitted to several
        observers from the same invocation context, before the buffer is
        flushed, becomes one item whose ``matched`` lists them all."""
        with self._lock:
            for item in self._open_emits.get(id(event), ()):
                if (
                    item.event is event
                    and item.context is context
                    and item.init.function_id == init.function_id
                    and observer_id not in item.init.matched
                ):
                    return self._merge_locked(item, observer_id, captures)
        init.matched = [observer_id]
        item = _ReportItem(init, event, context)
        if not self._build(item, captures):
            return False
        return self._enqueue(item, emit=True)

    def _merge_locked(self, item: _ReportItem, observer_id: str, captures: Iterable[str]) -> bool:
        """Adds an observer (and the parts it captures) to a buffered emit
        item. The grown item is held to the same bounds as a new one: on a
        violation it is restored and the emit is dropped and counted."""
        saved = (list(item.init.matched), dict(item.parts), set(item.captured), item.json, item.bytes)
        fn = item.init.function_id
        reason: str | None = None
        try:
            item.init.matched.append(observer_id)
            item.capture(captures)
            item.render()
        except (TypeError, ValueError, RecursionError) as error:
            self._log_unencodable(fn, error)
            reason = "unencodable"
        if reason is None:
            if item.bytes > self.limits.max_bytes or item.bytes + _FRAME_OVERHEAD > self.limits.max_frame_bytes:
                reason = "TOO_LARGE"
            elif self._bytes - saved[4] + item.bytes + self._host.unsent_bytes() > self.limits.max_bytes:
                reason = "BUFFER_FULL"
        if reason is None:
            self._bytes += item.bytes - saved[4]
            return True
        item.init.matched, item.parts, item.captured, item.json, item.bytes = saved
        if reason != "unencodable":
            self._drop_locked(fn, reason)
        return False

    def _build(self, item: _ReportItem, captures: Iterable[str]) -> bool:
        try:
            item.capture(captures)
            item.render()
            return True
        except (TypeError, ValueError, RecursionError) as error:
            self._log_unencodable(item.init.function_id, error)
            return False

    def _log_unencodable(self, function_id: str, error: BaseException) -> None:
        if function_id in self._unencodable:
            return
        self._unencodable.add(function_id)
        log.error("Cannot encode an observed invocation of %s as JSON; it is not reported: %s", function_id, error)

    def _enqueue(self, item: _ReportItem, emit: bool = False) -> bool:
        fn = item.init.function_id
        flush_now = False
        linger = False
        with self._lock:
            if item.bytes > self.limits.max_bytes or item.bytes + _FRAME_OVERHEAD > self.limits.max_frame_bytes:
                self._drop_locked(fn, "TOO_LARGE")
                return False
            if (
                len(self._items) >= self.limits.max_items
                or self._bytes + self._host.unsent_bytes() + item.bytes > self.limits.max_bytes
            ):
                self._drop_locked(fn, "BUFFER_FULL")
                return False
            self._items.append(item)
            self._bytes += item.bytes
            if emit:
                self._open_emits.setdefault(id(item.event), []).append(item)
            if len(self._items) >= self.limits.batch_items:
                flush_now = True
            elif not self._linger_scheduled:
                self._linger_scheduled = True
                linger = True
        if flush_now:
            self._host.call_soon(self.flush)
        elif linger:
            self._host.call_later(self.limits.linger_ms / 1000.0, self._linger_fire)
        return True

    def _linger_fire(self) -> None:
        with self._lock:
            self._linger_scheduled = False
        self.flush()

    # ── counters ───────────────────────────────────────────────────────────

    def _drop_locked(self, function_id: str, reason: str, count: int = 1) -> None:
        by_reason = self._counters.drops.setdefault(function_id, {})
        by_reason[reason] = by_reason.get(reason, 0) + count

    def drop(self, function_id: str, reason: str, count: int = 1) -> None:
        """Counts a report the SDK dropped (``BUFFER_FULL``, ``TOO_LARGE``)."""
        with self._lock:
            self._drop_locked(function_id, reason, count)

    def record_local(self, function_id: str, ok: bool, duration_ms: float) -> None:
        """Counts one local invocation (for ``localinvocationstats``)."""
        with self._lock:
            s = self._counters.stats.get(function_id)
            if s is None:
                s = self._counters.stats[function_id] = _FunctionStats()
            s.count += 1
            if not ok:
                s.errors += 1
            s.duration_ms_sum += duration_ms

    # ── sending (SDK loop) ─────────────────────────────────────────────────

    def flush(self) -> None:
        """Sends what can be sent: items whose function the engine confirmed
        on the current connection, in frames under the frame limit. The rest
        stays. Items of a function no longer registered here are dropped
        (``INACTIVE``)."""
        if not self._items or not self._host.online():
            return
        with self._lock:
            ready: list[_ReportItem] = []
            held: list[_ReportItem] = []
            gone = False
            for item in self._items:
                status = self._host.status(item.init.function_id)
                if status == "ready":
                    ready.append(item)
                elif status == "wait":
                    held.append(item)
                else:
                    gone = True
                    self._drop_locked(item.init.function_id, "INACTIVE")
            if not ready and not gone:
                return
            self._items = held
            self._bytes = sum(it.bytes for it in held)
            self._open_emits = {}
            for item in held:
                if item.event is not ABSENT:
                    self._open_emits.setdefault(id(item.event), []).append(item)

        frame: list[_ReportItem] = []
        frame_bytes = _FRAME_OVERHEAD
        unsent: list[_ReportItem] = []

        def send() -> None:
            nonlocal frame, frame_bytes
            if not frame:
                return
            batch = frame
            data = f"{_FRAME_HEAD}{','.join(it.json for it in batch)}{_FRAME_TAIL}"
            # The socket closed under us: the rest waits in the buffer, in order.
            if unsent or not self._host.send_frame(data, lambda error: self._lost(batch, error)):
                unsent.extend(batch)
            frame = []
            frame_bytes = _FRAME_OVERHEAD

        for item in ready:
            if item.bytes + _FRAME_OVERHEAD > self.limits.max_frame_bytes:
                self.drop(item.init.function_id, "TOO_LARGE")
                continue
            separator = 1 if frame else 0
            if frame_bytes + separator + item.bytes > self.limits.max_frame_bytes:
                send()
            frame_bytes += (1 if frame else 0) + item.bytes
            frame.append(item)
        send()
        if unsent:
            with self._lock:
                # Back in front, in order; items pushed meanwhile are newer, so
                # what passes the bounds now is dropped from the newest end.
                items = unsent + self._items
                total = sum(it.bytes for it in items)
                while items and (len(items) > self.limits.max_items or total > self.limits.max_bytes):
                    newest = items.pop()
                    total -= newest.bytes
                    self._drop_locked(newest.init.function_id, "BUFFER_FULL")
                self._items = items
                self._bytes = total
                self._open_emits = {}
                for item in items:
                    if item.event is not ABSENT:
                        self._open_emits.setdefault(id(item.event), []).append(item)

    def _lost(self, batch: list[_ReportItem], error: BaseException) -> None:
        """A flushed frame whose write failed: its items are counted, not lost silently."""
        log.error("Lost a batch of %d observed invocations: the frame could not be written: %s", len(batch), error)
        counts: dict[str, int] = {}
        for item in batch:
            counts[item.init.function_id] = counts.get(item.init.function_id, 0) + 1
        for function_id, count in counts.items():
            self.drop(function_id, "BUFFER_FULL", count)

    def send_stats(self, now: float | None = None) -> bool:
        """Sends ``localinvocationstats`` when there is anything to report;
        keeps counting while offline."""
        now = time.monotonic() if now is None else now
        with self._lock:
            if not self._counters.stats and not self._counters.drops:
                self._window_start = now
                return False
            if not self._host.online():
                return False
            message = build_local_invocation_stats(
                (now - self._window_start) * 1000.0, self._counters.stats, self._counters.drops
            )
            counters, self._counters = self._counters, _Counters()
            window_start, self._window_start = self._window_start, now
        data = message.model_dump(by_alias=True, exclude_none=True)
        data["type"] = MessageType.LOCAL_INVOCATION_STATS.value

        def lost(error: BaseException) -> None:
            log.error("localinvocationstats could not be written; its counts go in the next window: %s", error)
            self._restore(counters)

        if self._host.send_frame(json.dumps(data, separators=(",", ":")), lost):
            return True
        # Not sent: keep the counts for the next window.
        self._restore(counters, window_start)
        return False

    def _restore(self, counters: _Counters, window_start: float | None = None) -> None:
        """Merges counts that did not reach the engine back into the current window."""
        with self._lock:
            for fn, s in counters.stats.items():
                cur = self._counters.stats.setdefault(fn, _FunctionStats())
                cur.count += s.count
                cur.errors += s.errors
                cur.duration_ms_sum += s.duration_ms_sum
            for fn, by_reason in counters.drops.items():
                for reason, count in by_reason.items():
                    self._drop_locked(fn, reason, count)
            if window_start is not None:
                self._window_start = window_start

    def discard(self, function_id: str) -> None:
        """Releases the buffered items of a function that cannot be confirmed
        (a namespace conflict, unregistered), counted as ``INACTIVE``."""
        with self._lock:
            kept = [it for it in self._items if it.init.function_id != function_id]
            dropped = len(self._items) - len(kept)
            if dropped == 0:
                return
            self._items = kept
            self._bytes = sum(it.bytes for it in kept)
            self._open_emits = {}
            for item in kept:
                if item.event is not ABSENT:
                    self._open_emits.setdefault(id(item.event), []).append(item)
            self._drop_locked(function_id, "INACTIVE", dropped)

    def clear(self) -> None:
        """Forgets everything buffered (the engine does not support observe)."""
        with self._lock:
            self._items = []
            self._bytes = 0
            self._open_emits = {}


def build_local_invocation_stats(
    window_ms: float,
    stats: dict[str, _FunctionStats],
    drops: dict[str, dict[str, int]],
) -> LocalInvocationStatsMessage:
    """``localinvocationstats`` from the counters, in insertion order."""
    return LocalInvocationStatsMessage(
        window_ms=max(0, round(window_ms)),
        functions=[
            LocalInvocationCount(function_id=fn, count=s.count, errors=s.errors, duration_ms_sum=s.duration_ms_sum)
            for fn, s in stats.items()
        ],
        dropped_reports=[
            DroppedReportCount(function_id=fn, reason=reason, count=count)
            for fn, by_reason in drops.items()
            for reason, count in by_reason.items()
        ],
    )


def function_stats(count: int, errors: int, duration_ms_sum: float) -> _FunctionStats:
    """A counter entry (for tests that build ``localinvocationstats`` directly)."""
    return _FunctionStats(count, errors, duration_ms_sum)


def encode_report_item(init: ReportInit, captures: Iterable[str]) -> str:
    """One item's wire JSON (for the fixture tests)."""
    item = _ReportItem(init)
    item.capture(captures)
    item.render()
    return item.json
