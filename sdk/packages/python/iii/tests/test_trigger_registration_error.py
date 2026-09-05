"""Tests for engine-reported trigger registration errors."""

import json
from unittest.mock import AsyncMock, patch

from iii.iii import III, InitOptions
from iii.triggers import Trigger


def _send_message(client: III, payload: dict) -> None:
    with patch.object(client, "_send", new_callable=AsyncMock):
        client._run_on_loop(client._handle_message(json.dumps(payload)))


def test_trigger_registration_result_error_is_logged(caplog):
    client = III(address="ws://localhost:9999", options=InitOptions(worker_name="test"))
    caplog.set_level("ERROR", logger="iii")

    _send_message(
        client,
        {
            "type": "triggerregistrationresult",
            "id": "trig-1",
            "trigger_type": "http",
            "function_id": "fn-1",
            "error": {
                "code": "trigger_type_not_found",
                "message": (
                    'Trigger type "http" not found — worker http is missing. Run: '
                    "iii trigger -n <compose-daemon-namespace> compose::add worker=http"
                ),
            },
        },
    )

    messages = [record.getMessage() for record in caplog.records]
    assert any("<compose-daemon-namespace>" in m for m in messages), messages
    assert any("compose::add worker=http" in m for m in messages), messages
    assert any("trig-1" in m for m in messages), messages

    client.shutdown()


def test_trigger_registration_result_success_does_not_log(caplog):
    client = III(address="ws://localhost:9999", options=InitOptions(worker_name="test"))
    caplog.set_level("ERROR", logger="iii")

    _send_message(
        client,
        {
            "type": "triggerregistrationresult",
            "id": "trig-2",
            "trigger_type": "http",
            "function_id": "fn-2",
        },
    )

    messages = [record.getMessage() for record in caplog.records]
    assert not any("Trigger registration" in m for m in messages), messages

    client.shutdown()


def test_trigger_registration_error_is_readable_by_the_caller():
    """A retry loop has to branch on the cause, and a log line cannot be
    branched on. ``Trigger.registration_error`` is that programmatic half."""
    client = III(address="ws://localhost:9999", options=InitOptions(worker_name="test"))

    _send_message(
        client,
        {
            "type": "triggerregistrationresult",
            "id": "trig-1",
            "trigger_type": "harness::hook::pre-generate",
            "function_id": "memory::on-pre-generate",
            "error": {
                "code": "trigger_type_not_found",
                "message": "Trigger type not found",
            },
        },
    )

    recorded = client._trigger_registration_errors["trig-1"]
    assert recorded["code"] == "trigger_type_not_found"
    # Another binding's id is unaffected: the record is per-trigger, which is
    # the whole point of the change.
    assert "trig-2" not in client._trigger_registration_errors

    client.shutdown()


def test_handle_reads_through_to_the_live_record():
    """A snapshot taken at construction would stay ``None`` forever: the ack
    always arrives after ``register_trigger`` has returned."""
    errors: dict[str, dict] = {}
    trigger = Trigger(lambda: None, lambda: errors.get("trig-1"))

    assert trigger.registration_error is None

    errors["trig-1"] = {"code": "trigger_type_not_found", "message": "nope"}

    assert trigger.registration_error is not None
    assert trigger.registration_error["code"] == "trigger_type_not_found"


def test_handle_without_a_source_reports_nothing():
    """``Trigger(unregister_fn)`` stays usable on its own, which is what keeps
    the existing one-argument constructor working."""
    assert Trigger(lambda: None).registration_error is None
