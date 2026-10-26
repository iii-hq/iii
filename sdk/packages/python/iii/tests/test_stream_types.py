"""StreamRequest / StreamResponse streaming types and buffered Http* types."""

from typing import Any

import pytest
from iii_helpers.http import HttpRequest, HttpResponse

from iii import StreamRequest, StreamResponse


def test_stream_types_exported() -> None:
    assert StreamRequest is not None
    assert StreamResponse is not None


def test_http_names_are_buffered_types() -> None:
    assert HttpRequest is not StreamRequest
    assert HttpResponse is not StreamResponse


class _Writer:
    def __init__(self) -> None:
        self.messages: list[str] = []

    async def send_message_async(self, msg: str) -> None:
        self.messages.append(msg)


@pytest.mark.asyncio
async def test_stream_response_control_messages_are_compact_utf8_json() -> None:
    writer: Any = _Writer()
    response = StreamResponse(writer)
    await response.status(201)
    await response.headers({"content-disposition": 'attachment; filename="relatório.pdf"'})
    assert writer.messages == [
        '{"type":"set_status","status_code":201}',
        '{"type":"set_headers","headers":{"content-disposition":"attachment; filename=\\"relatório.pdf\\""}}',
    ]
