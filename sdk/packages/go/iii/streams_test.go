package iii

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/coder/websocket"
)

// This file tests call-native streams: the stream values on their own, then the rules
// of the calls that carry them, in-process (local dispatch, no pipe) and through pipes
// (streamEngine routes a DispatchRemote call back to the same worker over a relay).

func streamCtx(t *testing.T) context.Context {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	t.Cleanup(cancel)
	return ctx
}

// trackedReader is a reader that records its Close.
type trackedReader struct {
	io.Reader
	closed atomic.Bool
}

func (r *trackedReader) Close() error {
	r.closed.Store(true)
	return nil
}

// failingReader yields data, then fails with err.
type failingReader struct {
	data []byte
	err  error
}

func (r *failingReader) Read(p []byte) (int, error) {
	if len(r.data) == 0 {
		return 0, r.err
	}
	n := copy(p, r.data)
	r.data = r.data[n:]
	return n, nil
}

// sink is a writer that records its bytes, Flush and Close.
type sink struct {
	mu      sync.Mutex
	buf     bytes.Buffer
	flushed bool
	closed  chan struct{}
	once    sync.Once
}

func newSink() *sink { return &sink{closed: make(chan struct{})} }

func (s *sink) Write(p []byte) (int, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.buf.Write(p)
}

func (s *sink) Flush() error {
	s.mu.Lock()
	s.flushed = true
	s.mu.Unlock()
	return nil
}

func (s *sink) Close() error {
	s.once.Do(func() { close(s.closed) })
	return nil
}

func (s *sink) isClosed() bool {
	select {
	case <-s.closed:
		return true
	default:
		return false
	}
}

func (s *sink) String() string {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.buf.String()
}

func jsonOf(t *testing.T, v any) json.RawMessage {
	t.Helper()
	out, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	return out
}

func wantCode(t *testing.T, err error, code string) {
	t.Helper()
	if got := streamCode(err); got != code {
		t.Fatalf("err = %v, want %s", err, code)
	}
}

// =====================================================================================
// Stream values
// =====================================================================================

func TestInputStreamReads(t *testing.T) {
	ctx := streamCtx(t)
	data := []byte("id,total\r\n1,10\n2,20")
	small := make([]byte, 3)
	var got []byte
	in := InputStreamFromBytes(data)
	for {
		n, err := in.Read(small)
		got = append(got, small[:n]...)
		if err == io.EOF {
			break
		}
		if err != nil {
			t.Fatal(err)
		}
	}
	if !bytes.Equal(got, data) {
		t.Errorf("Read = %q", got)
	}
	if text, err := InputStreamFromBytes(data).Text(ctx); err != nil || text != string(data) {
		t.Errorf("Text = %q, %v", text, err)
	}
	var lines []string
	for line, err := range InputStreamFromBytes(data).Lines(ctx) {
		if err != nil {
			t.Fatal(err)
		}
		lines = append(lines, line)
	}
	if strings.Join(lines, "|") != "id,total|1,10|2,20" {
		t.Errorf("Lines = %q", lines)
	}
	big := bytes.Repeat([]byte("x"), 3*channelFrameSize+5)
	var frames int
	for f, err := range InputStreamFromBytes(big).Frames(ctx) {
		if err != nil || f.Type != FrameBinary || len(f.Data) > channelFrameSize {
			t.Fatalf("frame %v %d %v", f.Type, len(f.Data), err)
		}
		frames++
	}
	if frames != 4 {
		t.Errorf("frames = %d, want 4", frames)
	}
}

func TestInputStreamFromReaderOwnsTheReader(t *testing.T) {
	ctx := streamCtx(t)
	r := &trackedReader{Reader: strings.NewReader("hello")}
	if text, err := InputStreamFromReader(r).Text(ctx); err != nil || text != "hello" {
		t.Fatalf("Text = %q, %v", text, err)
	}
	if !r.closed.Load() {
		t.Error("the reader was not closed at the end of the stream")
	}

	// A read error aborts the stream, after the bytes read before it.
	got, err := InputStreamFromReader(&failingReader{data: []byte("part"), err: errors.New("disk gone")}).ReadAll(ctx)
	wantCode(t, err, CodeStreamAborted)
	if string(got) != "part" || !strings.Contains(err.Error(), "disk gone") {
		t.Errorf("ReadAll = %q, %v", got, err)
	}

	// Breaking out of a loop cancels the stream: the reader is closed.
	r = &trackedReader{Reader: strings.NewReader("a\nb\nc\n")}
	for range InputStreamFromReader(r).Lines(ctx) {
		break
	}
	if !r.closed.Load() {
		t.Error("break did not cancel the stream")
	}
}

func TestInputStreamHasOneReader(t *testing.T) {
	ctx := streamCtx(t)
	in := InputStreamFromBytes([]byte("abc"))
	if _, err := in.ReadAll(ctx); err != nil {
		t.Fatal(err)
	}
	_, err := in.ReadAll(ctx)
	wantCode(t, err, CodeStreamInUse)

	in = InputStreamFromBytes([]byte("abc"))
	if _, err := in.Read(make([]byte, 1)); err != nil {
		t.Fatal(err)
	}
	for _, err := range in.Lines(ctx) {
		wantCode(t, err, CodeStreamInUse)
	}
	// Read is one reader across its calls.
	rest, err := io.ReadAll(in)
	if err != nil || string(rest) != "bc" {
		t.Errorf("rest = %q, %v", rest, err)
	}
}

func TestOutputStreamToWriter(t *testing.T) {
	ctx := streamCtx(t)
	s := newSink()
	out := OutputStreamToWriter(s)
	if err := out.Write(ctx, []byte("hello ")); err != nil {
		t.Fatal(err)
	}
	if err := out.Send(ctx, map[string]string{"dropped": "yes"}); err != nil {
		t.Fatal(err)
	}
	if _, err := io.WriteString(out.Writer(ctx), "world"); err != nil {
		t.Fatal(err)
	}
	if err := out.Close(); err != nil {
		t.Fatal(err)
	}
	if s.String() != "hello world" || !s.flushed || !s.isClosed() {
		t.Errorf("sink = %q flushed %v closed %v", s.String(), s.flushed, s.isClosed())
	}
	wantCode(t, out.Write(ctx, []byte("late")), CodeStreamClosed)
	if err := out.Abort("too late"); err != nil {
		t.Errorf("Abort after Close = %v", err)
	}

	// An abort closes an *io.PipeWriter with the error; another writer is left as is.
	pr, pw := io.Pipe()
	out = OutputStreamToWriter(pw)
	go func() {
		_ = out.Write(ctx, []byte("part"))
		_ = out.Abort("disk full")
	}()
	got, err := io.ReadAll(pr)
	wantCode(t, err, CodeStreamAborted)
	if string(got) != "part" || !strings.Contains(err.Error(), "disk full") {
		t.Errorf("pipe = %q, %v", got, err)
	}
	s = newSink()
	if err := OutputStreamToWriter(s).Abort("x"); err != nil || s.isClosed() {
		t.Errorf("abort closed a plain writer: %v", err)
	}
}

// An abort of a local sink does not wait behind a write blocked on it (an io.Pipe
// nobody reads): the call's timeout aborts its sinks, and must not hang there.
func TestLocalSinkAbortEndsABlockedWrite(t *testing.T) {
	_, pw := io.Pipe()
	out := OutputStreamToWriter(pw)
	wrote := make(chan error, 1)
	go func() { wrote <- out.Write(context.Background(), []byte("nobody reads this")) }()
	time.Sleep(20 * time.Millisecond)
	aborted := make(chan struct{})
	go func() {
		_ = out.Abort("the call timed out")
		close(aborted)
	}()
	select {
	case <-aborted:
	case <-time.After(2 * time.Second):
		t.Fatal("Abort waited behind the blocked write")
	}
	select {
	case err := <-wrote:
		if err == nil {
			t.Error("the blocked write succeeded after the abort")
		}
	case <-time.After(2 * time.Second):
		t.Fatal("the blocked write did not end with the abort")
	}
}

func TestStreamPlaceholders(t *testing.T) {
	ctx := streamCtx(t)
	in := InputStreamFromBytes([]byte("data"))
	first := jsonOf(t, in)
	if again := jsonOf(t, in); !bytes.Equal(first, again) {
		t.Errorf("a stream marshals to one placeholder: %s then %s", first, again)
	}
	var ref StreamChannelRef
	if err := json.Unmarshal(first, &ref); err != nil || !strings.HasPrefix(ref.ChannelID, "local:") || ref.AccessKey != "" || ref.Direction != ChannelRead {
		t.Fatalf("placeholder = %s", first)
	}
	var back InputStream
	if err := json.Unmarshal(first, &back); err != nil {
		t.Fatal(err)
	}
	if back.inner != in.inner {
		t.Error("the placeholder did not unmarshal to the stream itself")
	}
	if text, err := back.Text(ctx); err != nil || text != "data" {
		t.Errorf("Text = %q, %v", text, err)
	}
	var twice InputStream
	if err := json.Unmarshal(first, &twice); err == nil || !strings.Contains(err.Error(), "no longer available") {
		t.Errorf("second unmarshal = %v, want gone", err)
	}

	var wrong OutputStream
	if err := json.Unmarshal([]byte(`{"channel_id":"c","access_key":"k","direction":"read"}`), &wrong); err == nil {
		t.Error("a read ref unmarshalled to an OutputStream")
	}
	var none struct {
		Body *InputStream `json:"body"`
	}
	if err := json.Unmarshal([]byte(`{"body":null}`), &none); err != nil || none.Body != nil {
		t.Errorf("null = %v, %v", none.Body, err)
	}
	if err := json.Unmarshal([]byte(`{"body":{"channel_id":"c"}}`), &none); err == nil {
		t.Error("a partial ref unmarshalled")
	}
}

func TestStreamSchema(t *testing.T) {
	schema, err := InferSchema[struct {
		Body *InputStream `json:"body"`
		Out  OutputStream `json:"out"`
	}]()
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{`"x-iii-stream":"read"`, `"x-iii-stream":"write"`, `"channel_id"`} {
		if !bytes.Contains(schema, []byte(want)) {
			t.Errorf("schema %s lacks %s", schema, want)
		}
	}
}

func TestUnboundStreamAsksForBind(t *testing.T) {
	a, b := New("ws://127.0.0.1:1"), New("ws://127.0.0.1:1")
	defer a.Close()
	defer b.Close()
	var in InputStream
	if err := json.Unmarshal([]byte(`{"channel_id":"c","access_key":"k","direction":"read"}`), &in); err != nil {
		t.Fatal(err)
	}
	if _, err := in.ReadAll(streamCtx(t)); err == nil || !strings.Contains(err.Error(), "Bind") {
		t.Errorf("read of an unbound stream = %v, want a request to Bind", err)
	}
}

// =====================================================================================
// What every call checks first
// =====================================================================================

func TestCallRefusesStreamsItCannotSend(t *testing.T) {
	se := newStreamEngine(t)
	c := se.client()
	ctx := streamCtx(t)
	mark := se.mark()

	// R9: no stream in an enqueued call, a local one or a raw ref.
	_, err := c.Call(ctx, CallRequest{FunctionID: "jobs::run", Action: EnqueueAction("q"),
		Payload: jsonOf(t, map[string]any{"body": InputStreamFromBytes([]byte("x"))})})
	wantCode(t, err, CodeStreamNotAllowed)
	_, err = c.Call(ctx, CallRequest{FunctionID: "jobs::run", Action: EnqueueAction("q"),
		Payload: json.RawMessage(`{"body":{"channel_id":"c","access_key":"k","direction":"read"}}`)})
	wantCode(t, err, CodeStreamNotAllowed)

	// One stream twice in a payload.
	in := InputStreamFromBytes([]byte("x"))
	_, err = c.Call(ctx, CallRequest{FunctionID: "jobs::run", Payload: jsonOf(t, []any{in, in})})
	wantCode(t, err, CodeStreamInUse)

	// A stream already read to the end.
	done := InputStreamFromBytes([]byte("x"))
	if _, err := done.ReadAll(ctx); err != nil {
		t.Fatal(err)
	}
	_, err = c.Call(ctx, CallRequest{FunctionID: "jobs::run", Payload: jsonOf(t, map[string]any{"body": done})})
	wantCode(t, err, CodeStreamInUse)

	// One raw ref twice.
	ref := `{"channel_id":"c","access_key":"k","direction":"read"}`
	_, err = c.Call(ctx, CallRequest{FunctionID: "jobs::run", Payload: json.RawMessage(`[` + ref + `,` + ref + `]`)})
	wantCode(t, err, CodeStreamInUse)

	// No stream in metadata.
	_, err = c.Call(ctx, CallRequest{FunctionID: "jobs::run", Payload: json.RawMessage(`{}`),
		Metadata: jsonOf(t, map[string]any{"s": InputStreamFromBytes(nil)})})
	if !errors.Is(err, errStreamInMetadata) {
		t.Errorf("metadata stream = %v", err)
	}
	time.Sleep(20 * time.Millisecond)
	if n := se.countFrames(mark); n != 0 {
		var types []string
		for _, raw := range se.m.receivedMessages()[mark:] {
			types = append(types, messageType(raw)+" "+stringField(raw, "function_id"))
		}
		t.Errorf("%d frames sent for refused calls: %v", n, types)
	}
}

// =====================================================================================
// In-process: the handler gets the caller's streams themselves, with no pipe
// =====================================================================================

type copyArgs struct {
	Src *InputStream  `json:"src"`
	Dst *OutputStream `json:"dst"`
}

func TestLocalSourceAndSink(t *testing.T) {
	se := newStreamEngine(t)
	c := se.client()
	registerLocal(t, c, "files::upper", func(ctx context.Context, data json.RawMessage) (any, error) {
		var args copyArgs
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		body, err := args.Src.ReadAll(ctx)
		if err != nil {
			return nil, err
		}
		if err := args.Dst.Send(ctx, "a message a byte sink drops"); err != nil {
			return nil, err
		}
		return len(body), args.Dst.Write(ctx, bytes.ToUpper(body))
	})
	src := &trackedReader{Reader: strings.NewReader("hello")}
	dst := newSink()
	out := call(t, c, CallRequest{FunctionID: "files::upper", Payload: jsonOf(t, copyArgs{InputStreamFromReader(src), OutputStreamToWriter(dst)})})
	if string(out) != "5" || dst.String() != "HELLO" || !dst.isClosed() || !src.closed.Load() {
		t.Errorf("result %s, sink %q closed %v, source closed %v", out, dst.String(), dst.isClosed(), src.closed.Load())
	}
	if n := invokeFrames(se.fakeEngine, FnCreateChannel); n != 0 {
		t.Errorf("%d pipes created for an in-process call", n)
	}
}

// R2 and R4 in-process: a used sink is closed on success and aborted on failure; one
// untouched is closed on success; one aborted fails the call even when the handler
// succeeded.
func TestLocalSinkOutcomes(t *testing.T) {
	se := newStreamEngine(t)
	c := se.client()
	registerLocal(t, c, "sink::do", func(ctx context.Context, data json.RawMessage) (any, error) {
		var args struct {
			Do  string        `json:"do"`
			Dst *OutputStream `json:"dst"`
		}
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		switch args.Do {
		case "write-fail":
			_ = args.Dst.Write(ctx, []byte("partial"))
			return nil, errors.New("boom")
		case "abort":
			_ = args.Dst.Write(ctx, []byte("partial"))
			return "fine", args.Dst.Abort("disk full")
		case "untouched-fail":
			return nil, errors.New("boom")
		}
		return "fine", nil
	})
	run := func(do string) (string, error, error) {
		pr, pw := io.Pipe()
		got := make(chan error, 1)
		var body []byte
		go func() {
			var err error
			body, err = io.ReadAll(pr)
			got <- err
		}()
		_, err := c.Call(streamCtx(t), CallRequest{FunctionID: "sink::do",
			Payload: jsonOf(t, map[string]any{"do": do, "dst": OutputStreamToWriter(pw)})})
		readErr := <-got
		return string(body), err, readErr
	}

	if body, err, readErr := run("untouched"); err != nil || readErr != nil || body != "" {
		t.Errorf("untouched: %q, %v, %v", body, err, readErr)
	}
	body, err, readErr := run("write-fail")
	if !strings.Contains(fmtErr(err), "boom") || streamCode(readErr) != CodeStreamAborted || body != "partial" {
		t.Errorf("write-fail: %q, %v, %v", body, err, readErr)
	}
	body, err, readErr = run("abort")
	wantCode(t, err, CodeStreamAborted)
	if !strings.Contains(err.Error(), "disk full") || streamCode(readErr) != CodeStreamAborted || body != "partial" {
		t.Errorf("abort: %q, %v, %v", body, err, readErr)
	}
	// Untouched on failure: the guard leaves it, and the call aborts it with its error.
	if _, err, readErr := run("untouched-fail"); !strings.Contains(fmtErr(err), "boom") || streamCode(readErr) != CodeStreamAborted {
		t.Errorf("untouched-fail: %v, %v", err, readErr)
	}
}

func fmtErr(err error) string {
	if err == nil {
		return "<nil>"
	}
	return err.Error()
}

// R3 in-process: a stream the handler started reading and did not finish is cancelled
// when it returns; one it never read is left alone.
func TestLocalPartlyReadInputIsCancelled(t *testing.T) {
	se := newStreamEngine(t)
	c := se.client()
	registerLocal(t, c, "src::peek", func(ctx context.Context, data json.RawMessage) (any, error) {
		var args struct {
			Read bool         `json:"read"`
			Src  *InputStream `json:"src"`
		}
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		if args.Read {
			_, err := args.Src.Read(make([]byte, 10))
			return nil, err
		}
		return nil, nil
	})
	big := bytes.Repeat([]byte("x"), 3*channelFrameSize)
	peeked := &trackedReader{Reader: bytes.NewReader(big)}
	call(t, c, CallRequest{FunctionID: "src::peek", Payload: jsonOf(t, map[string]any{"read": true, "src": InputStreamFromReader(peeked)})})
	waitUntil(t, "the partly read source is cancelled", peeked.closed.Load)

	unread := &trackedReader{Reader: bytes.NewReader(big)}
	call(t, c, CallRequest{FunctionID: "src::peek", Payload: jsonOf(t, map[string]any{"read": false, "src": InputStreamFromReader(unread)})})
	time.Sleep(30 * time.Millisecond)
	if unread.closed.Load() {
		t.Error("an unread source was cancelled")
	}
}

// R4: the call's timeout bounds its wait for its sinks; the sinks still open are
// aborted before the call fails.
func TestLocalCallTimeoutAbortsOpenSinks(t *testing.T) {
	se := newStreamEngine(t)
	c := se.client()
	registerLocal(t, c, "sink::hold", func(ctx context.Context, data json.RawMessage) (any, error) {
		var args struct {
			Dst *OutputStream `json:"dst"`
		}
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		args.Dst.KeepOpen()
		return nil, args.Dst.Write(ctx, []byte("started"))
	})
	pr, pw := io.Pipe()
	got := make(chan error, 1)
	go func() {
		_, err := io.ReadAll(pr)
		got <- err
	}()
	start := time.Now()
	dst := OutputStreamToWriter(pw)
	_, err := c.Call(streamCtx(t), CallRequest{FunctionID: "sink::hold", Timeout: 200 * time.Millisecond,
		Payload: jsonOf(t, map[string]any{"dst": dst})})
	if !errors.Is(err, ErrTimeout) || time.Since(start) > 2*time.Second {
		t.Fatalf("err = %v after %v, want ErrTimeout", err, time.Since(start))
	}
	if _, aborted, reason := dst.inner.outcome(); !aborted || reason != sinkTimeout {
		t.Errorf("sink outcome when the call failed: aborted %v (%q), want aborted by the timeout", aborted, reason)
	}
	if readErr := <-got; streamCode(readErr) != CodeStreamAborted || !strings.Contains(readErr.Error(), sinkTimeout) {
		t.Errorf("sink = %v, want aborted by the timeout", readErr)
	}
}

// R5 in-process: a received stream passed on unread is the next handler's to read.
func TestLocalForwardingUnread(t *testing.T) {
	se := newStreamEngine(t)
	c := se.client()
	registerLocal(t, c, "fwd::read", func(ctx context.Context, data json.RawMessage) (any, error) {
		var args struct {
			Src *InputStream `json:"src"`
		}
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		return args.Src.Text(ctx)
	})
	registerLocal(t, c, "fwd::pass", func(ctx context.Context, data json.RawMessage) (any, error) {
		var args struct {
			Src *InputStream `json:"src"`
		}
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		if _, err := args.Src.Read(make([]byte, 2)); err != nil { // partly read, then sent on
			return nil, err
		}
		return c.Call(ctx, CallRequest{FunctionID: "fwd::read", Payload: jsonOf(t, map[string]any{"src": args.Src})})
	})
	out := call(t, c, CallRequest{FunctionID: "fwd::pass", Payload: jsonOf(t, map[string]any{"src": InputStreamFromBytes([]byte("hello"))})})
	if string(out) != `"llo"` {
		t.Errorf("result %s, want the rest of the stream", out)
	}
	// Raw forwarding: the handler passes its input on as JSON.
	registerLocal(t, c, "fwd::raw", func(ctx context.Context, data json.RawMessage) (any, error) {
		return c.Call(ctx, CallRequest{FunctionID: "fwd::read", Payload: data})
	})
	out = call(t, c, CallRequest{FunctionID: "fwd::raw", Payload: jsonOf(t, map[string]any{"src": InputStreamFromBytes([]byte("raw"))})})
	if string(out) != `"raw"` {
		t.Errorf("raw forwarding result %s", out)
	}
}

// A local call's report carries its streams as StreamInfo: a placeholder never
// reaches the wire.
func TestLocalReportCarriesStreamInfo(t *testing.T) {
	se := newStreamEngine(t)
	c := se.client()
	registerLocal(t, c, "obs::streamed", func(ctx context.Context, data json.RawMessage) (any, error) {
		return "ok", nil
	})
	addObserver(t, se.fakeEngine, "obs::streamed", observerInfo("o1", "x::y", "", "", CaptureInput))
	call(t, c, CallRequest{FunctionID: "obs::streamed", Payload: jsonOf(t, map[string]any{"body": InputStreamFromBytes([]byte("x"))})})
	it := se.waitItems(1)[0]
	var input struct {
		Body StreamInfo `json:"body"`
	}
	if err := json.Unmarshal(it.Input, &input); err != nil || !input.Body.Stream || input.Body.Direction != ChannelRead || !strings.HasPrefix(input.Body.ID, "local:") {
		t.Errorf("reported input %s", it.Input)
	}
	for _, raw := range se.m.receivedMessages() {
		if data, _ := json.Marshal(raw); bytes.Contains(data, []byte(`"access_key":""`)) {
			t.Errorf("a placeholder reached the wire: %s", data)
		}
	}
}

// =====================================================================================
// Through pipes
// =====================================================================================

// remote is a DispatchRemote call: the stream engine routes it back to this worker.
func remote(fn string, payload json.RawMessage) CallRequest {
	return CallRequest{FunctionID: fn, Payload: payload, Dispatch: DispatchRemote}
}

func TestRemoteSourceSinkAndResult(t *testing.T) {
	se := newStreamEngine(t)
	c := se.client()
	registerLocal(t, c, "files::convert", func(ctx context.Context, data json.RawMessage) (any, error) {
		var args copyArgs
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		body, err := args.Src.ReadAll(ctx)
		if err != nil {
			return nil, err
		}
		if err := args.Dst.Write(ctx, bytes.ToUpper(body)); err != nil {
			return nil, err
		}
		return map[string]any{"size": len(body), "copy": InputStreamFromBytes(body)}, nil
	})
	big := bytes.Repeat([]byte("abc"), 100_000)
	dst := newSink()
	ctx := streamCtx(t)
	out, err := c.Call(ctx, remote("files::convert", jsonOf(t, copyArgs{InputStreamFromBytes(big), OutputStreamToWriter(dst)})))
	if err != nil {
		t.Fatal(err)
	}
	// R4: the call returned once the sink got the whole stream.
	if !dst.isClosed() || dst.String() != strings.ToUpper(string(big)) {
		t.Fatalf("sink closed %v, %d bytes", dst.isClosed(), len(dst.String()))
	}
	var res struct {
		Size int          `json:"size"`
		Copy *InputStream `json:"copy"`
	}
	if err := json.Unmarshal(out, &res); err != nil {
		t.Fatal(err)
	}
	if got, err := res.Copy.ReadAll(ctx); err != nil || !bytes.Equal(got, big) || res.Size != len(big) {
		t.Fatalf("result stream: %d bytes, %v", len(got), err)
	}
	if n := se.relay.created.Load(); n != 3 {
		t.Errorf("%d pipes, want 3 (source, sink, result)", n)
	}
	for _, d := range se.frames(MsgInvokeFunction) {
		if bytes.Contains(d.InvokeFunction.Data, []byte(placeholderPrefix)) {
			t.Errorf("a placeholder reached the wire: %s", d.InvokeFunction.Data)
		}
	}
}

// R5: a received stream sent on unread goes as its own ref (no new pipe, no byte through
// this worker); one sent on after a read is pumped from where it is.
func TestRemoteForwarding(t *testing.T) {
	se := newStreamEngine(t)
	c := se.client()
	var seenIDs sync.Map
	registerLocal(t, c, "fwd::read", func(ctx context.Context, data json.RawMessage) (any, error) {
		var raw struct {
			Src StreamChannelRef `json:"src"`
		}
		_ = json.Unmarshal(data, &raw)
		seenIDs.Store(raw.Src.ChannelID, true)
		var args struct {
			Src *InputStream `json:"src"`
		}
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		return args.Src.Text(ctx)
	})
	registerLocal(t, c, "fwd::pass", func(ctx context.Context, data json.RawMessage) (any, error) {
		var args struct {
			Peek bool         `json:"peek"`
			Src  *InputStream `json:"src"`
		}
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		if args.Peek {
			if _, err := args.Src.Read(make([]byte, 2)); err != nil {
				return nil, err
			}
		}
		return c.Call(ctx, remote("fwd::read", jsonOf(t, map[string]any{"src": args.Src})))
	})
	ctx := streamCtx(t)
	out, err := c.Call(ctx, remote("fwd::pass", jsonOf(t, map[string]any{"src": InputStreamFromBytes([]byte("hello"))})))
	if err != nil || string(out) != `"hello"` {
		t.Fatalf("unread forward = %s, %v", out, err)
	}
	if n := se.relay.created.Load(); n != 1 {
		t.Errorf("%d pipes for an unread forward, want 1", n)
	}
	out, err = c.Call(ctx, remote("fwd::pass", jsonOf(t, map[string]any{"peek": true, "src": InputStreamFromBytes([]byte("hello"))})))
	if err != nil || string(out) != `"llo"` {
		t.Fatalf("partly read forward = %s, %v", out, err)
	}
	if n := se.relay.created.Load(); n != 3 {
		t.Errorf("%d pipes after a partly read forward, want 3", n)
	}
}

// R6: one reader, a stream sent once, and no write after the end.
func TestRemoteStreamMisuse(t *testing.T) {
	se := newStreamEngine(t)
	c := se.client()
	registerLocal(t, c, "use::sink", func(ctx context.Context, data json.RawMessage) (any, error) { return nil, nil })
	registerLocal(t, c, "use::misuse", func(ctx context.Context, data json.RawMessage) (any, error) {
		var args struct {
			Do  string        `json:"do"`
			Src *InputStream  `json:"src"`
			Dst *OutputStream `json:"dst"`
		}
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		switch args.Do {
		case "twice":
			_, err := c.Call(ctx, remote("use::sink", jsonOf(t, []any{args.Src, args.Src})))
			return nil, err
		case "read-then-send":
			if _, err := args.Src.ReadAll(ctx); err != nil {
				return nil, err
			}
			_, err := c.Call(ctx, remote("use::sink", jsonOf(t, map[string]any{"src": args.Src})))
			return nil, err
		case "second-reader":
			if _, err := args.Src.Read(make([]byte, 1)); err != nil {
				return nil, err
			}
			_, err := args.Src.ReadAll(ctx)
			return nil, err
		case "write-after-close":
			if err := args.Dst.Close(); err != nil {
				return nil, err
			}
			return nil, args.Dst.Write(ctx, []byte("late"))
		}
		return nil, nil
	})
	for do, code := range map[string]string{
		"twice": CodeStreamInUse, "read-then-send": CodeStreamInUse,
		"second-reader": CodeStreamInUse, "write-after-close": CodeStreamClosed,
	} {
		_, err := c.Call(streamCtx(t), remote("use::misuse", jsonOf(t, map[string]any{
			"do": do, "src": InputStreamFromBytes([]byte("abc")), "dst": OutputStreamToWriter(newSink()),
		})))
		if streamCode(err) != code {
			t.Errorf("%s: %v, want %s", do, err, code)
		}
	}
}

// An abort reaches the other side: a source that fails aborts the reader's read, and a
// sink the handler aborts fails the call (R4) with STREAM_ABORTED.
func TestRemoteAbortPropagates(t *testing.T) {
	se := newStreamEngine(t)
	c := se.client()
	registerLocal(t, c, "abort::read", func(ctx context.Context, data json.RawMessage) (any, error) {
		var args copyArgs
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		if args.Dst != nil {
			_ = args.Dst.Write(ctx, []byte("partial"))
			return "ok", args.Dst.Abort("converter crashed")
		}
		got, err := args.Src.ReadAll(ctx)
		return string(got), err
	})
	ctx := streamCtx(t)
	_, err := c.Call(ctx, remote("abort::read", jsonOf(t, map[string]any{
		"src": InputStreamFromReader(&failingReader{data: []byte("part"), err: errors.New("disk gone")}),
	})))
	wantCode(t, err, CodeStreamAborted)

	pr, pw := io.Pipe()
	got := make(chan error, 1)
	go func() {
		_, err := io.ReadAll(pr)
		got <- err
	}()
	_, err = c.Call(ctx, remote("abort::read", jsonOf(t, map[string]any{"dst": OutputStreamToWriter(pw)})))
	wantCode(t, err, CodeStreamAborted)
	wantCode(t, <-got, CodeStreamAborted)
}

// R2: a handler that wrote to a stream it received and failed aborts it, so the
// stream's reader never gets a short stream that looks whole. The reader here is the
// one that made the pipe, as the HTTP worker makes a response: no failed call of its
// own aborts the stream for it (a caller's sinks are aborted by its call as well).
func TestFailedHandlerAbortsTheStreamItWrote(t *testing.T) {
	se := newStreamEngine(t)
	c := se.client()
	registerLocal(t, c, "sink::fail", func(ctx context.Context, data json.RawMessage) (any, error) {
		var args copyArgs
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		if err := args.Dst.Write(ctx, []byte("partial")); err != nil {
			return nil, err
		}
		return nil, errors.New("boom")
	})
	writer, reader := se.relay.create()
	res := se.waitResult(se.invoke(InvokeFunctionMessage{FunctionID: "sink::fail", Data: jsonOf(t, map[string]any{"dst": writer})}))
	if res.Error == nil {
		t.Fatal("the handler's error was not returned")
	}
	body := (&InputStream{inner: newInput(&reader, nil, nil)}).Bind(c)
	got, err := body.ReadAll(streamCtx(t))
	wantCode(t, err, CodeStreamAborted)
	if string(got) != "partial" {
		t.Errorf("read %q before the abort", got)
	}
}

// R4 through pipes: the call's timeout bounds the wait for its sinks.
func TestRemoteCallTimeoutAbortsOpenSinks(t *testing.T) {
	se := newStreamEngine(t)
	c := se.client()
	registerLocal(t, c, "sink::hold", func(ctx context.Context, data json.RawMessage) (any, error) {
		var args copyArgs
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		args.Dst.KeepOpen()
		return nil, args.Dst.Write(ctx, []byte("started"))
	})
	pr, pw := io.Pipe()
	got := make(chan error, 1)
	go func() {
		_, err := io.ReadAll(pr)
		got <- err
	}()
	dst := OutputStreamToWriter(pw)
	req := remote("sink::hold", jsonOf(t, map[string]any{"dst": dst}))
	req.Timeout = 300 * time.Millisecond
	_, err := c.Call(streamCtx(t), req)
	if !errors.Is(err, ErrTimeout) {
		t.Fatalf("err = %v, want ErrTimeout", err)
	}
	if _, aborted, reason := dst.inner.outcome(); !aborted || reason != sinkTimeout {
		t.Errorf("sink outcome when the call failed: aborted %v (%q), want aborted by the timeout", aborted, reason)
	}
	wantCode(t, <-got, CodeStreamAborted)
}

// A call whose result holds a stream to write returns without waiting for its sinks:
// the caller writes the result's stream, and the function copies it into the sink.
func TestRemoteWriteSideInResultDoesNotWait(t *testing.T) {
	se := newStreamEngine(t)
	c := se.client()
	registerLocal(t, c, "upload::begin", func(ctx context.Context, data json.RawMessage) (any, error) {
		var args copyArgs
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		in, out, err := c.Pipe(ctx)
		if err != nil {
			return nil, err
		}
		args.Dst.KeepOpen()
		go func() {
			body, err := in.ReadAll(context.Background())
			if err != nil {
				_ = args.Dst.Abort(err.Error())
				return
			}
			_ = args.Dst.Write(context.Background(), body)
			_ = args.Dst.Close()
		}()
		return map[string]any{"upload": out}, nil
	})
	dst := newSink()
	ctx := streamCtx(t)
	out, err := c.Call(ctx, remote("upload::begin", jsonOf(t, copyArgs{Dst: OutputStreamToWriter(dst)})))
	if err != nil {
		t.Fatal(err)
	}
	var res struct {
		Upload *OutputStream `json:"upload"`
	}
	if err := json.Unmarshal(out, &res); err != nil {
		t.Fatal(err)
	}
	if err := res.Upload.Write(ctx, []byte("uploaded")); err != nil {
		t.Fatal(err)
	}
	if err := res.Upload.Close(); err != nil {
		t.Fatal(err)
	}
	select {
	case <-dst.closed:
	case <-ctx.Done():
		t.Fatal("the sink never ended")
	}
	if dst.String() != "uploaded" {
		t.Errorf("sink = %q", dst.String())
	}
}

// R2 for an observer activation: an untouched response is closed only when the result
// is null; in a plain call it is closed whatever the result.
func TestGuardOfAnActivation(t *testing.T) {
	se := newStreamEngine(t)
	c := se.client()
	registerLocal(t, c, "http::route", func(ctx context.Context, data json.RawMessage) (any, error) {
		var args struct {
			Result json.RawMessage `json:"result"`
		}
		_ = json.Unmarshal(data, &args)
		return args.Result, nil
	})
	ctx := streamCtx(t)
	for _, tc := range []struct {
		name       string
		activation bool
		result     string
		closed     bool
	}{
		{"activation, null result", true, "null", true},
		{"activation, buffered result", true, `{"status_code":200}`, false},
		{"plain call, a result", false, `{"status_code":200}`, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			writer, reader := se.relay.create()
			msg := InvokeFunctionMessage{FunctionID: "http::route", Data: jsonOf(t, map[string]any{"result": json.RawMessage(tc.result), "response": writer})}
			if tc.activation {
				msg.Activation = &Activation{ObserverID: "o1", ActivationID: "a1"}
			}
			res := se.waitResult(se.invoke(msg))
			if res.Error != nil {
				t.Fatalf("error %+v", res.Error)
			}
			body := (&InputStream{inner: newInput(&reader, nil, nil)}).Bind(c)
			readCtx, cancel := context.WithTimeout(ctx, 300*time.Millisecond)
			defer cancel()
			_, err := body.ReadAll(readCtx)
			if closed := err == nil; closed != tc.closed {
				t.Errorf("response closed = %v (%v), want %v", closed, err, tc.closed)
			}
		})
	}
}

// An observer of an engine-built Observed envelope gets StreamInfo for the streams of
// the observed call, and no stream to read.
func TestObservedEnvelopeStreamsAreInfo(t *testing.T) {
	se := newStreamEngine(t)
	c := se.client()
	got := make(chan Observed, 1)
	registerLocal(t, c, "audit::seen", func(ctx context.Context, data json.RawMessage) (any, error) {
		var o Observed
		err := json.Unmarshal(data, &o)
		got <- o
		return nil, err
	})
	envelope := `{"activation_id":"act-9","observer_id":"o9","function_id":"files::convert","status":"success",` +
		`"input":{"src":{"channel_id":"ch-1","access_key":"secret","direction":"read"}},` +
		`"output":[{"channel_id":"ch-2","access_key":"secret","direction":"write"}]}`
	res := se.waitResult(se.invoke(InvokeFunctionMessage{FunctionID: "audit::seen", Data: json.RawMessage(envelope),
		Activation: &Activation{ObserverID: "o9", ActivationID: "act-9"}}))
	if res.Error != nil {
		t.Fatalf("error %+v", res.Error)
	}
	o := <-got
	var input struct {
		Src StreamInfo `json:"src"`
	}
	var output []StreamInfo
	if json.Unmarshal(o.Input, &input) != nil || json.Unmarshal(o.Output, &output) != nil {
		t.Fatalf("input %s output %s", o.Input, o.Output)
	}
	if input.Src != (StreamInfo{Stream: true, Direction: ChannelRead, ID: "ch-1"}) || len(output) != 1 ||
		output[0] != (StreamInfo{Stream: true, Direction: ChannelWrite, ID: "ch-2"}) {
		t.Errorf("input %+v output %+v", input, output)
	}
	if bytes.Contains(o.Input, []byte("secret")) {
		t.Error("the access key reached the handler")
	}
}

// Interop with SDKs before call-native streams: a raw ref in a payload is a stream, and
// a received stream reads like a raw channel. Here the caller makes the pipe itself and
// sends its raw reader ref.
func TestRawRefInterop(t *testing.T) {
	se := newStreamEngine(t)
	c := se.client()
	registerLocal(t, c, "raw::read", func(ctx context.Context, data json.RawMessage) (any, error) {
		var args struct {
			Body *InputStream `json:"body"`
		}
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		return args.Body.Text(ctx)
	})
	ctx := streamCtx(t)
	writer, reader := se.relay.create()
	out := (&OutputStream{inner: newOutput(&writer, nil, nil)}).Bind(c)
	go func() {
		_ = out.Write(context.Background(), []byte("from a 0.26 writer"))
		_ = out.Close()
	}()
	res, err := c.Call(ctx, remote("raw::read", jsonOf(t, map[string]any{"body": reader})))
	if err != nil || string(res) != `"from a 0.26 writer"` {
		t.Fatalf("result %s, %v", res, err)
	}
}

// =====================================================================================
// Cancels, failures and blocked ends
// =====================================================================================

// A stream cancelled before its end fails a read in flight and every later read with
// STREAM_CLOSED, never io.EOF: what was read is not the whole stream. One read to its
// end keeps returning io.EOF.
func TestCancelledStreamNeverLooksComplete(t *testing.T) {
	ctx := streamCtx(t)
	cancelled := func(t *testing.T, what string, err error) {
		t.Helper()
		if streamCode(err) != CodeStreamClosed || !strings.Contains(err.Error(), cancelledMessage) {
			t.Errorf("%s = %v, want STREAM_CLOSED (%s)", what, err, cancelledMessage)
		}
	}
	laterReads := func(t *testing.T, s *InputStream) {
		t.Helper()
		_, err := s.Read(make([]byte, 4))
		cancelled(t, "Read", err)
		_, err = s.ReadAll(ctx)
		cancelled(t, "ReadAll", err)
		_, err = s.Text(ctx)
		cancelled(t, "Text", err)
		for _, err := range s.Lines(ctx) {
			cancelled(t, "Lines", err)
		}
		for _, err := range s.Messages(ctx) {
			cancelled(t, "Messages", err)
		}
		for _, err := range s.Frames(ctx) {
			cancelled(t, "Frames", err)
		}
	}

	t.Run("read in flight", func(t *testing.T) {
		pr, pw := io.Pipe()
		defer pw.Close()
		s := InputStreamFromReader(pr)
		got := make(chan error, 1)
		go func() {
			_, err := s.ReadAll(context.Background())
			got <- err
		}()
		if _, err := pw.Write([]byte("partial")); err != nil {
			t.Fatal(err)
		}
		s.Cancel()
		select {
		case err := <-got:
			cancelled(t, "the read in flight", err)
		case <-time.After(2 * time.Second):
			t.Fatal("the read in flight did not end with the cancel")
		}
		laterReads(t, s)
	})
	t.Run("break out of Lines", func(t *testing.T) {
		s := InputStreamFromBytes([]byte("one\ntwo\nthree\n"))
		for range s.Lines(ctx) {
			break
		}
		laterReads(t, s)
	})
	t.Run("Close before the end", func(t *testing.T) {
		s := InputStreamFromBytes(bytes.Repeat([]byte("x"), 3*channelFrameSize))
		if _, err := s.Read(make([]byte, 4)); err != nil {
			t.Fatal(err)
		}
		_ = s.Close()
		laterReads(t, s)
	})
	t.Run("read to the end", func(t *testing.T) {
		s := InputStreamFromBytes([]byte("whole"))
		if data, err := io.ReadAll(s); err != nil || string(data) != "whole" {
			t.Fatalf("ReadAll = %q, %v", data, err)
		}
		s.Cancel()
		if n, err := s.Read(make([]byte, 4)); n != 0 || err != io.EOF {
			t.Errorf("Read of a stream read to the end, after Cancel = (%d, %v), want (0, EOF)", n, err)
		}
	})
}

// dispatches are the two paths of a call that carries streams.
var dispatches = []struct {
	name     string
	dispatch Dispatch
}{{"in-process", ""}, {"remote", DispatchRemote}}

// R3: the guard cancels a stream its handler was still reading when it returned, so a
// read left running (on a goroutine) fails with STREAM_CLOSED, not a clean end.
func TestGuardCancelFailsAReadInFlight(t *testing.T) {
	for _, d := range dispatches {
		t.Run(d.name, func(t *testing.T) {
			se := newStreamEngine(t)
			c := se.client()
			got := make(chan error, 1)
			registerLocal(t, c, "src::background", func(ctx context.Context, data json.RawMessage) (any, error) {
				var args copyArgs
				if err := json.Unmarshal(data, &args); err != nil {
					return nil, err
				}
				if _, err := args.Src.Read(make([]byte, 4)); err != nil {
					return nil, err
				}
				go func() {
					_, err := io.ReadAll(args.Src)
					got <- err
				}()
				time.Sleep(50 * time.Millisecond) // the read is in flight
				return nil, nil
			})
			pr, pw := io.Pipe()
			t.Cleanup(func() { _ = pw.Close() })
			go func() { _, _ = pw.Write([]byte("head")) }() // the rest never comes
			call(t, c, CallRequest{FunctionID: "src::background", Dispatch: d.dispatch,
				Payload: jsonOf(t, map[string]any{"src": InputStreamFromReader(pr)})})
			select {
			case err := <-got:
				if streamCode(err) != CodeStreamClosed || !strings.Contains(err.Error(), guardCancelledMessage) {
					t.Errorf("read left running = %v, want STREAM_CLOSED (%s)", err, guardCancelledMessage)
				}
			case <-time.After(3 * time.Second):
				t.Fatal("the read left running did not end when its handler returned")
			}
		})
	}
}

// stuckWriter is a sink whose Write blocks until release is closed, and that has no
// CloseWithError: an abort cannot end the Write.
type stuckWriter struct {
	entered chan struct{}
	once    sync.Once
	release chan struct{}
}

func newStuckWriter(t *testing.T) *stuckWriter {
	w := &stuckWriter{entered: make(chan struct{}), release: make(chan struct{})}
	t.Cleanup(func() { close(w.release) })
	return w
}

func (w *stuckWriter) Write(p []byte) (int, error) {
	w.once.Do(func() { close(w.entered) })
	<-w.release
	return len(p), nil
}

// R4: a sink whose Write blocks does not hold a remote call past its timeout, its ctx or
// its error: the call returns, and the sink is aborted.
func TestRemoteCallDoesNotWaitBehindABlockedSink(t *testing.T) {
	for _, tt := range []struct {
		name    string
		fail    bool
		timeout time.Duration
		ctx     time.Duration
		want    func(error) bool
	}{
		{name: "timeout", timeout: 300 * time.Millisecond, want: func(err error) bool { return errors.Is(err, ErrTimeout) }},
		{name: "ctx", ctx: 300 * time.Millisecond, want: func(err error) bool { return errors.Is(err, context.DeadlineExceeded) }},
		{name: "error", fail: true, want: func(err error) bool {
			var ie *InvocationError
			return errors.As(err, &ie) && strings.Contains(ie.Message, "handler failed")
		}},
	} {
		t.Run(tt.name, func(t *testing.T) {
			se := newStreamEngine(t)
			c := se.client()
			w := newStuckWriter(t)
			registerLocal(t, c, "sink::stuck", func(ctx context.Context, data json.RawMessage) (any, error) {
				var args copyArgs
				if err := json.Unmarshal(data, &args); err != nil {
					return nil, err
				}
				args.Dst.KeepOpen()
				if err := args.Dst.Write(ctx, []byte("started")); err != nil {
					return nil, err
				}
				select { // the caller's pump is blocked in the sink's Write
				case <-w.entered:
				case <-time.After(2 * time.Second):
					return nil, errors.New("the sink was never written")
				}
				if tt.fail {
					return nil, errors.New("handler failed")
				}
				return nil, nil
			})
			ctx := streamCtx(t)
			if tt.ctx > 0 {
				var cancel context.CancelFunc
				ctx, cancel = context.WithTimeout(ctx, tt.ctx)
				defer cancel()
			}
			dst := OutputStreamToWriter(w)
			req := remote("sink::stuck", jsonOf(t, map[string]any{"dst": dst}))
			req.Timeout = tt.timeout
			got := make(chan error, 1)
			go func() {
				_, err := c.Call(ctx, req)
				got <- err
			}()
			select {
			case err := <-got:
				if !tt.want(err) {
					t.Errorf("err = %v, want the call's %s", err, tt.name)
				}
			case <-time.After(3 * time.Second):
				t.Fatal("the call waited behind a sink whose Write blocks")
			}
			if _, aborted, _ := dst.inner.outcome(); !aborted {
				t.Error("the sink was not aborted when the call failed")
			}
			if err := dst.Write(context.Background(), []byte("late")); streamCode(err) != CodeStreamClosed {
				t.Errorf("write to the sink after the call failed = %v, want STREAM_CLOSED", err)
			}
		})
	}
}

// An abort of a pipe end does not wait behind a write blocked by backpressure (a reader
// that does not read): Abort returns at once, and the write returns STREAM_CLOSED.
func TestOutputStreamAbortDoesNotWaitBehindABlockedWrite(t *testing.T) {
	quit := make(chan struct{})
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, err := websocket.Accept(w, r, nil)
		if err != nil {
			return
		}
		defer conn.CloseNow()
		<-quit // never reads
	}))
	defer srv.Close()
	defer close(quit)

	out := writeStreamAt(wsBase(srv), StreamChannelRef{ChannelID: "c", AccessKey: "k", Direction: ChannelWrite})
	var written atomic.Int64
	wrote := make(chan error, 1)
	go func() {
		chunk := make([]byte, 64<<10)
		for {
			if err := out.Write(context.Background(), chunk); err != nil {
				wrote <- err
				return
			}
			written.Add(1)
		}
	}()
	// A write is blocked once no write ended for a while.
	deadline := time.Now().Add(10 * time.Second)
	for last := int64(-1); ; {
		time.Sleep(300 * time.Millisecond)
		n := written.Load()
		if n == last {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("the writes never blocked")
		}
		last = n
	}
	aborted := make(chan struct{})
	go func() {
		_ = out.Abort("stop")
		close(aborted)
	}()
	select {
	case <-aborted:
	case <-time.After(3 * time.Second):
		t.Fatal("Abort waited behind a write blocked by backpressure")
	}
	select {
	case err := <-wrote:
		wantCode(t, err, CodeStreamClosed)
	case <-time.After(3 * time.Second):
		t.Fatal("the blocked write did not end with the abort")
	}
}

// A failed in-process call (its timeout, its ctx, a handler error) aborts the sources it
// passed, as a remote one stops their pumps: the handler's read in flight fails with
// STREAM_ABORTED, and the caller's reader is closed.
func TestLocalFailedCallAbortsItsSources(t *testing.T) {
	se := newStreamEngine(t)
	c := se.client()
	reads := make(chan error, 1)
	registerLocal(t, c, "src::stall", func(ctx context.Context, data json.RawMessage) (any, error) {
		var args copyArgs
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		_, err := args.Src.ReadAll(context.Background())
		reads <- err
		return nil, err
	})
	registerLocal(t, c, "src::fail", func(ctx context.Context, data json.RawMessage) (any, error) {
		return nil, errors.New("handler failed")
	})
	for _, tt := range []struct {
		name    string
		timeout time.Duration
		ctx     time.Duration
		want    error
	}{
		{name: "timeout", timeout: 200 * time.Millisecond, want: ErrTimeout},
		{name: "ctx", ctx: 200 * time.Millisecond, want: context.DeadlineExceeded},
	} {
		t.Run(tt.name, func(t *testing.T) {
			pr, pw := io.Pipe() // nothing is ever written
			defer pw.Close()
			ctx := streamCtx(t)
			if tt.ctx > 0 {
				var cancel context.CancelFunc
				ctx, cancel = context.WithTimeout(ctx, tt.ctx)
				defer cancel()
			}
			_, err := c.Call(ctx, CallRequest{FunctionID: "src::stall", Timeout: tt.timeout,
				Payload: jsonOf(t, map[string]any{"src": InputStreamFromReader(pr)})})
			if !errors.Is(err, tt.want) {
				t.Fatalf("err = %v, want %v", err, tt.want)
			}
			select {
			case err := <-reads:
				wantCode(t, err, CodeStreamAborted)
			case <-time.After(3 * time.Second):
				t.Fatal("the handler's read did not end when the call failed")
			}
			if _, err := pw.Write([]byte("x")); !errors.Is(err, io.ErrClosedPipe) {
				t.Errorf("write to the caller's source after the call failed = %v, want its reader closed", err)
			}
		})
	}
	t.Run("handler error", func(t *testing.T) {
		src := &trackedReader{Reader: strings.NewReader("never read")}
		_, err := c.Call(streamCtx(t), CallRequest{FunctionID: "src::fail",
			Payload: jsonOf(t, map[string]any{"src": InputStreamFromReader(src)})})
		if err == nil {
			t.Fatal("the failed call returned no error")
		}
		waitUntil(t, "the source of a failed call is released", src.closed.Load)
	})
}

// An in-process call takes the streams it sends, as pipes do: sending one again, or
// reading it after it was sent, fails with STREAM_IN_USE, as on the remote path.
func TestLocalSentStreamIsTheReceivers(t *testing.T) {
	se := newStreamEngine(t)
	c := se.client()
	registerLocal(t, c, "src::ignore", func(ctx context.Context, data json.RawMessage) (any, error) {
		return nil, nil
	})
	registerLocal(t, c, "src::text", func(ctx context.Context, data json.RawMessage) (any, error) {
		var args copyArgs
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		return args.Src.Text(ctx)
	})
	for _, void := range []bool{false, true} {
		src := InputStreamFromBytes([]byte("once"))
		first := CallRequest{FunctionID: "src::ignore", Payload: jsonOf(t, map[string]any{"src": src})}
		if void {
			first.Action = VoidAction()
		}
		if _, err := c.Call(streamCtx(t), first); err != nil {
			t.Fatalf("first call (void %v): %v", void, err)
		}
		_, err := c.Call(streamCtx(t), CallRequest{FunctionID: "src::text", Payload: jsonOf(t, map[string]any{"src": src})})
		if streamCode(err) != CodeStreamInUse {
			t.Errorf("second call with a stream sent unread (void %v) = %v, want STREAM_IN_USE", void, err)
		}
		if _, err := src.Read(make([]byte, 4)); streamCode(err) != CodeStreamInUse {
			t.Errorf("caller's read of a stream it sent (void %v) = %v, want STREAM_IN_USE", void, err)
		}
	}
	// The receiver reads the whole stream; the caller no longer can.
	src := InputStreamFromBytes([]byte("whole"))
	if out := call(t, c, CallRequest{FunctionID: "src::text", Payload: jsonOf(t, map[string]any{"src": src})}); string(out) != `"whole"` {
		t.Errorf("result %s, want the whole stream", out)
	}
	if _, err := src.ReadAll(streamCtx(t)); streamCode(err) != CodeStreamInUse {
		t.Errorf("caller's read of a stream it sent = %v, want STREAM_IN_USE", err)
	}
}

// failingSink is a sink whose Write (or, with failClose, its Close) fails with err.
type failingSink struct {
	err       error
	failClose bool
	writes    atomic.Int32
}

func (s *failingSink) Write(p []byte) (int, error) {
	s.writes.Add(1)
	if s.failClose {
		return len(p), nil
	}
	return 0, s.err
}

func (s *failingSink) Close() error {
	if s.failClose {
		return s.err
	}
	return nil
}

// When the caller's own sink fails, the call fails with STREAM_ABORTED "the sink failed:
// <its error>", wrapping it, whatever the handler returned: nothing more is written to
// the sink, and the handler's next write fails with STREAM_CLOSED.
func TestCallerSinkFailure(t *testing.T) {
	disk := errors.New("the disk went away")
	for _, d := range dispatches {
		for _, tt := range []struct {
			name      string
			failClose bool
			handler   string // what the handler returns: "error" (its write's) or "ok"
		}{
			{name: "write, handler fails", handler: "error"},
			{name: "write, handler succeeds", handler: "ok"},
			{name: "close", failClose: true, handler: "ok"},
		} {
			t.Run(d.name+"/"+tt.name, func(t *testing.T) {
				se := newStreamEngine(t)
				c := se.client()
				writeErr := make(chan error, 1)
				registerLocal(t, c, "sink::fill", func(ctx context.Context, data json.RawMessage) (any, error) {
					var args copyArgs
					if err := json.Unmarshal(data, &args); err != nil {
						return nil, err
					}
					if tt.failClose {
						writeErr <- nil
						return nil, args.Dst.Write(ctx, []byte("all of it"))
					}
					// Write until a write fails: through a pipe, once the reader left.
					var err error
					for i := 0; i < 300 && err == nil; i++ {
						if err = args.Dst.Write(ctx, []byte("chunk")); err == nil {
							time.Sleep(10 * time.Millisecond)
						}
					}
					writeErr <- err
					if tt.handler == "error" {
						return nil, fmt.Errorf("handler gave up: %w", err)
					}
					return nil, nil
				})
				dst := &failingSink{err: disk, failClose: tt.failClose}
				_, err := c.Call(streamCtx(t), CallRequest{FunctionID: "sink::fill", Dispatch: d.dispatch,
					Payload: jsonOf(t, map[string]any{"dst": OutputStreamToWriter(dst)})})
				var ie *InvocationError
				if !errors.As(err, &ie) || ie.Code != CodeStreamAborted || ie.Message != "the sink failed: the disk went away" {
					t.Fatalf("err = %v, want STREAM_ABORTED (the sink failed: the disk went away)", err)
				}
				if !errors.Is(err, disk) {
					t.Errorf("err = %v does not wrap the sink's error", err)
				}
				if werr := <-writeErr; !tt.failClose && streamCode(werr) != CodeStreamClosed {
					t.Errorf("handler's write after the sink failed = %v, want STREAM_CLOSED", werr)
				}
				if !tt.failClose {
					if n := dst.writes.Load(); n != 1 {
						t.Errorf("the sink got %d writes, want 1: nothing after the one that failed", n)
					}
				}
			})
		}
	}
}

// TestWalkFindsARefAsDeepAsTheEngineRelays: the engine parses with serde_json, which
// refuses JSON nested deeper than 128 levels; a ref 120 levels down is found, and the
// walk stops past walkDepth.
func TestWalkFindsARefAsDeepAsTheEngineRelays(t *testing.T) {
	nest := func(depth int) any {
		var v any = map[string]any{"channel_id": "deep", "access_key": "k", "direction": "read"}
		for i := 0; i < depth; i++ {
			v = map[string]any{"next": v}
		}
		return v
	}
	found := func(v any) []string {
		var ids []string
		visitRefs(v, func(ref StreamChannelRef) { ids = append(ids, ref.ChannelID) })
		return ids
	}
	if ids := found(nest(120)); len(ids) != 1 || ids[0] != "deep" {
		t.Fatalf("refs found 120 levels down = %v, want [deep]", ids)
	}
	if ids := found(nest(walkDepth + 1)); len(ids) != 0 {
		t.Fatalf("refs found past walkDepth = %v, want none", ids)
	}
}
