"""The iii.runtime submodule exposes the runtime types; the root keeps the shims."""


def test_runtime_subpath() -> None:
    from iii.runtime import FunctionRef, TriggerTypeRef

    assert FunctionRef is not None
    assert TriggerTypeRef is not None


def test_runtime_root_shim() -> None:
    import iii
    from iii.runtime import FunctionRef as SubFunctionRef

    assert iii.FunctionRef is SubFunctionRef
