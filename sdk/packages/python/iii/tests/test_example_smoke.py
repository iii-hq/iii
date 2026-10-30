"""The example worker (``sdk/packages/python/iii-example``) sets itself up
against this SDK: every function it registers and every observer it creates
reaches the engine. Against a fake engine."""

from __future__ import annotations

import importlib
import importlib.util
import sys
from collections.abc import Iterator
from pathlib import Path
from types import ModuleType
from typing import Any

import pytest
from observe_fake_engine import FakeEngine

EXAMPLE_SRC = Path(__file__).resolve().parents[2] / "iii-example" / "src"


@pytest.fixture
def example() -> Iterator[ModuleType]:
    """The example's ``src`` package, imported as ``iii_example_src``."""
    name = "iii_example_src"
    spec = importlib.util.spec_from_file_location(
        name, EXAMPLE_SRC / "__init__.py", submodule_search_locations=[str(EXAMPLE_SRC)]
    )
    assert spec is not None and spec.loader is not None
    package = importlib.util.module_from_spec(spec)
    sys.modules[name] = package
    spec.loader.exec_module(package)
    try:
        yield package
    finally:
        for module in [m for m in sys.modules if m == name or m.startswith(f"{name}.")]:
            del sys.modules[module]


@pytest.mark.filterwarnings(r"ignore:.* is deprecated \(iii-stream\):FutureWarning")
def test_the_example_registers_its_functions_and_observers(
    engine: FakeEngine, connect: Any, example: ModuleType
) -> None:
    client = connect(engine)
    importlib.import_module(f"{example.__name__}.main")._setup(client)
    observable = importlib.import_module(f"{example.__name__}.observable_functions")

    ids = set(client._functions)
    assert {"example::on-webhook", "example::on-schedule", "example::webhook_handler"} <= ids

    def registrations() -> dict[str, Any]:
        return {f["id"]: f for f in engine.frames_of("registerfunction")}

    engine.wait_for(lambda: ids <= registrations().keys(), what="every registration of the example")
    registered = registrations()
    webhook = registered["example::on-webhook"]
    event_schema = observable.WebhookEvent.model_json_schema()
    assert webhook["request_format"]["properties"].keys() == event_schema["properties"].keys()
    assert webhook["response_format"]["properties"].keys() == event_schema["properties"].keys()
    assert webhook["args_format"]["required"] == ["path"]
    assert webhook["private"] is True
    assert registered["example::on-schedule"]["args_format"]["required"] == ["at"]

    expected = {
        ("example::on-webhook", "example::webhook_handler"),
        ("example::on-schedule", "example::send_report"),
        ("cron::on-tick", "example::scheduled_cleanup"),
        ("state::on-changed", "example::on_user_updated"),
        ("pubsub::publish", "example::on_order_created"),
        ("http::on-request", "api.post.todo"),
    }
    engine.wait_for(
        lambda: (
            expected <= {(f["source"]["function_id"], f["target"]["function_id"]) for f in engine.frames_of("observe")}
        ),
        what="the example's observers",
    )
