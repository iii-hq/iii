"""EngineFunctions / EngineSources parity with the Node SDK."""

import iii.engine
from iii.engine import EngineFunctions, EngineSources


def test_engine_function_ids() -> None:
    assert EngineFunctions.LIST_FUNCTIONS == "engine::functions::list"
    assert EngineFunctions.INFO_FUNCTIONS == "engine::functions::info"
    assert EngineFunctions.LIST_WORKERS == "engine::workers::list"
    assert EngineFunctions.INFO_WORKERS == "engine::workers::info"
    assert EngineFunctions.REGISTER_WORKER == "engine::workers::register"


def test_engine_source_ids() -> None:
    assert EngineSources.FUNCTIONS_CHANGED == "engine::on-functions-changed"
    assert EngineSources.WORKERS_CHANGED == "engine::on-workers-changed"
    assert EngineSources.OBSERVERS_CHANGED == "engine::on-observers-changed"
    assert EngineSources.LOG == "observability::on-log"
    assert EngineSources.SPAN == "observability::on-span"
    assert EngineSources.CONFIGURATION_UPDATED == "configuration::on-updated"
    assert EngineSources.STREAM_CHANGED == "stream::on-changed"
    assert EngineSources.STREAM_JOIN == "stream::on-join"
    assert EngineSources.STREAM_LEAVE == "stream::on-leave"
    assert not hasattr(iii.engine, "EngineTriggers")
