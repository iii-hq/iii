"""``III_TOKEN`` authenticates the WebSocket handshake of the main connection and of channels."""

from types import SimpleNamespace
from unittest.mock import AsyncMock, MagicMock

import pytest

from iii.channels import ChannelReader, ChannelWriter, build_channel_url
from iii.iii import III
from iii.iii_types import StreamChannelRef


@pytest.fixture(autouse=True)
def _no_token(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("III_TOKEN", raising=False)


def _ref(direction: str = "write") -> StreamChannelRef:
    return StreamChannelRef(channel_id="chan", access_key="key", direction=direction)


async def _main_connection_headers(monkeypatch: pytest.MonkeyPatch, headers: dict[str, str] | None):
    """The ``additional_headers`` the main connection hands to ``websockets.connect``."""
    sdk = III.__new__(III)
    sdk._address = "ws://engine.test"
    sdk._options = SimpleNamespace(headers=headers)
    sdk._on_connected = AsyncMock()
    connect = AsyncMock()
    monkeypatch.setattr("iii.iii.websockets.connect", connect)
    await sdk._do_connect()
    return connect.call_args.kwargs["additional_headers"]


@pytest.mark.asyncio
async def test_env_token_is_sent_as_bearer_authorization(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("III_TOKEN", "secret")
    headers = await _main_connection_headers(monkeypatch, {"x-other": "kept"})
    assert headers == {"x-other": "kept", "Authorization": "Bearer secret"}


@pytest.mark.asyncio
async def test_explicit_authorization_wins_whatever_its_case(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("III_TOKEN", "secret")
    headers = await _main_connection_headers(monkeypatch, {"authorization": "Bearer explicit"})
    assert headers == {"authorization": "Bearer explicit"}


@pytest.mark.asyncio
@pytest.mark.parametrize("value", [None, "", "   "])
async def test_blank_or_unset_token_adds_nothing(monkeypatch: pytest.MonkeyPatch, value: str | None) -> None:
    if value is not None:
        monkeypatch.setenv("III_TOKEN", value)
    assert await _main_connection_headers(monkeypatch, None) is None


async def _telemetry_config(monkeypatch: pytest.MonkeyPatch, otel: dict[str, object] | None):
    """The ``OtelConfig`` that ``connect_async`` hands to ``init_otel``."""
    init_otel = MagicMock()
    monkeypatch.setattr("iii_helpers.observability.telemetry.init_otel", init_otel)
    monkeypatch.setattr("iii_helpers.observability.telemetry.attach_event_loop", MagicMock())
    monkeypatch.setenv("III_URL", "ws://other.test")
    sdk = III.__new__(III)
    sdk._address = "ws://engine.test"
    sdk._options = SimpleNamespace(headers={"x-other": "kept"}, otel=otel)
    sdk._set_connection_state = MagicMock()
    sdk._do_connect = AsyncMock()

    await sdk.connect_async()

    return init_otel.call_args.kwargs["config"]


@pytest.mark.asyncio
async def test_telemetry_gets_the_main_connection_headers(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("III_TOKEN", "secret")
    config = await _telemetry_config(monkeypatch, None)
    assert config.engine_ws_url == "ws://engine.test"
    assert config.engine_ws_headers == {
        "x-other": "kept",
        "Authorization": "Bearer secret",
    }


@pytest.mark.asyncio
async def test_explicit_empty_telemetry_headers_are_kept(monkeypatch: pytest.MonkeyPatch) -> None:
    # `{}` selects no telemetry headers: only an omitted value inherits the main ones,
    # so the bearer token is not sent on the telemetry handshake.
    monkeypatch.setenv("III_TOKEN", "secret")
    config = await _telemetry_config(monkeypatch, {"engine_ws_headers": {}})
    assert config.engine_ws_headers == {}


@pytest.mark.parametrize(
    ("base", "expected"),
    [
        ("ws://engine.test", "ws://engine.test/ws/channels/chan?key=a%20b&dir=read"),
        ("ws://engine.test/", "ws://engine.test/ws/channels/chan?key=a%20b&dir=read"),
        ("ws://engine.test/?token=abc", "ws://engine.test/ws/channels/chan?token=abc&key=a%20b&dir=read"),
        ("ws://engine.test/p/?token=abc&x=1", "ws://engine.test/p/ws/channels/chan?token=abc&x=1&key=a%20b&dir=read"),
    ],
)
def test_channel_url_keeps_the_base_query(base: str, expected: str) -> None:
    assert build_channel_url(base, "chan", "a b", "read") == expected


@pytest.mark.asyncio
@pytest.mark.parametrize("channel_cls", [ChannelWriter, ChannelReader])
async def test_channels_send_the_handshake_headers(
    monkeypatch: pytest.MonkeyPatch, channel_cls: type[ChannelWriter] | type[ChannelReader]
) -> None:
    monkeypatch.setenv("III_TOKEN", "secret")
    connect = AsyncMock()
    monkeypatch.setattr("iii.channels.websockets.connect", connect)

    await channel_cls("ws://engine.test", _ref())._ensure_connected()
    await channel_cls("ws://engine.test", _ref(), {"Authorization": "Bearer explicit"})._ensure_connected()

    assert [call.kwargs["additional_headers"] for call in connect.call_args_list] == [
        {"Authorization": "Bearer secret"},
        {"Authorization": "Bearer explicit"},
    ]
