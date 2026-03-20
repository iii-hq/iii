"""Unit tests for the 10ms delay before WebSocket close frame in ChannelWriter.close_async()."""

import asyncio
from unittest.mock import AsyncMock, MagicMock, call, patch

import pytest

from iii.channels import ChannelWriter
from iii.iii_types import StreamChannelRef


def _make_ref(direction: str = "write") -> StreamChannelRef:
    """Create a minimal StreamChannelRef for test use."""
    return StreamChannelRef(
        channel_id="test-channel-id",
        access_key="test-access-key",
        direction=direction,
    )


def _make_writer() -> ChannelWriter:
    """Create a ChannelWriter without triggering a real WebSocket connection."""
    return ChannelWriter(engine_ws_base="ws://localhost:9999", ref=_make_ref())


@pytest.mark.asyncio
async def test_close_async_sleeps_before_ws_close():
    """close_async() calls asyncio.sleep(0.01) before ws.close() when connected."""
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

    with patch("iii.channels.asyncio.sleep", side_effect=fake_sleep) as mock_sleep:
        await writer.close_async()

    mock_sleep.assert_called_once_with(0.01)
    mock_ws.close.assert_called_once()
    assert call_order == ["sleep(0.01)", "ws.close()"], (
        "asyncio.sleep must be awaited before ws.close()"
    )


@pytest.mark.asyncio
async def test_close_async_skips_when_not_connected():
    """close_async() does nothing when _connected is False."""
    writer = _make_writer()

    mock_ws = AsyncMock()
    writer._ws = mock_ws
    writer._connected = False

    with patch("iii.channels.asyncio.sleep") as mock_sleep:
        await writer.close_async()

    mock_sleep.assert_not_called()
    mock_ws.close.assert_not_called()


@pytest.mark.asyncio
async def test_close_async_skips_when_ws_is_none():
    """close_async() does nothing when _ws is None."""
    writer = _make_writer()

    writer._ws = None
    writer._connected = True  # even if _connected is True, _ws being None prevents action

    with patch("iii.channels.asyncio.sleep") as mock_sleep:
        await writer.close_async()

    mock_sleep.assert_not_called()


@pytest.mark.asyncio
async def test_close_async_sets_connected_false():
    """close_async() sets _connected to False after closing the WebSocket."""
    writer = _make_writer()

    mock_ws = AsyncMock()
    writer._ws = mock_ws
    writer._connected = True

    with patch("iii.channels.asyncio.sleep", new_callable=AsyncMock):
        await writer.close_async()

    assert writer._connected is False
