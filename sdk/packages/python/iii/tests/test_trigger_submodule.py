"""The iii.trigger submodule exposes the trigger types; the root no longer does."""


def test_trigger_subpath() -> None:
    from iii.trigger import Trigger, TriggerConfig, TriggerHandler

    assert all(x is not None for x in (Trigger, TriggerConfig, TriggerHandler))


def test_trigger_types_not_at_root() -> None:
    import iii

    for name in ("Trigger", "TriggerConfig", "TriggerHandler", "TriggerTypeRef"):
        assert not hasattr(iii, name)


def test_trigger_action_void_at_root() -> None:
    import iii
    from iii.iii_types import TriggerActionVoid

    assert iii.TriggerActionVoid is TriggerActionVoid
    assert "TriggerActionVoid" in iii.__all__


def test_enqueue_result_at_root() -> None:
    from iii_helpers.queue import EnqueueResult

    import iii

    assert iii.EnqueueResult is EnqueueResult
    assert "EnqueueResult" in iii.__all__
