import importlib
import json
from pathlib import Path

import pytest

from motia.loader import generate_step_id
from motia.setup_step_endpoint import setup_step_endpoint

step_endpoint_module = importlib.import_module("motia.setup_step_endpoint")


class FakeIII:
    def __init__(self) -> None:
        self.functions: dict[str, object] = {}
        self.triggers: list[dict[str, object]] = []

    def register_function(self, func: dict[str, object] | str, handler: object) -> None:
        func_id = func["id"] if isinstance(func, dict) else func
        self.functions[func_id] = handler

    def register_trigger(self, trigger: dict[str, object]) -> None:
        self.triggers.append(trigger)


def _create_step_file(tmp_path: Path) -> Path:
    step_file = tmp_path / "src" / "steps" / "demo_step.py"
    step_file.parent.mkdir(parents=True)
    step_file.write_text("config = {}\n")
    return step_file


@pytest.mark.asyncio
async def test_setup_step_endpoint_returns_step_content_and_features(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    step_file = _create_step_file(tmp_path)
    features_path = Path(str(step_file).replace("/src/", "/tutorial/") + "-features.json")
    features_path.parent.mkdir(parents=True)
    features_path.write_text(json.dumps([{"title": "Demo"}]))

    monkeypatch.setattr("motia.cli.discover_steps", lambda directory, include_src=False: [str(step_file)])

    iii = FakeIII()
    setup_step_endpoint(iii)

    assert "motia_step_get" in iii.functions
    assert iii.triggers == [
        {
            "type": "http",
            "function_id": "motia_step_get",
            "config": {"api_path": "__motia/step/:stepId", "http_method": "GET"},
        }
    ]

    handler = iii.functions["motia_step_get"]
    response = await handler({"path_params": {"stepId": generate_step_id(str(step_file))}})

    assert response == {
        "status_code": 200,
        "body": {
            "id": generate_step_id(str(step_file)),
            "content": "config = {}\n",
            "features": [{"title": "Demo"}],
        },
    }


@pytest.mark.asyncio
async def test_setup_step_endpoint_handles_missing_and_unknown_step_ids(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    step_file = _create_step_file(tmp_path)
    monkeypatch.setattr("motia.cli.discover_steps", lambda directory, include_src=False: [str(step_file)])

    iii = FakeIII()
    setup_step_endpoint(iii)
    handler = iii.functions["motia_step_get"]

    missing = await handler({"path_params": {}})
    unknown = await handler({"path_params": {"stepId": "missing"}})

    assert missing == {"status_code": 400, "body": {"error": "stepId is required"}}
    assert unknown == {"status_code": 404, "body": {"error": "Step not found"}}


@pytest.mark.asyncio
async def test_setup_step_endpoint_ignores_invalid_features_file(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    step_file = _create_step_file(tmp_path)
    features_path = Path(str(step_file).replace("/src/", "/tutorial/") + "-features.json")
    features_path.parent.mkdir(parents=True)
    features_path.write_text("{not json")

    monkeypatch.setattr("motia.cli.discover_steps", lambda directory, include_src=False: [str(step_file)])

    iii = FakeIII()
    setup_step_endpoint(iii)
    handler = iii.functions["motia_step_get"]
    response = await handler({"path_params": {"stepId": generate_step_id(str(step_file))}})

    assert response["status_code"] == 200
    assert response["body"]["features"] == []


@pytest.mark.asyncio
async def test_setup_step_endpoint_returns_read_error(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    step_file = _create_step_file(tmp_path)
    monkeypatch.setattr("motia.cli.discover_steps", lambda directory, include_src=False: [str(step_file)])

    original_read_text = Path.read_text

    def fake_read_text(self: Path, *args: object, **kwargs: object) -> str:
        if self == step_file:
            raise OSError("boom")
        return original_read_text(self, *args, **kwargs)

    monkeypatch.setattr(step_endpoint_module.Path, "read_text", fake_read_text)

    iii = FakeIII()
    setup_step_endpoint(iii)
    handler = iii.functions["motia_step_get"]
    response = await handler({"path_params": {"stepId": generate_step_id(str(step_file))}})

    assert response == {
        "status_code": 500,
        "body": {"error": "Failed to read step: boom"},
    }
