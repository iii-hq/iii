"""The iii.runtime submodule exposes the runtime types; the root does not."""


def test_runtime_subpath() -> None:
    from iii.function_ref import FunctionRef as Defined
    from iii.runtime import FunctionRef, IIIConnectionState

    assert FunctionRef is Defined
    assert IIIConnectionState is not None


def test_runtime_types_not_at_root() -> None:
    import iii

    assert not hasattr(iii, "FunctionRef")
