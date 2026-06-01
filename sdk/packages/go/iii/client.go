package iii

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"math/rand"
	"os"
	"runtime"
	"sync"
	"time"

	"github.com/coder/websocket"
	"github.com/google/uuid"
)

// Client is a connection to the iii engine. A process becomes a worker by creating a
// Client, registering functions and/or trigger types, and calling Connect; the engine
// then invokes those functions over the same socket and the Client replies.
//
// Concurrency model (ported from the Rust SDK's single-thread + channel design,
// sdk/packages/rust/iii/src/iii.rs): all writes are funneled through one writer
// goroutine fed by the outbound channel, so the WebSocket is never written from two
// goroutines (coder/websocket, like gorilla, requires a single writer). One read-loop
// goroutine reads frames and dispatches them; user handlers run in their own goroutines
// so a slow or panicking handler never blocks the read loop. A supervisor goroutine owns
// the connect / reconnect-with-backoff lifecycle. Shared maps and the pending-invocation
// table are guarded by mu.
//
// A Client is safe for concurrent use. Register* may be called before or after Connect;
// registrations are kept in memory and (re)sent to the engine on every (re)connection.
type Client struct {
	url       string
	reconnect ReconnectConfig
	name      string

	// outbound carries already-marshaled frames to the single writer goroutine. It is
	// the only path to the socket, so writes are serialized without a write mutex.
	outbound chan []byte

	mu       sync.Mutex
	state    ConnectionState
	conn     *websocket.Conn
	writerWG sync.WaitGroup

	// In-memory registries, replayed on every (re)connect. Keyed by id.
	functions    map[string]registeredFunction
	triggers     map[string]*RegisterTriggerMessage
	triggerTypes map[string]registeredTriggerType

	// pending maps an outstanding await-style invocation_id to the channel its result
	// will be delivered on. Guarded by mu.
	pending map[uuid.UUID]chan invocationOutcome

	// offline buffers invoke/result frames sent while disconnected; flushed after
	// registrations on reconnect. Registration frames are NOT buffered here — they are
	// replayed from the registries above. Mirrors the Node SDK's messagesToSend.
	offline [][]byte

	// lifecycle
	shutdown  chan struct{}
	shutOnce  sync.Once
	connected chan struct{} // closed once the first connection is established
	connOnce  sync.Once
}

// Handler is a registered function's implementation. data is the raw JSON the engine
// sent; the returned value is marshaled into the InvocationResult.result. Returning an
// error produces an InvocationResult.error; if the error is an *InvocationError its code
// and stacktrace are preserved, otherwise it is reported with code "invocation_failed"
// (matching the Rust and Node SDKs).
type Handler func(ctx context.Context, data json.RawMessage) (any, error)

type registeredFunction struct {
	message *RegisterFunctionMessage
	handler Handler
}

type registeredTriggerType struct {
	message *RegisterTriggerTypeMessage
	handler TriggerHandler
}

// invocationOutcome is delivered to a waiting Trigger call when its InvocationResult
// arrives (or the call is cancelled on shutdown).
type invocationOutcome struct {
	result json.RawMessage
	err    error
}

// Option configures a Client. Pass options to New.
type Option func(*Client)

// WithReconnectConfig overrides the default reconnect schedule.
func WithReconnectConfig(c ReconnectConfig) Option {
	return func(cl *Client) { cl.reconnect = c }
}

// WithName sets the worker name reported to the engine (default: "hostname:pid").
func WithName(name string) Option {
	return func(cl *Client) { cl.name = name }
}

// New creates a Client for the engine at url (e.g. DefaultEngineURL). It does not
// connect; call Connect to start the connection lifecycle.
func New(url string, opts ...Option) *Client {
	c := &Client{
		url:          url,
		reconnect:    DefaultReconnectConfig(),
		name:         defaultWorkerName(),
		outbound:     make(chan []byte, 64),
		state:        StateDisconnected,
		functions:    map[string]registeredFunction{},
		triggers:     map[string]*RegisterTriggerMessage{},
		triggerTypes: map[string]registeredTriggerType{},
		pending:      map[uuid.UUID]chan invocationOutcome{},
		shutdown:     make(chan struct{}),
		connected:    make(chan struct{}),
	}
	for _, opt := range opts {
		opt(c)
	}
	return c
}

func defaultWorkerName() string {
	host, err := os.Hostname()
	if err != nil || host == "" {
		host = "unknown"
	}
	return fmt.Sprintf("%s:%d", host, os.Getpid())
}

// State returns the current connection state.
func (c *Client) State() ConnectionState {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.state
}

func (c *Client) setState(s ConnectionState) {
	c.mu.Lock()
	c.state = s
	c.mu.Unlock()
}

// RegisterFunction registers a function this worker can handle. It may be called before
// or after Connect; the registration is sent on the next (re)connect. Calling it again
// with the same id replaces the handler.
func (c *Client) RegisterFunction(id string, handler Handler) error {
	if handler == nil {
		return fmt.Errorf("iii: RegisterFunction(%q): handler is nil", id)
	}
	msg := &RegisterFunctionMessage{ID: id}
	c.mu.Lock()
	c.functions[id] = registeredFunction{message: msg, handler: handler}
	c.mu.Unlock()
	// Best-effort live send; if disconnected, the registry replay on reconnect covers
	// it (registration frames are deliberately not buffered — see offline).
	c.sendRegistration(msg)
	return nil
}

// RegisterTriggerType registers a custom trigger-type handler (e.g. "cron"). The engine
// will call the handler to start and stop individual trigger instances of this type.
func (c *Client) RegisterTriggerType(id, description string, handler TriggerHandler) error {
	if handler == nil {
		return fmt.Errorf("iii: RegisterTriggerType(%q): handler is nil", id)
	}
	msg := &RegisterTriggerTypeMessage{ID: id, Description: description}
	c.mu.Lock()
	c.triggerTypes[id] = registeredTriggerType{message: msg, handler: handler}
	c.mu.Unlock()
	c.sendRegistration(msg)
	return nil
}

// RegisterTrigger registers a trigger instance: fire functionID when a trigger of
// triggerType matches config. config and metadata are raw JSON (may be nil).
func (c *Client) RegisterTrigger(id, triggerType, functionID string, config, metadata json.RawMessage) error {
	if config == nil {
		config = json.RawMessage("{}")
	}
	msg := &RegisterTriggerMessage{
		ID:          id,
		TriggerType: triggerType,
		FunctionID:  functionID,
		Config:      config,
		Metadata:    metadata,
	}
	c.mu.Lock()
	c.triggers[id] = msg
	c.mu.Unlock()
	c.sendRegistration(msg)
	return nil
}

// TriggerRequest is the input to Trigger. Exactly the Action field selects the
// delivery semantics: nil/await (default) waits for the result; VoidAction is
// fire-and-forget; EnqueueAction routes through a named queue and awaits its receipt.
type TriggerRequest struct {
	// FunctionID is the engine function to invoke (e.g. an EngineFunctions constant).
	FunctionID string
	// Data is the JSON payload. nil is sent as {}.
	Data json.RawMessage
	// Action selects void/enqueue semantics; nil means the default await path.
	Action *TriggerAction
	// Timeout overrides DefaultInvocationTimeout for an await/enqueue call.
	Timeout time.Duration
}

// Trigger invokes a function on the engine. With the default (nil) or an enqueue action
// it waits for the matching InvocationResult and returns its result, mapping a remote
// error to *InvocationError and a missed deadline to ErrTimeout. With a void action it
// is fire-and-forget and returns immediately with a nil result.
//
// ctx bounds the wait independently of Timeout: if ctx is cancelled first, its error is
// returned and the pending entry is reclaimed.
func (c *Client) Trigger(ctx context.Context, req TriggerRequest) (json.RawMessage, error) {
	data := req.Data
	if data == nil {
		data = json.RawMessage("{}")
	}
	tc := extractTraceContext(ctx)

	// Void: fire-and-forget, no invocation_id, no pending entry, return immediately.
	if req.Action != nil && req.Action.Type == "void" {
		frame, err := MarshalMessage(&InvokeFunctionMessage{
			FunctionID:  req.FunctionID,
			Data:        data,
			Action:      req.Action,
			Traceparent: tc.traceparent,
			Baggage:     tc.baggage,
		})
		if err != nil {
			return nil, err
		}
		c.enqueueOutbound(frame)
		return nil, nil
	}

	// Await / enqueue: generate an id, register a pending channel, send, then wait for
	// the result keyed by that id. Enqueue uses the same path; the engine returns an
	// enqueue receipt as the result rather than the function's eventual output.
	id := uuid.New()
	resultCh := make(chan invocationOutcome, 1)
	c.mu.Lock()
	c.pending[id] = resultCh
	c.mu.Unlock()

	frame, err := MarshalMessage(&InvokeFunctionMessage{
		InvocationID: &id,
		FunctionID:   req.FunctionID,
		Data:         data,
		Action:       req.Action,
		Traceparent:  tc.traceparent,
		Baggage:      tc.baggage,
	})
	if err != nil {
		c.clearPending(id)
		return nil, err
	}
	c.enqueueOutbound(frame)

	timeout := req.Timeout
	if timeout <= 0 {
		timeout = DefaultInvocationTimeout
	}
	timer := time.NewTimer(timeout)
	defer timer.Stop()

	select {
	case outcome := <-resultCh:
		return outcome.result, outcome.err
	case <-timer.C:
		c.clearPending(id)
		return nil, ErrTimeout
	case <-ctx.Done():
		c.clearPending(id)
		return nil, ctx.Err()
	case <-c.shutdown:
		c.clearPending(id)
		return nil, ErrNotConnected
	}
}

func (c *Client) clearPending(id uuid.UUID) {
	c.mu.Lock()
	delete(c.pending, id)
	c.mu.Unlock()
}

// sendRegistration sends a registration frame if connected, and drops it otherwise:
// registrations are replayed from the in-memory registries on the next connect, so
// buffering them too would double-send. Mirrors the Node SDK's skipIfClosed=true.
func (c *Client) sendRegistration(msg any) {
	frame, err := MarshalMessage(msg)
	if err != nil {
		return
	}
	c.mu.Lock()
	connected := c.state == StateConnected
	c.mu.Unlock()
	if connected {
		c.enqueueOutbound(frame)
	}
}

// enqueueOutbound hands a frame to the writer goroutine when connected, or buffers it
// for the next connection otherwise. Only invoke/result frames reach here for buffering;
// registrations are gated by sendRegistration.
func (c *Client) enqueueOutbound(frame []byte) {
	c.mu.Lock()
	if c.state != StateConnected {
		c.offline = append(c.offline, frame)
		c.mu.Unlock()
		return
	}
	c.mu.Unlock()

	select {
	case c.outbound <- frame:
	case <-c.shutdown:
	}
}

// Connect starts the connection lifecycle and blocks until the first connection is
// established or ctx is cancelled. The supervisor goroutine then keeps the connection
// alive (reconnecting with backoff) until Close.
func (c *Client) Connect(ctx context.Context) error {
	go c.supervise()

	select {
	case <-c.connected:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	case <-c.shutdown:
		return ErrNotConnected
	}
}

// supervise owns the connect/reconnect loop. Each iteration dials, runs the connection
// until it drops, then backs off before retrying — until Close or MaxRetries.
func (c *Client) supervise() {
	attempt := 0
	for {
		select {
		case <-c.shutdown:
			return
		default:
		}

		if attempt == 0 {
			c.setState(StateConnecting)
		} else {
			c.setState(StateReconnecting)
		}

		err := c.runConnection()
		if err == nil {
			// Clean shutdown of the connection (Close was called).
			return
		}

		attempt++
		if c.reconnect.MaxRetries != -1 && attempt > c.reconnect.MaxRetries {
			c.setState(StateFailed)
			return
		}

		delay := c.backoffDelay(attempt - 1)
		select {
		case <-time.After(delay):
		case <-c.shutdown:
			return
		}
	}
}

// backoffDelay computes the delay before reconnect attempt n (0-based), matching the
// Node SDK's scheduleReconnect (sdk/packages/node/iii/src/iii.ts): an exponential base
// capped at MaxDelay, then symmetric ±JitterFactor jitter.
func (c *Client) backoffDelay(attempt int) time.Duration {
	base := float64(c.reconnect.InitialDelay) * math.Pow(c.reconnect.BackoffMultiplier, float64(attempt))
	capped := math.Min(base, float64(c.reconnect.MaxDelay))
	jitter := capped * c.reconnect.JitterFactor * (2*rand.Float64() - 1)
	d := time.Duration(capped + jitter)
	if d < 0 {
		d = 0
	}
	return d
}

// runConnection dials, runs the read loop and writer until the socket drops or Close is
// called, and returns nil only on a clean Close. A returned error means "reconnect".
func (c *Client) runConnection() error {
	// A connection-scoped context cancelled when this connection ends, so the writer
	// goroutine and any per-connection work stop together.
	connCtx, cancel := context.WithCancel(context.Background())
	defer cancel()

	conn, _, err := websocket.Dial(connCtx, c.url, nil)
	if err != nil {
		// Treat dial failure as a reconnectable error unless we're shutting down.
		select {
		case <-c.shutdown:
			return nil
		default:
			return err
		}
	}
	// The engine can send large registration payloads; lift the default read limit.
	conn.SetReadLimit(-1)

	c.mu.Lock()
	c.conn = conn
	c.state = StateConnected
	c.mu.Unlock()

	// Start the single writer for this connection.
	writerDone := make(chan struct{})
	c.writerWG.Add(1)
	go c.writeLoop(connCtx, conn, writerDone)

	// Signal the first successful connection to Connect's caller.
	c.connOnce.Do(func() { close(c.connected) })

	// On (re)connect, replay registrations, flush the offline buffer, then register
	// worker metadata — in that exact order (matching Node and Rust).
	c.onConnect()

	// Read loop runs until the socket errors or closes.
	readErr := c.readLoop(connCtx, conn)

	// Tear down this connection: stop the writer, close the socket, clear conn.
	cancel()
	<-writerDone
	_ = conn.CloseNow()
	c.mu.Lock()
	c.conn = nil
	if c.state == StateConnected {
		c.state = StateDisconnected
	}
	c.mu.Unlock()

	// If Close was called, report a clean stop; otherwise ask for a reconnect.
	select {
	case <-c.shutdown:
		return nil
	default:
		if readErr == nil {
			readErr = fmt.Errorf("iii: connection closed")
		}
		return readErr
	}
}

// onConnect replays the in-memory registries and flushes buffered frames in the order
// the engine expects: trigger types, functions, triggers, offline buffer, then worker
// metadata last.
func (c *Client) onConnect() {
	c.mu.Lock()
	var frames [][]byte
	for _, tt := range c.triggerTypes {
		if f, err := MarshalMessage(tt.message); err == nil {
			frames = append(frames, f)
		}
	}
	for _, fn := range c.functions {
		if f, err := MarshalMessage(fn.message); err == nil {
			frames = append(frames, f)
		}
	}
	for _, tr := range c.triggers {
		if f, err := MarshalMessage(tr); err == nil {
			frames = append(frames, f)
		}
	}
	// Drain the offline buffer after registrations.
	frames = append(frames, c.offline...)
	c.offline = nil
	c.mu.Unlock()

	for _, f := range frames {
		select {
		case c.outbound <- f:
		case <-c.shutdown:
			return
		}
	}

	c.registerWorkerMetadata()
}

// registerWorkerMetadata announces this worker to the engine as a fire-and-forget
// invocation of engine::workers::register, tagged runtime "go". Sent last on connect.
func (c *Client) registerWorkerMetadata() {
	pid := os.Getpid()
	meta := workerMetadata{
		Runtime: runtimeTag,
		Version: sdkVersion,
		Name:    c.name,
		OS:      fmt.Sprintf("%s %s", runtime.GOOS, runtime.GOARCH),
		PID:     &pid,
	}
	data, err := json.Marshal(meta)
	if err != nil {
		return
	}
	frame, err := MarshalMessage(&InvokeFunctionMessage{
		FunctionID: FnRegisterWorker,
		Data:       data,
		Action:     VoidAction(),
	})
	if err != nil {
		return
	}
	select {
	case c.outbound <- frame:
	case <-c.shutdown:
	}
}

// workerMetadata is the payload sent to engine::workers::register. v1 reports the
// essentials and omits the optional telemetry sub-object (deferred with full OTel —
// open question #3 in iii-hq/iii#1719). Optional fields use omitempty to match the
// engine's skip_serializing_if discipline.
type workerMetadata struct {
	Runtime string `json:"runtime"`
	Version string `json:"version"`
	Name    string `json:"name"`
	OS      string `json:"os"`
	PID     *int   `json:"pid,omitempty"`
}

// sdkVersion is reported in the worker metadata. Kept as a const for v1; a release
// process can wire this to the module version later.
const sdkVersion = "0.1.0"

// writeLoop is the single writer for one connection: it drains outbound until the
// connection context is cancelled. Routing every send through here keeps the socket
// single-writer, as coder/websocket requires.
func (c *Client) writeLoop(ctx context.Context, conn *websocket.Conn, done chan struct{}) {
	defer c.writerWG.Done()
	defer close(done)
	for {
		select {
		case <-ctx.Done():
			return
		case <-c.shutdown:
			return
		case frame := <-c.outbound:
			if err := conn.Write(ctx, websocket.MessageText, frame); err != nil {
				return // write failure ends the connection; supervisor reconnects
			}
		}
	}
}

// readLoop reads and dispatches frames until the socket errors or closes.
func (c *Client) readLoop(ctx context.Context, conn *websocket.Conn) error {
	for {
		typ, data, err := conn.Read(ctx)
		if err != nil {
			return err
		}
		if typ != websocket.MessageText {
			continue
		}
		dec, err := UnmarshalMessage(data)
		if err != nil {
			continue // ignore undecodable frames rather than tearing down the socket
		}
		c.dispatch(ctx, dec)
	}
}

// dispatch routes one decoded inbound frame. Handler execution is offloaded to its own
// goroutine so the read loop is never blocked by user code.
func (c *Client) dispatch(ctx context.Context, dec *DecodedMessage) {
	switch dec.Type {
	case MsgInvokeFunction:
		go c.handleInvoke(ctx, dec.InvokeFunction)
	case MsgInvocationResult:
		c.handleInvocationResult(dec.InvocationResult)
	case MsgRegisterTrigger:
		go c.handleRegisterTrigger(ctx, dec.RegisterTrigger)
	case MsgPing:
		if frame, err := MarshalMessage(&PongMessage{}); err == nil {
			c.enqueueOutboundDirect(frame)
		}
	case MsgWorkerRegistered, MsgTriggerRegistrationResult:
		// Informational; nothing to do. (A trigger-registration error is the engine's
		// to surface; the reference SDKs only log it.)
	default:
		// Unknown/unhandled inbound type; ignore.
	}
}

// enqueueOutboundDirect sends a frame to the writer without the offline-buffering path —
// used for protocol replies (pong, invocation results) that are only meaningful on the
// connection they answer.
func (c *Client) enqueueOutboundDirect(frame []byte) {
	select {
	case c.outbound <- frame:
	case <-c.shutdown:
	}
}

// handleInvoke runs a registered function for an inbound InvokeFunction and, unless the
// call is fire-and-forget (no invocation_id), replies with an InvocationResult.
func (c *Client) handleInvoke(ctx context.Context, msg *InvokeFunctionMessage) {
	c.mu.Lock()
	fn, ok := c.functions[msg.FunctionID]
	c.mu.Unlock()

	tc := traceContext{traceparent: msg.Traceparent, baggage: msg.Baggage}

	if !ok {
		c.replyInvocation(msg, nil, &ErrorBody{
			Code:    "function_not_found",
			Message: fmt.Sprintf("no handler registered for %q", msg.FunctionID),
		}, tc)
		return
	}

	result, herr := c.runHandler(injectTraceContext(ctx, tc), fn.handler, msg.Data)
	if herr != nil {
		c.replyInvocation(msg, nil, errorBodyFromHandlerError(herr), tc)
		return
	}
	resultJSON, err := json.Marshal(result)
	if err != nil {
		c.replyInvocation(msg, nil, &ErrorBody{Code: "invocation_failed", Message: err.Error()}, tc)
		return
	}
	c.replyInvocation(msg, resultJSON, nil, tc)
}

// runHandler invokes h, recovering a panic into an error so a panicking handler yields
// an InvocationResult.error instead of leaving the caller to time out.
func (c *Client) runHandler(ctx context.Context, h Handler, data json.RawMessage) (result any, err error) {
	defer func() {
		if r := recover(); r != nil {
			err = fmt.Errorf("iii: handler panic: %v", r)
		}
	}()
	return h(ctx, data)
}

// errorBodyFromHandlerError maps a handler error to a wire ErrorBody. A typed
// *InvocationError carries its code and stacktrace through; any other error is reported
// with code "invocation_failed", matching the Rust and Node SDKs.
func errorBodyFromHandlerError(err error) *ErrorBody {
	var ie *InvocationError
	if errors.As(err, &ie) {
		body := &ErrorBody{Code: ie.Code, Message: ie.Message}
		if ie.Stacktrace != "" {
			s := ie.Stacktrace
			body.Stacktrace = &s
		}
		return body
	}
	return &ErrorBody{Code: "invocation_failed", Message: err.Error()}
}

// replyInvocation sends the InvocationResult for an invocation, unless it was
// fire-and-forget (no invocation_id), in which case there is nowhere to reply. Echoes
// the incoming trace context back on the result, as the engine expects.
func (c *Client) replyInvocation(msg *InvokeFunctionMessage, result json.RawMessage, errBody *ErrorBody, tc traceContext) {
	if msg.InvocationID == nil {
		return // fire-and-forget: no reply expected
	}
	frame, err := MarshalMessage(&InvocationResultMessage{
		InvocationID: *msg.InvocationID,
		FunctionID:   msg.FunctionID,
		Result:       result,
		Error:        errBody,
		Traceparent:  tc.traceparent,
		Baggage:      tc.baggage,
	})
	if err != nil {
		return
	}
	c.enqueueOutboundDirect(frame)
}

// handleInvocationResult resolves the pending Trigger call keyed by the result's
// invocation_id. A result with no matching pending entry (e.g. one that already timed
// out) is dropped.
func (c *Client) handleInvocationResult(msg *InvocationResultMessage) {
	c.mu.Lock()
	ch, ok := c.pending[msg.InvocationID]
	if ok {
		delete(c.pending, msg.InvocationID)
	}
	c.mu.Unlock()
	if !ok {
		return
	}

	if msg.Error != nil {
		ch <- invocationOutcome{err: newInvocationError(msg.Error, msg.FunctionID)}
		return
	}
	ch <- invocationOutcome{result: msg.Result}
}

// handleRegisterTrigger routes an inbound RegisterTrigger to the matching trigger-type
// handler and replies with a TriggerRegistrationResult: error nil on success,
// trigger_registration_failed if the handler errors, trigger_type_not_found if no
// handler is registered for the type. Mirrors the Rust and Node SDKs.
func (c *Client) handleRegisterTrigger(ctx context.Context, msg *RegisterTriggerMessage) {
	c.mu.Lock()
	tt, ok := c.triggerTypes[msg.TriggerType]
	c.mu.Unlock()

	res := &TriggerRegistrationResultMessage{
		ID:          msg.ID,
		TriggerType: msg.TriggerType,
		FunctionID:  msg.FunctionID,
	}
	if !ok {
		res.Error = &ErrorBody{Code: "trigger_type_not_found", Message: "Trigger type not found"}
	} else {
		cfg := TriggerConfig{
			ID:         msg.ID,
			FunctionID: msg.FunctionID,
			Config:     msg.Config,
			Metadata:   msg.Metadata,
		}
		if err := tt.handler.RegisterTrigger(ctx, cfg); err != nil {
			res.Error = &ErrorBody{Code: "trigger_registration_failed", Message: err.Error()}
		}
	}
	if frame, err := MarshalMessage(res); err == nil {
		c.enqueueOutboundDirect(frame)
	}
}

// Close shuts the client down: it stops the reconnect loop, cancels all pending Trigger
// calls with ErrNotConnected, and closes the socket. It does not send unregister frames
// (matching the reference SDKs). Close is idempotent.
func (c *Client) Close() error {
	c.shutOnce.Do(func() {
		close(c.shutdown)

		// Cancel all pending invocations.
		c.mu.Lock()
		for id, ch := range c.pending {
			ch <- invocationOutcome{err: ErrNotConnected}
			delete(c.pending, id)
		}
		conn := c.conn
		c.state = StateDisconnected
		c.mu.Unlock()

		if conn != nil {
			_ = conn.CloseNow()
		}
	})
	return nil
}
