"""Tests for metadata support in trigger registration types."""
from iii.iii_types import RegisterTriggerInput, RegisterTriggerMessage, TriggerInfo


def test_register_trigger_input_accepts_metadata():
    inp = RegisterTriggerInput(
        type="http",
        function_id="my_fn",
        config={"api_path": "/test"},
        metadata={"team": "platform"},
    )
    assert inp.metadata == {"team": "platform"}


def test_register_trigger_input_metadata_defaults_none():
    inp = RegisterTriggerInput(type="cron", function_id="fn", config={})
    assert inp.metadata is None


def test_register_trigger_message_includes_metadata():
    msg = RegisterTriggerMessage(
        id="t1",
        trigger_type="http",
        function_id="fn",
        config={},
        metadata={"env": "prod"},
    )
    assert msg.metadata == {"env": "prod"}


def test_trigger_info_includes_metadata():
    info = TriggerInfo(
        id="t1",
        trigger_type="http",
        function_id="fn",
        config={},
        metadata={"team": "api"},
    )
    assert info.metadata == {"team": "api"}


def test_trigger_info_metadata_defaults_none():
    info = TriggerInfo(id="t2", trigger_type="cron", function_id="fn")
    assert info.metadata is None
