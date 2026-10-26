//go:build integration

package iii_test

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/coder/websocket"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// Helpers for the observe tests against a real engine (III_URL).

// runID and the per-test suffix make function ids unique per test run (also under
// -count=N): observers of an earlier run's workers outlive them on the engine
// (suspended, 30 s grace) and would attach to a function of the same id.
var runID = func() string {
	b := make([]byte, 4)
	_, _ = rand.Read(b)
	return hex.EncodeToString(b)
}()

var testSuffixes sync.Map // *testing.T -> string

func fnID(t *testing.T, name string) string {
	s, _ := testSuffixes.LoadOrStore(t, fmt.Sprintf("%s-%d", runID, suffixCounter.Add(1)))
	return name + "-" + s.(string)
}

// observeWorker connects a worker of its own with a fast reconnect schedule. Pass a
// proxy to count its frames or drop its connection.
func observeWorker(t *testing.T, role string, proxy *frameProxy, opts ...iii.Option) *iii.Client {
	t.Helper()
	url := engineWSURL()
	if proxy != nil {
		url = proxy.url
	}
	opts = append([]iii.Option{
		iii.WithName(role + "-" + uniqueSuffix(t)),
		iii.WithReconnectConfig(iii.ReconnectConfig{InitialDelay: 50 * time.Millisecond, MaxDelay: 200 * time.Millisecond, BackoffMultiplier: 1, MaxRetries: -1}),
	}, opts...)
	c := iii.New(url, opts...)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatalf("connect to engine at %s: %v", url, err)
	}
	t.Cleanup(func() { _ = c.Close() })
	return c
}

func waitFor(t *testing.T, what string, timeout time.Duration, cond func() bool) {
	t.Helper()
	deadline := time.Now().Add(timeout)
	for !cond() {
		if time.Now().After(deadline) {
			t.Fatalf("timed out after %s waiting for %s", timeout, what)
		}
		time.Sleep(10 * time.Millisecond)
	}
}

func ready(t *testing.T, h *iii.ObserverHandle) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := h.Ready(ctx); err != nil {
		t.Fatalf("observer %s not ready: %v (state %s)", h.ID(), err, h.State())
	}
}

func readyFails(t *testing.T, h *iii.ObserverHandle) *iii.ObserveRejectionError {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	err := h.Ready(ctx)
	rej, ok := err.(*iii.ObserveRejectionError)
	if !ok {
		t.Fatalf("Ready: %v (state %s), want an observe rejection", err, h.State())
	}
	return rej
}

// localDispatchReady waits until the engine confirmed fn for in-process calls.
func localDispatchReady(t *testing.T, c *iii.Client, fn string) {
	t.Helper()
	waitFor(t, "functionregistered{local_dispatch: true} for "+fn, 10*time.Second, func() bool { return c.LocalDispatchActive(fn) })
}

// activation is one invocation a collector received.
type activation struct {
	Data     map[string]any
	Raw      json.RawMessage
	Metadata json.RawMessage
	Inv      iii.Invocation
}

type collected struct {
	mu  sync.Mutex
	got []activation
}

func (c *collected) all() []activation {
	c.mu.Lock()
	defer c.mu.Unlock()
	return append([]activation(nil), c.got...)
}

func (c *collected) len() int { return len(c.all()) }

// collector registers fn as a target that records every activation it receives.
func collector(t *testing.T, c *iii.Client, fn string) *collected {
	t.Helper()
	col := &collected{}
	err := c.RegisterFunction(fn, func(ctx context.Context, data json.RawMessage) (any, error) {
		a := activation{Raw: append(json.RawMessage(nil), data...)}
		_ = json.Unmarshal(data, &a.Data)
		a.Metadata, _ = iii.MetadataFromContext(ctx)
		a.Inv, _ = iii.InvocationFromContext(ctx)
		col.mu.Lock()
		col.got = append(col.got, a)
		col.mu.Unlock()
		return nil, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	return col
}

// exactly waits for n activations, then a little longer to catch extra ones.
func exactly(t *testing.T, col *collected, n int, what string, timeout time.Duration) []activation {
	t.Helper()
	waitFor(t, what, timeout, func() bool { return col.len() >= n })
	time.Sleep(300 * time.Millisecond)
	got := col.all()
	if len(got) != n {
		t.Fatalf("%s: %d activations, want %d", what, len(got), n)
	}
	return got
}

func call(t *testing.T, c *iii.Client, req iii.TriggerRequest) json.RawMessage {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	out, err := c.Trigger(ctx, req)
	if err != nil {
		t.Fatalf("Trigger(%s): %v", req.FunctionID, err)
	}
	return out
}

func raw(v any) json.RawMessage {
	data, _ := json.Marshal(v)
	return data
}

// field reads a nested member of a decoded JSON object ("caller.worker_name").
func field(m map[string]any, path string) any {
	var cur any = m
	for _, k := range strings.Split(path, ".") {
		obj, ok := cur.(map[string]any)
		if !ok {
			return nil
		}
		cur = obj[k]
	}
	return cur
}

// frameProxy sits between one worker and the engine: it counts the frames the worker
// sends (the zero-wire tests) and can drop the connection (the reconnect tests). It
// forwards each connection to the same path on the engine, so the worker's telemetry
// socket (<address>/otel) reaches the engine's /otel endpoint; only the worker endpoint's
// frames are counted.
type frameProxy struct {
	t        *testing.T
	server   *httptest.Server
	url      string
	upstream string

	mu      sync.Mutex
	sent    []string // types of worker -> engine frames, in order
	clients []*websocket.Conn
	held    bool // refuse new connections (the worker stays away)
}

func newFrameProxy(t *testing.T) *frameProxy {
	t.Helper()
	p := &frameProxy{t: t, upstream: engineWSURL()}
	p.server = httptest.NewServer(http.HandlerFunc(p.handle))
	p.url = "ws" + strings.TrimPrefix(p.server.URL, "http")
	t.Cleanup(func() {
		p.drop()
		p.server.Close()
	})
	return p
}

func (p *frameProxy) handle(w http.ResponseWriter, r *http.Request) {
	p.mu.Lock()
	held := p.held
	p.mu.Unlock()
	if held {
		http.Error(w, "held by the test", http.StatusServiceUnavailable)
		return
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	header := http.Header{}
	if auth := r.Header.Get("Authorization"); auth != "" {
		header.Set("Authorization", auth)
	}
	up, _, err := websocket.Dial(ctx, p.upstreamFor(r), &websocket.DialOptions{HTTPHeader: header})
	if err != nil {
		http.Error(w, err.Error(), http.StatusBadGateway)
		return
	}
	down, err := websocket.Accept(w, r, nil)
	if err != nil {
		_ = up.CloseNow()
		return
	}
	up.SetReadLimit(64 << 20)
	down.SetReadLimit(64 << 20)
	p.mu.Lock()
	p.clients = append(p.clients, down)
	p.mu.Unlock()
	done := make(chan struct{}, 2)
	pump := func(from, to *websocket.Conn, record bool) {
		defer func() { done <- struct{}{} }()
		for {
			typ, data, err := from.Read(ctx)
			if err != nil {
				return
			}
			if record && typ == websocket.MessageText {
				var head struct {
					Type string `json:"type"`
				}
				_ = json.Unmarshal(data, &head)
				p.mu.Lock()
				p.sent = append(p.sent, head.Type)
				p.mu.Unlock()
			}
			if err := to.Write(ctx, typ, data); err != nil {
				return
			}
		}
	}
	go pump(down, up, isWorkerEndpoint(r))
	go pump(up, down, false)
	<-done
	cancel()
	_ = up.CloseNow()
	_ = down.CloseNow()
}

// upstreamFor is the engine URL for a proxied request: the request's path and query on
// the upstream address. The worker endpoint is the engine root; the client's telemetry
// socket asks for /otel.
func (p *frameProxy) upstreamFor(r *http.Request) string {
	u, err := url.Parse(p.upstream)
	if err != nil {
		return p.upstream
	}
	u.Path = strings.TrimRight(u.Path, "/") + r.URL.Path
	u.RawPath = ""
	if r.URL.RawQuery != "" {
		u.RawQuery = r.URL.RawQuery
	}
	return u.String()
}

// isWorkerEndpoint reports whether r is a connection to the worker endpoint (the engine
// root), whose frames the proxy counts.
func isWorkerEndpoint(r *http.Request) bool {
	return r.URL.Path == "" || r.URL.Path == "/"
}

// mark is the number of frames the worker sent so far.
func (p *frameProxy) mark() int {
	p.mu.Lock()
	defer p.mu.Unlock()
	return len(p.sent)
}

// since returns the frame types sent after mark.
func (p *frameProxy) since(mark int) []string {
	p.mu.Lock()
	defer p.mu.Unlock()
	return append([]string(nil), p.sent[mark:]...)
}

// hold refuses new connections until release: drop then hold keeps the worker away.
func (p *frameProxy) hold() {
	p.mu.Lock()
	p.held = true
	p.mu.Unlock()
}

func (p *frameProxy) release() {
	p.mu.Lock()
	p.held = false
	p.mu.Unlock()
}

// drop closes the worker's connection the way a network failure does.
func (p *frameProxy) drop() {
	p.mu.Lock()
	clients := p.clients
	p.clients = nil
	p.mu.Unlock()
	for _, c := range clients {
		_ = c.CloseNow()
	}
}

func countOf(types []string, want string) int {
	n := 0
	for _, t := range types {
		if t == want {
			n++
		}
	}
	return n
}

func without(types []string, skip string) []string {
	var out []string
	for _, t := range types {
		if t != skip {
			out = append(out, t)
		}
	}
	return out
}
