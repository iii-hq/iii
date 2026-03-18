"""Tests for the public async API surface of the III class."""

import inspect

from iii.iii import III


def test_trigger_async_is_coroutine_function():
    assert hasattr(III, "trigger_async")
    assert inspect.iscoroutinefunction(III.trigger_async)


def test_list_functions_async_is_coroutine_function():
    assert hasattr(III, "list_functions_async")
    assert inspect.iscoroutinefunction(III.list_functions_async)


def test_list_workers_async_is_coroutine_function():
    assert hasattr(III, "list_workers_async")
    assert inspect.iscoroutinefunction(III.list_workers_async)


def test_list_triggers_async_is_coroutine_function():
    assert hasattr(III, "list_triggers_async")
    assert inspect.iscoroutinefunction(III.list_triggers_async)


def test_create_channel_async_is_coroutine_function():
    assert hasattr(III, "create_channel_async")
    assert inspect.iscoroutinefunction(III.create_channel_async)


def test_connect_async_is_coroutine_function():
    assert hasattr(III, "connect_async")
    assert inspect.iscoroutinefunction(III.connect_async)


def test_shutdown_async_is_coroutine_function():
    assert hasattr(III, "shutdown_async")
    assert inspect.iscoroutinefunction(III.shutdown_async)


def test_async_methods_have_docstrings():
    """All public async methods must have docstrings."""
    async_methods = [
        "trigger_async",
        "list_functions_async",
        "list_workers_async",
        "list_triggers_async",
        "create_channel_async",
        "connect_async",
        "shutdown_async",
    ]
    for name in async_methods:
        method = getattr(III, name)
        assert method.__doc__ is not None, f"{name} is missing a docstring"
        assert len(method.__doc__.strip()) > 0, f"{name} has an empty docstring"


def test_no_private_async_methods_remain():
    """Ensure no _async_* private methods remain on III."""
    for name in dir(III):
        assert not name.startswith("_async_"), (
            f"Private async method {name} still exists — should be renamed to public *_async pattern"
        )
