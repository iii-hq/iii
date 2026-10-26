package http

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	nethttp "net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/coder/websocket"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// frame is one WebSocket message the fake engine received on a write channel.
type frame struct {
	typ  websocket.MessageType
	data []byte
}

// writeSession is everything a channel writer sent on one connection, up to its close.
type writeSession struct {
	uri         string
	frames      []frame
	closeStatus websocket.StatusCode
}

// body concatenates the binary frames of the session.
func (s writeSession) body() []byte {
	var out []byte
	for _, f := range s.frames {
		if f.typ == websocket.MessageBinary {
			out = append(out, f.data...)
		}
	}
	return out
}

// fakeEngine stands in for the engine's channel endpoint /ws/channels/{id}: a reader
// (dir=read) receives requestChunks as binary frames followed by a close, the way the
// http worker streams a request body; a writer (dir=write) is drained and recorded up to
// its close, the way the http worker reads the response channel.
type fakeEngine struct {
	srv    *httptest.Server
	reads  chan string
	writes chan writeSession
}

func newFakeEngine(t *testing.T, requestChunks ...[]byte) *fakeEngine {
	t.Helper()
	f := &fakeEngine{reads: make(chan string, 4), writes: make(chan writeSession, 4)}
	f.srv = httptest.NewServer(nethttp.HandlerFunc(func(w nethttp.ResponseWriter, r *nethttp.Request) {
		if !strings.HasPrefix(r.URL.Path, "/ws/channels/") {
			nethttp.NotFound(w, r)
			return
		}
		conn, err := websocket.Accept(w, r, nil)
		if err != nil {
			return
		}
		conn.SetReadLimit(-1)
		ctx := r.Context()
		switch r.URL.Query().Get("dir") {
		case "read":
			f.reads <- r.URL.RequestURI()
			for _, chunk := range requestChunks {
				if err := conn.Write(ctx, websocket.MessageBinary, chunk); err != nil {
					return
				}
			}
			_ = conn.Close(websocket.StatusNormalClosure, "stream_complete")
		case "write":
			s := writeSession{uri: r.URL.RequestURI()}
			for {
				typ, data, err := conn.Read(ctx)
				if err != nil {
					s.closeStatus = websocket.CloseStatus(err)
					break
				}
				s.frames = append(s.frames, frame{typ, data})
			}
			f.writes <- s
		}
	}))
	t.Cleanup(f.srv.Close)
	return f
}

func (f *fakeEngine) address() string { return "ws" + strings.TrimPrefix(f.srv.URL, "http") }

// client returns an unconnected client whose Address is the fake engine.
func (f *fakeEngine) client(t *testing.T) *iii.Client {
	t.Helper()
	c := iii.New(f.address())
	t.Cleanup(func() { _ = c.Close() })
	return c
}

func (f *fakeEngine) nextWrite(t *testing.T) writeSession {
	t.Helper()
	select {
	case s := <-f.writes:
		return s
	case <-time.After(5 * time.Second):
		t.Fatal("the response channel was never written and closed")
		return writeSession{}
	}
}

// expectNoWrite asserts the response channel was never dialled.
func (f *fakeEngine) expectNoWrite(t *testing.T) {
	t.Helper()
	select {
	case s := <-f.writes:
		t.Fatalf("response channel was used: %+v", s)
	case <-time.After(150 * time.Millisecond):
	}
}

// workerPayload is an invocation payload shaped exactly like the http worker's
// HttpRequest (workers/http/src/types.rs), channel refs included.
func workerPayload(t *testing.T, withRefs bool) json.RawMessage {
	t.Helper()
	m := map[string]any{
		"query_params": map[string]string{"q": "go"},
		"path_params":  map[string]string{"id": "42"},
		"headers":      map[string]string{"content-type": "application/octet-stream"},
		"path":         "/upload/:id",
		"method":       "POST",
		"body":         nil,
		"trigger":      map[string]string{"type": "http", "path": "/upload/42", "method": "POST"},
	}
	if withRefs {
		m["request_body"] = iii.StreamChannelRef{ChannelID: "req-ch", AccessKey: "req key", Direction: iii.ChannelRead}
		m["response"] = iii.StreamChannelRef{ChannelID: "res-ch", AccessKey: "res-key", Direction: iii.ChannelWrite}
	}
	data, err := json.Marshal(m)
	if err != nil {
		t.Fatal(err)
	}
	return data
}

func testContext(t *testing.T) context.Context {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	t.Cleanup(cancel)
	return ctx
}

// pattern returns n deterministic bytes.
func pattern(n int, seed byte) []byte {
	b := make([]byte, n)
	for i := range b {
		b[i] = byte(i%251) ^ seed
	}
	return b
}

// TestWrapStreamsRequestAndResponse drives the adapter end to end: the request body
// arrives over the read channel in several frames, and the status, headers and a body
// larger than one 64 KiB frame go out over the write channel, which is closed when the
// handler returns without calling Close.
func TestWrapStreamsRequestAndResponse(t *testing.T) {
	reqChunks := [][]byte{pattern(70_000, 1), pattern(10, 2), pattern(33_000, 3)}
	wantReq := bytes.Join(reqChunks, nil)
	f := newFakeEngine(t, reqChunks...)

	responseBody := pattern(200_000, 9)
	var gotReq []byte
	var seen StreamRequest
	handler := Wrap(f.client(t), func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
		var err error
		if gotReq, err = io.ReadAll(req); err != nil {
			return nil, err
		}
		seen = StreamRequest{PathParams: req.PathParams, QueryParams: req.QueryParams, Headers: req.Headers, Path: req.Path, Method: req.Method, Body: req.Body}
		if err := res.Status(201); err != nil {
			return nil, err
		}
		if err := res.Headers(map[string]string{"content-type": "application/octet-stream"}); err != nil {
			return nil, err
		}
		if _, err := res.Write(responseBody); err != nil {
			return nil, err
		}
		return nil, nil
	})

	result, err := handler(testContext(t), workerPayload(t, true))
	if err != nil || result != nil {
		t.Fatalf("handler = (%v, %v), want (nil, nil)", result, err)
	}
	if !bytes.Equal(gotReq, wantReq) {
		t.Fatalf("request body: got %d bytes, want %d", len(gotReq), len(wantReq))
	}
	if seen.Method != "POST" || seen.Path != "/upload/:id" || seen.PathParams["id"] != "42" ||
		seen.QueryParams["q"].First() != "go" || seen.Headers["content-type"].First() != "application/octet-stream" ||
		string(seen.Body) != "null" {
		t.Errorf("decoded request = %+v", seen)
	}

	select {
	case uri := <-f.reads:
		if want := "/ws/channels/req-ch?key=req%20key&dir=read"; uri != want {
			t.Errorf("reader URI = %q, want %q", uri, want)
		}
	default:
		t.Error("the request body channel was never opened")
	}

	s := f.nextWrite(t)
	if want := "/ws/channels/res-ch?key=res-key&dir=write"; s.uri != want {
		t.Errorf("writer URI = %q, want %q", s.uri, want)
	}
	if len(s.frames) < 2 {
		t.Fatalf("got %d frames, want control messages then body", len(s.frames))
	}
	wantControl := []string{
		`{"type":"set_status","status_code":201}`,
		`{"type":"set_headers","headers":{"content-type":"application/octet-stream"}}`,
	}
	for i, want := range wantControl {
		if s.frames[i].typ != websocket.MessageText || string(s.frames[i].data) != want {
			t.Errorf("frame %d = %v %s, want text %s", i, s.frames[i].typ, s.frames[i].data, want)
		}
	}
	binary := s.frames[len(wantControl):]
	if len(binary) != 4 {
		t.Errorf("body frames = %d, want 4 (64 KiB chunks of 200000 bytes)", len(binary))
	}
	for _, fr := range binary {
		if fr.typ != websocket.MessageBinary || len(fr.data) > 64*1024 {
			t.Errorf("body frame: type %v, %d bytes", fr.typ, len(fr.data))
		}
	}
	if !bytes.Equal(s.body(), responseBody) {
		t.Errorf("response body: got %d bytes, want %d", len(s.body()), len(responseBody))
	}
	if s.closeStatus != websocket.StatusNormalClosure {
		t.Errorf("close status = %v, want normal closure (end of stream)", s.closeStatus)
	}
}

// TestWrapExplicitClose checks Close ends the stream once, and that the adapter does not
// dial the channel again after it.
func TestWrapExplicitClose(t *testing.T) {
	f := newFakeEngine(t)
	var afterClose error
	handler := Wrap(f.client(t), func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
		if _, err := io.WriteString(res, "hello"); err != nil {
			return nil, err
		}
		if err := res.Close(); err != nil {
			return nil, err
		}
		if err := res.Close(); err != nil {
			return nil, fmt.Errorf("second Close: %w", err)
		}
		_, afterClose = res.Write([]byte("late"))
		return nil, nil
	})
	if _, err := handler(testContext(t), workerPayload(t, true)); err != nil {
		t.Fatal(err)
	}
	if !errors.Is(afterClose, ErrResponseClosed) {
		t.Errorf("Write after Close = %v, want ErrResponseClosed", afterClose)
	}
	s := f.nextWrite(t)
	if len(s.frames) != 1 || string(s.body()) != "hello" || s.closeStatus != websocket.StatusNormalClosure {
		t.Errorf("session = %+v", s)
	}
	f.expectNoWrite(t)
}

// TestWrapNilResultEndsEmptyStream: a handler that returns nil without writing still
// ends the stream, so the http worker answers 200 at once instead of waiting for its
// idle timeout.
func TestWrapNilResultEndsEmptyStream(t *testing.T) {
	f := newFakeEngine(t)
	handler := Wrap(f.client(t), func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
		return nil, nil
	})
	if _, err := handler(testContext(t), workerPayload(t, true)); err != nil {
		t.Fatal(err)
	}
	s := f.nextWrite(t)
	if len(s.frames) != 0 || s.closeStatus != websocket.StatusNormalClosure {
		t.Errorf("session = %+v, want a bare close", s)
	}
	select {
	case uri := <-f.reads:
		t.Errorf("request body channel opened (%s) though the handler never read it", uri)
	default:
	}
}

// TestWrapStatusOnlyIsClosed: control messages without a body are flushed by the close.
func TestWrapStatusOnlyIsClosed(t *testing.T) {
	f := newFakeEngine(t)
	handler := Wrap(f.client(t), func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
		return nil, res.Status(204)
	})
	if _, err := handler(testContext(t), workerPayload(t, true)); err != nil {
		t.Fatal(err)
	}
	s := f.nextWrite(t)
	if len(s.frames) != 1 || string(s.frames[0].data) != `{"type":"set_status","status_code":204}` ||
		s.closeStatus != websocket.StatusNormalClosure {
		t.Errorf("session = %+v", s)
	}
}

// TestWrapBufferedResponse: returning a Response answers buffered and never dials the
// response channel, so the http worker uses the returned value.
func TestWrapBufferedResponse(t *testing.T) {
	f := newFakeEngine(t)
	handler := Wrap(f.client(t), func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
		return Response[map[string]string]{
			StatusCode: 202,
			Headers:    map[string]string{"x-id": "1"},
			Body:       map[string]string{"ok": "yes"},
		}, nil
	})
	result, err := handler(testContext(t), workerPayload(t, true))
	if err != nil {
		t.Fatal(err)
	}
	out, err := json.Marshal(result)
	if err != nil {
		t.Fatal(err)
	}
	if want := `{"status_code":202,"headers":{"x-id":"1"},"body":{"ok":"yes"}}`; string(out) != want {
		t.Errorf("result = %s, want %s", out, want)
	}
	f.expectNoWrite(t)
}

// TestWrapErrorBeforeBody: an error with no streamed body leaves the channel alone so
// the http worker reports the invocation error.
func TestWrapErrorBeforeBody(t *testing.T) {
	f := newFakeEngine(t)
	boom := errors.New("boom")
	handler := Wrap(f.client(t), func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
		return nil, boom
	})
	if _, err := handler(testContext(t), workerPayload(t, true)); !errors.Is(err, boom) {
		t.Fatalf("err = %v, want boom", err)
	}
	f.expectNoWrite(t)
}

// TestWrapErrorAfterBody: once a body is streamed the response is committed, so an
// error still ends the stream rather than leaving the HTTP client hanging.
func TestWrapErrorAfterBody(t *testing.T) {
	f := newFakeEngine(t)
	boom := errors.New("boom")
	handler := Wrap(f.client(t), func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
		if _, err := res.Write([]byte("partial")); err != nil {
			return nil, err
		}
		return nil, boom
	})
	if _, err := handler(testContext(t), workerPayload(t, true)); !errors.Is(err, boom) {
		t.Fatalf("err = %v, want boom", err)
	}
	s := f.nextWrite(t)
	if string(s.body()) != "partial" || s.closeStatus != websocket.StatusNormalClosure {
		t.Errorf("session = %+v", s)
	}
}

// TestWrapWithoutChannelRefs: invoked directly (not through the http worker) the handler
// still runs; the streams report ErrMissingChannel and a buffered answer works.
func TestWrapWithoutChannelRefs(t *testing.T) {
	f := newFakeEngine(t)
	var readErr, statusErr, writeErr, closeErr error
	handler := Wrap(f.client(t), func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
		_, readErr = req.Read(make([]byte, 8))
		statusErr = res.Status(200)
		_, writeErr = res.Write([]byte("x"))
		closeErr = res.Close()
		return &Response[string]{StatusCode: 200, Body: "ok"}, nil
	})
	result, err := handler(testContext(t), workerPayload(t, false))
	if err != nil {
		t.Fatal(err)
	}
	for name, e := range map[string]error{"Read": readErr, "Status": statusErr, "Write": writeErr, "Close": closeErr} {
		if !errors.Is(e, ErrMissingChannel) {
			t.Errorf("%s = %v, want ErrMissingChannel", name, e)
		}
	}
	if r, ok := result.(*Response[string]); !ok || r.Body != "ok" {
		t.Errorf("result = %#v", result)
	}

	// An empty payload decodes as an empty request.
	if _, err := handler(testContext(t), nil); err != nil {
		t.Errorf("empty payload: %v", err)
	}
	f.expectNoWrite(t)
}

func TestWrapRejectsMalformedPayload(t *testing.T) {
	f := newFakeEngine(t)
	called := false
	handler := Wrap(f.client(t), func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
		called = true
		return nil, nil
	})
	if _, err := handler(testContext(t), json.RawMessage(`"not an object"`)); err == nil {
		t.Fatal("want a decode error")
	}
	if called {
		t.Error("handler ran on a malformed payload")
	}
}

func TestWrapNilHandler(t *testing.T) {
	c := iii.New("ws://127.0.0.1:1")
	defer c.Close()
	if h := Wrap(c, nil); h != nil {
		t.Fatal("Wrap(client, nil) should be nil")
	}
	if err := c.RegisterFunction("api::nil", Wrap(c, nil)); err == nil {
		t.Fatal("RegisterFunction accepted a nil wrapped handler")
	}
	defer func() {
		if recover() == nil {
			t.Error("Wrap(nil, fn) did not panic")
		}
	}()
	Wrap(nil, func(context.Context, *StreamRequest, *StreamResponse) (any, error) { return nil, nil })
}

func TestStreamRequestReadStopsAtEOF(t *testing.T) {
	f := newFakeEngine(t, []byte("abc"))
	req := &StreamRequest{RequestBody: iii.OpenReader(f.address(), iii.StreamChannelRef{ChannelID: "c", AccessKey: "k", Direction: iii.ChannelRead})}
	defer req.RequestBody.Close()
	buf := make([]byte, 2)
	var got []byte
	for {
		n, err := req.Read(buf)
		got = append(got, buf[:n]...)
		if err == io.EOF {
			break
		}
		if err != nil {
			t.Fatal(err)
		}
	}
	if string(got) != "abc" {
		t.Fatalf("read %q, want abc", got)
	}
	if n, err := req.Read(buf); n != 0 || err != io.EOF {
		t.Fatalf("Read after EOF = (%d, %v), want (0, EOF)", n, err)
	}
	if n, err := req.Read(nil); n != 0 || err != nil {
		t.Fatalf("Read(nil) = (%d, %v), want (0, nil)", n, err)
	}
}

func TestIsNilResult(t *testing.T) {
	var nilMap map[string]any
	var nilResp *Response[string]
	for _, v := range []any{nil, nilMap, nilResp, json.RawMessage(nil), json.RawMessage(" null ")} {
		if !isNilResult(v) {
			t.Errorf("isNilResult(%#v) = false", v)
		}
	}
	for _, v := range []any{0, "", map[string]any{}, Response[any]{}, json.RawMessage(`{}`)} {
		if isNilResult(v) {
			t.Errorf("isNilResult(%#v) = true", v)
		}
	}
}
