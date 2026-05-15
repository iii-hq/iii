"""Tests for engine-reported trigger registration errors."""

import json

from unittest.mock import AsyncMock, patch

from iii.iii import III, InitOptions


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
                "message": 'Trigger type "http" not found — worker iii-http is missing. Run: iii worker add iii-http',
            },
        },
    )

    messages = [record.getMessage() for record in caplog.records]
    assert any("iii worker add iii-http" in m for m in messages), messages
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
