"""StreamRequest / StreamResponse rename, with Http* back-compat aliases."""

from iii import HttpRequest, HttpResponse, StreamRequest, StreamResponse


def test_stream_types_exported() -> None:
    assert StreamRequest is not None
    assert StreamResponse is not None


def test_http_names_alias_stream_types() -> None:
    assert HttpRequest is StreamRequest
    assert HttpResponse is StreamResponse
