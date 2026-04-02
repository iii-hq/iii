import importlib


def test_import_iii_module_does_not_raise_runtime_typeerror() -> None:
    module = importlib.import_module("iii")
    assert hasattr(module, "IStream")
