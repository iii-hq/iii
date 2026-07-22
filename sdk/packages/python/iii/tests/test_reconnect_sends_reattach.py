"""iii-hq/iii#1975 regression: on reconnect the SDK must present the previous
engine-assigned worker id via a ``reattach`` frame BEFORE replaying its
registration batch, so the engine retires the old connection and the replay
lands on a clean slate instead of racing its cleanup. On the first connect,
with no prior id, no reattach is sent.

Driven directly against ``_on_connected`` (no socket needed): the reattach is
the first thing sent, gated on a stored ``_worker_id``.
"""

import asyncio

from iii.iii import III
from iii.iii_constants import InitOptions
from iii.iii_types import MessageType


def test_reconnect_sends_reattach_first(monkeypatch) -> None:
    monkeypatch.setattr(
        "iii_helpers.observability.telemetry.init_otel", lambda **kwargs: None
    )
    monkeypatch.setattr(
        "iii_helpers.observability.telemetry.attach_event_loop", lambda loop: None
    )

    # The constructor auto-connects on a background thread; it never reaches a
    # real engine here (unused.test fails to resolve), so it never calls
    # _on_connected on its own — we drive that directly below. shutdown() in
    # the finally stops the (non-daemon) loop thread.
    client = III("ws://unused.test", InitOptions())

    sent: list = []

    async def fake_send(msg) -> None:
        sent.append(msg)

    monkeypatch.setattr(client, "_send", fake_send)
    monkeypatch.setattr(client, "_register_worker_metadata", lambda: None)
    client._ws = None  # _receive_loop returns immediately

    # One binding to replay, so ordering (reattach before replay) is observable.
    client._triggers["t1"] = {"type": "registertrigger", "id": "t1"}

    async def drive() -> None:
        # First connect: no stored id yet -> no reattach.
        await client._on_connected()
        assert not any(
            isinstance(m, dict) and m.get("type") == MessageType.REATTACH.value
            for m in sent
        ), f"no reattach must be sent on first connect: {sent}"

        # Engine assigns an identity + secret; the socket then drops and
        # reconnects.
        client._worker_id = "w-old"
        client._reattach_token = "tok-old"
        sent.clear()
        await client._on_connected()

    loop = asyncio.new_event_loop()
    try:
        loop.run_until_complete(drive())
    finally:
        loop.close()
        client.shutdown()

    assert sent, "reconnect must send frames"
    first = sent[0]
    assert isinstance(first, dict) and first.get("type") == MessageType.REATTACH.value, (
        f"reattach must be the first frame on reconnect, got: {sent}"
    )
    assert first.get("previous_worker_id") == "w-old"
    assert first.get("reattach_token") == "tok-old"
    # And it must precede the replayed registration.
    assert any(
        (m.get("type") if isinstance(m, dict) else getattr(m, "type", None)) == "registertrigger"
        for m in sent[1:]
    ), f"registration replay must follow the reattach: {sent}"
