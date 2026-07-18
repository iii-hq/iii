"""A typed middleware must read the target namespace the engine sends so it can
re-target the forwarded call at the caller's namespace."""

from iii import MiddlewareFunctionInput


def test_exposes_namespace_from_engine_middleware_input() -> None:
    # Mirrors the wire object the engine builds (engine/src/engine/mod.rs).
    data = {
        "function_id": "orders::create",
        "payload": {},
        "context": {},
        "namespace": "orders",
    }
    mid = MiddlewareFunctionInput(**data)
    assert mid.namespace == "orders"


def test_namespace_is_optional() -> None:
    data = {
        "function_id": "orders::create",
        "payload": {},
        "context": {},
    }
    mid = MiddlewareFunctionInput(**data)
    assert mid.namespace is None
