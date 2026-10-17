"""Filter conformance (sdk.md §7): every shared vector in ``sdk/fixtures/filters``,
plus the differential corpus produced by the Rust evaluator (``crates/iii-filter``,
``diffgen``). A file is never skipped."""

from __future__ import annotations

import dataclasses
import enum
import json
import uuid
from datetime import datetime, timezone
from decimal import Decimal
from pathlib import Path
from typing import Any

import pytest
from pydantic import BaseModel

from iii.filter import MATCH_ALL, FilterError, compile_filter

FIXTURES = Path(__file__).resolve().parents[4] / "fixtures" / "filters"


def _json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def _files(directory: Path) -> list[Path]:
    return sorted(p for p in directory.iterdir() if p.suffix == ".json")


VALID = _files(FIXTURES)
INVALID = _files(FIXTURES / "invalid")
CORPUS = _json(FIXTURES / "diff" / "corpus.json")


def _doc(doc: dict[str, Any]) -> dict[str, Any]:
    """The vector document as the SDK hands it to a filter: missing keys are absent roots."""
    return {k: doc[k] for k in ("input", "output", "error", "caller", "metadata") if k in doc}


def _code(src: Any) -> str | None:
    try:
        compile_filter(src)
    except FilterError as error:
        return error.code
    except Exception as error:  # pragma: no cover - reported as a failure
        return f"not a FilterError: {error!r}"
    return None


def test_finds_the_vector_files() -> None:
    assert len(VALID) > 100
    assert len(INVALID) > 10


@pytest.mark.parametrize("path", VALID, ids=lambda p: p.name)
def test_valid_vector(path: Path) -> None:
    vector = _json(path)
    f = compile_filter(vector["filters"])
    for i, case in enumerate(vector["cases"]):
        assert f.matches(_doc(case["doc"])) is case["expect"], f"case {i} {case.get('note', '')}"


@pytest.mark.parametrize("path", INVALID, ids=lambda p: p.name)
def test_invalid_vector(path: Path) -> None:
    vector = _json(path)
    for i, case in enumerate(vector["cases"]):
        assert _code(case["filters"]) == case["code"], f"case {i} {case.get('note', '')}"


def test_corpus_holds_the_committed_runs() -> None:
    assert CORPUS["runs"]
    assert sum(len(run["cases"]) for run in CORPUS["runs"]) >= 1000


@pytest.mark.parametrize("run", CORPUS["runs"], ids=lambda r: f"seed-{r['seed']}")
def test_agrees_with_the_rust_evaluator(run: dict[str, Any]) -> None:
    disagreements = []
    for i, case in enumerate(run["cases"]):
        try:
            got: object = compile_filter(case["filters"]).matches(_doc(case["doc"]))
        except FilterError as error:
            got = f"rejected: {error}"
        if got is not case["expect"]:
            disagreements.append(
                f"#{i}: expected {case['expect']}, got {got}\n"
                f"  filters {json.dumps(case['filters'])}\n  doc {json.dumps(case['doc'])}"
            )
    assert disagreements == []


# ── Python host values (filters.md §3) ──────────────────────────────────────


def test_none_is_null_and_a_missing_key_is_absent() -> None:
    exists = compile_filter({"output": {"$exists": True}})
    assert exists.matches({"output": None}) is True
    assert exists.matches({}) is False
    assert compile_filter({"output": {"$eq": None}}).matches({"output": None}) is True
    assert compile_filter({"input.a": {"$ne": None}}).matches({"input": {}}) is True


def test_bool_is_not_a_number() -> None:
    assert compile_filter({"input": {"$eq": 1}}).matches({"input": True}) is False
    assert compile_filter({"input": {"$eq": True}}).matches({"input": 1}) is False
    assert compile_filter({"input": {"$in": [0]}}).matches({"input": False}) is False
    assert compile_filter({"input": {"$gte": 0}}).matches({"input": True}) is False
    with pytest.raises(FilterError):
        compile_filter({"input": {"$gt": True}})


def test_numbers_compare_as_doubles() -> None:
    big = compile_filter({"input": {"$eq": 9007199254740992}})
    assert big.matches({"input": 9007199254740993}) is True  # rounded to the nearest double
    assert compile_filter({"input": {"$eq": 1}}).matches({"input": 1.0}) is True
    assert compile_filter({"input": {"$eq": 0}}).matches({"input": -0.0}) is True
    # a number no double holds is null, like a non-finite one (JSON cannot carry it as a number)
    assert compile_filter({"input": {"$gt": 2**53}}).matches({"input": 10**400}) is False
    assert compile_filter({"input": {"$eq": None}}).matches({"input": 10**400}) is True
    assert compile_filter({"input": {"$eq": None}}).matches({"input": Decimal("1e400")}) is True
    assert compile_filter({"input": {"$eq": None}}).matches({"input": Decimal("-Infinity")}) is True
    assert compile_filter({"input": {"$gt": 2**53}}).matches({"input": 2**1023}) is True
    assert compile_filter({"input": {"$eq": None}}).matches({"input": float("nan")}) is True


def test_index_segments_are_canonical_only() -> None:
    f = compile_filter({"input.01": {"$exists": True}})
    assert f.matches({"input": ["a", "b"]}) is False
    assert f.matches({"input": {"01": 1}}) is True
    assert compile_filter({"input.1": {"$eq": "b"}}).matches({"input": ("a", "b")}) is True
    assert compile_filter({"input.٣": {"$exists": True}}).matches({"input": [0, 1, 2, 3]}) is False


def test_own_keys_only() -> None:
    for key in ("__class__", "keys", "get", "__dict__"):
        assert compile_filter({f"input.{key}": {"$exists": True}}).matches({"input": {}}) is False
    assert compile_filter({"input.length": {"$exists": True}}).matches({"input": "abc"}) is False
    assert compile_filter({"input.0": {"$exists": True}}).matches({"input": "abc"}) is False


def test_strings_order_by_code_point() -> None:
    assert compile_filter({"input": {"$gt": "｡"}}).matches({"input": "😀"}) is True
    assert compile_filter({"input": {"$prefix": "😀"}}).matches({"input": "😀x"}) is True


def test_encoded_size_counts_utf8_bytes() -> None:
    # 4 bytes per emoji: 2047 of them plus the envelope is over 8 KiB even
    # though the string is far shorter than 8192 characters.
    with pytest.raises(FilterError) as error:
        compile_filter({"input": {"$eq": "😀" * 2047}})
    assert error.value.code == "INVALID_FILTER"
    compile_filter({"input": {"$eq": "😀" * 2030}})


def test_host_values_through_their_json_encoding() -> None:
    class Color(str, enum.Enum):
        RED = "red"

    class Order(BaseModel):
        id: str
        total: Decimal
        note: str | None = None

    @dataclasses.dataclass
    class Point:
        x: int

    when = datetime(2026, 11, 1, 9, 0, tzinfo=timezone.utc)
    assert compile_filter({"input": {"$eq": "2026-11-01T09:00:00Z"}}).matches({"input": when}) is True
    assert compile_filter({"input": {"$gte": "2026-11-01T00:00:00Z"}}).matches({"input": when}) is True
    assert compile_filter({"input": {"$eq": 1.5}}).matches({"input": Decimal("1.50")}) is True
    assert compile_filter({"input": {"$gt": 1}}).matches({"input": Decimal("1.5")}) is True
    assert compile_filter({"input": {"$eq": "red"}}).matches({"input": Color.RED}) is True
    assert compile_filter({"input.x": {"$eq": 3}}).matches({"input": Point(3)}) is True
    ident = uuid.UUID("12345678-1234-5678-1234-567812345678")
    assert compile_filter({"input": {"$prefix": "12345678-"}}).matches({"input": ident}) is True
    order = Order(id="o-1", total=Decimal("12.5"))
    assert compile_filter({"input.id": {"$eq": "o-1"}}).matches({"input": order}) is True
    assert compile_filter({"input.note": {"$exists": False}}).matches({"input": order}) is True
    # A value JSON cannot encode is absent, and null inside an array.
    opaque = object()
    assert compile_filter({"input.a": {"$exists": False}}).matches({"input": {"a": opaque}}) is True
    assert compile_filter({"input": {"$eq": {}}}).matches({"input": {"a": opaque}}) is True
    assert compile_filter({"input.0": {"$eq": None}}).matches({"input": [opaque]}) is True


@pytest.mark.parametrize(
    "operand",
    [
        object(),
        float("inf"),
        float("nan"),
        Decimal("1.5"),
        Decimal("NaN"),
        datetime(2026, 11, 1, tzinfo=timezone.utc),
        {1, 2},
        "\ud800",
        {1: "non-string key"},
        {"\udc00": "lone surrogate key"},
    ],
    ids=repr,
)
def test_non_json_filter_values_are_rejected_not_coerced(operand: Any) -> None:
    with pytest.raises(FilterError) as error:
        compile_filter({"input": {"$eq": operand}})
    assert error.value.code == "INVALID_FILTER"


def test_the_filter_must_be_a_dict() -> None:
    from types import MappingProxyType

    for src in (None, [], "x", MappingProxyType({"input": {"$exists": True}})):
        with pytest.raises(FilterError):
            compile_filter(src)
    # a tuple is how Python spells a JSON array
    assert compile_filter({"input": {"$in": (1, 2)}}).matches({"input": 2}) is True


def test_engine_normal_forms_skip_the_size_limit() -> None:
    long = {"$and": [{"input.s": {"$ne": "x" * 8180}}]}
    with pytest.raises(FilterError):
        compile_filter(long)
    assert compile_filter(long, trusted=True).matches({"input": {"s": "y"}}) is True


def test_reports_the_roots_a_filter_reads() -> None:
    f = compile_filter({"input.a": {"$eq": 1}, "$or": [{"caller.worker_name": {"$eq": "x"}}], "error": {"$any": {}}})
    assert f.roots == frozenset({"input", "caller", "error"})
    assert MATCH_ALL.match_all is True
    assert MATCH_ALL.matches({}) is True


def test_evaluation_is_total() -> None:
    class Hostile(str):
        def __lt__(self, other: object) -> bool:
            raise RuntimeError("boom")

    class Exploding(dict[str, Any]):
        def __contains__(self, key: object) -> bool:
            raise RuntimeError("boom")

    assert compile_filter({"input": {"$lt": "b"}}).matches({"input": Hostile("a")}) is False
    assert compile_filter({"input": {"$eq": 1}}).matches("not a mapping") is False  # type: ignore[arg-type]
    assert compile_filter({"input.a": {"$exists": True}}).matches({"input": Exploding(a=1)}) is False
    # deep host values against a deep literal: bounded by the literal, not the host
    deep: Any = 1
    for _ in range(5000):
        deep = [deep]
    assert compile_filter({"input": {"$eq": [[1]]}}).matches({"input": deep}) is False


def test_dict_keys_match_by_their_json_form() -> None:
    # json.dumps writes 1 as "1", True as "true", None as "null", 1.5 as "1.5"
    assert compile_filter({"input.1": {"$eq": 5}}).matches({"input": {1: 5}}) is True
    assert compile_filter({"input.true": {"$eq": "t"}}).matches({"input": {True: "t"}}) is True
    assert compile_filter({"input.false": {"$exists": True}}).matches({"input": {False: 0}}) is True
    assert compile_filter({"input.null": {"$eq": 1}}).matches({"input": {None: 1}}) is True
    assert compile_filter({"input.1\\.5": {"$eq": 2}}).matches({"input": {1.5: 2}}) is True
    assert compile_filter({"input.-3": {"$eq": 2}}).matches({"input": {-3: 2}}) is True
    assert compile_filter({"input": {"$eq": {"1": 5, "null": 1}}}).matches({"input": {1: 5, None: 1}}) is True
    assert compile_filter({"input": {"$eq": {"1": 5}}}).matches({"input": {1: 5, (1, 2): 6}}) is True  # not JSON
    assert compile_filter({"input.1": {"$exists": False}}).matches({"input": {"01": 5, 1.0: 3}}) is True
    assert compile_filter({"input.1": {"$exists": True}}).matches({"input": [0, 1]}) is True  # arrays: an index


def test_filter_errors_and_rejections_pickle_whole() -> None:
    import pickle

    from iii import ObserveRejection

    try:
        compile_filter({"input.op": "insert"})
    except FilterError as error:
        copy = pickle.loads(pickle.dumps(error))
        assert (copy.code, copy.message, str(copy)) == (error.code, error.message, str(error))
    rejection = pickle.loads(pickle.dumps(ObserveRejection("INVALID_CRON_EXPRESSION", "bad")))
    assert (rejection.code, rejection.message, str(rejection)) == (
        "INVALID_CRON_EXPRESSION",
        "bad",
        "INVALID_CRON_EXPRESSION: bad",
    )


def test_keys_that_collide_in_json_read_as_the_last_inserted() -> None:
    # json.dumps writes {"1": "b", 1: "a"} as {"1":"b","1":"a"}: a JSON parser keeps "a"
    host = {"1": "b", 1: "a"}
    assert compile_filter({"input.1": {"$eq": "a"}}).matches({"input": host}) is True
    assert compile_filter({"input": {"$eq": {"1": "a"}}}).matches({"input": host}) is True
    reverse = {1: "a", "1": "b"}
    assert compile_filter({"input.1": {"$eq": "b"}}).matches({"input": reverse}) is True


def test_a_colliding_key_whose_last_value_json_drops_leaves_the_member_absent() -> None:
    from iii.filter._value import ABSENT, to_json_value

    host = {"1": "b", 1: object()}
    assert compile_filter({"input.1": {"$exists": False}}).matches({"input": host}) is True
    assert to_json_value(host) == {}
    assert to_json_value({1: object(), "1": "b"}) == {"1": "b"}
    assert to_json_value(object()) is ABSENT
