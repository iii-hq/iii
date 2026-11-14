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
	"sync"
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
	closeReason string
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

// fakeEngine stands in for the engine: the worker socket at / runs the http worker's
// side of an invocation (it sends invokefunction as an observer activation and reads the
// invocationresult), and the channel endpoint /ws/channels/{id} serves the streams. A
// reader (dir=read) receives requestChunks as binary frames followed by a close, the
// way the http worker streams a request body; a writer (dir=write) is drained and
// recorded up to its close, the way the http worker reads the response. Any other path
// is refused: the client's OpenTelemetry socket (/otel) is never taken for the worker's.
type fakeEngine struct {
	srv        *httptest.Server
	reads      chan string
	writes     chan writeSession
	handshakes chan handshake

	mu      sync.Mutex
	worker  *websocket.Conn
	results map[string]chan invocationResult
	// connected is closed once the worker's socket was accepted (worker is set).
	connected chan struct{}
	once      sync.Once
}

// invocationResult is the worker's answer to one invocation.
type invocationResult struct {
	Result json.RawMessage `json:"result"`
	Error  *struct {
		Code    string `json:"code"`
		Message string `json:"message"`
	} `json:"error"`
}

// handshake is the direction and request headers of one channel WebSocket upgrade.
type handshake struct {
	dir    string
	header nethttp.Header
}

// fakeOptions changes how the fake engine ends or drains its channels.
type fakeOptions struct {
	// requestEnd ends the request body channel after its chunks: "" closes it cleanly,
	// "abort" closes it the way the engine reports an aborted writer (1011 and a
	// reason), and "drop" cuts the connection with no close frame.
	requestEnd string
	// stallWrites delays reading a write channel, so a large write blocks meanwhile.
	stallWrites time.Duration
}

func newFakeEngine(t *testing.T, requestChunks ...[]byte) *fakeEngine {
	t.Helper()
	return newFakeEngineWith(t, fakeOptions{}, requestChunks...)
}

func newFakeEngineWith(t *testing.T, opts fakeOptions, requestChunks ...[]byte) *fakeEngine {
	t.Helper()
	f := &fakeEngine{
		reads:      make(chan string, 4),
		writes:     make(chan writeSession, 4),
		handshakes: make(chan handshake, 16),
		results:    map[string]chan invocationResult{},
		connected:  make(chan struct{}),
	}
	f.srv = httptest.NewServer(nethttp.HandlerFunc(func(w nethttp.ResponseWriter, r *nethttp.Request) {
		switch {
		case r.URL.Path == "" || r.URL.Path == "/":
			f.serveWorker(w, r)
			return
		case !strings.HasPrefix(r.URL.Path, "/ws/channels/"):
			// Not a socket this engine serves: the client's OpenTelemetry connection
			// (/otel) is refused, never taken for the worker's.
			nethttp.NotFound(w, r)
			return
		}
		select {
		case f.handshakes <- handshake{dir: r.URL.Query().Get("dir"), header: r.Header.Clone()}:
		default:
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
			switch opts.requestEnd {
			case "abort":
				_ = conn.Close(websocket.StatusInternalError, "channel writer aborted: connection lost before close")
			case "drop":
				_ = conn.CloseNow()
			default:
				_ = conn.Close(websocket.StatusNormalClosure, "stream_complete")
			}
		case "write":
			s := writeSession{uri: r.URL.RequestURI()}
			time.Sleep(opts.stallWrites)
			for {
				typ, data, err := conn.Read(ctx)
				if err != nil {
					s.closeStatus = websocket.CloseStatus(err)
					var ce websocket.CloseError
					if errors.As(err, &ce) {
						s.closeReason = ce.Reason
					}
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

// serveWorker accepts the worker's socket and routes the invocation results it sends.
func (f *fakeEngine) serveWorker(w nethttp.ResponseWriter, r *nethttp.Request) {
	conn, err := websocket.Accept(w, r, nil)
	if err != nil {
		return
	}
	conn.SetReadLimit(-1)
	f.mu.Lock()
	f.worker = conn
	f.mu.Unlock()
	f.once.Do(func() { close(f.connected) })
	for {
		_, data, err := conn.Read(r.Context())
		if err != nil {
			return
		}
		var msg struct {
			Type         string `json:"type"`
			InvocationID string `json:"invocation_id"`
		}
		if json.Unmarshal(data, &msg) != nil || msg.Type != "invocationresult" {
			continue
		}
		var res invocationResult
		_ = json.Unmarshal(data, &res)
		f.mu.Lock()
		ch := f.results[msg.InvocationID]
		f.mu.Unlock()
		if ch != nil {
			ch <- res
		}
	}
}

func (f *fakeEngine) address() string { return "ws" + strings.TrimPrefix(f.srv.URL, "http") }

// run registers fn (wrapped) on a client connected to the fake engine and invokes it
// with payload the way the http worker does, as an observer activation, and returns
// the worker's answer.
func (f *fakeEngine) run(t *testing.T, fn HandlerFunc, payload json.RawMessage, opts ...iii.Option) invocationResult {
	t.Helper()
	c := iii.New(f.address(), opts...)
	t.Cleanup(func() { _ = c.Close() })
	if _, err := c.RegisterFunction("api::handler", Wrap(fn)); err != nil {
		t.Fatal(err)
	}
	ctx := testContext(t)
	if err := c.Connect(ctx); err != nil {
		t.Fatal(err)
	}
	// Connect returns once the client's handshake is done, which can be before this
	// side accepted the socket.
	select {
	case <-f.connected:
	case <-ctx.Done():
		t.Fatal("the worker never connected")
	}
	id := fmt.Sprintf("00000000-0000-4000-8000-%012d", time.Now().UnixNano()%1_000_000_000_000)
	ch := make(chan invocationResult, 1)
	f.mu.Lock()
	f.results[id] = ch
	conn := f.worker
	f.mu.Unlock()
	invoke, err := json.Marshal(map[string]any{
		"type":          "invokefunction",
		"invocation_id": id,
		"function_id":   "api::handler",
		"data":          payload,
		"activation":    map[string]string{"observer_id": "obs-1", "activation_id": "act-1"},
	})
	if err != nil {
		t.Fatal(err)
	}
	if err := conn.Write(ctx, websocket.MessageText, invoke); err != nil {
		t.Fatal(err)
	}
	select {
	case res := <-ch:
		return res
	case <-ctx.Done():
		t.Fatal("the worker never answered the invocation")
		return invocationResult{}
	}
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
// HttpRequest (workers/http/src/types.rs), stream refs included.
func workerPayload(t *testing.T, withRefs bool) json.RawMessage {
	t.Helper()
	m := map[string]any{
		"query_params": map[string]string{"q": "go"},
		"path_params":  map[string]string{"id": "42"},
		"headers":      map[string]string{"content-type": "application/octet-stream"},
		"path":         "/upload/:id",
		"method":       "POST",
		"body":         nil,
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

// streamCode is the stream error code of err, "" for another error.
func streamCode(err error) string {
	var ie *iii.InvocationError
	if errors.As(err, &ie) {
		return ie.Code
	}
	return ""
}

// TestWrapStreamsRequestAndResponse drives the adapter end to end: the request body
// arrives over the read channel in several frames, and the status, headers and a body
// larger than one 64 KiB frame go out over the write channel, which the SDK closes when
// the handler returns without calling Close.
func TestWrapStreamsRequestAndResponse(t *testing.T) {
	reqChunks := [][]byte{pattern(70_000, 1), pattern(10, 2), pattern(33_000, 3)}
	wantReq := bytes.Join(reqChunks, nil)
	f := newFakeEngine(t, reqChunks...)

	responseBody := pattern(200_000, 9)
	var gotReq []byte
	var seen StreamRequest
	res := f.run(t, func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
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
	}, workerPayload(t, true))
	if res.Error != nil || string(res.Result) != "null" {
		t.Fatalf("answer = (%s, %+v), want (null, no error)", res.Result, res.Error)
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

// TestWrapSendsClientHeaders checks the streams dial through the client that received
// them, so the headers set with iii.WithHeaders reach the channel WebSocket handshakes,
// together with the Authorization header III_TOKEN adds.
func TestWrapSendsClientHeaders(t *testing.T) {
	t.Setenv("III_TOKEN", "tok-123")
	f := newFakeEngine(t, []byte("ping"))
	res := f.run(t, func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
		body, err := io.ReadAll(req)
		if err != nil {
			return nil, err
		}
		_, err = res.Write(body)
		return nil, err
	}, workerPayload(t, true), iii.WithHeaders(nethttp.Header{"X-Tenant": {"acme"}}))
	if res.Error != nil {
		t.Fatalf("error = %+v", res.Error)
	}
	if s := f.nextWrite(t); string(s.body()) != "ping" {
		t.Fatalf("response body = %q, want ping", s.body())
	}

	seen := map[string]nethttp.Header{}
	for len(seen) < 2 {
		select {
		case h := <-f.handshakes:
			seen[h.dir] = h.header
		case <-time.After(2 * time.Second):
			t.Fatalf("handshakes seen = %v, want read and write", seen)
		}
	}
	for _, dir := range []string{"read", "write"} {
		h := seen[dir]
		if got := h.Get("X-Tenant"); got != "acme" {
			t.Errorf("%s handshake X-Tenant = %q, want acme (WithHeaders)", dir, got)
		}
		if got := h.Get("Authorization"); got != "Bearer tok-123" {
			t.Errorf("%s handshake Authorization = %q, want Bearer tok-123 (III_TOKEN)", dir, got)
		}
	}
}

// TestWrapExplicitClose checks Close ends the stream once, that a write after it fails
// with STREAM_CLOSED, and that the SDK does not end the stream again.
func TestWrapExplicitClose(t *testing.T) {
	f := newFakeEngine(t)
	var afterClose error
	res := f.run(t, func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
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
	}, workerPayload(t, true))
	if res.Error != nil {
		t.Fatalf("error = %+v", res.Error)
	}
	if streamCode(afterClose) != iii.CodeStreamClosed {
		t.Errorf("Write after Close = %v, want STREAM_CLOSED", afterClose)
	}
	s := f.nextWrite(t)
	if len(s.frames) != 1 || string(s.body()) != "hello" || s.closeStatus != websocket.StatusNormalClosure {
		t.Errorf("session = %+v", s)
	}
	f.expectNoWrite(t)
}

// TestWrapNilResultEndsEmptyStream: a handler that returns nil without writing still
// ends the stream, so the http worker answers 200 at once instead of waiting for its
// idle timeout. A typed nil (a nil *Response) and a JSON null are nil results too.
func TestWrapNilResultEndsEmptyStream(t *testing.T) {
	var nilResp *Response[string]
	for name, result := range map[string]any{"nil": nil, "nil pointer": nilResp, "raw null": json.RawMessage(" null ")} {
		t.Run(name, func(t *testing.T) {
			f := newFakeEngine(t)
			res := f.run(t, func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
				return result, nil
			}, workerPayload(t, true))
			if res.Error != nil {
				t.Fatalf("error = %+v", res.Error)
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
		})
	}
}

// TestWrapStatusOnlyIsClosed: control messages without a body are flushed by the close.
func TestWrapStatusOnlyIsClosed(t *testing.T) {
	f := newFakeEngine(t)
	res := f.run(t, func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
		return nil, res.Status(204)
	}, workerPayload(t, true))
	if res.Error != nil {
		t.Fatalf("error = %+v", res.Error)
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
	res := f.run(t, func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
		return Response[map[string]string]{
			StatusCode: 202,
			Headers:    map[string]string{"x-id": "1"},
			Body:       map[string]string{"ok": "yes"},
		}, nil
	}, workerPayload(t, true))
	if res.Error != nil {
		t.Fatalf("error = %+v", res.Error)
	}
	if want := `{"status_code":202,"headers":{"x-id":"1"},"body":{"ok":"yes"}}`; string(res.Result) != want {
		t.Errorf("result = %s, want %s", res.Result, want)
	}
	f.expectNoWrite(t)
}

// TestWrapErrorBeforeBody: an error with an untouched response leaves the channel alone
// so the http worker reports the invocation error.
func TestWrapErrorBeforeBody(t *testing.T) {
	f := newFakeEngine(t)
	res := f.run(t, func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
		return nil, errors.New("boom")
	}, workerPayload(t, true))
	if res.Error == nil || res.Error.Message != "boom" {
		t.Fatalf("error = %+v, want boom", res.Error)
	}
	f.expectNoWrite(t)
}

// TestWrapErrorAfterBodyAborts: a handler error after body bytes were streamed must not
// end the stream cleanly, or the truncated body would look complete: the SDK aborts the
// response (close code 1011 with the error), which the engine relays to the http worker
// as an abort.
func TestWrapErrorAfterBodyAborts(t *testing.T) {
	f := newFakeEngine(t)
	res := f.run(t, func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
		if _, err := res.Write([]byte("partial")); err != nil {
			return nil, err
		}
		return nil, errors.New("boom")
	}, workerPayload(t, true))
	if res.Error == nil || res.Error.Message != "boom" {
		t.Fatalf("error = %+v, want boom", res.Error)
	}
	s := f.nextWrite(t)
	if string(s.body()) != "partial" {
		t.Errorf("body = %q, want partial", s.body())
	}
	if s.closeStatus != websocket.StatusInternalError || !strings.Contains(s.closeReason, "boom") {
		t.Errorf("close = %v %q, want an abort (1011) with the error", s.closeStatus, s.closeReason)
	}
	for _, fr := range s.frames {
		if fr.typ == websocket.MessageText {
			t.Errorf("unexpected text frame %q", fr.data)
		}
	}
}

// TestWrapFailedWriteAborts: a response write cut short by its context (the engine is
// not reading) aborts the stream, so the engine sees the socket end with no clean close,
// even when the handler swallows the error; every later write fails with STREAM_CLOSED
// and Close returns nil.
func TestWrapFailedWriteAborts(t *testing.T) {
	for _, swallow := range []bool{false, true} {
		t.Run(fmt.Sprintf("swallow=%v", swallow), func(t *testing.T) {
			f := newFakeEngineWith(t, fakeOptions{stallWrites: 300 * time.Millisecond})
			var writeErr, again, statusAgain, closeErr error
			res := f.run(t, func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
				if err := res.Status(201); err != nil {
					return nil, err
				}
				wctx, cancel := context.WithTimeout(ctx, 50*time.Millisecond)
				defer cancel()
				writeErr = res.Stream().Write(wctx, pattern(8<<20, 5))
				_, again = res.Write([]byte("more"))
				statusAgain = res.Status(500)
				if swallow {
					closeErr = res.Close()
					return nil, nil
				}
				return nil, writeErr
			}, workerPayload(t, true))
			if !errors.Is(writeErr, context.DeadlineExceeded) {
				t.Fatalf("Write = %v, want its context's deadline", writeErr)
			}
			if streamCode(again) != iii.CodeStreamClosed || streamCode(statusAgain) != iii.CodeStreamClosed {
				t.Errorf("later calls = (%v, %v), want STREAM_CLOSED", again, statusAgain)
			}
			if closeErr != nil {
				t.Errorf("Close after a failed write = %v, want nil", closeErr)
			}
			if !swallow && res.Error == nil {
				t.Error("the handler's error did not reach the answer")
			}
			s := f.nextWrite(t)
			if s.closeStatus != -1 {
				t.Errorf("close status = %v, want none (an abort, not a clean end)", s.closeStatus)
			}
			if len(s.frames) == 0 || string(s.frames[0].data) != `{"type":"set_status","status_code":201}` {
				t.Errorf("first frame missing or wrong: %d frames", len(s.frames))
			}
			if n := len(s.body()); n >= 8<<20 {
				t.Errorf("the whole %d-byte body arrived", n)
			}
		})
	}
}

// TestStreamRequestReadSurfacesAbort: a request body that ends without a clean close
// (the engine's 1011 for an aborted writer, or a dropped connection) makes Read fail
// with STREAM_ABORTED instead of io.EOF; io.ReadAll returns the bytes received before
// it, later reads return the same error, and the response channel is left untouched.
func TestStreamRequestReadSurfacesAbort(t *testing.T) {
	for _, tc := range []struct {
		end    string
		reason string
	}{
		{"abort", "channel writer aborted: connection lost before close"},
		{"drop", "connection lost"},
	} {
		t.Run(tc.end, func(t *testing.T) {
			f := newFakeEngineWith(t, fakeOptions{requestEnd: tc.end}, []byte("part-1 "), []byte("part-2"))
			var got []byte
			var readErr, again error
			res := f.run(t, func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
				got, readErr = io.ReadAll(req)
				_, again = req.Read(make([]byte, 8))
				return nil, readErr
			}, workerPayload(t, true))
			if readErr == nil {
				t.Fatalf("io.ReadAll = (%q, nil), want the abort error", got)
			}
			if string(got) != "part-1 part-2" {
				t.Errorf("bytes before the abort = %q, want part-1 part-2", got)
			}
			var ie *iii.InvocationError
			if !errors.As(readErr, &ie) || ie.Code != iii.CodeStreamAborted || !strings.Contains(ie.Message, tc.reason) {
				t.Errorf("read error = %v, want STREAM_ABORTED with %q", readErr, tc.reason)
			}
			if again != readErr {
				t.Errorf("Read after the abort = %v, want the same error", again)
			}
			if res.Error == nil || res.Error.Code != iii.CodeStreamAborted {
				t.Errorf("answer error = %+v, want STREAM_ABORTED", res.Error)
			}
			f.expectNoWrite(t)
		})
	}
}

// TestWrapWithoutChannelRefs: invoked directly (not through the http worker) the handler
// still runs; the streams report ErrMissingChannel and a buffered answer works.
func TestWrapWithoutChannelRefs(t *testing.T) {
	var readErr, statusErr, writeErr, closeErr error
	handler := Wrap(func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
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
}

func TestWrapRejectsMalformedPayload(t *testing.T) {
	called := false
	handler := Wrap(func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error) {
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
	if h := Wrap(nil); h != nil {
		t.Fatal("Wrap(nil) should be nil")
	}
	if _, err := c.RegisterFunction("api::nil", Wrap(nil)); err == nil {
		t.Fatal("RegisterFunction accepted a nil wrapped handler")
	}
}

func TestStreamRequestReadStopsAtEOF(t *testing.T) {
	f := newFakeEngine(t, []byte("abc"))
	c := iii.New(f.address())
	defer c.Close()
	var body iii.InputStream
	if err := json.Unmarshal([]byte(`{"channel_id":"c","access_key":"k","direction":"read"}`), &body); err != nil {
		t.Fatal(err)
	}
	req := &StreamRequest{RequestBody: body.Bind(c)}
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
