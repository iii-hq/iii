"""Unit tests for collect_engine_worker_interface.py.

Run with: python -m pytest .github/scripts/test_collect_engine_worker_interface.py -v
"""
from __future__ import annotations

import pytest

import collect_engine_worker_interface as collect


def _fake_run_iii(calls: list[dict[str, object]]):
    def fake(function_path: str, payload: dict[str, object]) -> dict[str, object]:
        assert function_path == "engine::functions::info"
        calls.append(payload)
        ids = payload["function_ids"]
        return {"functions": [{"function_id": fn_id} for fn_id in ids]}

    return fake


def test_collects_details_in_one_batch(monkeypatch: pytest.MonkeyPatch) -> None:
    calls: list[dict[str, object]] = []
    monkeypatch.setattr(collect, "run_iii", _fake_run_iii(calls))

    details = collect.collect_function_details(["a", "b", "c"])

    assert calls == [{"function_ids": ["a", "b", "c"]}]
    assert [d["function_id"] for d in details] == ["a", "b", "c"]


def test_chunks_batches_at_engine_limit(monkeypatch: pytest.MonkeyPatch) -> None:
    calls: list[dict[str, object]] = []
    monkeypatch.setattr(collect, "run_iii", _fake_run_iii(calls))
    ids = [f"fn-{i}" for i in range(collect.FUNCTIONS_INFO_BATCH_MAX + 1)]

    details = collect.collect_function_details(ids)

    assert len(calls) == 2
    assert calls[0]["function_ids"] == ids[: collect.FUNCTIONS_INFO_BATCH_MAX]
    assert calls[1]["function_ids"] == ids[collect.FUNCTIONS_INFO_BATCH_MAX :]
    assert [d["function_id"] for d in details] == ids


def test_no_ids_makes_no_calls(monkeypatch: pytest.MonkeyPatch) -> None:
    calls: list[dict[str, object]] = []
    monkeypatch.setattr(collect, "run_iii", _fake_run_iii(calls))

    assert collect.collect_function_details([]) == []
    assert calls == []


def test_per_item_error_marker_fails_collection(monkeypatch: pytest.MonkeyPatch) -> None:
    def fake(function_path: str, payload: dict[str, object]) -> dict[str, object]:
        return {
            "functions": [
                {"function_id": "a"},
                {"function_id": "b", "error": "not_found"},
            ]
        }

    monkeypatch.setattr(collect, "run_iii", fake)

    with pytest.raises(RuntimeError, match="'b': not_found"):
        collect.collect_function_details(["a", "b"])


def test_malformed_envelope_fails_collection(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(collect, "run_iii", lambda *_: {"function_id": "a"})

    with pytest.raises(RuntimeError, match="missing `functions` array"):
        collect.collect_function_details(["a"])
