"""Launched only by the isolated full-engine Rust integration test."""

import os
import time

from iii import InitOptions
from iii.errors import InvocationError
from iii.iii import III

# Outbound JSON envelope: 16 MiB (16,777,216 bytes).
JSON_FRAME_LIMIT_BYTES = 16 * 1024 * 1024

sdk = III(
    os.environ["JSON_SIZE_TEST_URL"],
    InitOptions(worker_name="json-size-python", enable_metrics_reporting=False, otel={"enabled": False}),
)
sdk.register_function("size::python", lambda data: "x" * data["size"])
sdk.register_function("size::python-length", lambda data: len(data))
try:
    for attempt in range(50):
        try:
            assert sdk.trigger({"function_id": "size::node", "payload": {"size": 1}, "timeout_ms": 1000}) == "x"
            break
        except Exception:
            if attempt == 49:
                raise
            time.sleep(0.1)
    identity = sdk.worker_id
    for target in ["size::node", "size::rust"]:
        try:
            sdk.trigger({"function_id": target, "payload": {"size": JSON_FRAME_LIMIT_BYTES}})
            raise AssertionError("oversized result was accepted")
        except InvocationError as error:
            assert error.code == "payload_too_large", error
        assert sdk.trigger({"function_id": target, "payload": {"size": 2}}) == "xx"
        # Incoming results above the library's former 1 MiB default.
        assert len(sdk.trigger({"function_id": target, "payload": {"size": 2 * 1024 * 1024}})) == 2 * 1024 * 1024
    assert sdk.worker_id == identity
    print("JSON_SIZE_PYTHON_CROSS_SDK_OK", flush=True)
    time.sleep(8)
finally:
    sdk.shutdown()
