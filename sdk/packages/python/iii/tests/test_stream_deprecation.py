"""Offline tests for the legacy Streams deprecation warnings (MOT-3619).

No engine is needed: ``create_stream`` is exercised against a stub client
that records the ``_helpers_create_stream`` shim call.
"""

from __future__ import annotations

import inspect
import subprocess
import sys
import warnings
from typing import Any

import pytest

from iii.helpers import create_stream
from iii.stream import IStream

GUIDE_URL = "https://iii.dev/docs/upgrading/migrate-from-streams"


def _expected(entry: str) -> str:
    return (
        f"{entry} is deprecated (iii-stream) and will be removed in a future release (version TBD). "
        f"Behavior is unchanged for now. Migration guide: {GUIDE_URL}"
    )


def _current_line() -> int:
    frame = inspect.currentframe()
    assert frame is not None and frame.f_back is not None
    return frame.f_back.f_lineno


class _StubClient:
    def __init__(self) -> None:
        self.calls: list[tuple[str, Any]] = []

    def _helpers_create_stream(self, stream_name: str, stream: Any) -> None:
        self.calls.append((stream_name, stream))


def _future_warnings(caught: list[warnings.WarningMessage]) -> list[warnings.WarningMessage]:
    return [w for w in caught if issubclass(w.category, FutureWarning)]


def test_istream_subclass_warns_at_class_statement() -> None:
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        class_line = _current_line() + 2

        class _Legacy(IStream[Any]):
            async def get(self, input: Any) -> Any:
                return None

            async def set(self, input: Any) -> Any:
                return None

            async def delete(self, input: Any) -> Any:
                return None

            async def list(self, input: Any) -> Any:
                return []

            async def list_groups(self, input: Any) -> Any:
                return []

            async def update(self, input: Any) -> Any:
                return None

    found = _future_warnings(caught)
    assert len(found) == 1
    assert str(found[0].message) == _expected("IStream")
    assert found[0].filename == __file__
    assert found[0].lineno == class_line


def test_create_stream_warns_at_caller_line_and_still_delegates() -> None:
    with pytest.warns(FutureWarning):

        class _Legacy(IStream[Any]):
            async def get(self, input: Any) -> Any:
                return None

            async def set(self, input: Any) -> Any:
                return None

            async def delete(self, input: Any) -> Any:
                return None

            async def list(self, input: Any) -> Any:
                return []

            async def list_groups(self, input: Any) -> Any:
                return []

            async def update(self, input: Any) -> Any:
                return None

    client = _StubClient()
    impl = _Legacy()
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        call_line = _current_line() + 1
        create_stream(client, "legacy", impl)  # type: ignore[arg-type]

    found = _future_warnings(caught)
    assert len(found) == 1
    assert str(found[0].message) == _expected("create_stream")
    assert found[0].filename == __file__
    assert found[0].lineno == call_line
    # Behavior unchanged: the call is still forwarded to the shim.
    assert client.calls == [("legacy", impl)]


def test_importing_legacy_modules_does_not_warn() -> None:
    code = (
        "import iii, iii.stream, iii.helpers, iii_helpers.stream\n"
        "from iii.helpers import create_stream\n"
        "from iii.stream import IStream\n"
        "from typing import Any\n"
        "IStream[Any]\n"
    )
    result = subprocess.run(
        [sys.executable, "-W", "error::FutureWarning", "-c", code],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr
