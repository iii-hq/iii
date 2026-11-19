"""Unit tests for the 10ms delay before the WebSocket close frame in the channel writer's close()."""

from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from iii._channels import ChannelWriter
from iii.iii_types import StreamChannelRef


def _make_ref(direction: str = "write") -> StreamChannelRef:
    """Create a minimal StreamChannelRef for test use."""
    return StreamChannelRef(
        channel_id="test-channel-id",
        access_key="test-access-key",
        direction=direction,
    )


def _make_writer() -> ChannelWriter:
    """Create a ChannelWriter without opening a real WebSocket connection."""
    return ChannelWriter(engine_ws_base="ws://localhost:9999", ref=_make_ref())


@pytest.mark.asyncio
async def test_close_sleeps_before_ws_close():
    """close() calls asyncio.sleep(0.01) before ws.close() when connected."""
    writer = _make_writer()

    mock_ws = AsyncMock()
    writer._ws = mock_ws
    writer._connected = True

    call_order: list[str] = []

    async def fake_sleep(delay: float) -> None:
        call_order.append(f"sleep({delay})")

    async def fake_ws_close() -> None:
        call_order.append("ws.close()")

    mock_ws.close = AsyncMock(side_effect=fake_ws_close)

    with patch("iii._channels.asyncio.sleep", side_effect=fake_sleep) as mock_sleep:
        await writer.close()

    mock_sleep.assert_called_once_with(0.01)
    mock_ws.close.assert_called_once()
    assert call_order == ["sleep(0.01)", "ws.close()"], (
        "asyncio.sleep must be awaited before ws.close()"
    )


@pytest.mark.asyncio
async def test_close_skips_a_socket_already_closed():
    """close() sends no second close frame once the socket is closed (_connected is False)."""
    writer = _make_writer()

    mock_ws = AsyncMock()
    writer._ws = mock_ws
    writer._connected = False

    # The delay still runs (a sync call queued before the close may be connecting); no close frame goes out.
    with patch("iii._channels.asyncio.sleep", new_callable=AsyncMock):
        await writer.close()

    mock_ws.close.assert_not_called()


@pytest.mark.asyncio
async def test_close_opens_a_writer_that_never_connected_then_closes_it():
    """close() on a writer that never wrote opens the socket, so the reader sees an empty stream."""
    writer = _make_writer()
    mock_ws = AsyncMock()
    mock_ws.transport = MagicMock()

    async def fake_connect(*args: object, **kwargs: object) -> AsyncMock:
        return mock_ws

    with patch("iii._channels.asyncio.sleep", new_callable=AsyncMock), patch(
        "iii._channels.websockets.connect", side_effect=fake_connect
    ):
        await writer.close()

    mock_ws.close.assert_called_once()
    assert writer._connected is False


@pytest.mark.asyncio
async def test_close_after_a_failed_write_does_not_reconnect():
    """A writer whose write failed lost data: close() must not open a new socket and end it cleanly."""
    writer = _make_writer()
    writer._abort(ConnectionError("lost"))

    with patch("iii._channels.asyncio.sleep", new_callable=AsyncMock), patch(
        "iii._channels.websockets.connect", new_callable=AsyncMock
    ) as connect:
        await writer.close()

    connect.assert_not_called()


@pytest.mark.asyncio
async def test_close_sets_connected_false():
    """close() sets _connected to False after closing the WebSocket."""
    writer = _make_writer()

    mock_ws = AsyncMock()
    writer._ws = mock_ws
    writer._connected = True

    with patch("iii._channels.asyncio.sleep", new_callable=AsyncMock):
        await writer.close()

    assert writer._connected is False
