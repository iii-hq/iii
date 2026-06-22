"""EngineFunctions / EngineTriggers parity with the Node SDK."""

from iii.engine import EngineFunctions, EngineTriggers


def test_engine_function_ids() -> None:
    assert EngineFunctions.LIST_FUNCTIONS == "engine::functions::list"
    assert EngineFunctions.INFO_FUNCTIONS == "engine::functions::info"
    assert EngineFunctions.LIST_WORKERS == "engine::workers::list"
    assert EngineFunctions.INFO_WORKERS == "engine::workers::info"
    assert EngineFunctions.LIST_TRIGGERS == "engine::triggers::list"
    assert EngineFunctions.INFO_TRIGGERS == "engine::triggers::info"
    assert EngineFunctions.LIST_REGISTERED_TRIGGERS == "engine::registered-triggers::list"
    assert EngineFunctions.INFO_REGISTERED_TRIGGERS == "engine::registered-triggers::info"
    assert EngineFunctions.REGISTER_WORKER == "engine::workers::register"


def test_engine_trigger_ids() -> None:
    assert EngineTriggers.FUNCTIONS_AVAILABLE == "engine::functions-available"
    assert EngineTriggers.LOG == "log"
