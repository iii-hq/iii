"""Unit tests for collect_engine_worker_interface.py.

Run with: python -m pytest .github/scripts/test_collect_engine_worker_interface.py -v
"""
from __future__ import annotations

import json
import pathlib
import subprocess
import sys

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


def test_run_iii_shells_iii_call(monkeypatch: pytest.MonkeyPatch) -> None:
    argv: list[list[str]] = []

    def fake_run(command: list[str], **_: object) -> subprocess.CompletedProcess[str]:
        argv.append(command)
        return subprocess.CompletedProcess(command, 0, stdout='{"workers": []}', stderr="")

    monkeypatch.setattr(collect.subprocess, "run", fake_run)

    assert collect.run_iii("engine::workers::list", {}) == {"workers": []}
    assert argv == [["iii", "call", "engine::workers::list", "--json", "{}"]]


# `engine::workers::info { name: "configuration" }` and
# `engine::functions::info { function_ids }` as a 0.26 engine answers them: the
# function rows carry `observer_count` always, `args_format` when set,
# `private: true` when private and `observable: false` when not observable.
WORKER_INFO = {
    "worker": {
        "name": "configuration",
        "id": "configuration",
        "namespace": "default",
        "status": "available",
        "function_count": 3,
        "internal": True,
    },
    "functions": [
        {
            "function_id": "configuration::get",
            "namespace": "default",
            "worker_name": "configuration",
            "observer_count": 0,
        },
        {
            "function_id": "configuration::on-updated",
            "namespace": "default",
            "worker_name": "configuration",
            "metadata": {"observe": "event", "source": "builtin"},
            "private": True,
            "observer_count": 2,
        },
        {
            "function_id": "configuration::schema",
            "namespace": "default",
            "worker_name": "configuration",
            "observable": False,
            "args_format": {"type": "object", "properties": {"id": {"type": "string"}}},
            "observer_count": 0,
        },
    ],
    "observers": [],
}

FUNCTION_DETAILS = {
    "configuration::get": {
        "function_id": "configuration::get",
        "namespace": "default",
        "worker_name": "configuration",
        "description": "Read one configuration entry",
        "request_schema": {"type": "object", "properties": {"id": {"type": "string"}}},
        "response_schema": {"type": "object"},
        "observer_count": 0,
    },
    "configuration::on-updated": {
        "function_id": "configuration::on-updated",
        "namespace": "default",
        "worker_name": "configuration",
        "description": "Engine-hosted source: a configuration entry changed.",
        "request_schema": {"type": "object", "properties": {"id": {"type": "string"}}},
        "response_schema": {"type": "object", "properties": {"id": {"type": "string"}}},
        "metadata": {"observe": "event", "source": "builtin"},
        "private": True,
        "observer_count": 2,
    },
    "configuration::schema": {
        "function_id": "configuration::schema",
        "namespace": "default",
        "worker_name": "configuration",
        "request_schema": {"type": "object"},
        "response_schema": {"type": "object"},
        "args_format": {"type": "object", "properties": {"id": {"type": "string"}}},
        "observable": False,
        "observer_count": 0,
    },
}


def _fake_v2_engine(calls: list[str]):
    def fake(function_path: str, payload: dict[str, object]) -> dict[str, object]:
        calls.append(function_path)
        if function_path == "engine::workers::list":
            return {"workers": [WORKER_INFO["worker"]]}
        if function_path == "engine::workers::info":
            assert payload == {"name": "configuration"}
            return WORKER_INFO
        if function_path == "engine::functions::info":
            return {"functions": [FUNCTION_DETAILS[i] for i in payload["function_ids"]]}
        return {"code": "function_not_found", "message": f"Function {function_path} not found"}

    return fake


def test_main_describes_a_worker_from_its_function_rows(
    monkeypatch: pytest.MonkeyPatch, tmp_path: pathlib.Path
) -> None:
    calls: list[str] = []
    monkeypatch.setattr(collect, "run_iii", _fake_v2_engine(calls))
    out = tmp_path / "worker-interface.json"
    monkeypatch.setattr(
        sys, "argv", ["collect", "--worker", "configuration", "--out", str(out)]
    )

    assert collect.main() == 0

    assert calls == [
        "engine::workers::list",
        "engine::workers::info",
        "engine::functions::info",
    ]
    interface = json.loads(out.read_text(encoding="utf-8"))
    assert set(interface) == {"functions"}
    rows = {row["name"]: row for row in interface["functions"]}
    assert list(rows) == [
        "configuration::get",
        "configuration::on-updated",
        "configuration::schema",
    ]
    assert rows["configuration::get"] == {
        "name": "configuration::get",
        "description": "Read one configuration entry",
        "request_schema": {"type": "object", "properties": {"id": {"type": "string"}}},
        "response_schema": {"type": "object"},
        "args_format": None,
        "private": False,
        "observable": True,
        "metadata": {},
    }
    source = rows["configuration::on-updated"]
    assert (source["private"], source["observable"], source["args_format"]) == (True, True, None)
    assert source["metadata"] == {"observe": "event", "source": "builtin"}
    closed = rows["configuration::schema"]
    assert closed["observable"] is False
    assert closed["args_format"] == {"type": "object", "properties": {"id": {"type": "string"}}}
