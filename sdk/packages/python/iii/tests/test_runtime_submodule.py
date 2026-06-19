"""The iii.runtime submodule exposes the runtime types; the root no longer does."""


def test_runtime_subpath() -> None:
    from iii.runtime import FunctionRef, TriggerTypeRef

    assert FunctionRef is not None
    assert TriggerTypeRef is not None


def test_runtime_types_not_at_root() -> None:
    import iii

    for name in ("FunctionRef", "TriggerTypeRef"):
        assert not hasattr(iii, name)
