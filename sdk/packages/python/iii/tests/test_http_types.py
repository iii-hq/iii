from iii import HttpRequest, HttpResponse, StreamingRequest


def test_http_request_buffered_has_body_no_reader():
    req = HttpRequest(path="/x", method="GET", body={"a": 1})
    assert req.body == {"a": 1}
    assert not hasattr(req, "request_body")


def test_http_response_buffered_shape():
    resp = HttpResponse(statusCode=201, body={"ok": True})
    assert resp.status_code == 201


def test_streaming_request_has_reader_field():
    import dataclasses
    fields = {f.name for f in dataclasses.fields(StreamingRequest)}
    assert "request_body" in fields
    assert "body" not in fields
