"""A typed ``CallRequest`` must carry the target namespace so ``call()`` routes
to it, and names a function or an observer. ``TriggerRequest`` is the same
type under its 0.25 name."""

import pytest
from pydantic import ValidationError

from iii import CallRequest
from iii.protocol import TriggerRequest


def test_typed_call_request_preserves_namespace() -> None:
    req = CallRequest(function_id="orders::create", payload={}, namespace="orders")

    # The attribute is a real field, not a silently-ignored kwarg.
    assert req.namespace == "orders"
    assert req.model_dump().get("namespace") == "orders"


def test_typed_call_request_namespace_is_optional() -> None:
    req = CallRequest(function_id="orders::create", payload={})

    assert req.namespace is None


def test_a_call_request_names_a_function_or_an_observer_not_both() -> None:
    observer = object()
    assert CallRequest(observer=observer, payload={}).observer is observer
    with pytest.raises(ValidationError):
        CallRequest(payload={})
    with pytest.raises(ValidationError):
        CallRequest(function_id="orders::create", observer=observer, payload={})


def test_trigger_request_is_call_request() -> None:
    assert TriggerRequest is CallRequest
