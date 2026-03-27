"""Tests for migration to iii-sdk trigger() API."""


def test_bridge_uses_native_iii_methods_without_instance_aliases() -> None:
    """Bridge instance should expose native iii-sdk methods without monkey-patched aliases."""
    import motia.iii as motia_iii
    from motia.bridge import bridge

    try:
        assert hasattr(bridge, "trigger")
        assert hasattr(bridge, "shutdown")

        # Legacy aliases should not be injected on the instance in the migrated codebase.
        bridge_dict = getattr(bridge, "__dict__", {})
        assert "invoke_function" not in bridge_dict
        assert "invoke_function_async" not in bridge_dict
        assert "disconnect" not in bridge_dict
    finally:
        bridge.shutdown()
        motia_iii._instance = None
