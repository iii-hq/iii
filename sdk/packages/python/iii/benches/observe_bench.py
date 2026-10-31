"""Local calls of an observed function in the Python SDK, against the fake
engine of the unit tests (``tests/observe_fake_engine.py``): what a call costs
the worker that hosts the function, with no engine in the way.

Run from ``sdk/packages/python/iii``::

    uv run python benches/observe_bench.py            # every case, 3 rounds
    uv run python benches/observe_bench.py --rounds 5 --json out.json
    uv run python benches/observe_bench.py --case call_local/1000_equality_observers_1_matching

Each case registers ``bench::fn`` (an async echo handler), announces its
observers with ``functionregistered`` (local dispatch on), waits for their
acks, then times ``await iii.call_async`` from an event loop of the bench's
own, one call at a time after a warm-up: the handler and its delivery run on
that loop, so a row is the SDK's cost of a local call (a sync ``iii.call``
adds the thread it runs a sync handler on). A row prints the mean, p50 and
p99 per call over every round, the mean of each round, and the report items
a call produced.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import platform
import statistics
import sys
import time
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import iii

# The fake engine lives with the tests of the SDK being measured.
sys.path.insert(0, str(Path(iii.__file__).resolve().parents[2] / "tests"))

import iii_helpers.observability.telemetry as telemetry  # noqa: E402
from iii_helpers.observability import ReconnectionConfig  # noqa: E402
from observe_fake_engine import (  # noqa: E402
    FakeEngine,
    acks,
    function_registered,
    observer_info,
    reported_items,
    settle,
)

from iii.iii import III  # noqa: E402
from iii.iii_constants import InitOptions  # noqa: E402

FN = "bench::fn"


@dataclass
class Case:
    name: str
    observers: Callable[[], list[dict[str, Any]]]
    payload: Callable[[int], Any]
    calls: int


def _tenants(n: int) -> list[dict[str, Any]]:
    return [observer_info(f"t{i}", "bench::target", filters={"input.tenant": {"$eq": f"tenant-{i}"}}) for i in range(n)]


def _ranges(n: int) -> list[dict[str, Any]]:
    return [observer_info(f"r{i}", "bench::target", filters={"input.amount": {"$gte": 1000 + i}}) for i in range(n)]


CASES = [
    Case("call_local/no_observers", lambda: [], lambda i: {"tenant": "tenant-7", "amount": 1}, 20_000),
    Case(
        "call_local/10_observers_none_matching",
        lambda: _tenants(10),
        lambda i: {"tenant": "nobody", "amount": 1},
        20_000,
    ),
    Case(
        "call_local/1_observer_matching",
        lambda: _tenants(1),
        lambda i: {"tenant": "tenant-0", "amount": 1},
        20_000,
    ),
    Case(
        "call_local/1000_equality_observers_1_matching",
        lambda: _tenants(1000),
        lambda i: {"tenant": f"tenant-{i % 1000}", "amount": 1},
        5_000,
    ),
    Case(
        "call_local/1000_equality_observers_none_matching",
        lambda: _tenants(1000),
        lambda i: {"tenant": "nobody", "amount": 1},
        5_000,
    ),
    Case(
        "call_local/1000_range_observers_none_matching",
        lambda: _ranges(1000),
        lambda i: {"tenant": "tenant-7", "amount": 1},
        2_000,
    ),
]


def _connect(engine: FakeEngine) -> III:
    client = III(
        engine.url,
        InitOptions(
            worker_name="observe-bench",
            enable_metrics_reporting=False,
            reconnection_config=ReconnectionConfig(initial_delay_ms=20, max_delay_ms=50, jitter_factor=0.0),
        ),
    )
    client._wait_until_connected()
    deadline = time.monotonic() + 10
    while not (client._registered_on_connection and engine.sockets):
        if time.monotonic() > deadline:
            raise RuntimeError("the client did not register with the fake engine")
        time.sleep(0.01)
    return client


def run_case(case: Case) -> dict[str, Any]:
    engine = FakeEngine()
    client = _connect(engine)
    try:

        async def echo(data: Any) -> Any:
            return data

        client.register_function(FN, echo)
        engine.wait_for(
            lambda: any(f.get("id") == FN for f in engine.frames_of("registerfunction")), what="the registration"
        )
        observers = case.observers()
        engine.deliver(function_registered(FN, observers))
        wanted = {o["id"] for o in observers}
        engine.wait_for(lambda: wanted <= {a["id"] for a in acks(engine)}, timeout=60, what="the acks")
        state = client._functions[FN].observe
        deadline = time.monotonic() + 10
        while not state.local_dispatch or len(state.active.observers) != len(observers):
            if time.monotonic() > deadline:
                raise RuntimeError("the function is not confirmed for local dispatch")
            time.sleep(0.01)
        warm = max(200, case.calls // 10)
        times: list[int] = []

        async def calls(n: int, timed: bool) -> None:
            clock = time.perf_counter_ns
            for i in range(n):
                payload = case.payload(i)
                t0 = clock()
                await client.call_async({"function_id": FN, "payload": payload})
                if timed:
                    times.append(clock() - t0)

        asyncio.run(calls(warm, False))
        settle(client, engine)  # every report item flushed and received
        before = len(reported_items(engine))
        asyncio.run(calls(case.calls, True))
        settle(client, engine)
        items = len(reported_items(engine)) - before
        return {"times_ns": times, "items": items, "calls": case.calls}
    finally:
        client.shutdown()
        engine.stop()


def _percentile(sorted_ns: list[int], q: float) -> float:
    return sorted_ns[min(len(sorted_ns) - 1, int(q * len(sorted_ns)))] / 1000.0


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--case", action="append", help="run only this case (repeatable)")
    parser.add_argument("--json", help="write every round's figures to this file")
    args = parser.parse_args()
    telemetry.init_otel = lambda **kwargs: None  # type: ignore[assignment]
    telemetry.attach_event_loop = lambda loop: None  # type: ignore[assignment]
    cases = [c for c in CASES if not args.case or c.name in args.case]
    results: dict[str, list[dict[str, Any]]] = {c.name: [] for c in cases}
    for _ in range(args.rounds):
        for case in cases:
            results[case.name].append(run_case(case))
    rows = []
    print(f"python {platform.python_version()}, iii-sdk at {Path(iii.__file__).resolve().parent}")
    print(f"{'case':<52} {'mean µs':>9} {'p50 µs':>9} {'p99 µs':>9} {'round means µs':>26} {'items/call':>10}")
    for case in cases:
        rounds = results[case.name]
        every = sorted(t for r in rounds for t in r["times_ns"])
        means = [statistics.fmean(r["times_ns"]) / 1000.0 for r in rounds]
        mean = statistics.fmean(every) / 1000.0
        items = sum(r["items"] for r in rounds) / sum(r["calls"] for r in rounds)
        row = {
            "case": case.name,
            "mean_us": round(mean, 2),
            "p50_us": round(_percentile(every, 0.5), 2),
            "p99_us": round(_percentile(every, 0.99), 2),
            "round_means_us": [round(m, 2) for m in means],
            "items_per_call": round(items, 3),
        }
        rows.append(row)
        print(
            f"{case.name:<52} {row['mean_us']:>9} {row['p50_us']:>9} {row['p99_us']:>9} "
            f"{', '.join(f'{m:.1f}' for m in means):>26} {row['items_per_call']:>10}"
        )
    if args.json:
        Path(args.json).write_text(json.dumps({"python": platform.python_version(), "rows": rows}, indent=2))


if __name__ == "__main__":
    main()
