"""The reporter with a fake host and manual timers."""

from __future__ import annotations

import json
from collections.abc import Callable
from typing import Any

from iii.reporter import Reporter, ReporterLimits, ReportInit


class FakeHost:
    def __init__(self) -> None:
        self.is_online = True
        self.statuses: dict[str, str] = {}
        self.frames: list[str] = []
        self.timers: list[tuple[float, Callable[[], None]]] = []
        self.unsent = 0
        #: Writes that fail after the frame was handed over (the socket died, ...).
        self.failing = False

    def online(self) -> bool:
        return self.is_online

    def status(self, function_id: str) -> Any:
        return self.statuses.get(function_id, "ready")

    def send_frame(self, data: str, on_failure: Callable[[BaseException], None] | None = None) -> bool:
        if self.failing:
            if on_failure is not None:
                on_failure(ConnectionError("the socket died mid-write"))
            return True
        self.frames.append(data)
        return True

    def unsent_bytes(self) -> int:
        return self.unsent

    def call_soon(self, callback: Callable[[], None]) -> None:
        callback()

    def call_later(self, delay_s: float, callback: Callable[[], None]) -> None:
        self.timers.append((delay_s, callback))

    def fire_timers(self) -> list[float]:
        timers, self.timers = self.timers, []
        for _, callback in timers:
            callback()
        return [delay for delay, _ in timers]

    def items(self) -> list[dict[str, Any]]:
        out: list[dict[str, Any]] = []
        for frame in self.frames:
            parsed = json.loads(frame)
            if parsed["type"] == "invocationsobserved":
                out.extend(parsed["items"])
        return out


def init(function_id: str = "f", n: int = 0, **values: Any) -> ReportInit:
    return ReportInit(
        function_id=function_id,
        invocation_id=f"inv-{n}",
        matched=["o"],
        status="success",
        values={"input": n, **values},
        started_at_ms=1760350000000,
        duration_ms=0.5,
    )


def test_flushes_at_256_items_in_one_frame_without_waiting() -> None:
    host = FakeHost()
    reporter = Reporter(host)
    for i in range(256):
        reporter.push(init(n=i), ["input"])
    assert len(host.frames) == 1
    assert len(json.loads(host.frames[0])["items"]) == 256
    assert reporter.size == 0


def test_flushes_a_smaller_batch_after_the_5ms_linger() -> None:
    host = FakeHost()
    reporter = Reporter(host)
    reporter.push(init(n=1), ["input"])
    reporter.push(init(n=2), ["input"])
    assert host.frames == []
    assert host.fire_timers() == [0.005]  # one linger timer per batch
    assert [i["input"] for i in host.items()] == [1, 2]


def test_is_bounded_by_items_drops_the_newest_and_counts_buffer_full() -> None:
    host = FakeHost()
    host.is_online = False
    reporter = Reporter(host, ReporterLimits(max_items=3))
    results = [reporter.push(init(n=i), ["input"]) for i in range(5)]
    assert results == [True, True, True, False, False]
    host.is_online = True
    assert reporter.send_stats() is True
    stats = json.loads(host.frames[-1])
    assert stats["dropped_reports"] == [{"function_id": "f", "reason": "BUFFER_FULL", "count": 2}]
    reporter.flush()
    assert [i["input"] for i in host.items()] == [0, 1, 2]  # the oldest are kept


def test_is_bounded_by_bytes_and_an_item_larger_than_the_buffer_is_too_large() -> None:
    host = FakeHost()
    host.is_online = False
    reporter = Reporter(host, ReporterLimits(max_bytes=700))  # an item here is ~130 bytes + its output
    assert reporter.push(init(n=1, output="x" * 800), ["input", "output"]) is False  # TOO_LARGE
    assert reporter.push(init(n=2, output="y" * 150), ["input", "output"]) is True
    assert reporter.push(init(n=3, output="z" * 150), ["input", "output"]) is True
    assert reporter.push(init(n=4, output="w" * 150), ["input", "output"]) is False  # BUFFER_FULL
    host.is_online = True
    reporter.send_stats()
    drops = {d["reason"]: d["count"] for d in json.loads(host.frames[-1])["dropped_reports"]}
    assert drops == {"TOO_LARGE": 1, "BUFFER_FULL": 1}


def test_unsent_socket_bytes_count_against_the_bound() -> None:
    host = FakeHost()
    reporter = Reporter(host, ReporterLimits(max_bytes=1000))
    host.unsent = 950  # a peer that stopped reading
    assert reporter.push(init(n=1), ["input"]) is False
    host.unsent = 0
    assert reporter.push(init(n=2), ["input"]) is True


def test_splits_a_flush_to_stay_under_the_frame_limit() -> None:
    host = FakeHost()
    host.is_online = False
    reporter = Reporter(host, ReporterLimits(max_frame_bytes=1000))
    for i in range(10):
        reporter.push(init(n=i, output="x" * 200), ["input", "output"])
    host.is_online = True
    reporter.flush()
    assert len(host.frames) > 1
    assert all(len(f.encode()) <= 1000 for f in host.frames)
    assert [i["input"] for i in host.items()] == list(range(10))


def test_keeps_items_while_offline_or_unconfirmed_then_flushes_them_and_drops_them_when_gone() -> None:
    host = FakeHost()
    reporter = Reporter(host)
    host.is_online = False
    reporter.push(init("a", 1), ["input"])
    reporter.flush()
    assert host.frames == []
    host.is_online = True
    host.statuses = {"a": "wait"}
    reporter.push(init("b", 2), ["input"])
    reporter.flush()
    assert [i["function_id"] for i in host.items()] == ["b"]
    host.statuses = {"a": "ready"}
    reporter.flush()
    assert [i["function_id"] for i in host.items()] == ["b", "a"]
    host.statuses = {"c": "gone"}
    reporter.push(init("c", 3), ["input"])
    reporter.flush()
    assert reporter.size == 0
    reporter.send_stats()
    assert json.loads(host.frames[-1])["dropped_reports"] == [{"function_id": "c", "reason": "INACTIVE", "count": 1}]


def test_discard_releases_the_held_items_of_a_function_as_inactive() -> None:
    host = FakeHost()
    host.statuses = {"conflicted": "wait"}
    reporter = Reporter(host)
    reporter.push(init("conflicted", 1), ["input"])
    reporter.push(init("conflicted", 2), ["input"])
    reporter.flush()
    assert reporter.size == 2
    reporter.discard("conflicted")
    assert reporter.size == 0
    reporter.send_stats()
    assert json.loads(host.frames[-1])["dropped_reports"] == [
        {"function_id": "conflicted", "reason": "INACTIVE", "count": 2}
    ]


def test_sends_localinvocationstats_only_when_non_zero_and_keeps_counting_while_offline() -> None:
    host = FakeHost()
    reporter = Reporter(host)
    assert reporter.send_stats(now=0.0) is False
    host.is_online = False
    reporter.record_local("f", True, 1.5)
    reporter.record_local("f", False, 0.5)
    assert reporter.send_stats(now=5.0) is False
    reporter.record_local("g", True, 1.0)
    host.is_online = True
    assert reporter.send_stats(now=10.0) is True
    stats = json.loads(host.frames[-1])
    assert stats == {
        "type": "localinvocationstats",
        "window_ms": 10000,
        "functions": [
            {"function_id": "f", "count": 2, "errors": 1, "duration_ms_sum": 2.0},
            {"function_id": "g", "count": 1, "errors": 0, "duration_ms_sum": 1.0},
        ],
        "dropped_reports": [],
    }
    assert reporter.send_stats(now=20.0) is False


def test_sends_stats_on_its_10s_timer() -> None:
    host = FakeHost()
    reporter = Reporter(host)
    reporter.start()
    assert [d for d, _ in host.timers] == [10.0]
    reporter.record_local("f", True, 1.0)
    host.fire_timers()
    assert json.loads(host.frames[-1])["type"] == "localinvocationstats"
    assert [d for d, _ in host.timers] == [10.0]  # rescheduled
    reporter.stop()
    host.fire_timers()
    assert host.timers == []


def test_never_raises_on_a_value_json_cannot_encode() -> None:
    from decimal import Decimal

    host = FakeHost()
    reporter = Reporter(host)
    assert reporter.push(init(n=1, output={"price": Decimal("1.5"), "lock": object()}), ["input", "output"]) is True
    deep: Any = 1
    for _ in range(100_000):
        deep = [deep]
    assert reporter.push(init(n=2, output=deep), ["input", "output"]) is False  # logged once, not reported
    host.fire_timers()
    assert host.items()[0]["output"] == {"price": 1.5}
    nan = init(n=3, output=float("nan"))
    assert reporter.push(nan, ["input", "output"]) is True
    host.fire_timers()
    assert host.items()[-1]["output"] is None


def test_numbers_no_double_can_hold_are_reported_as_null() -> None:
    from decimal import Decimal

    host = FakeHost()
    reporter = Reporter(host)
    in_range = 10**308  # 309 digits, still a double
    assert reporter.push(init(n=1, output={"big": 2**2048, "dec": Decimal("1e400"), "ok": in_range}), ["output"])
    host.fire_timers()
    [frame] = host.frames
    assert '"big":null' in frame and '"dec":null' in frame
    assert f'"ok":{in_range}' in frame  # written exactly
    # every number decodes as a double (serde_json refuses literals that overflow f64)
    json.loads(frame, parse_int=lambda text: float(text) if abs(float(text)) != float("inf") else 1 / 0)


def test_the_slow_path_maps_only_numbers_no_double_can_hold() -> None:
    host = FakeHost()
    reporter = Reporter(host)
    in_range = 10**308  # 309 digits: takes the slow path, still a double
    assert reporter.push(init(n=1, output=[2**2048, in_range, -(2**2048)]), ["output"])
    host.fire_timers()
    assert host.items()[0]["output"] == [None, in_range, None]
    assert f"[null,{in_range},null]" in host.frames[0]


def test_lone_surrogates_are_replaced_so_the_whole_frame_still_decodes() -> None:
    host = FakeHost()
    reporter = Reporter(host)
    broken = b"caf\xe9".decode("utf-8", "surrogateescape")  # "caf\udce9"
    assert reporter.push(init(n=1, output={"name": broken, broken: 1}), ["input", "output"])
    assert reporter.push(init(n=2, output="fine"), ["input", "output"])  # an unrelated item in the same batch
    host.fire_timers()
    [frame] = host.frames
    frame.encode("utf-8")  # valid UTF-8: no lone surrogate left, raw or escaped
    assert "\\ud" not in frame
    first, second = host.items()
    assert first["output"] == {"name": "caf\ufffd", "caf\ufffd": 1}
    assert second["output"] == "fine"


def test_a_frame_whose_write_fails_counts_its_items_as_disconnected() -> None:
    host = FakeHost()
    reporter = Reporter(host)
    host.failing = True
    reporter.push(init("a", 1), ["input"])
    reporter.push(init("a", 2), ["input"])
    reporter.push(init("b", 3), ["input"])
    host.fire_timers()
    host.failing = False
    reporter.send_stats()
    drops = json.loads(host.frames[-1])["dropped_reports"]
    assert drops == [
        {"function_id": "a", "reason": "DISCONNECTED", "count": 2},
        {"function_id": "b", "reason": "DISCONNECTED", "count": 1},
    ]


def test_output_is_input_carries_the_value_once_when_the_item_captures_both() -> None:
    host = FakeHost()
    reporter = Reporter(host)
    event = {"op": "insert"}

    def source(n: int, **flags: Any) -> ReportInit:
        return ReportInit(
            function_id="src",
            invocation_id=f"inv-{n}",
            matched=["o"],
            status="success",
            values={"input": event, "output": event},
            started_at_ms=0,
            duration_ms=0,
            **flags,
        )

    reporter.push(source(1, output_is_input=True), ["input", "output"])
    reporter.push(source(2, output_is_input=True), ["output"])  # output alone travels as output
    reporter.push(source(3), ["input", "output"])
    host.fire_timers()
    both, alone, plain = host.items()
    assert both["input"] == event and both["output_is_input"] is True and "output" not in both
    assert alone["output"] == event and "input" not in alone and "output_is_input" not in alone
    assert plain["input"] == plain["output"] == event and "output_is_input" not in plain


def test_a_frame_the_closed_socket_refuses_stays_buffered_in_order() -> None:
    class Closing(FakeHost):
        def __init__(self) -> None:
            super().__init__()
            self.refuse = True

        def send_frame(self, data: str, on_failure: Callable[[BaseException], None] | None = None) -> bool:
            if self.refuse:
                return False
            return super().send_frame(data, on_failure)

    host = Closing()
    reporter = Reporter(host, ReporterLimits(max_frame_bytes=500))  # one ~280-byte item per frame
    for i in range(3):
        reporter.push(init(n=i, output="x" * 100), ["input", "output"])
    reporter.flush()
    assert reporter.size == 3
    host.refuse = False
    reporter.flush()
    assert [i["input"] for i in host.items()] == [0, 1, 2]


def test_a_requeue_after_a_refused_flush_stays_within_the_bounds() -> None:
    class Refusing(FakeHost):
        def __init__(self) -> None:
            super().__init__()
            self.during_send: Callable[[], None] | None = None

        def send_frame(self, data: str, on_failure: Callable[[BaseException], None] | None = None) -> bool:
            if self.during_send is not None:
                push_more, self.during_send = self.during_send, None
                push_more()  # other threads keep pushing while the batch is in hand
            return False  # the socket closed under the flush

    host = Refusing()
    reporter = Reporter(host, ReporterLimits(max_items=3))
    for i in range(3):
        reporter.push(init(n=i), ["input"])
    host.during_send = lambda: [reporter.push(init(n=10 + i), ["input"]) for i in range(2)]
    reporter.flush()
    assert reporter.size == 3  # the bound holds: the newest two are dropped
    reporter.send_stats()  # refused too: the counts stay
    stats = reporter._counters.drops
    assert stats == {"f": {"BUFFER_FULL": 2}}
    host2 = FakeHost()
    reporter._host = host2
    reporter.flush()
    assert [i["input"] for i in host2.items()] == [0, 1, 2]


def test_surrogate_pairs_spelled_as_two_code_points_are_kept_as_the_character() -> None:
    host = FakeHost()
    reporter = Reporter(host)
    smile = chr(0xD83D) + chr(0xDE00)
    assert reporter.push(init(n=1, output={"face": smile + chr(0xDC00), smile: 1}), ["output"])
    host.fire_timers()
    [frame] = host.frames
    frame.encode("utf-8")
    assert host.items()[0]["output"] == {"face": "\U0001f600\ufffd", "\U0001f600": 1}


def test_a_failed_localinvocationstats_write_keeps_its_counts() -> None:
    host = FakeHost()
    reporter = Reporter(host)
    reporter.record_local("f", True, 1.0)
    host.failing = True
    assert reporter.send_stats(now=10.0) is True  # handed over, then the write failed
    host.failing = False
    reporter.record_local("f", False, 2.0)
    assert reporter.send_stats(now=20.0) is True
    stats = json.loads(host.frames[-1])
    assert stats["functions"] == [{"function_id": "f", "count": 2, "errors": 1, "duration_ms_sum": 3.0}]


def test_report_and_stats_frames_write_non_ascii_text_as_compact_utf8() -> None:
    host = FakeHost()
    reporter = Reporter(host)
    item = init("pedidos::criação", n=1, output="olá 😀")
    item.matched = ["o", "p"]
    assert reporter.push(item, ["output"])
    host.fire_timers()
    [frame] = host.frames
    assert frame.startswith('{"type":"invocationsobserved","items":[{"function_id":"pedidos::criação"')
    assert '"matched":["o","p"]' in frame
    assert '"output":"olá 😀"' in frame
    reporter.record_local("pedidos::criação", True, 1.0)
    assert reporter.send_stats(now=10.0) is True
    assert '{"function_id":"pedidos::criação","count":1,' in host.frames[-1]
