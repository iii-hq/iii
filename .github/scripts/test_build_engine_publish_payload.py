"""Unit tests for build_engine_publish_payload.py.

Run with: python -m pytest .github/scripts/test_build_engine_publish_payload.py -v
"""
from __future__ import annotations

import pathlib

import pytest

import build_engine_publish_payload as build

INTERFACE = {
    "functions": [
        {
            "name": "stream::on-changed",
            "description": "Engine-hosted source: a stream item changed.",
            "request_schema": {"type": "object"},
            "response_schema": {"type": "object"},
            "args_format": None,
            "private": True,
            "observable": True,
            "metadata": {"observe": "event", "source": "builtin"},
        }
    ]
}


def _repo(tmp_path: pathlib.Path) -> pathlib.Path:
    (tmp_path / "engine").mkdir()
    (tmp_path / "engine/Cargo.toml").write_text(
        '[package]\nname = "iii"\nversion = "0.26.0"\n', encoding="utf-8"
    )
    worker_dir = tmp_path / "engine/src/workers/stream"
    worker_dir.mkdir(parents=True)
    (worker_dir / "iii.worker.yaml").write_text(
        "name: iii-stream\ntype: engine\ndescription: Streams\n", encoding="utf-8"
    )
    (worker_dir / "README.md").write_text("# iii-stream\n", encoding="utf-8")
    return tmp_path


def test_payload_publishes_function_rows_only(tmp_path: pathlib.Path) -> None:
    payload = build.build_payload(
        repo_root=_repo(tmp_path),
        worker="iii-stream",
        worker_dir=pathlib.Path("engine/src/workers/stream"),
        expected_version="0.26.0",
        registry_tag="latest",
        repo_url="https://github.com/iii-hq/iii",
        interface=INTERFACE,
    )

    assert "triggers" not in payload
    assert payload["functions"] == INTERFACE["functions"]
    assert payload["worker_name"] == "iii-stream"
    assert payload["version"] == "0.26.0"


def test_function_rows_state_every_observe_field() -> None:
    interface = build.normalize_worker_interface(
        worker_name="iii-stream",
        worker_info={"worker": {"name": "iii-stream"}},
        function_details=[
            # The engine leaves out `private: false`, `observable: true` and an
            # absent `args_format`; the Registry row states all three.
            {"function_id": "stream::get", "observer_count": 0},
            {
                "function_id": "stream::on-changed",
                "private": True,
                "observer_count": 1,
            },
            {
                "function_id": "stream::internal",
                "observable": False,
                "args_format": {"type": "object"},
                "observer_count": 0,
            },
        ],
    )

    observe = {
        row["name"]: (row["args_format"], row["private"], row["observable"])
        for row in interface["functions"]
    }
    assert observe == {
        "stream::get": (None, False, True),
        "stream::on-changed": (None, True, True),
        "stream::internal": ({"type": "object"}, False, False),
    }
    assert set(interface) == {"functions"}


def test_args_format_must_be_an_object() -> None:
    with pytest.raises(ValueError, match="args_format"):
        build.normalize_worker_interface(
            worker_name="w",
            worker_info={"worker": {"name": "w"}},
            function_details=[{"function_id": "w::on-x", "args_format": "string"}],
        )
