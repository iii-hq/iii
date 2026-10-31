"""The filter index (decision 55): iii's delivery of a local call matches its
observers through an index of their filters' equality conditions, and the
index answers exactly what evaluating every filter answers.

Differential tests against the evaluator: random filters and documents from
a generator of every operator, type and path shape, an equality-heavy
corpus, churn (an index per active set, as observers come and go), every
conformance vector and the Rust diff corpus, and the Python host values the
evaluator reads through JSON."""

from __future__ import annotations

import dataclasses
import enum
import json
import random
from collections.abc import Callable
from datetime import datetime, timezone
from decimal import Decimal
from pathlib import Path
from typing import Any

import pytest
from observe_fake_engine import (
    FakeEngine,
    acks,
    function_registered,
    observer_added,
    observer_info,
    reported_items,
    settle,
    wait_until,
)
from pydantic import BaseModel

from iii import Call, Observer
from iii.filter import CompiledFilter, FilterError, compile_filter
from iii.filter._index import NO_SCALAR, UNKNOWN, FilterIndex, literal_key, probe
from iii.iii import III
from iii.observable import SCAN_MAX

FIXTURES = Path(__file__).resolve().parents[4] / "fixtures" / "filters"
ROOTS = ("input", "output", "error", "caller", "metadata")


def scan(entries: list[tuple[Any, CompiledFilter]], doc: Any, accept: Callable[[Any], bool] | None = None) -> list[Any]:
    """What the index must answer: every filter evaluated, in order."""
    return [key for key, f in entries if (accept is None or accept(key)) and f.matches(doc)]


def check(entries: list[tuple[Any, CompiledFilter]], docs: list[Any], rng: random.Random | None = None) -> int:
    """Asserts the index of ``entries`` answers what a scan answers for every
    doc, for ``matches`` and for ``select`` of a subset. Returns how many
    filters the index evaluated in all."""
    index = FilterIndex(entries)
    assert index.keys() == [key for key, _ in entries]
    evaluated = 0
    for doc in docs:
        expected = scan(entries, doc)
        got = index.matches(doc)
        assert got == expected, f"doc {doc!r}: index {got} != scan {expected}"
        evaluated += index.candidates(doc)
        if rng is not None and entries:
            keys = [key for key, _ in entries]
            subset = rng.sample(keys, rng.randint(0, len(keys)))
            by_key = dict(entries)
            want = [key for key in subset if by_key[key].matches(doc)]
            assert index.select(subset, doc) == want, f"doc {doc!r}: select of {subset}"
            odd = {key for key in keys if hash(str(key)) % 2}
            assert index.matches(doc, lambda key: key in odd) == [key for key in expected if key in odd]
    return evaluated


# ── the generator ──────────────────────────────────────────────────────────

#: Scalars that are, and are not, one value: `1`, `1.0` and `True`; `0` and `-0.0`; `"1"`; doubles past 2^53.
SCALARS: list[Any] = [
    None,
    True,
    False,
    0,
    -0.0,
    0.0,
    1,
    1.0,
    2,
    -3,
    1.5,
    2**53,
    2**53 + 1,
    9007199254740992.0,
    "",
    "a",
    "b",
    "1",
    "true",
    "null",
    "é",
    "😀",
    "a😀",
]
LITERAL_SCALARS = [s for s in SCALARS if not (isinstance(s, int) and not isinstance(s, bool) and abs(s) > 2**53)]
KEYS = ["a", "b", "0", "1", "c.d", "k"]


def segment(key: str) -> str:
    return key.replace("\\", "\\\\").replace(".", "\\.")


def gen_value(rng: random.Random, depth: int) -> Any:
    roll = rng.random()
    if depth <= 0 or roll < 0.55:
        return rng.choice(SCALARS)
    if roll < 0.75:
        return [gen_value(rng, depth - 1) for _ in range(rng.randint(0, 3))]
    return {key: gen_value(rng, depth - 1) for key in rng.sample(KEYS, rng.randint(0, 3))}


def gen_doc(rng: random.Random) -> dict[str, Any]:
    return {root: gen_value(rng, 3) for root in ROOTS if rng.random() < 0.7}


def gen_path(rng: random.Random, root: str | None = None) -> str:
    parts = [root or rng.choice(ROOTS)] + [segment(rng.choice(KEYS)) for _ in range(rng.randint(0, 2))]
    return ".".join(parts)


def gen_literal(rng: random.Random, depth: int = 2) -> Any:
    roll = rng.random()
    if depth <= 0 or roll < 0.7:
        return rng.choice(LITERAL_SCALARS)
    if roll < 0.85:
        return [gen_literal(rng, depth - 1) for _ in range(rng.randint(0, 2))]
    return {key: gen_literal(rng, depth - 1) for key in rng.sample(KEYS, rng.randint(0, 2))}


def gen_ops(rng: random.Random, depth: int) -> dict[str, Any]:
    ops: dict[str, Any] = {}
    for _ in range(rng.randint(1, 2)):
        name = rng.choice(
            ["$eq", "$eq", "$in", "$ne", "$nin", "$gt", "$gte", "$lt", "$lte", "$exists", "$prefix", "$any"]
        )
        if name in ("$eq", "$ne"):
            ops[name] = gen_literal(rng)
        elif name in ("$in", "$nin"):
            ops[name] = [gen_literal(rng, 1) for _ in range(rng.randint(0, 3))]
        elif name in ("$gt", "$gte", "$lt", "$lte"):
            ops[name] = rng.choice([0, 1, 1.5, -3, 2**53, "a", "b", "", "é"])
        elif name == "$exists":
            ops[name] = rng.random() < 0.5
        elif name == "$prefix":
            ops[name] = rng.choice(["", "a", "é", "😀"])
        elif depth > 0:
            if rng.random() < 0.5:
                ops["$any"] = gen_ops(rng, depth - 1)
            else:
                ops["$any"] = {gen_path(rng, "@"): gen_ops(rng, depth - 1)}
    return ops


def gen_filter(rng: random.Random, depth: int = 2) -> dict[str, Any]:
    clauses: list[dict[str, Any]] = []
    for _ in range(rng.randint(0, 4)):
        roll = rng.random()
        if depth > 0 and roll < 0.12:
            clauses.append({"$or": [gen_filter(rng, depth - 1) for _ in range(rng.randint(1, 2))]})
        elif depth > 0 and roll < 0.2:
            clauses.append({"$not": gen_filter(rng, depth - 1)})
        else:
            clauses.append({gen_path(rng): gen_ops(rng, depth)})
    if rng.random() < 0.3:
        return {"$and": clauses} if clauses else {}
    merged: dict[str, Any] = {}
    for clause in clauses:
        for key, value in clause.items():
            if key in merged:
                merged.setdefault("$and", []).append({key: value})
            else:
                merged[key] = value
    return merged


def compiled(src: Any) -> CompiledFilter | None:
    try:
        return compile_filter(src)
    except FilterError:
        return None


def gen_entries(rng: random.Random, n: int, make: Callable[[random.Random], Any]) -> list[tuple[Any, CompiledFilter]]:
    out: list[tuple[Any, CompiledFilter]] = []
    while len(out) < n:
        f = compiled(make(rng))
        if f is not None:
            out.append((f"f{len(out)}", f))
    return out


def gen_equality_filter(rng: random.Random) -> dict[str, Any]:
    """Equality-heavy: one to three `$eq` / `$in` of scalars on a few paths with few values, now and then a
    condition the index does not serve beside them."""
    paths = ["input.tenant", "input.op", "caller.worker_name", "metadata.k", "input.a.0", "output.status"]
    out: dict[str, Any] = {}
    for path in rng.sample(paths, rng.randint(1, 3)):
        if rng.random() < 0.7:
            out[path] = {"$eq": rng.choice(LITERAL_SCALARS[:12])}
        else:
            out[path] = {"$in": rng.sample(LITERAL_SCALARS[:12], rng.randint(1, 3))}
        if rng.random() < 0.15:
            out[path]["$ne"] = rng.choice(LITERAL_SCALARS)
    if rng.random() < 0.2:
        out["$or"] = [{gen_path(rng): gen_ops(rng, 0)}]
    return out


def gen_equality_doc(rng: random.Random) -> dict[str, Any]:
    def value() -> Any:
        return rng.choice(LITERAL_SCALARS[:12] + [[1], {"a": 1}])

    doc: dict[str, Any] = {}
    if rng.random() < 0.9:
        doc["input"] = {key: value() for key in rng.sample(["tenant", "op", "a"], rng.randint(0, 3))}
        if "a" in doc["input"] and rng.random() < 0.5:
            doc["input"]["a"] = [value(), value()]
    if rng.random() < 0.6:
        doc["caller"] = {"worker_name": value()}
    if rng.random() < 0.5:
        doc["metadata"] = {"k": value()} if rng.random() < 0.8 else None
    if rng.random() < 0.5:
        doc["output"] = {"status": value()}
    return doc


# ── differential: index against scan ───────────────────────────────────────


@pytest.mark.parametrize("seed", range(40))
def test_random_filters_and_documents_match_as_a_scan_does(seed: int) -> None:
    rng = random.Random(seed)
    entries = gen_entries(rng, rng.randint(1, 60), gen_filter)
    check(entries, [gen_doc(rng) for _ in range(60)], rng)


@pytest.mark.parametrize("seed", range(40))
def test_an_equality_heavy_corpus_matches_as_a_scan_does_and_evaluates_fewer_filters(seed: int) -> None:
    rng = random.Random(1000 + seed)
    entries = gen_entries(rng, 200, gen_equality_filter)
    docs = [gen_equality_doc(rng) for _ in range(80)]
    evaluated = check(entries, docs, rng)
    assert evaluated < len(entries) * len(docs) / 2  # the index rules most of them out


@pytest.mark.parametrize("seed", range(10))
def test_churn_an_index_per_active_set_matches_as_a_scan_does(seed: int) -> None:
    """An active set's index is built once per set; observers come and go in any order, so each set is any
    subset of observers in the order they were added."""
    rng = random.Random(5000 + seed)
    pool = gen_entries(rng, 120, lambda r: gen_equality_filter(r) if r.random() < 0.7 else gen_filter(r))
    active: list[tuple[Any, CompiledFilter]] = []
    docs = [gen_equality_doc(rng) for _ in range(10)] + [gen_doc(rng) for _ in range(10)]
    for _ in range(60):
        if active and rng.random() < 0.4:
            active.pop(rng.randrange(len(active)))
        else:
            free = [entry for entry in pool if entry not in active]
            if free:
                active.append(rng.choice(free))
        check(list(active), docs, rng)


def _vectors() -> tuple[list[CompiledFilter], list[dict[str, Any]]]:
    filters: list[CompiledFilter] = []
    docs: list[dict[str, Any]] = []
    for path in sorted(p for p in FIXTURES.iterdir() if p.suffix == ".json"):
        vector = json.loads(path.read_text(encoding="utf-8"))
        filters.append(compile_filter(vector["filters"]))
        docs.extend(case["doc"] for case in vector["cases"])
    return filters, docs


def _doc(doc: dict[str, Any]) -> dict[str, Any]:
    return {k: doc[k] for k in ROOTS if k in doc}


def test_every_conformance_vector_matches_as_a_scan_does() -> None:
    filters, docs = _vectors()
    assert len(filters) > 100
    entries = [(i, f) for i, f in enumerate(filters)]
    check(entries, [_doc(doc) for doc in docs], random.Random(7))


def test_the_rust_diff_corpus_matches_as_a_scan_does() -> None:
    corpus = json.loads((FIXTURES / "diff" / "corpus.json").read_text(encoding="utf-8"))
    rng = random.Random(11)
    for run in corpus["runs"]:
        entries = []
        for i, case in enumerate(run["cases"]):
            f = compiled(case["filters"])
            if f is not None:
                entries.append((i, f))
        docs = [_doc(case["doc"]) for case in run["cases"]]
        check(entries, docs[:120], rng)
        # each case's own expectation holds through the index too
        index = FilterIndex(entries)
        by_key = dict(entries)
        for i, case in enumerate(run["cases"]):
            if i in by_key:
                assert (i in index.matches(_doc(case["doc"]))) is case["expect"], f"seed {run['seed']} case {i}"


# ── the rule: what is indexed, how values are keyed ────────────────────────


def test_numbers_are_one_value_whatever_their_spelling_and_types_never_mix() -> None:
    entries = [
        ("one", compile_filter({"input.n": {"$eq": 1}})),
        ("zero", compile_filter({"input.n": {"$eq": 0}})),
        ("in", compile_filter({"input.n": {"$in": [1.5, -2]}})),
        ("true", compile_filter({"input.n": {"$eq": True}})),
        ("text", compile_filter({"input.n": {"$eq": "1"}})),
        ("null", compile_filter({"input.n": {"$eq": None}})),
    ]
    index = FilterIndex(entries)
    for value, expected in [
        (1.0, ["one"]),
        (1, ["one"]),
        (-0.0, ["zero"]),
        (0, ["zero"]),
        (-2.0, ["in"]),
        (1.5, ["in"]),
        ("1", ["text"]),
        (True, ["true"]),
        (False, []),
        (None, ["null"]),
        ([1], []),
        ({"a": 1}, []),
        (7, []),
    ]:
        doc = {"input": {"n": value}}
        assert index.matches(doc) == expected == scan(entries, doc), value
        assert index.candidates(doc) == len(expected)
    assert index.matches({"input": {}}) == [] and index.candidates({"input": {}}) == 0


def test_a_filter_without_a_condition_the_index_serves_is_scanned() -> None:
    entries = [
        (1, compile_filter({"input.n": {"$gt": 1}})),
        (2, compile_filter({"input.a": {"$eq": [1, 2]}})),
        (3, compile_filter({"$or": [{"input.n": {"$eq": 1}}]})),
        (4, compile_filter({})),
        (5, compile_filter({"input.k": {"$eq": "x"}})),
        (6, compile_filter({"input.k": {"$in": []}})),
        (7, compile_filter({"input.k": {"$in": ["x", [1]]}})),
        (8, compile_filter({"$not": {"input.k": {"$eq": "x"}}})),
        (9, compile_filter({"input.l": {"$any": {"$eq": "x"}}})),
    ]
    index = FilterIndex(entries)
    doc = {"input": {"n": 1, "a": [1, 2], "k": "y"}}
    assert index.candidates(doc) == 8  # every one but 5
    assert index.matches(doc) == [2, 3, 4, 8] == scan(entries, doc)
    doc = {"input": {"n": 2, "k": "x", "l": ["x"]}}
    assert index.matches(doc) == [1, 4, 5, 7, 9] == scan(entries, doc)


def test_an_entry_goes_under_the_condition_fewest_entries_share() -> None:
    entries = [(i, compile_filter({"input.op": {"$eq": "insert"}, "input.table": {"$eq": f"t{i}"}})) for i in range(10)]
    index = FilterIndex(entries)
    doc = {"input": {"op": "insert", "table": "t7"}}
    # the first under `input.op` (nothing shared yet), every later one under its own table
    assert index.candidates(doc) == 2
    assert index.matches(doc) == [7]


def test_conditions_of_a_nested_top_level_and_are_indexed_and_those_under_or_not_or_any_are_not() -> None:
    f = compile_filter({"$and": [{"input.a": {"$eq": 1}}, {"$and": [{"input.b": {"$in": ["x", None]}}]}]})
    assert [(term[0], sorted(map(repr, term[2]))) for term in f.equality_terms] == [
        (("input", ("a",)), ["('n', 1.0)"]),
        (("input", ("b",)), ["('s', 'x')", "('z', None)"]),
    ]
    # a path is one path however it is written
    assert compile_filter({"input.c\\.d": {"$eq": 1}}).equality_terms[0][0] == ("input", ("c.d",))
    assert compile_filter({"$or": [{"input.a": {"$eq": 1}}]}).equality_terms == ()
    assert compile_filter({"$not": {"input.a": {"$eq": 1}}}).equality_terms == ()
    assert compile_filter({"input.l": {"$any": {"@.a": {"$eq": 1}}}}).equality_terms == ()
    assert compile_filter({"input.a": {"$eq": {"k": 1}}}).equality_terms == ()
    assert literal_key(2**53) == ("n", 9007199254740992.0) and literal_key(True) == ("b", True)


def test_a_key_given_twice_is_refused() -> None:
    f = compile_filter({})
    with pytest.raises(ValueError):
        FilterIndex([("a", f), ("a", f)])


# ── Python host values ─────────────────────────────────────────────────────


class Color(str, enum.Enum):
    RED = "red"


class Level(enum.IntEnum):
    HIGH = 3


class Mood(enum.Enum):
    CALM = "calm"


class Shouty(str):
    """A ``str`` whose equality is its own: the index cannot key it."""

    def __eq__(self, other: object) -> bool:
        return isinstance(other, str) and other.upper() == str.upper(self)

    __hash__ = str.__hash__


class Raising(str):
    def __eq__(self, other: object) -> bool:
        raise RuntimeError("boom")

    __hash__ = str.__hash__


class OddFloat(int):
    """An ``int`` whose ``__float__`` is its own."""

    def __float__(self) -> float:
        return 42.0


class BadFloat(int):
    def __float__(self) -> float:
        raise RuntimeError("no double")


class Exploding(dict[str, Any]):
    def __contains__(self, key: object) -> bool:
        raise RuntimeError("boom")

    def items(self) -> Any:
        raise RuntimeError("boom")


class Order(BaseModel):
    id: str
    total: Decimal


@dataclasses.dataclass
class Point:
    x: int


HOST_VALUES: list[Any] = [
    Color.RED,
    Level.HIGH,
    Mood.CALM,
    Shouty("red"),
    Shouty("calm"),
    Raising("red"),
    OddFloat(3),
    BadFloat(3),
    Decimal("3"),
    Decimal("3.0"),
    Decimal("1e400"),
    Decimal("NaN"),
    float("nan"),
    float("inf"),
    10**400,
    2**53 + 1,
    datetime(2026, 11, 1, 9, 0, tzinfo=timezone.utc),
    Order(id="o-1", total=Decimal("3")),
    Point(3),
    (3, "red"),
    {1: "red", "1": "calm"},
    {"x": 3},
    Exploding(x=3),
    object(),
    b"red",
    {3},
]


def test_python_host_values_match_as_the_evaluator_reads_them() -> None:
    literals = [
        "red",
        "RED",
        "calm",
        3,
        3.0,
        42,
        None,
        True,
        "2026-11-01T09:00:00Z",
        9007199254740992,
        "o-1",
    ]
    entries: list[tuple[Any, CompiledFilter]] = []
    for path in ("input", "input.x", "input.id", "input.0", "input.1", "input.total"):
        for i, literal in enumerate(literals):
            entries.append((f"{path}={i}", compile_filter({path: {"$eq": literal}})))
        entries.append((f"{path} in", compile_filter({path: {"$in": ["red", 3, None]}})))
    docs = [{"input": value} for value in HOST_VALUES] + [{"input": {"x": value}} for value in HOST_VALUES]
    docs += [{"input": [value, value]} for value in HOST_VALUES]
    check(entries, docs, random.Random(3))


def test_a_value_the_index_cannot_key_rules_nothing_out() -> None:
    assert probe(Shouty("red")) is UNKNOWN
    assert probe(OddFloat(3)) == ("n", 42.0)
    assert probe(BadFloat(3)) is NO_SCALAR
    assert probe(Color.RED) == ("s", "red") and type(probe(Color.RED)[1]) is str  # type: ignore[index]
    assert probe(Level.HIGH) == ("n", 3.0)
    assert probe([1]) is NO_SCALAR and probe({}) is NO_SCALAR and probe((1,)) is NO_SCALAR
    entries = [(i, compile_filter({"input": {"$eq": v}})) for i, v in enumerate(["RED", "red", "blue"])]
    index = FilterIndex(entries)
    doc = {"input": Shouty("red")}
    assert index.candidates(doc) == 3
    assert index.matches(doc) == scan(entries, doc) == [0, 1]


# ── iii's delivery of local calls ──────────────────────────────────────────


def _confirm(client: III, engine: FakeEngine, function_id: str, observers: list[dict[str, Any]]) -> None:
    engine.deliver(function_registered(function_id, observers))
    wanted = {o["id"] for o in observers}
    engine.wait_for(lambda: wanted <= {a["id"] for a in acks(engine)}, what="the acks")


def _items(engine: FakeEngine, function_id: str) -> list[dict[str, Any]]:
    return [i for i in reported_items(engine) if i["function_id"] == function_id]


def _expected(client: III, function_id: str, call: Call[Any, Any]) -> list[str]:
    """The observers a scan delivers ``call`` to: each active one whose ``on`` and filters match, in order."""
    return [obs.id for obs in client.observers(function_id) if obs.matches(call)]


def _tenant_observers(n: int) -> list[dict[str, Any]]:
    observers = [observer_info(f"t{i}", "t", filters={"input.tenant": {"$eq": f"tenant-{i}"}}) for i in range(n)]
    observers += [
        observer_info("big", "t", filters={"input.total": {"$gte": 100}}),
        observer_info("errors", "t", on="error", filters={"input.tenant": {"$in": ["tenant-1", "tenant-2"]}}),
        observer_info("eu", "t", filters={"input.tenant": {"$eq": "tenant-3"}, "caller.worker_name": {"$eq": "x"}}),
        observer_info("all", "t", on="complete"),
    ]
    return observers


def test_a_local_call_is_delivered_to_the_observers_a_scan_finds_through_the_index(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)

    def create(order: dict[str, Any]) -> dict[str, Any]:
        if order.get("fail"):
            raise ValueError("refused")
        return {"ok": True}

    fid = "orders::create"
    client.register_function(fid, create)
    _confirm(client, engine, fid, _tenant_observers(60))
    state = client._functions[fid].observe
    assert len(state.active.observers) > SCAN_MAX
    payloads = [
        {"tenant": "tenant-7", "total": 5},
        {"tenant": "tenant-3", "total": 500},
        {"tenant": "tenant-2", "fail": True},
        {"tenant": "nobody"},
        {"tenant": 7},
        {},
    ]
    expected: list[list[str]] = []
    for payload in payloads:
        failed = bool(payload.get("fail"))
        try:
            output = client.call({"function_id": fid, "payload": payload})
        except Exception:
            output = None
        call = Call(
            function_id=fid,
            invocation_id="i",
            status="error" if failed else "success",
            input=payload,
            output=output,
            error={"code": "invocation_failed", "message": "refused"} if failed else None,
            caller=client._observe_host.identity(),
        )
        expected.append(_expected(client, fid, call))
    settle(client, engine)
    reported = [(i["matched"], i["input"]) for i in _items(engine, fid)]
    assert reported == [(e, p) for e, p in zip(expected, payloads) if e]
    assert expected[0] == ["t7", "all"] and expected[1] == ["t3", "big", "all"] and expected[2] == ["errors", "all"]
    # one indexed path: a call evaluates the filters filed under its tenant and the scanned ones
    assert state.active.filters.candidates({"input": {"tenant": "tenant-7"}}) == 1 + 2


class _CountingFilter:
    """An observer's compiled filter that counts its evaluations."""

    def __init__(self, inner: CompiledFilter) -> None:
        self.inner = inner
        self.equality_terms = inner.equality_terms  # what the index files it by
        self.calls = 0

    def matches(self, doc: Any) -> bool:
        self.calls += 1
        return bool(self.inner.matches(doc))


def _count_filters(client: III, function_id: str) -> dict[str, _CountingFilter]:
    """Each active observer's filter, counting its evaluations from now on
    (the filter index a set builds afterwards files these)."""
    counted: dict[str, _CountingFilter] = {}
    for obs in client.observers(function_id):
        counted[obs.id] = _CountingFilter(obs._filter)
        obs._filter = counted[obs.id]  # type: ignore[assignment]
    return counted


def _evaluated(counted: dict[str, _CountingFilter]) -> dict[str, int]:
    """The observers whose filter was evaluated since the last read, and how often."""
    out = {key: f.calls for key, f in counted.items() if f.calls}
    for f in counted.values():
        f.calls = 0
    return out


def test_a_local_call_past_scan_max_evaluates_only_the_filters_the_index_does_not_rule_out(
    engine: FakeEngine, connect: Any
) -> None:
    client = connect(engine)

    def create(order: dict[str, Any]) -> dict[str, Any]:
        if order.get("fail"):
            raise ValueError("refused")
        return {"ok": True}

    fid = "orders::counted"
    client.register_function(fid, create)
    _confirm(client, engine, fid, _tenant_observers(60))
    counted = _count_filters(client, fid)
    assert len(counted) > SCAN_MAX
    client.call({"function_id": fid, "payload": {"tenant": "tenant-7", "total": 5}})
    # t7 under its tenant, `big` and `all` scanned; `errors` (on: error) is not evaluated for a success
    assert _evaluated(counted) == {"t7": 1, "big": 1, "all": 1}
    with pytest.raises(Exception, match="refused"):
        client.call({"function_id": fid, "payload": {"tenant": "tenant-2", "fail": True}})
    # t2 and big are on: success, so only `errors` and `all` (on: complete) are evaluated
    assert _evaluated(counted) == {"errors": 1, "all": 1}
    settle(client, engine)
    assert [i["matched"] for i in _items(engine, fid)] == [["t7", "all"], ["errors", "all"]]


def test_a_match_hooks_choice_is_checked_for_on_through_the_index(engine: FakeEngine, connect: Any) -> None:
    """The match hook picks more than SCAN_MAX observers, among them `errors` (on: error) whose filter
    matches the call: a successful call is not delivered to it, whether the set evaluates the hook's
    choice one by one (its first call) or through its index (once built)."""
    client = connect(engine)
    fid = "rows::on"
    client.register_function(fid, lambda change: change, private=True).on_observer_match(
        lambda obs, call: obs.id != "t5"
    )
    _confirm(client, engine, fid, _tenant_observers(30))
    counted = _count_filters(client, fid)
    evaluations = []
    for _ in range(3):
        client.call({"function_id": fid, "payload": {"tenant": "tenant-1"}})
        evaluations.append(_evaluated(counted))
    # 33 observers matched of 34: the first call evaluates each one `on` accepts, which is as many
    # filters as building the index costs, so the next calls match through the index
    assert evaluations[0] == {obs: 1 for obs in counted if obs not in ("t5", "errors")}
    assert evaluations[1] == evaluations[2] == {"t1": 1, "big": 1, "all": 1}
    settle(client, engine)
    assert [i["matched"] for i in _items(engine, fid)] == [["t1", "all"]] * 3


def test_a_match_hooks_few_picks_build_the_index_once_the_set_has_served_enough_of_them(
    engine: FakeEngine, connect: Any
) -> None:
    """A set builds its index once its calls have evaluated as many filters one by one as it has
    observers. A set that changes before then (observers that come and go) evaluates only the
    observers the match hook picks, and builds no index of the others."""
    client = connect(engine)
    fid = "rows::few"
    picked = {f"t{i}" for i in range(10, 20)}
    client.register_function(fid, lambda change: change, private=True).on_observer_match(
        lambda obs, call: obs.id in picked
    )
    _confirm(client, engine, fid, _tenant_observers(30))
    state = client._functions[fid].observe
    counted = _count_filters(client, fid)

    def calls(churn: bool) -> list[int]:
        out = []
        for _ in range(5):
            if churn:
                with state._lock:
                    state.active = state.active.renewed()  # what an observer added or removed makes
            client.call({"function_id": fid, "payload": {"tenant": "tenant-12"}})
            out.append(sum(_evaluated(counted).values()))
        return out

    # 10 picked of 34: three calls evaluate 30 filters, the fourth builds the index and evaluates t12 alone
    assert calls(churn=False) == [10, 10, 10, 1, 1]
    assert calls(churn=True) == [10] * 5
    assert "filters" not in state.active.__dict__
    settle(client, engine)
    assert [i["matched"] for i in _items(engine, fid)] == [["t12"]] * 10


def test_a_match_hooks_choice_is_filtered_through_the_index(engine: FakeEngine, connect: Any) -> None:
    client = connect(engine)
    fid = "rows::changed"
    asked: list[str] = []

    def wants(obs: Observer, call: Call[Any, Any]) -> bool:
        asked.append(obs.id)
        return obs.id != "t5"

    client.register_function(fid, lambda change: change, private=True).on_observer_match(wants)
    _confirm(client, engine, fid, _tenant_observers(30))
    for payload in ({"tenant": "tenant-5", "total": 200}, {"tenant": "tenant-6"}):
        client.call({"function_id": fid, "payload": payload})
    settle(client, engine)
    assert [i["matched"] for i in _items(engine, fid)] == [["big", "all"], ["t6", "all"]]
    assert len(asked) == 2 * 34  # the predicate runs for every observer, whatever the index rules out


def test_filters_announced_again_are_the_ones_the_index_serves(engine: FakeEngine, connect: Any) -> None:
    """Decision 44: an observer announced again with the same args is served with the filters it was
    announced with, also through the index of a function with many observers."""
    client = connect(engine)
    fid = "ra::many"
    client.register_function(fid, lambda data: data)
    observers = [observer_info(f"o{i}", "t", filters={"input.n": {"$eq": i}}) for i in range(20)]
    _confirm(client, engine, fid, observers)
    client.call({"function_id": fid, "payload": {"n": 4}})
    settle(client, engine)
    assert [i["matched"] for i in _items(engine, fid)] == [["o4"]]
    engine.deliver(observer_added(fid, 1, observer_info("o4", "t", filters={"input.n": {"$eq": 400}})))
    engine.wait_for(lambda: len([a for a in acks(engine) if a["id"] == "o4"]) == 2, what="the re-ack")
    wait_until(lambda: client.observers(fid)[4].filters == {"input.n": {"$eq": 400}}, what="the new view")
    client.call({"function_id": fid, "payload": {"n": 4}})
    client.call({"function_id": fid, "payload": {"n": 400}})
    settle(client, engine)
    # the second call of 4 reaches no one, the call of 400 reaches o4
    assert [(i["matched"], i["input"]["n"]) for i in _items(engine, fid)] == [(["o4"], 4), (["o4"], 400)]


def test_an_observer_announced_again_takes_its_filters_before_the_set_that_serves_them(
    engine: FakeEngine, connect: Any, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A local call on another thread (a sync ``iii.call`` delivers on its caller's thread) may build the
    filter index of the function's current set at any moment of a re-announcement. Simulated here at the
    moment the observer takes its new filters: the set it reads then serves the old filters, and the set
    every later call reads files the new ones."""
    client = connect(engine)
    fid = "ra::race"
    client.register_function(fid, lambda data: data)
    _confirm(client, engine, fid, [observer_info(f"o{i}", "t", filters={"input.n": {"$eq": i}}) for i in range(20)])
    state = client._functions[fid].observe
    update = Observer._update

    def racing_update(self: Observer, info: Any, filter_: Any) -> None:
        if self.function_id == fid and self.id == "o4" and self._active:
            assert len(state.active.filters) == 20  # a concurrent call builds the current set's index here
        update(self, info, filter_)

    monkeypatch.setattr(Observer, "_update", racing_update)
    engine.deliver(observer_added(fid, 1, observer_info("o4", "t", filters={"input.n": {"$eq": 400}})))
    engine.wait_for(lambda: len([a for a in acks(engine) if a["id"] == "o4"]) == 2, what="the re-ack")
    wait_until(lambda: client.observers(fid)[4].filters == {"input.n": {"$eq": 400}}, what="the new view")
    client.call({"function_id": fid, "payload": {"n": 4}})
    client.call({"function_id": fid, "payload": {"n": 400}})
    settle(client, engine)
    assert [o.id for o in client.observers(fid) if o.matches({"input": {"n": 400}})] == ["o4"]
    assert [(i["matched"], i["input"]["n"]) for i in _items(engine, fid)] == [(["o4"], 400)]
