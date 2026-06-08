"""The iii.trigger submodule exposes the trigger types; the root keeps the shims."""


def test_trigger_subpath() -> None:
    from iii.trigger import Trigger, TriggerConfig, TriggerHandler

    assert all(x is not None for x in (Trigger, TriggerConfig, TriggerHandler))


def test_trigger_root_shim() -> None:
    import iii
    from iii.trigger import Trigger as SubTrigger

    assert iii.Trigger is SubTrigger
