"""Tests for stream/state group listing via SDK calls."""

import sys
from unittest.mock import MagicMock, patch

import pytest

from motia.state import StateManager
from motia.streams import Stream

_state_mod = sys.modules["motia.state"]


def test_stream_get_group_uses_stream_list() -> None:
    """Stream.get_group should use stream.list SDK call."""
    mock_iii = MagicMock()
    mock_iii.trigger = MagicMock(return_value=[{"id": "a"}])

    with patch("motia.streams.get_instance", return_value=mock_iii):
        stream = Stream("todos")
        result = stream.get_group("default")

    assert result == [{"id": "a"}]
    mock_iii.trigger.assert_called_once_with({
        "function_id": "stream::list",
        "payload": {
            "stream_name": "todos",
            "group_id": "default",
        },
    })


def test_stream_get_group_propagates_errors() -> None:
    """Stream.get_group should propagate errors from stream.list."""
    mock_iii = MagicMock()
    mock_iii.trigger = MagicMock(side_effect=Exception("function_not_found"))

    with patch("motia.streams.get_instance", return_value=mock_iii):
        stream = Stream("todos")
        with pytest.raises(Exception, match="function_not_found"):
            stream.get_group("default")

    mock_iii.trigger.assert_called_once_with({
        "function_id": "stream::list",
        "payload": {"stream_name": "todos", "group_id": "default"},
    })


def test_state_list_uses_state_list() -> None:
    """StateManager.list should use state.list SDK call."""
    mock_iii = MagicMock()
    mock_iii.trigger = MagicMock(return_value=[{"id": "x"}])

    with patch.object(_state_mod, "get_instance", return_value=mock_iii):
        manager = StateManager()
        result = manager.list("users")

    assert result == [{"id": "x"}]
    mock_iii.trigger.assert_called_once_with({
        "function_id": "state::list",
        "payload": {
            "scope": "users",
        },
    })


def test_state_list_propagates_errors() -> None:
    """StateManager.list should propagate errors from state.list."""
    mock_iii = MagicMock()
    mock_iii.trigger = MagicMock(side_effect=Exception("function_not_found"))

    with patch.object(_state_mod, "get_instance", return_value=mock_iii):
        manager = StateManager()
        with pytest.raises(Exception, match="function_not_found"):
            manager.list("users")

    mock_iii.trigger.assert_called_once_with({
        "function_id": "state::list",
        "payload": {"scope": "users"},
    })
