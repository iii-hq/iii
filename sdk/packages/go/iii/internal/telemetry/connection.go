package telemetry

import (
	"context"
	"errors"
	"log"
	"math"
	"math/rand/v2"
	"net/http"
	"sync"
	"time"

	"github.com/coder/websocket"
)

// ConnState is the lifecycle state of the telemetry WebSocket (Node ConnectionState).
type ConnState int32

// Connection states.
const (
	StateDisconnected ConnState = iota
	StateConnecting
	StateConnected
	StateReconnecting
	StateFailed
)

// String returns the Node name of the state.
func (s ConnState) String() string {
	switch s {
	case StateConnecting:
		return "connecting"
	case StateConnected:
		return "connected"
	case StateReconnecting:
		return "reconnecting"
	case StateFailed:
		return "failed"
	default:
		return "disconnected"
	}
}

const (
	// MaxPendingMessages bounds the frames queued while the socket is not open
	// (SharedEngineConnection.MAX_PENDING_MESSAGES).
	MaxPendingMessages = 1000
	// handshakeTimeout caps the WebSocket handshake, as the Node and Rust SDKs do.
	handshakeTimeout = 10 * time.Second
	// writeTimeout bounds one frame write.
	writeTimeout = 10 * time.Second
)

var (
	// ErrConnectionShutdown reports a frame offered after shutdown began with no open
	// socket to carry it.
	ErrConnectionShutdown = errors.New("telemetry: connection shutdown before message could be sent")
	// ErrConnectionFailed reports a frame offered after the reconnect budget ran out.
	ErrConnectionFailed = errors.New("telemetry: connection failed after max retries")
)

// logf reports connection problems; a variable so tests can silence it.
var logf = log.Printf

// Connection is the WebSocket shared by the span, metric and log exporters (Node
// SharedEngineConnection). It dials on construction, reconnects with backoff and queues
// frames while the socket is down.
type Connection struct {
	url     string
	headers http.Header
	cfg     Reconnect

	mu           sync.Mutex
	ws           *websocket.Conn
	state        ConnState
	shuttingDown bool
	pending      [][]byte
	onConnected  []func()
	onFailed     []func()

	// writeMu keeps frames whole and ordered on the socket.
	writeMu sync.Mutex

	// reconnectCtx is cancelled by BeginShutdown: no new dial or backoff wait.
	reconnectCtx    context.Context
	cancelReconnect context.CancelFunc
	// lifeCtx is cancelled by Shutdown: the read side of the socket stops.
	lifeCtx    context.Context
	cancelLife context.CancelFunc
	done       chan struct{}
}

// NewConnection starts connecting to url (the /otel endpoint) in the background, sending
// headers on every handshake.
func NewConnection(url string, headers http.Header, cfg Reconnect) *Connection {
	c := &Connection{
		url:     url,
		headers: headers.Clone(),
		cfg:     cfg,
		done:    make(chan struct{}),
	}
	c.reconnectCtx, c.cancelReconnect = context.WithCancel(context.Background())
	c.lifeCtx, c.cancelLife = context.WithCancel(context.Background())
	go c.run()
	return c
}

func (c *Connection) run() {
	defer close(c.done)
	attempt := 0
	for {
		c.mu.Lock()
		if c.shuttingDown {
			c.state = StateDisconnected
			c.mu.Unlock()
			return
		}
		c.state = StateConnecting
		c.mu.Unlock()

		dialCtx, cancel := context.WithTimeout(c.reconnectCtx, handshakeTimeout)
		ws, _, err := websocket.Dial(dialCtx, c.url, &websocket.DialOptions{HTTPHeader: c.headers})
		cancel()
		if err == nil {
			attempt = 0
			c.opened(ws)
			c.readLoop(ws)
			c.closed(ws)
		} else if !c.IsShuttingDown() {
			logf("[OTel] WebSocket error: %v", err)
		}

		c.mu.Lock()
		if c.shuttingDown {
			c.state = StateDisconnected
			c.mu.Unlock()
			return
		}
		if c.cfg.MaxRetries != -1 && attempt >= c.cfg.MaxRetries {
			c.state = StateFailed
			c.pending = nil
			failed := append([]func(){}, c.onFailed...)
			c.mu.Unlock()
			logf("[OTel] Max retries (%d) reached, giving up", c.cfg.MaxRetries)
			for _, cb := range failed {
				cb()
			}
			return
		}
		c.state = StateReconnecting
		c.mu.Unlock()

		timer := time.NewTimer(backoffDelay(c.cfg, attempt))
		select {
		case <-timer.C:
		case <-c.reconnectCtx.Done():
			timer.Stop()
		}
		attempt++
	}
}

// maxLoggedMessage caps how much of an unexpected engine message is logged.
const maxLoggedMessage = 512

// readLoop reads the socket until it closes or lifeCtx ends, answering pings and the
// close handshake (coder/websocket does that while a Read is pending). The engine sends
// nothing on /otel except, when it refuses the connection, one error text frame before
// closing it; text frames are logged and every message is discarded.
//
// It replaces coder/websocket's CloseRead, whose goroutine answers any data message with
// Close(StatusPolicyViolation) and then waits up to 15s for itself to exit, so a single
// frame from the peer stalled the loop, and a concurrent Shutdown, for 15s.
func (c *Connection) readLoop(ws *websocket.Conn) {
	for {
		typ, data, err := ws.Read(c.lifeCtx)
		if err != nil {
			return
		}
		if typ == websocket.MessageText {
			if len(data) > maxLoggedMessage {
				data = append(data[:maxLoggedMessage:maxLoggedMessage], "..."...)
			}
			logf("[OTel] Unexpected message from the engine: %s", data)
		}
	}
}

// opened publishes ws, flushes the frames queued while it was down, then runs the
// connected callbacks (the exporters flush their own queues there), in Node's order.
func (c *Connection) opened(ws *websocket.Conn) {
	c.mu.Lock()
	c.ws = ws
	c.state = StateConnected
	pending := c.pending
	c.pending = nil
	callbacks := append([]func(){}, c.onConnected...)
	c.mu.Unlock()
	for _, frame := range pending {
		if err := c.write(ws, frame); err != nil {
			logf("[OTel] Failed to send queued telemetry: %v", err)
			break
		}
	}
	for _, cb := range callbacks {
		cb()
	}
}

func (c *Connection) closed(ws *websocket.Conn) {
	c.mu.Lock()
	if c.ws == ws {
		c.ws = nil
		if c.state == StateConnected {
			c.state = StateDisconnected
		}
	}
	c.mu.Unlock()
	_ = ws.CloseNow()
}

// Send writes prefix+data as one binary frame. With no open socket the frame is queued
// (the oldest is dropped past MaxPendingMessages) and nil is returned; after shutdown
// began or the connection failed it returns an error instead.
func (c *Connection) Send(prefix string, data []byte) error {
	frame := make([]byte, 0, len(prefix)+len(data))
	frame = append(frame, prefix...)
	frame = append(frame, data...)

	c.mu.Lock()
	if c.state == StateConnected && c.ws != nil {
		ws := c.ws
		c.mu.Unlock()
		return c.write(ws, frame)
	}
	if c.shuttingDown {
		c.mu.Unlock()
		return ErrConnectionShutdown
	}
	if c.state == StateFailed {
		c.mu.Unlock()
		return ErrConnectionFailed
	}
	if len(c.pending) >= MaxPendingMessages {
		c.pending = append(c.pending[:0:0], c.pending[1:]...)
		logf("[OTel] Pending message queue full, dropping oldest message")
	}
	c.pending = append(c.pending, frame)
	c.mu.Unlock()
	return nil
}

func (c *Connection) write(ws *websocket.Conn, frame []byte) error {
	c.writeMu.Lock()
	defer c.writeMu.Unlock()
	ctx, cancel := context.WithTimeout(c.lifeCtx, writeTimeout)
	defer cancel()
	return ws.Write(ctx, websocket.MessageBinary, frame)
}

// OnConnected registers cb to run on every (re)connection, and now when connected.
func (c *Connection) OnConnected(cb func()) {
	c.mu.Lock()
	c.onConnected = append(c.onConnected, cb)
	now := c.state == StateConnected
	c.mu.Unlock()
	if now {
		cb()
	}
}

// OnFailed registers cb to run when the connection gives up (or shutdown begins without
// an open socket), and now when already failed. Exporters drop their queues there.
func (c *Connection) OnFailed(cb func()) {
	c.mu.Lock()
	c.onFailed = append(c.onFailed, cb)
	now := c.state == StateFailed
	c.mu.Unlock()
	if now {
		cb()
	}
}

// State returns the current connection state.
func (c *Connection) State() ConnState {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.state
}

// IsShuttingDown reports whether BeginShutdown or Shutdown was called.
func (c *Connection) IsShuttingDown() bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.shuttingDown
}

// BeginShutdown stops reconnecting and queueing while an open socket stays up for the
// final flush. With no open socket there is nothing to flush to, so queued frames and
// exporter queues are dropped now (Node beginShutdown).
func (c *Connection) BeginShutdown() {
	c.mu.Lock()
	if c.shuttingDown {
		c.mu.Unlock()
		return
	}
	c.shuttingDown = true
	var failed []func()
	if c.state != StateConnected {
		c.pending = nil
		failed = append(failed, c.onFailed...)
	}
	c.mu.Unlock()
	c.cancelReconnect()
	for _, cb := range failed {
		cb()
	}
}

// Shutdown closes the socket with a normal closure, drops what is still queued and waits
// for the connection goroutine, bounded by ctx.
func (c *Connection) Shutdown(ctx context.Context) error {
	c.mu.Lock()
	c.shuttingDown = true
	ws := c.ws
	c.ws = nil
	c.pending = nil
	c.onConnected = nil
	c.onFailed = nil
	c.state = StateDisconnected
	c.mu.Unlock()
	c.cancelReconnect()

	if ws != nil {
		closed := make(chan struct{})
		go func() {
			defer close(closed)
			c.writeMu.Lock()
			defer c.writeMu.Unlock()
			_ = ws.Close(websocket.StatusNormalClosure, "")
		}()
		select {
		case <-closed:
		case <-ctx.Done():
			// Out of time. Cancelling lifeCtx makes coder/websocket close the connection
			// under the pending read and any blocked write, which ends the close handshake
			// at once; CloseNow would only wait for that same handshake.
			c.cancelLife()
		}
	}
	c.cancelLife()
	select {
	case <-c.done:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

// backoffDelay is Node scheduleReconnect's delay: an exponential base capped at
// MaxDelay, then symmetric jitter.
func backoffDelay(cfg Reconnect, attempt int) time.Duration {
	base := float64(cfg.InitialDelay) * math.Pow(cfg.BackoffMultiplier, float64(attempt))
	capped := math.Min(base, float64(cfg.MaxDelay))
	jitter := capped * cfg.JitterFactor * (2*rand.Float64() - 1)
	d := time.Duration(capped + jitter)
	if d < 0 {
		d = 0
	}
	return d
}
