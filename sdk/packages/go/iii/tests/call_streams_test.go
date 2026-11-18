//go:build integration

package iii_test

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/url"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/coder/websocket"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// Mirrors sdk/packages/rust/iii/tests/call_streams.rs: streams as values of call
// payloads and results, through the real engine's pipes, and in-process.

// streamPair runs a scenario twice: through the engine (the caller and the callee are
// two workers) and in-process (one worker calls its own function, local dispatch).
func streamPair(t *testing.T, run func(t *testing.T, caller, callee *iii.Client, register func(id string, h iii.Handler) string)) {
	t.Run("remote", func(t *testing.T) {
		// Two workers of this process need names of their own: the engine refuses a
		// second live worker of the same name.
		caller, callee := connectNamed(t, "caller-"+uniqueSuffix(t)), connectNamed(t, "callee-"+uniqueSuffix(t))
		run(t, caller, callee, func(id string, h iii.Handler) string {
			id = id + "-" + uniqueSuffix(t)
			if _, err := callee.RegisterFunction(id, h); err != nil {
				t.Fatal(err)
			}
			settle()
			return id
		})
	})
	t.Run("in-process", func(t *testing.T) {
		c := connect(t)
		run(t, c, c, func(id string, h iii.Handler) string {
			id = id + "-" + uniqueSuffix(t)
			if _, err := c.RegisterFunction(id, h); err != nil {
				t.Fatal(err)
			}
			localDispatchReady(t, c, id)
			return id
		})
	})
}

func streamCode(err error) string {
	var ie *iii.InvocationError
	if errors.As(err, &ie) {
		return ie.Code
	}
	return ""
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

// sinkBuffer is a writer that records its bytes and its Close.
type sinkBuffer struct {
	mu     sync.Mutex
	buf    bytes.Buffer
	closed chan struct{}
	once   sync.Once
}

func newSinkBuffer() *sinkBuffer { return &sinkBuffer{closed: make(chan struct{})} }

func (s *sinkBuffer) Write(p []byte) (int, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.buf.Write(p)
}

func (s *sinkBuffer) Close() error {
	s.once.Do(func() { close(s.closed) })
	return nil
}

func (s *sinkBuffer) isClosed() bool {
	select {
	case <-s.closed:
		return true
	default:
		return false
	}
}

func (s *sinkBuffer) String() string {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.buf.String()
}

// pipeSink is an io.Pipe sink and the result of reading it to the end.
func pipeSink() (*iii.OutputStream, <-chan readResult) {
	pr, pw := io.Pipe()
	done := make(chan readResult, 1)
	go func() {
		data, err := io.ReadAll(pr)
		done <- readResult{data, err}
	}()
	return iii.OutputStreamToWriter(pw), done
}

type readResult struct {
	data []byte
	err  error
}

type convertArgs struct {
	Src *iii.InputStream  `json:"src"`
	Dst *iii.OutputStream `json:"dst"`
}

// Source and sink in the payload, a stream in the result.
func TestStreamsSourceSinkAndResult(t *testing.T) {
	streamPair(t, func(t *testing.T, caller, callee *iii.Client, register func(string, iii.Handler) string) {
		fn := register("streams::convert", func(ctx context.Context, data json.RawMessage) (any, error) {
			var args convertArgs
			if err := json.Unmarshal(data, &args); err != nil {
				return nil, err
			}
			body, err := args.Src.ReadAll(ctx)
			if err != nil {
				return nil, err
			}
			if err := args.Dst.Send(ctx, map[string]string{"progress": "a byte sink drops it"}); err != nil {
				return nil, err
			}
			if err := args.Dst.Write(ctx, bytes.ToUpper(body)); err != nil {
				return nil, err
			}
			return map[string]any{"size": len(body), "copy": iii.InputStreamFromBytes(body)}, nil
		})
		big := bytes.Repeat([]byte("stream-"), 300_000) // 2.1 MB, many frames
		src := &trackedReader{Reader: bytes.NewReader(big)}
		dst := newSinkBuffer()
		payload, _ := json.Marshal(convertArgs{iii.InputStreamFromReader(src), iii.OutputStreamToWriter(dst)})
		ctx := ctxFor(t, 30*time.Second)
		out, err := caller.Call(ctx, iii.CallRequest{FunctionID: fn, Payload: payload, Timeout: 30 * time.Second})
		if err != nil {
			t.Fatalf("Call: %v", err)
		}
		if !dst.isClosed() || dst.String() != strings.ToUpper(string(big)) {
			t.Fatalf("the call returned before the sink got the whole stream: closed %v, %d bytes", dst.isClosed(), len(dst.String()))
		}
		if !src.closed.Load() {
			t.Error("the source reader was not closed once read")
		}
		var res struct {
			Size int              `json:"size"`
			Copy *iii.InputStream `json:"copy"`
		}
		if err := json.Unmarshal(out, &res); err != nil {
			t.Fatal(err)
		}
		got, err := res.Copy.ReadAll(ctx)
		if err != nil || !bytes.Equal(got, big) || res.Size != len(big) {
			t.Fatalf("result stream: %d bytes, %v", len(got), err)
		}
	})
}

// R5: a received stream passed on unread goes as its own ref; one passed on after a
// read is pumped from where it is.
func TestStreamsForwarding(t *testing.T) {
	streamPair(t, func(t *testing.T, caller, callee *iii.Client, register func(string, iii.Handler) string) {
		reader := register("streams::read", func(ctx context.Context, data json.RawMessage) (any, error) {
			var raw struct {
				Src iii.StreamChannelRef `json:"src"`
			}
			_ = json.Unmarshal(data, &raw)
			var args convertArgs
			if err := json.Unmarshal(data, &args); err != nil {
				return nil, err
			}
			text, err := args.Src.Text(ctx)
			return map[string]string{"text": text, "id": raw.Src.ChannelID}, err
		})
		pass := register("streams::pass", func(ctx context.Context, data json.RawMessage) (any, error) {
			var raw struct {
				Src iii.StreamChannelRef `json:"src"`
			}
			_ = json.Unmarshal(data, &raw)
			var args struct {
				Peek bool             `json:"peek"`
				Src  *iii.InputStream `json:"src"`
			}
			if err := json.Unmarshal(data, &args); err != nil {
				return nil, err
			}
			if args.Peek {
				if _, err := args.Src.Read(make([]byte, 2)); err != nil {
					return nil, err
				}
			}
			payload, _ := json.Marshal(map[string]any{"src": args.Src})
			out, err := callee.Call(ctx, iii.CallRequest{FunctionID: reader, Payload: payload})
			if err != nil {
				return nil, err
			}
			var res map[string]string
			_ = json.Unmarshal(out, &res)
			res["received"] = raw.Src.ChannelID
			return res, nil
		})
		for _, peek := range []bool{false, true} {
			payload, _ := json.Marshal(map[string]any{"peek": peek, "src": iii.InputStreamFromBytes([]byte("hello"))})
			out, err := caller.Call(ctxFor(t, 15*time.Second), iii.CallRequest{FunctionID: pass, Payload: payload})
			if err != nil {
				t.Fatalf("peek %v: %v", peek, err)
			}
			var res map[string]string
			_ = json.Unmarshal(out, &res)
			wantText, sameRef := "hello", true
			if peek {
				wantText, sameRef = "llo", false
			}
			if res["text"] != wantText {
				t.Errorf("peek %v: text %q, want %q", peek, res["text"], wantText)
			}
			// Through the engine an unread stream is forwarded as its own ref; in-process
			// both are the caller's stream (a placeholder each time).
			if caller != callee && (res["id"] == res["received"]) != sameRef {
				t.Errorf("peek %v: forwarded ref %q, received %q, want the same ref %v", peek, res["id"], res["received"], sameRef)
			}
		}
	})
}

// R6 and the misuses: one reader, a stream sent once, no write after the end, and no
// stream in an enqueued call.
func TestStreamsMisuse(t *testing.T) {
	streamPair(t, func(t *testing.T, caller, callee *iii.Client, register func(string, iii.Handler) string) {
		noop := register("streams::noop", func(ctx context.Context, data json.RawMessage) (any, error) { return nil, nil })
		misuse := register("streams::misuse", func(ctx context.Context, data json.RawMessage) (any, error) {
			var args struct {
				Do  string            `json:"do"`
				Src *iii.InputStream  `json:"src"`
				Dst *iii.OutputStream `json:"dst"`
			}
			if err := json.Unmarshal(data, &args); err != nil {
				return nil, err
			}
			send := func(v any) error {
				payload, _ := json.Marshal(v)
				_, err := callee.Call(ctx, iii.CallRequest{FunctionID: noop, Payload: payload})
				return err
			}
			switch args.Do {
			case "twice":
				return nil, send([]any{args.Src, args.Src})
			case "read-then-send":
				if _, err := args.Src.ReadAll(ctx); err != nil {
					return nil, err
				}
				return nil, send(map[string]any{"src": args.Src})
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
			case "enqueue":
				payload, _ := json.Marshal(map[string]any{"src": args.Src})
				_, err := callee.Call(ctx, iii.CallRequest{FunctionID: noop, Payload: payload, Action: iii.EnqueueAction("default")})
				return nil, err
			}
			return nil, nil
		})
		for do, code := range map[string]string{
			"twice": iii.CodeStreamInUse, "read-then-send": iii.CodeStreamInUse, "second-reader": iii.CodeStreamInUse,
			"write-after-close": iii.CodeStreamClosed, "enqueue": iii.CodeStreamNotAllowed,
		} {
			payload, _ := json.Marshal(map[string]any{"do": do, "src": iii.InputStreamFromBytes([]byte("abc")), "dst": iii.OutputStreamToWriter(newSinkBuffer())})
			_, err := caller.Call(ctxFor(t, 15*time.Second), iii.CallRequest{FunctionID: misuse, Payload: payload})
			if streamCode(err) != code {
				t.Errorf("%s: %v, want %s", do, err, code)
			}
		}
		// The caller's own enqueue is refused before anything is sent.
		payload, _ := json.Marshal(map[string]any{"src": iii.InputStreamFromBytes([]byte("abc"))})
		_, err := caller.Call(ctxFor(t, 5*time.Second), iii.CallRequest{FunctionID: noop, Payload: payload, Action: iii.EnqueueAction("default")})
		if streamCode(err) != iii.CodeStreamNotAllowed {
			t.Errorf("enqueue: %v", err)
		}
	})
}

// failingReader yields data, then fails.
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

// An abort reaches the other side: the reader of a source that fails gets
// STREAM_ABORTED, and a sink its writer aborts fails the call with STREAM_ABORTED even
// though the function succeeded.
func TestStreamsAbortPropagates(t *testing.T) {
	streamPair(t, func(t *testing.T, caller, callee *iii.Client, register func(string, iii.Handler) string) {
		fn := register("streams::abort", func(ctx context.Context, data json.RawMessage) (any, error) {
			var args convertArgs
			if err := json.Unmarshal(data, &args); err != nil {
				return nil, err
			}
			if args.Dst != nil {
				_ = args.Dst.Write(ctx, []byte("partial"))
				return "ok", args.Dst.Abort("converter crashed")
			}
			body, err := args.Src.ReadAll(ctx)
			return map[string]any{"read": string(body), "code": streamCode(err)}, nil
		})
		ctx := ctxFor(t, 15*time.Second)
		payload, _ := json.Marshal(map[string]any{"src": iii.InputStreamFromReader(&failingReader{data: []byte("part"), err: errors.New("disk gone")})})
		out, err := caller.Call(ctx, iii.CallRequest{FunctionID: fn, Payload: payload})
		if err != nil {
			t.Fatal(err)
		}
		if string(out) != `{"code":"STREAM_ABORTED","read":"part"}` {
			t.Errorf("reader of a failed source: %s", out)
		}
		dst, done := pipeSink()
		payload, _ = json.Marshal(map[string]any{"dst": dst})
		_, err = caller.Call(ctx, iii.CallRequest{FunctionID: fn, Payload: payload})
		if streamCode(err) != iii.CodeStreamAborted {
			t.Errorf("call with an aborted sink: %v, want STREAM_ABORTED", err)
		}
		if r := <-done; streamCode(r.err) != iii.CodeStreamAborted || string(r.data) != "partial" {
			t.Errorf("sink: %q, %v", r.data, r.err)
		}
	})
}

// R2 and R3: when the handler returns, a stream it used is closed (aborted when it
// failed), one untouched is closed on success and left alone on failure (the call then
// aborts its sink), and an input it started reading is cancelled.
func TestStreamsHandlerGuard(t *testing.T) {
	streamPair(t, func(t *testing.T, caller, callee *iii.Client, register func(string, iii.Handler) string) {
		fn := register("streams::guard", func(ctx context.Context, data json.RawMessage) (any, error) {
			var args struct {
				Do  string            `json:"do"`
				Src *iii.InputStream  `json:"src"`
				Dst *iii.OutputStream `json:"dst"`
			}
			if err := json.Unmarshal(data, &args); err != nil {
				return nil, err
			}
			switch args.Do {
			case "write":
				return "a value", args.Dst.Write(ctx, []byte("body"))
			case "write-fail":
				_ = args.Dst.Write(ctx, []byte("body"))
				return nil, errors.New("boom")
			case "untouched-fail":
				return nil, errors.New("boom")
			case "peek":
				_, err := args.Src.Read(make([]byte, 4))
				return nil, err
			}
			return "a value", nil
		})
		ctx := ctxFor(t, 20*time.Second)
		for _, tc := range []struct {
			do, body, code string
			callFails      bool
		}{
			{do: "write", body: "body"},
			{do: "untouched", body: ""},
			{do: "write-fail", body: "body", code: iii.CodeStreamAborted, callFails: true},
			{do: "untouched-fail", code: iii.CodeStreamAborted, callFails: true},
		} {
			dst, done := pipeSink()
			payload, _ := json.Marshal(map[string]any{"do": tc.do, "dst": dst})
			_, err := caller.Call(ctx, iii.CallRequest{FunctionID: fn, Payload: payload})
			if (err != nil) != tc.callFails {
				t.Errorf("%s: call error %v", tc.do, err)
			}
			r := <-done
			if streamCode(r.err) != tc.code || (tc.code == "" && string(r.data) != tc.body) {
				t.Errorf("%s: sink %q, %v, want %q %s", tc.do, r.data, r.err, tc.body, tc.code)
			}
		}
		big := &trackedReader{Reader: bytes.NewReader(bytes.Repeat([]byte("x"), 4<<20))}
		payload, _ := json.Marshal(map[string]any{"do": "peek", "src": iii.InputStreamFromReader(big)})
		if _, err := caller.Call(ctx, iii.CallRequest{FunctionID: fn, Payload: payload}); err != nil {
			t.Fatal(err)
		}
		waitFor(t, "the partly read source cancelled", 10*time.Second, big.closed.Load)
	})
}

// R2 for an observer activation: a handler that leaves its response untouched and
// returns a result has the worker that called it build the response (an HTTP route),
// so the guard leaves the response open; with a null result it closes it. A plain call
// closes an untouched response whatever the result. The activation is a call through
// an observer, as the HTTP worker makes one.
func TestStreamsGuardOfAnActivation(t *testing.T) {
	host := connectNamed(t, "host-"+uniqueSuffix(t))
	app := connectNamed(t, "app-"+uniqueSuffix(t))
	caller := connectNamed(t, "caller-"+uniqueSuffix(t))
	onRequest := "streams::on-request-" + uniqueSuffix(t)
	route := "streams::route-" + uniqueSuffix(t)

	var mu sync.Mutex
	var obs *iii.Observer
	host.OnObserverAddedFor(onRequest, func(_ context.Context, o *iii.Observer) error {
		mu.Lock()
		obs = o
		mu.Unlock()
		return nil
	})
	host.OnObservationDeliverFor(onRequest, func(ctx context.Context, call *iii.Call, _ []*iii.Observer) (any, error) {
		mu.Lock()
		o := obs
		mu.Unlock()
		if o == nil {
			return nil, errors.New("no route")
		}
		return host.Call(ctx, iii.CallRequest{Observer: o, Payload: call.Input})
	})
	if _, err := host.RegisterFunction(onRequest, iii.Identity, iii.FunctionOptions{Private: true}); err != nil {
		t.Fatal(err)
	}
	var activations atomic.Int32
	if _, err := app.RegisterFunction(route, func(ctx context.Context, data json.RawMessage) (any, error) {
		if inv, _ := iii.InvocationFromContext(ctx); inv.Activation != nil {
			activations.Add(1)
		}
		var args struct {
			Result   json.RawMessage   `json:"result"`
			Response *iii.OutputStream `json:"response"`
		}
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		return args.Result, nil
	}); err != nil {
		t.Fatal(err)
	}
	settle()
	h, err := app.Observe(iii.Source{FunctionID: onRequest}, route)
	if err != nil {
		t.Fatal(err)
	}
	ready(t, h)
	waitFor(t, "the route observer", 10*time.Second, func() bool { mu.Lock(); defer mu.Unlock(); return obs != nil })

	// The host calls its private on-request function as the HTTP worker does; its
	// deliver hook calls the route through the observer.
	for _, tc := range []struct {
		name, fn, result   string
		from               *iii.Client
		activation, closed bool
	}{
		{"activation, null result", onRequest, "null", host, true, true},
		{"activation, buffered result", onRequest, `{"status_code":200}`, host, true, false},
		{"plain call, a result", route, `{"status_code":200}`, caller, false, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			before := activations.Load()
			in, out, err := tc.from.Pipe(ctxFor(t, 5*time.Second))
			if err != nil {
				t.Fatal(err)
			}
			defer in.Cancel()
			payload, _ := json.Marshal(map[string]any{"result": json.RawMessage(tc.result), "response": out})
			if _, err := tc.from.Call(ctxFor(t, 10*time.Second), iii.CallRequest{FunctionID: tc.fn, Payload: payload}); err != nil {
				t.Fatal(err)
			}
			if got := activations.Load() > before; got != tc.activation {
				t.Fatalf("the route ran as an activation: %v, want %v", got, tc.activation)
			}
			readCtx, cancel := context.WithTimeout(context.Background(), 500*time.Millisecond)
			defer cancel()
			_, err = in.ReadAll(readCtx)
			if closed := err == nil; closed != tc.closed {
				t.Errorf("response closed = %v (%v), want %v", closed, err, tc.closed)
			}
		})
	}
}

// R4: the call's timeout bounds its wait for its sinks; the sinks still open are
// aborted before the call fails with a timeout.
func TestStreamsCallTimeoutAbortsSinks(t *testing.T) {
	streamPair(t, func(t *testing.T, caller, callee *iii.Client, register func(string, iii.Handler) string) {
		fn := register("streams::hold", func(ctx context.Context, data json.RawMessage) (any, error) {
			var args convertArgs
			if err := json.Unmarshal(data, &args); err != nil {
				return nil, err
			}
			args.Dst.KeepOpen()
			return nil, args.Dst.Write(ctx, []byte("started"))
		})
		dst, done := pipeSink()
		payload, _ := json.Marshal(map[string]any{"dst": dst})
		start := time.Now()
		_, err := caller.Call(ctxFor(t, 15*time.Second), iii.CallRequest{FunctionID: fn, Payload: payload, Timeout: time.Second})
		if !errors.Is(err, iii.ErrTimeout) || time.Since(start) > 5*time.Second {
			t.Fatalf("err = %v after %v, want ErrTimeout", err, time.Since(start))
		}
		select {
		case r := <-done:
			if streamCode(r.err) != iii.CodeStreamAborted {
				t.Errorf("sink = %v, want aborted", r.err)
			}
		case <-time.After(2 * time.Second):
			t.Error("the sink was not aborted")
		}
	})
}

// A call whose result holds a stream to write returns without waiting for its sinks:
// the caller writes that stream, and the function copies it into the sink.
func TestStreamsWriteSideInResult(t *testing.T) {
	streamPair(t, func(t *testing.T, caller, callee *iii.Client, register func(string, iii.Handler) string) {
		fn := register("streams::upload", func(ctx context.Context, data json.RawMessage) (any, error) {
			var args convertArgs
			if err := json.Unmarshal(data, &args); err != nil {
				return nil, err
			}
			in, out, err := callee.Pipe(ctx)
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
		dst := newSinkBuffer()
		payload, _ := json.Marshal(map[string]any{"dst": iii.OutputStreamToWriter(dst)})
		ctx := ctxFor(t, 15*time.Second)
		out, err := caller.Call(ctx, iii.CallRequest{FunctionID: fn, Payload: payload})
		if err != nil {
			t.Fatal(err)
		}
		var res struct {
			Upload *iii.OutputStream `json:"upload"`
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
	})
}

// An observer of a call that carried streams gets a StreamInfo for each, not a stream.
func TestStreamsObserversSeeStreamInfo(t *testing.T) {
	app, watcher := connectNamed(t, "app-"+uniqueSuffix(t)), connectNamed(t, "watcher-"+uniqueSuffix(t))
	src := "streams::observed-" + uniqueSuffix(t)
	target := "streams::audit-" + uniqueSuffix(t)
	if _, err := app.RegisterFunction(src, func(ctx context.Context, data json.RawMessage) (any, error) {
		var args convertArgs
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		text, err := args.Src.Text(ctx)
		return map[string]any{"text": text, "copy": iii.InputStreamFromBytes([]byte(text))}, err
	}); err != nil {
		t.Fatal(err)
	}
	seen := make(chan iii.Observed, 4)
	if _, err := watcher.RegisterFunction(target, func(ctx context.Context, data json.RawMessage) (any, error) {
		var o iii.Observed
		err := json.Unmarshal(data, &o)
		seen <- o
		return nil, err
	}); err != nil {
		t.Fatal(err)
	}
	settle()
	h, err := watcher.Observe(iii.Source{FunctionID: src, Capture: []iii.Capture{iii.CaptureInput, iii.CaptureOutput}}, target)
	if err != nil {
		t.Fatal(err)
	}
	ready(t, h)

	caller := connectNamed(t, "caller-"+uniqueSuffix(t))
	payload, _ := json.Marshal(map[string]any{"src": iii.InputStreamFromBytes([]byte("observed"))})
	out, err := caller.Call(ctxFor(t, 15*time.Second), iii.CallRequest{FunctionID: src, Payload: payload})
	if err != nil {
		t.Fatal(err)
	}
	var res struct {
		Copy *iii.InputStream `json:"copy"`
	}
	_ = json.Unmarshal(out, &res)
	if text, err := res.Copy.Text(ctxFor(t, 10*time.Second)); err != nil || text != "observed" {
		t.Errorf("the caller's result stream: %q, %v", text, err)
	}
	select {
	case o := <-seen:
		var input struct {
			Src iii.StreamInfo `json:"src"`
		}
		var output struct {
			Copy iii.StreamInfo `json:"copy"`
		}
		if json.Unmarshal(o.Input, &input) != nil || json.Unmarshal(o.Output, &output) != nil {
			t.Fatalf("input %s output %s", o.Input, o.Output)
		}
		if !input.Src.Stream || input.Src.Direction != iii.ChannelRead || input.Src.ID == "" ||
			!output.Copy.Stream || output.Copy.Direction != iii.ChannelRead {
			t.Errorf("observed input %s output %s", o.Input, o.Output)
		}
		if bytes.Contains(o.Input, []byte("access_key")) {
			t.Errorf("the observer got a stream's access key: %s", o.Input)
		}
	case <-time.After(10 * time.Second):
		t.Fatal("the observer was not activated")
	}
}

// Interop with SDKs before call-native streams: a stream this SDK sends is a raw ref a
// 0.26 handler opens itself, and a raw ref a 0.26 caller sends is a stream here.
func TestStreamsRawRefInterop(t *testing.T) {
	caller, callee := connectNamed(t, "caller-"+uniqueSuffix(t)), connectNamed(t, "callee-"+uniqueSuffix(t))
	// A 0.26-style handler: it reads the raw ref with its own socket.
	legacy := "streams::legacy-" + uniqueSuffix(t)
	if _, err := callee.RegisterFunction(legacy, func(ctx context.Context, data json.RawMessage) (any, error) {
		var args struct {
			Body iii.StreamChannelRef `json:"body"`
		}
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		return rawRead(ctx, args.Body)
	}); err != nil {
		t.Fatal(err)
	}
	modern := "streams::modern-" + uniqueSuffix(t)
	if _, err := callee.RegisterFunction(modern, func(ctx context.Context, data json.RawMessage) (any, error) {
		var args struct {
			Body *iii.InputStream `json:"body"`
		}
		if err := json.Unmarshal(data, &args); err != nil {
			return nil, err
		}
		return args.Body.Text(ctx)
	}); err != nil {
		t.Fatal(err)
	}
	settle()
	ctx := ctxFor(t, 15*time.Second)

	payload, _ := json.Marshal(map[string]any{"body": iii.InputStreamFromBytes([]byte("to a 0.26 handler"))})
	out, err := caller.Call(ctx, iii.CallRequest{FunctionID: legacy, Payload: payload})
	if err != nil || string(out) != `"to a 0.26 handler"` {
		t.Fatalf("legacy handler: %s, %v", out, err)
	}

	// A 0.26-style caller: it makes the pipe with engine::channels::create, writes the
	// writer end with its own socket, and sends the reader ref.
	res, err := caller.Call(ctx, iii.CallRequest{FunctionID: "engine::channels::create", Payload: json.RawMessage(`{"buffer_size":null}`)})
	if err != nil {
		t.Fatal(err)
	}
	var refs struct {
		Writer iii.StreamChannelRef `json:"writer"`
		Reader iii.StreamChannelRef `json:"reader"`
	}
	if err := json.Unmarshal(res, &refs); err != nil {
		t.Fatal(err)
	}
	go func() { _ = rawWrite(context.Background(), refs.Writer, []byte("from a 0.26 caller")) }()
	payload, _ = json.Marshal(map[string]any{"body": refs.Reader})
	out, err = caller.Call(ctx, iii.CallRequest{FunctionID: modern, Payload: payload})
	if err != nil || string(out) != `"from a 0.26 caller"` {
		t.Fatalf("modern handler: %s, %v", out, err)
	}
}

func rawURL(ref iii.StreamChannelRef) string {
	return fmt.Sprintf("%s/ws/channels/%s?key=%s&dir=%s", strings.TrimRight(engineWSURL(), "/"), ref.ChannelID, url.QueryEscape(ref.AccessKey), ref.Direction)
}

// rawRead reads a pipe end as an SDK before 0.27 does: its binary frames, to the close.
func rawRead(ctx context.Context, ref iii.StreamChannelRef) (string, error) {
	conn, _, err := websocket.Dial(ctx, rawURL(ref), nil)
	if err != nil {
		return "", err
	}
	conn.SetReadLimit(-1)
	var out []byte
	for {
		typ, data, err := conn.Read(ctx)
		if err != nil {
			if s := websocket.CloseStatus(err); s == websocket.StatusNormalClosure || s == websocket.StatusNoStatusRcvd {
				return string(out), nil
			}
			return string(out), err
		}
		if typ == websocket.MessageBinary {
			out = append(out, data...)
		}
	}
}

// rawWrite writes a pipe end as an SDK before 0.27 does.
func rawWrite(ctx context.Context, ref iii.StreamChannelRef, data []byte) error {
	conn, _, err := websocket.Dial(ctx, rawURL(ref), nil)
	if err != nil {
		return err
	}
	if err := conn.Write(ctx, websocket.MessageBinary, data); err != nil {
		return err
	}
	time.Sleep(10 * time.Millisecond)
	return conn.Close(websocket.StatusNormalClosure, "stream_complete")
}
