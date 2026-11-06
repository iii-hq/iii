"""Participant for the isolated, opt-in full-engine fragmentation integration."""

import os
import re
import threading

from iii import InitOptions
from iii.errors import InvocationError
from iii.iii import III

MESSAGE_LIMIT_BYTES = 64 * 1024 * 1024
ALPHABET = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
PATTERN_PERIOD = 97
PATTERN_CYCLE = PATTERN_PERIOD * len(ALPHABET)
sdk = III(
    os.environ["JSON_SIZE_TEST_URL"],
    InitOptions(worker_name="json-size-python", enable_metrics_reporting=False, otel={"enabled": False}),
)


def pattern(size: int, marker: str) -> str:
    seed = ord(marker)
    cycle = "".join(
        ALPHABET[(index % PATTERN_PERIOD + seed) % len(ALPHABET)]
        for index in range(PATTERN_CYCLE)
    )
    return (cycle * ((size + PATTERN_CYCLE - 1) // PATTERN_CYCLE))[:size]


def assert_pattern(value: object, size: int, marker: str) -> None:
    assert isinstance(value, str), type(value)
    assert len(value) == size
    assert value == pattern(size, marker), "fragmented payload content/order mismatch"


def check_limit_message(message: str) -> None:
    match = re.fullmatch(
        r"Serialized JSON envelope is (\d+) bytes; limit is 67108864 bytes\. "
        r"Use channels for larger payloads: https://iii\.dev/docs/creating-workers/channels#payload-size",
        message,
    )
    assert match, message
    assert int(match.group(1)) > MESSAGE_LIMIT_BYTES


def ready(_: object) -> bool:
    return True


def echo(data: object) -> object:
    return data


def roundtrip(data: dict[str, object]) -> bool:
    target = str(data["target"])
    size = int(data["size"])
    marker = str(data["marker"])
    result = sdk.call({"function_id": target, "payload": pattern(size, marker), "timeout_ms": 120_000})
    assert_pattern(result, size, marker)
    print("JSON_SIZE_PYTHON_CROSS_SDK_OK", flush=True)
    return True


def local_limit(_: object) -> bool:
    try:
        sdk.call({"function_id": "size::rust", "payload": pattern(MESSAGE_LIMIT_BYTES, "Z")})
        raise AssertionError("Python accepted an envelope over 64 MiB")
    except InvocationError as error:
        assert error.code == "payload_too_large", error
        check_limit_message(error.message)
    assert sdk.call({"function_id": "size::rust", "payload": "after-python-limit"}) == "after-python-limit"
    return True


sdk.register_function("size::python-ready", ready)
sdk.register_function("size::python", echo)
sdk.register_function("size::python-roundtrip", roundtrip)
sdk.register_function("size::python-local-limit", local_limit)
threading.Event().wait()
