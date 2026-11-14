"""StreamRequest / StreamResponse streaming types and buffered Http* types."""

from iii_helpers.http import HttpRequest, HttpResponse

from iii import StreamRequest, StreamResponse


def test_stream_types_exported() -> None:
    assert StreamRequest is not None
    assert StreamResponse is not None


def test_http_names_are_buffered_types() -> None:
    assert HttpRequest is not StreamRequest
    assert HttpResponse is not StreamResponse
