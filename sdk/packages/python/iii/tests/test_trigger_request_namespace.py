"""A typed ``TriggerRequest`` must carry the target namespace so ``trigger()``
routes to it. The dict path already worked (``req.get("namespace")``); this pins
the typed path, which silently dropped the field when it was not declared —
pydantic ignores unknown kwargs, so ``model_dump()`` lost the namespace and the
call fell back to the default namespace.
"""

from iii.iii_types import TriggerRequest


def test_typed_trigger_request_preserves_namespace() -> None:
    req = TriggerRequest(function_id="orders::create", payload={}, namespace="orders")

    # The attribute is a real field, not a silently-ignored kwarg.
    assert req.namespace == "orders"
    # `trigger()` reads the routing namespace from the dumped request; it must
    # survive the round-trip to the wire.
    assert req.model_dump().get("namespace") == "orders"


def test_typed_trigger_request_namespace_is_optional() -> None:
    req = TriggerRequest(function_id="orders::create", payload={})

    assert req.namespace is None
