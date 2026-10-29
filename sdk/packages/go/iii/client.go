package iii

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"math"
	"math/rand"
	"net/http"
	"os"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/coder/websocket"
	"github.com/google/uuid"
)

// Client is a connection to the iii engine. A process becomes a worker by creating a
// Client, registering functions, and calling Connect; the engine then invokes those
// functions over the same socket and the Client replies.
//
// Concurrency model (ported from the Rust SDK's single-thread + channel design): all
// data writes are funneled through one writer goroutine fed by the outbound channel. A separate keepalive goroutine sends WebSocket
// control pings, which coder/websocket permits concurrently with writes and its Reader.
// One read-loop goroutine reads frames and dispatches them; user handlers run in their
// own goroutines so a slow or panicking handler never blocks the read loop. A supervisor
// goroutine owns the connect / reconnect-with-backoff lifecycle. Shared maps and the
// pending-invocation table are guarded by mu.
//
// A Client is safe for concurrent use. Register* may be called before or after Connect;
// registrations are kept in memory and (re)sent to the engine on every (re)connection.
type Client struct {
	url       string
	reconnect ReconnectConfig
	// keepalive timings are internal so all clients use protocol-compatible defaults.
	// Tests override them to exercise the lifecycle without waiting for wall-clock seconds.
	keepaliveInterval time.Duration
	keepaliveTimeout  time.Duration
	name              string
	description       string
	// headers are the caller's WithHeaders, sent on the WebSocket handshake of the
	// main connection and of channels. handshakeHeaders adds the III_TOKEN bearer.
	headers http.Header
	// namespace is the routing dimension this worker registers into. Empty means
	// the engine's default namespace. Resolved once at construction: an explicit
	// WithNamespace wins, else the III_NAMESPACE env var, else default.
	namespace string
	// namespaceSet records that WithNamespace was called, which "" alone cannot
	// say. Without it, a namespace named and left blank is indistinguishable
	// from one never given, and they ask for opposite things.
	namespaceSet bool

	// outbound carries connection-AGNOSTIC frames (registrations + offline-buffered
	// invokes) to the single writer goroutine. It is shared across the client's lifetime
	// and its contents are meant to (re)send on whichever connection is live.
	outbound chan []byte

	// reply is the per-connection channel for connection-SCOPED frames (pong,
	// InvocationResult, observer acks, reports, calls through an observer): frames that
	// are only meaningful on the socket that prompted them, written in the order they
	// were sent. A fresh channel is created per connection and discarded on teardown,
	// so a reply can never leak onto the next connection (iii-hq/iii#1749).
	// Guarded by replyMu; nil when there is no live connection.
	replyMu sync.Mutex
	reply   chan []byte
	// replyEpoch is the connection reply belongs to; replyDone is closed when that
	// connection ends, so a send blocked on a full channel gives up.
	replyEpoch uint64
	replyDone  chan struct{}
	// replyQueued counts the bytes of report frames (invocationsobserved) in reply
	// that the writer has not written yet: they count against the reporter's bound.
	replyQueued *atomic.Int64
	// replySenders tracks report sends into reply, so a teardown can count the report
	// frames its writer never wrote.
	replySenders *sync.WaitGroup

	// observeOut carries observe / unobserve frames stamped with the connection they
	// are meant for; the writer drops (or, for an unobserve, re-routes) a frame of
	// another connection.
	observeOut chan observeFrame

	mu    sync.Mutex
	state ConnectionState
	conn  *websocket.Conn
	// connCancel cancels the live connection's context (its reads, writes and pings);
	// a Shutdown out of time uses it to end the close handshake at once. Set and cleared
	// with conn.
	connCancel context.CancelFunc
	writerWG   sync.WaitGroup

	// workerID/reattachToken are the engine-assigned identity from the last
	// WorkerRegistered frame, presented back via a Reattach frame on reconnect so
	// the engine retires the previous connection before the registration replay.
	// The token authorizes the eviction (ids alone are publicly listable).
	// Guarded by mu.
	workerID      string
	reattachToken string

	// functions is the in-memory registry, replayed on every (re)connect. Keyed by id.
	functions map[string]registeredFunction

	// Observer hooks, guarded by mu: the worker's, and each function's by id (they
	// stay across Unregister and a later registration of the id). hookGen counts the
	// settings of added hooks.
	workerHooks   observerHooks
	functionHooks map[string]*observerHooks
	hookGen       uint64
	// hookIDWarned: the hook ids already reported as not registered by this worker.
	hookIDWarned map[string]bool

	// pending maps an outstanding await-style invocation_id to the channel its result
	// will be delivered on. Guarded by mu.
	pending map[uuid.UUID]chan invocationOutcome

	// offline buffers invoke/result frames sent while disconnected; flushed after
	// registrations on reconnect. Registration frames are NOT buffered here: they are
	// replayed from the registries above. Mirrors the Node SDK's messagesToSend.
	offline [][]byte

	// lifecycle
	shutdown chan struct{}
	shutOnce sync.Once
	shutDone chan struct{} // closed when the first Close or Shutdown has finished
	// shutForce cuts a running Shutdown short (a Close called meanwhile). Guarded by mu.
	shutForce     context.CancelFunc
	connected     chan struct{} // closed once the first connection is established
	connOnce      sync.Once
	failed        chan struct{} // closed when the supervisor gives up (MaxRetries exceeded)
	failOnce      sync.Once
	superviseOnce sync.Once // ensures only one supervisor goroutine runs

	// fatal is closed when the engine terminally rejects this worker's
	// registration (e.g. a WORKER_NAMESPACE_CONFLICT). The supervisor observes it
	// and stops reconnecting; fatalErr (guarded by mu) holds the typed cause.
	fatal     chan struct{}
	fatalOnce sync.Once
	fatalErr  error

	// lifeCtx lasts until Close: observe hooks and locally dispatched handlers run on
	// it, as remote handlers run on their connection's context.
	lifeCtx    context.Context
	lifeCancel context.CancelFunc

	// observe state, guarded by mu. epoch counts connections: functionregistered
	// confirms a function on one, and acks / reports are only sent on it.
	epoch uint64
	// engineFeatures are the last WorkerRegistered's features, kept across a
	// disconnect until the next WorkerRegistered replaces them.
	engineFeatures map[string]bool
	// registeredOnConnection: WorkerRegistered arrived on the current connection.
	registeredOnConnection bool
	// observers this worker created, replayed (same id) on every connection.
	observers   map[string]*ObserverHandle
	observerSeq uint64
	// pendingUnobserve holds unobserves of observers disposed while disconnected.
	pendingUnobserve []string
	// unsupportedWarned: the functions already reported as needing observe.v2.
	unsupportedWarned map[string]bool

	// observeMu orders observe and unobserve frames (Observe, Dispose, the replay);
	// observeQueue holds them for the pump goroutine (observePumping), in order.
	observeMu      sync.Mutex
	observeQueue   []observeFrame
	observePumping bool

	reporter *reporter
	// hookTimeout overrides observeHookTimeout (tests).
	hookTimeout time.Duration
	// standbyInterval overrides standbyRetryInterval (tests).
	standbyInterval time.Duration

	// invocationTimeout is how long Call waits when CallRequest.Timeout is zero:
	// WithInvocationTimeout, else DefaultInvocationTimeout.
	invocationTimeout time.Duration
	// telemetry is WithTelemetry; blank fields take their defaults on every
	// engine::workers::register (worker_metadata.go).
	telemetry TelemetryOptions
	// started is set once the supervisor runs (Connect or RegisterWorker).
	started atomic.Bool
	// registeredSignal is closed, and replaced, each time WorkerRegistered arrives, to
	// wake WaitUntilRegistered. Guarded by mu.
	registeredSignal chan struct{}
	// Connection-state listeners and the state changes queued for them, delivered in
	// order by one goroutine at a time (lifecycle.go). Guarded by mu.
	connStateListeners  []*connStateListener
	connStateEvents     []connStateEvent
	connStateDelivering bool
	// otel is the client's OpenTelemetry pipeline (otel_client.go).
	otel otelState
}

// Handler is a registered function's implementation. data is the raw JSON the engine
// sent. The returned value is marshaled into the InvocationResult.result. Returning an
// error produces an InvocationResult.error; if the error is an *InvocationError its code
// and stacktrace are preserved, otherwise it is reported with code "invocation_failed"
// (matching the Rust and Node SDKs).
//
// Per-invocation metadata, when the call carries any, rides on ctx rather than the
// signature (so adding it is non-breaking): read it with [MetadataFromContext].
type Handler func(ctx context.Context, data json.RawMessage) (any, error)

// RegisterFunctionOptions configures a function registration. The zero value preserves
// the default registration shape. [FunctionOptions] is the same type.
//
// ArgsFormat, Private and Observable false need an engine with observe.v2: against an
// older one the function stays unregistered, and the SDK logs UNSUPPORTED once. What
// the worker does when the function is observed is set with hooks
// ([FunctionRef.OnObserverAdded] and its siblings), not options.
type RegisterFunctionOptions struct {
	// Metadata is arbitrary JSON attached to the function registration. It is stored
	// with the function and is distinct from the per-invocation metadata passed to
	// handlers.
	Metadata json.RawMessage
	// LocalDispatch false keeps every Call of this function on the engine path; nil or
	// true lets Call run it in-process once the engine confirms it (see [Client.Call]:
	// observers see snapshots of the payload and metadata taken at call start, and a
	// handler may outlive a caller whose deadline passed).
	LocalDispatch *bool
	// Description is a human/LLM-readable summary of the function, sent as
	// "description" and surfaced by engine::functions::list and
	// engine::functions::info. Empty omits it.
	Description string
	// RequestFormat is the JSON Schema of the function's request, sent as
	// "request_format". It must be valid JSON; nil omits it. With
	// [RegisterFunctionTyped] it replaces the schema inferred from Req.
	RequestFormat json.RawMessage
	// ResponseFormat is the JSON Schema of the function's response, sent as
	// "response_format": the handler's return value, which is what observers observe.
	// It must be valid JSON; nil omits it. With [RegisterFunctionTyped] it replaces the
	// schema inferred from Resp.
	ResponseFormat json.RawMessage
	// ArgsFormat is the JSON Schema of the observers' args, sent as "args_format": the
	// engine validates every observer's args against it (INVALID_ARGS), and refuses a
	// schema that is not valid (INVALID_ARGS_FORMAT). It must be valid JSON; nil means
	// the function takes no args.
	ArgsFormat json.RawMessage
	// Private lets only this worker call the function (the engine answers anyone else
	// with FORBIDDEN); other workers may observe it, and it cannot be an observer's
	// target. A call this worker makes to it passes the input and the result by
	// reference: the handler and the hooks treat both as read-only.
	Private bool
	// Observable false refuses every observer of the function (NOT_OBSERVABLE), for a
	// function whose payloads carry credentials. nil means true.
	Observable *bool
	// Standby makes this worker wait for the function when another connection already
	// hosts its id (FUNCTION_NAMESPACE_CONFLICT): the SDK registers it again about once
	// a second until the engine accepts it or it is unregistered, and its hooks run for
	// the current observers then. It stays in the SDK.
	Standby bool
}

func resolveRegisterFunctionOptions(name, id string, opts []RegisterFunctionOptions) (RegisterFunctionOptions, error) {
	if len(opts) == 0 {
		return RegisterFunctionOptions{}, nil
	}
	if len(opts) > 1 {
		return RegisterFunctionOptions{}, fmt.Errorf("iii: %s(%q): expected at most one RegisterFunctionOptions, got %d", name, id, len(opts))
	}
	return opts[0], nil
}

type registeredFunction struct {
	message *RegisterFunctionMessage
	handler Handler
	observe *functionObservers
	standby bool
}

// invocationOutcome is delivered to a waiting Call when its InvocationResult arrives
// (or the call is cancelled on shutdown).
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

// WithInvocationTimeout sets how long [Client.Call] waits for a result when
// [CallRequest].Timeout is zero, in place of [DefaultInvocationTimeout]. It is the
// invocationTimeoutMs option of the Node SDK's registerWorker. A d of zero or less
// keeps the default.
func WithInvocationTimeout(d time.Duration) Option {
	return func(c *Client) {
		if d > 0 {
			c.invocationTimeout = d
		}
	}
}

// WithDescription sets a one-line, human/LLM-readable summary of what this
// worker does. Surfaces in engine::workers::list / engine::workers::info.
func WithDescription(description string) Option {
	return func(c *Client) { c.description = description }
}

// WithHeaders sets HTTP headers sent on the WebSocket handshake of the main connection
// and of channels created with [Client.CreateChannel]. The III_TOKEN environment
// variable, when set and not blank, adds "Authorization: Bearer <token>" unless h
// already carries an Authorization header.
func WithHeaders(h http.Header) Option {
	return func(c *Client) { c.headers = h }
}

// handshakeHeaders returns explicit plus "Authorization: Bearer $III_TOKEN" when the
// variable is set and not blank. An Authorization header already in explicit wins
// (names match case-insensitively). explicit is never modified.
func handshakeHeaders(explicit http.Header) http.Header {
	token := strings.TrimSpace(os.Getenv("III_TOKEN"))
	if token == "" {
		return explicit
	}
	for name := range explicit {
		if strings.EqualFold(name, "Authorization") {
			return explicit
		}
	}
	h := explicit.Clone()
	if h == nil {
		h = http.Header{}
	}
	h.Set("Authorization", "Bearer "+token)
	return h
}

// WithName sets the worker name reported to the engine. A non-empty
// III_WORKER_NAME takes precedence. The default is "hostname:pid".
func WithName(name string) Option {
	return func(cl *Client) { cl.name = name }
}

// WithNamespace sets the namespace this worker registers into. Takes precedence
// over the III_NAMESPACE environment variable.
//
// A namespace named and left blank is refused, not read as "no namespace": the
// client is put in a terminal failed state, so [Client.Connect] returns the
// reason and [Client.FatalError] reports it. Absent and blank ask for opposite
// things: absent asks for the engine's `default`, blank names a namespace and
// gives nothing to name it with. Read as absent, the worker registers in
// `default`, and since a worker's calls and observers follow its namespace, the
// whole project quietly serves from a place its declaration never named.
//
// To register in the engine's default namespace, do not call this.
func WithNamespace(namespace string) Option {
	return func(cl *Client) {
		cl.namespace = namespace
		cl.namespaceSet = true
	}
}

// New creates a [Client] for the engine at url (e.g. [DefaultEngineURL]). An empty url
// means [EngineURLFromEnv]: III_URL when set, else DefaultEngineURL. It does not
// connect; call [Client.Connect] to start the connection lifecycle, or use
// [RegisterWorker] to create and connect in one step.
func New(url string, opts ...Option) *Client {
	if url == "" {
		url = EngineURLFromEnv()
	}
	c := &Client{
		url:               url,
		reconnect:         DefaultReconnectConfig(),
		keepaliveInterval: 20 * time.Second,
		keepaliveTimeout:  60 * time.Second,
		name:              defaultWorkerName(),
		outbound:          make(chan []byte, 64),
		observeOut:        make(chan observeFrame, 64),
		state:             StateDisconnected,
		functions:         map[string]registeredFunction{},
		functionHooks:     map[string]*observerHooks{},
		hookIDWarned:      map[string]bool{},
		pending:           map[uuid.UUID]chan invocationOutcome{},
		shutdown:          make(chan struct{}),
		connected:         make(chan struct{}),
		failed:            make(chan struct{}),
		fatal:             make(chan struct{}),

		engineFeatures:    map[string]bool{},
		observers:         map[string]*ObserverHandle{},
		unsupportedWarned: map[string]bool{},

		invocationTimeout: DefaultInvocationTimeout,
		shutDone:          make(chan struct{}),
		registeredSignal:  make(chan struct{}),
	}
	c.lifeCtx, c.lifeCancel = context.WithCancel(context.Background())
	c.reporter = newReporter(c, defaultReporterLimits)
	for _, opt := range opts {
		opt(c)
	}
	// The supervisor owns the managed identity. Apply it after options so a
	// name embedded in worker code cannot hide the process from the supervisor.
	if managedName := os.Getenv("III_WORKER_NAME"); managedName != "" {
		c.name = managedName
	}
	if c.namespaceSet {
		// Refused rather than read as absent, and refused here so nothing is
		// dialled: the mistake is in the program text, and every call and
		// observer this worker makes would inherit it.
		if strings.TrimSpace(c.namespace) == "" {
			c.failFatally(fmt.Errorf(
				"iii: namespace is empty: WithNamespace was called with %q. "+
					"Give it a name, or do not call it to register in the engine's default namespace",
				c.namespace,
			))
		}
	} else {
		// III_NAMESPACE is left alone when blank. `FOO=` is how a shell says
		// "not set" (`III_NAMESPACE=${NS}` with NS unset produces exactly
		// that), so reading it as absent is what the caller meant. Only the
		// option is a mistake: nobody writes a namespace parameter and passes
		// nothing on purpose. Mirrors the other SDKs.
		if managed := os.Getenv("III_NAMESPACE"); strings.TrimSpace(managed) != "" {
			c.namespace = managed
		}
	}
	return c
}

// RegisterWorker creates a [Client] for the engine at url and starts the connection
// lifecycle in the background, returning immediately. An empty url means
// [EngineURLFromEnv], as with [New]. It is the idiomatic entry point,
// matching registerWorker in the Node SDK and register_worker in the Rust SDK. Register
// functions and observers on the returned client; they are (re)sent on each connection.
// Call [Client.Close] to stop.
//
// To wait for the first connection (or fail fast on a bad URL), call [Client.Connect]
// instead of (or after) RegisterWorker; it blocks until connected, ctx is done, or the
// reconnect budget is exhausted. Use [New] if you want to build a client without starting
// the connection.
func RegisterWorker(url string, opts ...Option) *Client {
	c := New(url, opts...)
	c.startSupervisor()
	return c
}

func defaultWorkerName() string {
	host, err := os.Hostname()
	if err != nil || host == "" {
		host = "unknown"
	}
	return fmt.Sprintf("%s:%d", host, os.Getpid())
}

// Address returns the engine WebSocket base URL this client dials (e.g.
// "ws://127.0.0.1:49134"): the url given to [New] or [RegisterWorker], else
// [EngineURLFromEnv]. Channel readers/writers build their own URLs from it.
func (c *Client) Address() string { return c.url }

// ConnectionState returns the current state of the connection to the engine. It
// mirrors getConnectionState in the Node SDK; [Client.OnConnectionStateChange] reports
// every change.
func (c *Client) ConnectionState() ConnectionState {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.state
}

// State returns the current connection state.
//
// Deprecated: use [Client.ConnectionState].
func (c *Client) State() ConnectionState { return c.ConnectionState() }

// WorkerID returns the worker id the engine assigned with its last WorkerRegistered, or
// "" before the first one. The id is kept across a disconnect (the next connection
// presents it to reattach) until the engine assigns the next one. It mirrors worker_id
// in the Python SDK.
func (c *Client) WorkerID() string {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.workerID
}

func (c *Client) setState(s ConnectionState) {
	c.mu.Lock()
	c.setStateLocked(s)
	c.mu.Unlock()
}

// RegisterFunction registers a function this worker can handle and returns its handle;
// [FunctionRef.Unregister] removes it. It may be called before or after Connect; the
// registration is sent on the next (re)connect. As in the Node, Python and Rust SDKs, an
// empty id is refused ("id is required") and so is an id this client already registered
// ("function id already registered"): to replace a function, unregister it first. Pass a
// single [RegisterFunctionOptions] value for a description, request and response
// formats, metadata, args_format, private, observable, standby or the local dispatch
// setting.
//
// The handler's return value is what observers observe. Every call of the function,
// local or routed, runs the handler and then delivers to the observers its deliver hook
// picks ([Client.OnObserverDeliverFor]), or to every observer whose on and filters
// match. Set hooks by id before registering: the SDK acks no observer before
// RegisterFunction returns, but a hook set on the returned handle can miss observers
// already accepted, and is caught up for them.
//
// Handlers read the optional per-invocation metadata sidecar from their ctx with
// [MetadataFromContext].
func (c *Client) RegisterFunction(id string, handler Handler, opts ...RegisterFunctionOptions) (*FunctionRef, error) {
	if handler == nil {
		return nil, fmt.Errorf("iii: RegisterFunction(%q): handler is nil", id)
	}
	return c.registerFunction("RegisterFunction", id, handler, opts)
}

func (c *Client) registerFunction(name, id string, handler Handler, opts []RegisterFunctionOptions) (*FunctionRef, error) {
	cfg, err := resolveRegisterFunctionOptions(name, id, opts)
	if err != nil {
		return nil, err
	}
	msg, err := registerFunctionMessage(name, id, cfg)
	if err != nil {
		return nil, err
	}
	return c.storeFunction(name, id, msg, handler, cfg)
}

// registerFunctionMessage builds the registerfunction frame for id from cfg: the
// description, the request, response and args formats (each checked for valid JSON and
// copied), the metadata, and private and observable, as the Node SDK spreads its
// RegisterFunctionOptions into the message. Standby and the hooks stay in the SDK.
func registerFunctionMessage(name, id string, cfg RegisterFunctionOptions) (*RegisterFunctionMessage, error) {
	for _, format := range []struct {
		field string
		data  json.RawMessage
	}{
		{"RequestFormat", cfg.RequestFormat},
		{"ResponseFormat", cfg.ResponseFormat},
		{"ArgsFormat", cfg.ArgsFormat},
	} {
		if len(format.data) > 0 && !json.Valid(format.data) {
			return nil, fmt.Errorf("iii: %s(%q): %s must contain valid JSON", name, id, format.field)
		}
	}
	msg := &RegisterFunctionMessage{
		ID:             id,
		RequestFormat:  bytes.Clone(cfg.RequestFormat),
		ResponseFormat: bytes.Clone(cfg.ResponseFormat),
		ArgsFormat:     bytes.Clone(cfg.ArgsFormat),
		Metadata:       cfg.Metadata,
		Private:        cfg.Private,
	}
	if cfg.Description != "" {
		description := cfg.Description
		msg.Description = &description
	}
	if cfg.Observable != nil && !*cfg.Observable {
		observable := false
		msg.Observable = &observable
	}
	return msg, nil
}

// storeFunction validates the id, records the function with its observe state and
// sends the registration. name is the public method, for errors. Like the Node, Python
// and Rust SDKs it refuses an empty id and an id already registered on this client (the
// check and the insert happen under one lock).
func (c *Client) storeFunction(name, id string, msg *RegisterFunctionMessage, handler Handler, cfg RegisterFunctionOptions) (*FunctionRef, error) {
	if strings.TrimSpace(id) == "" {
		return nil, fmt.Errorf("iii: %s: id is required", name)
	}
	if _, err := prepareJSON(msg); err != nil {
		return nil, err
	}
	// A function without a local handler (RegisterHTTPFunction) never runs in-process.
	localDispatch := handler != nil && (cfg.LocalDispatch == nil || *cfg.LocalDispatch)
	observe := newFunctionObservers(c, id, localDispatch, msg.Private)
	c.mu.Lock()
	if _, exists := c.functions[id]; exists {
		c.mu.Unlock()
		return nil, fmt.Errorf("iii: %s(%q): function id already registered", name, id)
	}
	fn := registeredFunction{message: msg, handler: handler, observe: observe, standby: cfg.Standby}
	c.functions[id] = fn
	if deliver := c.deliverHookLocked(id); deliver != nil {
		// before the registration is sent: no call reaches the function without it
		observe.setDeliver(deliver)
	}
	c.mu.Unlock()
	// No observer of the function is acked before this call returns: its hook queue
	// waits for the return.
	registered := make(chan struct{})
	defer close(registered)
	observe.enqueue(func() { <-registered })
	c.sendFunction(fn)
	return &FunctionRef{ID: id, client: c, observe: observe}, nil
}

// needsV2Locked: the function declares args_format, private or observable false, or
// has a deliver hook, so it is registered only with an engine that advertised
// observe.v2. The caller holds mu.
func (c *Client) needsV2Locked(fn registeredFunction) bool {
	msg := fn.message
	return len(msg.ArgsFormat) > 0 || msg.Private || (msg.Observable != nil && !*msg.Observable) ||
		c.deliverHookLocked(msg.ID) != nil
}

// sendFunction sends a registration on the live connection, or leaves it to the next
// connection's replay. A function that needs observe.v2 waits for WorkerRegistered, and
// stays unregistered (UNSUPPORTED, logged once) when the engine lacks observe.v2.
func (c *Client) sendFunction(fn registeredFunction) {
	c.mu.Lock()
	if c.state != StateConnected {
		c.mu.Unlock()
		return
	}
	needsV2 := c.needsV2Locked(fn)
	if needsV2 && !c.registeredOnConnection {
		c.mu.Unlock()
		return // onWorkerRegistered sends it
	}
	unsupported := needsV2 && !c.engineFeatures[FeatureObserveV2]
	warn := unsupported && c.markUnsupportedLocked(fn)
	c.mu.Unlock()
	if warn {
		logUnsupported(fn.message.ID)
	}
	if !unsupported {
		// registration frames are not buffered: the replay on reconnect covers them
		c.sendRegistration(fn.message)
	}
}

// markUnsupportedLocked records that fn stays unregistered on the current connection,
// and reports whether UNSUPPORTED is still to be logged for it. The caller holds mu.
func (c *Client) markUnsupportedLocked(fn registeredFunction) bool {
	fn.observe.mu.Lock()
	fn.observe.unsupportedEpoch = int64(c.epoch)
	fn.observe.mu.Unlock()
	if c.unsupportedWarned[fn.message.ID] {
		return false
	}
	c.unsupportedWarned[fn.message.ID] = true
	return true
}

func logUnsupported(functionID string) {
	log.Printf("iii: UNSUPPORTED: function %q declares args_format, private or observable: false, or has a deliver hook, "+
		"which need an engine with observe.v2; it stays unregistered until the worker connects to one", functionID)
}

// reconsiderRegistration follows a deliver hook set or removed for functionID on a
// connection to an engine without observe.v2: a registered function that now needs it
// is unregistered (UNSUPPORTED), and one left unregistered that no longer needs it is
// registered.
func (c *Client) reconsiderRegistration(functionID string) {
	c.mu.Lock()
	fn, ok := c.functions[functionID]
	if !ok || c.state != StateConnected || !c.registeredOnConnection || c.engineFeatures[FeatureObserveV2] {
		c.mu.Unlock()
		return
	}
	needsV2 := c.needsV2Locked(fn)
	fn.observe.mu.Lock()
	unsupportedHere := fn.observe.unsupportedEpoch == int64(c.epoch)
	fn.observe.mu.Unlock()
	warn := false
	switch {
	case needsV2 && !unsupportedHere:
		warn = c.markUnsupportedLocked(fn)
	case !needsV2 && unsupportedHere:
		fn.observe.mu.Lock()
		fn.observe.unsupportedEpoch = -1
		fn.observe.mu.Unlock()
	}
	c.mu.Unlock()
	switch {
	case needsV2 && !unsupportedHere:
		if warn {
			logUnsupported(functionID)
		}
		fn.observe.release()
		c.sendUnregistration(&UnregisterFunctionMessage{ID: functionID})
	case !needsV2 && unsupportedHere:
		c.sendRegistration(fn.message)
	}
}

// refuseBlankCallNamespace refuses a namespace that was named and left blank on
// one call or one observer.
//
// Named and left empty asks for the opposite of what absent asks for, and the
// two are only ever confused by accident: Go dropped the empty string with
// `omitempty` and sent the call to the engine's default, while Node and Rust
// forwarded it verbatim and Python replaced it with the worker's. One mistake,
// four behaviours.
//
// Absent is not blank: it inherits this worker's namespace.
func refuseBlankCallNamespace(namespace, source string) error {
	if namespace != "" && strings.TrimSpace(namespace) == "" {
		return fmt.Errorf(
			"iii: namespace is empty: %s was set to %q. Give it a name, or leave it unset "+
				"to stay in this worker's namespace", source, namespace)
	}
	return nil
}

// invocationNamespace resolves one function invocation. An explicit namespace
// always wins. Otherwise engine-owned builtins (engine::*, and configuration::*
// from the engine-owned configuration worker) stay in the engine's default
// namespace, and every other call inherits the worker's.
func invocationNamespace(functionID, explicit, worker string) string {
	switch {
	case explicit != "":
		return explicit
	case strings.HasPrefix(functionID, "engine::") || strings.HasPrefix(functionID, "configuration::"):
		return "default"
	default:
		return worker
	}
}

// CallRequest is the input to [Client.Call]. The Action field selects the delivery
// semantics: nil/await (default) waits for the result; [VoidAction] is fire-and-forget;
// [EnqueueAction] routes through a named queue and awaits its receipt.
type CallRequest struct {
	// FunctionID is the function to invoke (e.g. an EngineFunctions constant). Empty
	// with Observer.
	FunctionID string
	// Observer calls through an observer of a function this worker hosts: the engine
	// runs the observer's target with Payload as given, after checking that this
	// worker hosts the observer's function (NOT_FUNCTION_OWNER) and that the observer
	// exists and is active (OBSERVER_NOT_FOUND, OBSERVER_NOT_ACTIVE), and applies its
	// policy (max_fires, expiry, max_in_flight: OVERFLOW), metadata sidecar and loop
	// guard. The call goes out on the current connection, after the acks already sent
	// on it; FunctionID, Namespace and an enqueue action do not apply. A deliver hook
	// that calls targets itself applies the observer's filters with [Observer.Matches].
	Observer *Observer
	// Payload is the JSON input of the function (payload in the Node, Python and Rust
	// SDKs). Nil is sent as {}.
	Payload json.RawMessage
	// Data is the JSON input of the function, used only when Payload is nil.
	//
	// Deprecated: use Payload.
	Data json.RawMessage
	// Metadata is optional per-invocation metadata (arbitrary JSON) sent alongside the
	// payload on the wire's metadata channel, not folded into it. nil is omitted.
	Metadata json.RawMessage
	// Action selects void/enqueue semantics; nil means the default await path.
	Action *CallAction
	// Timeout bounds an await/enqueue call. Zero means the client's invocation timeout:
	// [WithInvocationTimeout], else [DefaultInvocationTimeout].
	Timeout time.Duration
	// Namespace routes the invocation to a specific namespace. Empty means this
	// worker's namespace, except engine::* and configuration::* builtins, which
	// resolve in the engine's default namespace.
	Namespace string
	// Dispatch selects the path to a function this worker hosts: the zero value (or
	// [DispatchAuto]) runs it in-process once the engine confirmed it for local
	// dispatch; [DispatchRemote] always goes through the engine.
	Dispatch Dispatch
}

// TriggerRequest is [CallRequest] under its 0.25 name.
//
// Deprecated: use [CallRequest].
type TriggerRequest = CallRequest

// payload is the request's input: Payload, else the deprecated Data.
func (req CallRequest) payload() json.RawMessage {
	if req.Payload != nil {
		return req.Payload
	}
	return req.Data
}

// maxLocalDepth bounds nested local calls: deeper ones go through the engine, so
// recursive self-calls cannot pile up in-process (SDK-PARITY decision 20).
const maxLocalDepth = 64

// Call invokes a function. With the default (nil) or an enqueue action it waits for the
// matching invocation result and returns it, mapping a remote error to
// [InvocationError] and a missed deadline to [ErrTimeout]. With a [VoidAction] it is
// fire-and-forget and returns immediately with a nil result. With
// [CallRequest].Observer it calls through that observer.
//
// ctx bounds the wait independently of [CallRequest.Timeout]: if ctx is cancelled
// first, its error is returned and the pending entry is reclaimed. Inside a handler or
// a deliver hook, pass its ctx: the call then carries that invocation as its caller
// (and the observe depth), which observers' filters and the engine's loop guard read.
//
// A function this worker hosts runs in-process, without touching the wire, once the
// engine confirmed it for local dispatch, unless the action is enqueue or Dispatch is
// [DispatchRemote]. The handler gets its own copy of the payload and the caller its own
// encoding of the result, except for a [RegisterFunctionOptions].Private function,
// whose input and result pass by reference; errors and timeouts look as on the remote
// path. The call is delivered to the function's observers as a routed one is, from
// snapshots of the payload and metadata taken when it started. The handler runs on a
// goroutine of its own, so the caller always returns on its Timeout or ctx deadline,
// even when the handler blocks and ignores its ctx; the handler then runs to completion
// (as a remote one would on its worker), its result is discarded, and the call is
// still counted and delivered. A handler that can block for long should honor its ctx
// (cancelled on Close), or opt out with [RegisterFunctionOptions].LocalDispatch.
func (c *Client) Call(ctx context.Context, req CallRequest) (json.RawMessage, error) {
	if req.Observer != nil {
		return c.callThroughObserver(ctx, req)
	}
	if err := refuseBlankCallNamespace(req.Namespace, "CallRequest.Namespace"); err != nil {
		return nil, err
	}
	switch req.Dispatch {
	case "", DispatchAuto, DispatchRemote:
	default:
		return nil, fmt.Errorf("iii: CallRequest.Dispatch must be %q or %q, got %q", DispatchAuto, DispatchRemote, req.Dispatch)
	}
	namespace := invocationNamespace(req.FunctionID, req.Namespace, c.namespace)
	if req.Dispatch != DispatchRemote && (req.Action == nil || req.Action.Type != "enqueue") {
		if parent := invocationFrom(ctx); parent == nil || parent.localDepth < maxLocalDepth {
			if fn, ok := c.localTarget(req.FunctionID, namespace); ok {
				return c.invokeLocal(ctx, fn, req)
			}
		}
	}
	return c.invokeRemote(ctx, req, namespace, nil)
}

// Trigger is [Client.Call] under its 0.25 name.
//
// Deprecated: use [Client.Call].
func (c *Client) Trigger(ctx context.Context, req TriggerRequest) (json.RawMessage, error) {
	return c.Call(ctx, req)
}

// callThroughObserver sends an invokefunction to the observer's target with
// activation.observer_id and no namespace: the engine routes it to the target it
// stored for the observer and checks the rest.
func (c *Client) callThroughObserver(ctx context.Context, req CallRequest) (json.RawMessage, error) {
	obs := req.Observer
	if obs.owner == nil {
		return nil, errors.New("iii: CallRequest.Observer is not an observer handed out by the client")
	}
	if req.FunctionID != "" || req.Namespace != "" {
		return nil, fmt.Errorf("iii: CallRequest.FunctionID and Namespace must be empty with Observer: the call goes to observer %s's target", obs.id)
	}
	if req.Action != nil && req.Action.Type == "enqueue" {
		return nil, fmt.Errorf("iii: a call through observer %s cannot be enqueued", obs.id)
	}
	req.FunctionID = obs.target
	return c.invokeRemote(ctx, req, "", &Activation{ObserverID: obs.id})
}

// invokeRemote is the routed path: an InvokeFunction through the engine. Inside a
// handler (ctx), and when the engine supports observe, it carries caller and
// observe_depth. activation marks a call through an observer: it goes on the current
// connection's own channel, behind the observer acks sent on it, and fails with
// ErrNotConnected without one.
func (c *Client) invokeRemote(ctx context.Context, req CallRequest, namespace string, activation *Activation) (json.RawMessage, error) {
	data := req.payload()
	if data == nil {
		data = json.RawMessage("{}")
	}
	tc := extractTraceContext(ctx)
	var caller *Caller
	var depth uint32
	if inv := invocationFrom(ctx); inv != nil && c.engineSupportsObserve() {
		caller, depth = callerFor(inv), inv.observeDepth
	}
	send := func(frame []byte) bool {
		c.enqueueOutbound(frame)
		return true
	}
	if activation != nil {
		epoch := c.currentEpoch()
		send = func(frame []byte) bool { return c.sendOnEpoch(epoch, frame) }
	}

	// Void: fire-and-forget, no invocation_id, no pending entry, return immediately.
	if req.Action != nil && req.Action.Type == "void" {
		frame, err := prepareJSON(&InvokeFunctionMessage{
			FunctionID:   req.FunctionID,
			Data:         data,
			Metadata:     req.Metadata,
			Action:       req.Action,
			Traceparent:  tc.traceparent,
			Baggage:      tc.baggage,
			Namespace:    namespace,
			Caller:       caller,
			ObserveDepth: depth,
			Activation:   activation,
		})
		if err != nil {
			return nil, err
		}
		if !send(frame) {
			return nil, ErrNotConnected
		}
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

	frame, err := prepareJSON(&InvokeFunctionMessage{
		InvocationID: &id,
		FunctionID:   req.FunctionID,
		Data:         data,
		Metadata:     req.Metadata,
		Action:       req.Action,
		Traceparent:  tc.traceparent,
		Baggage:      tc.baggage,
		Namespace:    namespace,
		Caller:       caller,
		ObserveDepth: depth,
		Activation:   activation,
	})
	if err != nil {
		c.clearPending(id)
		return nil, err
	}
	if !send(frame) {
		c.clearPending(id)
		return nil, ErrNotConnected
	}

	timer := time.NewTimer(c.callTimeout(req.Timeout))
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

// callTimeout is how long a Call waits: its own Timeout when positive, else the
// client's invocation timeout (WithInvocationTimeout or DefaultInvocationTimeout).
func (c *Client) callTimeout(requested time.Duration) time.Duration {
	if requested > 0 {
		return requested
	}
	return c.invocationTimeout
}

// sendRegistration sends a registration frame if connected, and drops it otherwise:
// registrations are replayed from the in-memory registries on the next connect, so
// buffering them too would double-send. Mirrors the Node SDK's skipIfClosed=true.
func (c *Client) sendRegistration(msg any) {
	frame, err := prepareJSON(msg)
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
	var err error
	frame, err = prepareFrame(frame)
	if err != nil {
		c.rejectFrame(frame, err)
		return
	}
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

// startSupervisor launches the connect/reconnect loop exactly once, no matter how many
// times Connect (or RegisterWorker) is called, so there is never more than one goroutine
// driving the socket.
func (c *Client) startSupervisor() {
	c.superviseOnce.Do(func() {
		c.started.Store(true)
		c.startTelemetry() // otel_client.go: OpenTelemetry starts with the lifecycle
		c.reporter.start()
		// Lets `iii ps` find this worker until Close ends the supervisor.
		remove := announceProcess(c.url, c.name, "")
		go func() {
			defer remove()
			c.supervise()
		}()
	})
}

// Connect starts the connection lifecycle (if not already started) and blocks until the
// first connection is established, ctx is cancelled, or the reconnect budget is
// exhausted. The supervisor goroutine then keeps the connection alive (reconnecting with
// backoff) until Close. Connect is safe to call multiple times and from multiple
// goroutines; only one supervisor ever runs.
func (c *Client) Connect(ctx context.Context) error {
	// A client that is already terminally failed (a namespace named and left
	// blank, or a rejection from an earlier attempt) has its reason on hand.
	// Reporting retry exhaustion instead would describe the symptom.
	if err := c.FatalError(); err != nil {
		return err
	}

	c.startSupervisor()

	select {
	case <-c.connected:
		return nil
	case <-c.fatal:
		if err := c.FatalError(); err != nil {
			return err
		}
		return ErrNotConnected
	case <-c.failed:
		// The supervisor gave up (MaxRetries exceeded) before ever connecting.
		return fmt.Errorf("iii: connection failed after exhausting retries: %w", ErrNotConnected)
	case <-ctx.Done():
		return ctx.Err()
	case <-c.shutdown:
		return ErrNotConnected
	}
}

// supervise owns the connect/reconnect loop. Each iteration dials, runs the connection
// until it drops, then backs off before retrying, until Close or MaxRetries.
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

		registered, err := c.runConnection()
		if err == nil {
			// Clean shutdown of the connection (Close was called).
			return
		}

		// A terminal registration rejection stops the supervisor: reconnecting
		// would only earn the same rejection. State/failed are already set by the
		// handler.
		select {
		case <-c.fatal:
			return
		default:
		}

		// Once the engine has accepted this worker, the connection was healthy.
		// Back off only across consecutive failures to establish a healthy session.
		if registered {
			attempt = 0
		}
		attempt++
		if c.reconnect.MaxRetries != -1 && attempt > c.reconnect.MaxRetries {
			c.setState(StateFailed)
			// Unblock any Connect callers waiting on the first connection.
			c.failOnce.Do(func() { close(c.failed) })
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
// called, and returns nil only on a clean Close. The bool reports whether the engine
// registered the worker on this connection; a returned error means "reconnect".
func (c *Client) runConnection() (bool, error) {
	// A connection-scoped context cancelled when this connection ends, so the writer
	// goroutine and any per-connection work stop together.
	connCtx, cancel := context.WithCancel(context.Background())
	defer cancel()

	conn, _, err := websocket.Dial(connCtx, c.url, &websocket.DialOptions{HTTPHeader: handshakeHeaders(c.headers)})
	if err != nil {
		// Treat dial failure as a reconnectable error unless we're shutting down.
		select {
		case <-c.shutdown:
			return false, nil
		default:
			return false, err
		}
	}
	// Bound inbound main-protocol messages consistently with the engine message limit.
	conn.SetReadLimit(maxJSONMessageBytes)

	// Reconnect: present the previous engine-assigned identity as the very
	// first frame (written directly here, before the writer goroutine,
	// StateConnected, or onConnect expose this socket to any other traffic),
	// so the engine retires the old connection before anything can hit its
	// stale registrations. The token proves we ARE that worker (ids alone
	// are publicly listable).
	c.mu.Lock()
	prevWorkerID, prevToken := c.workerID, c.reattachToken
	c.mu.Unlock()
	if prevWorkerID != "" {
		frame, merr := prepareJSON(&ReattachMessage{
			PreviousWorkerID: prevWorkerID,
			ReattachToken:    prevToken,
		})
		if merr != nil {
			log.Printf("iii: reattach JSON rejected: %v", merr)
		}
		if merr == nil {
			if werr := c.writeJSON(connCtx, conn, frame); werr != nil {
				_ = conn.CloseNow()
				select {
				case <-c.shutdown:
					return false, nil
				default:
					return false, werr
				}
			}
		}
	}

	// A reply channel owned by THIS connection. Connection-scoped replies go here so they
	// can never be drained by a later connection's writer (iii-hq/iii#1749).
	reply := make(chan []byte, 64)
	replyDone := make(chan struct{})
	replyQueued := new(atomic.Int64)
	replySenders := new(sync.WaitGroup)
	c.mu.Lock()
	c.epoch++
	epoch := c.epoch
	c.registeredOnConnection = false
	c.mu.Unlock()
	c.replyMu.Lock()
	c.reply = reply
	c.replyEpoch = epoch
	c.replyDone = replyDone
	c.replyQueued = replyQueued
	c.replySenders = replySenders
	c.replyMu.Unlock()

	c.mu.Lock()
	c.conn = conn
	c.connCancel = cancel
	c.setStateLocked(StateConnected)
	c.mu.Unlock()

	// Start the single writer for this connection. It drains the shared outbound channel
	// and this connection's own reply channel.
	writerDone := make(chan struct{})
	c.writerWG.Add(1)
	go c.writeLoop(connCtx, conn, reply, replyQueued, epoch, writerDone)
	keepaliveDone := make(chan struct{})
	go c.keepaliveLoop(connCtx, conn, keepaliveDone)

	// Signal the first successful connection to Connect's caller.
	c.connOnce.Do(func() { close(c.connected) })

	// On (re)connect, replay registrations, flush the offline buffer, then register
	// worker metadata, in that exact order (matching Node and Rust).
	c.onConnect()

	// Read loop runs until the socket errors or closes.
	readErr := c.readLoop(connCtx, conn)

	// Tear down this connection: stop the writer, close the socket, clear conn. Detach the
	// reply channel first so any reply enqueued during teardown is dropped (it has no live
	// socket to go to) rather than lingering for the next connection.
	c.detachReply()
	close(replyDone)
	cancel()
	<-writerDone
	<-keepaliveDone
	// Report frames handed to this connection's writer and never written are counted
	// INACTIVE (no new send can start on the detached channel).
	c.discardUnwritten(reply, replySenders)
	_ = conn.CloseNow()
	c.mu.Lock()
	c.conn = nil
	c.connCancel = nil
	registered := c.registeredOnConnection
	// Local dispatch and the compiled observer sets survive; what was sent on this
	// connection is replayed on the next one.
	c.registeredOnConnection = false
	if c.state == StateConnected {
		c.setStateLocked(StateDisconnected)
	}
	c.mu.Unlock()

	c.stopMetricsReporting() // otel_client.go: the gauges stop with the connection

	// If Close was called, report a clean stop; otherwise ask for a reconnect.
	select {
	case <-c.shutdown:
		return registered, nil
	default:
		if readErr == nil {
			readErr = fmt.Errorf("iii: connection closed")
		}
		return registered, readErr
	}
}

// keepaliveLoop prevents an otherwise idle worker connection from being retired by
// proxies and detects half-open sockets. Ping waits for the read loop to observe the
// matching pong; a timeout closes the socket so the supervisor reconnects.
func (c *Client) keepaliveLoop(ctx context.Context, conn *websocket.Conn, done chan<- struct{}) {
	defer close(done)
	ticker := time.NewTicker(c.keepaliveInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			pingCtx, cancel := context.WithTimeout(ctx, c.keepaliveTimeout)
			err := conn.Ping(pingCtx)
			cancel()
			if err != nil {
				if ctx.Err() == nil {
					_ = conn.CloseNow()
				}
				return
			}
		}
	}
}

// onConnect replays the in-memory registry and flushes buffered frames in the order
// the engine expects: functions, offline buffer, then worker metadata last. Functions
// that need observe.v2 wait for WorkerRegistered (onWorkerRegistered), which says
// whether this engine has it.
func (c *Client) onConnect() {
	c.mu.Lock()
	var frames [][]byte
	for _, fn := range c.functions {
		if c.needsV2Locked(fn) {
			continue
		}
		if f, err := prepareJSON(fn.message); err == nil {
			frames = append(frames, f)
		}
	}
	// Drain the offline buffer after registrations. Observe-family frames wait for
	// WorkerRegistered, which says whether this engine supports observe: observes are
	// replayed from the registry then, and unobserves sent only if it does.
	for _, f := range c.offline {
		if family, unobserveID := isObserveFrame(f); family {
			if unobserveID != "" {
				c.pendingUnobserve = append(c.pendingUnobserve, unobserveID)
			}
			continue
		}
		frames = append(frames, f)
	}
	c.offline = nil
	c.mu.Unlock()

	for _, f := range frames {
		checked, err := prepareFrame(f)
		if err != nil {
			c.rejectFrame(f, err)
			continue
		}
		f = checked
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
		Runtime:     runtimeTag,
		Version:     sdkVersion,
		Name:        c.name,
		Description: c.description,
		Namespace:   c.namespace,
		OS:          fmt.Sprintf("%s %s", runtime.GOOS, runtime.GOARCH),
		PID:         &pid,
		Isolation:   isolationFromEnv(),
		Telemetry:   c.resolveTelemetry(),
		// Opts this connection into the observe messages (CONTRACT D2).
		Features: sdkFeatures,
	}
	data, err := json.Marshal(meta)
	if err != nil {
		return
	}
	frame, err := prepareJSON(&InvokeFunctionMessage{
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

// workerMetadata is the payload sent to engine::workers::register, built like the Node
// SDK's registerWorkerMetadata (sdk/packages/node/iii/src/iii.ts). Optional fields use
// omitempty to match the engine's skip_serializing_if discipline, except isolation,
// which Node always sends (null when III_ISOLATION is unset).
type workerMetadata struct {
	Runtime     string          `json:"runtime"`
	Version     string          `json:"version"`
	Name        string          `json:"name"`
	Description string          `json:"description,omitempty"`
	Namespace   string          `json:"namespace,omitempty"`
	OS          string          `json:"os"`
	PID         *int            `json:"pid,omitempty"`
	Isolation   *string         `json:"isolation"`
	Telemetry   workerTelemetry `json:"telemetry"`
	Features    []string        `json:"features,omitempty"`
}

// sdkVersion is reported in the worker metadata. Kept as a const for v1; a release
// process can wire this to the module version later.
const sdkVersion = "0.25.0"

// writeLoop is the single writer for one connection. It drains the shared outbound
// channel (connection-agnostic frames) and this connection's own reply channel
// (connection-scoped replies) until the connection context is cancelled. reply is passed
// in (not read from c.reply) so this loop only ever touches its own connection's replies,
// even if a reconnect swaps c.reply. Routing every send through here keeps the socket
// single-writer, as coder/websocket requires.
func (c *Client) writeLoop(ctx context.Context, conn *websocket.Conn, reply <-chan []byte, queued *atomic.Int64, epoch uint64, done chan struct{}) {
	defer c.writerWG.Done()
	defer close(done)
	// write reports false when the connection is over (an oversized frame is rejected
	// and the loop goes on).
	write := func(frame []byte) bool {
		if err := c.writeJSON(ctx, conn, frame); err != nil {
			var sizeErr *InvocationError
			if errors.As(err, &sizeErr) && sizeErr.Code == "payload_too_large" {
				c.rejectFrame(frame, err)
				return true
			}
			return false // write failure ends the connection; supervisor reconnects
		}
		return true
	}
	for {
		select {
		case <-ctx.Done():
			return
		case <-c.shutdown:
			return
		case frame := <-c.outbound:
			if !write(frame) {
				return
			}
		case frame := <-reply:
			ok := write(frame)
			if queued != nil && bytes.HasPrefix(frame, reportFrameHead) {
				queued.Add(-int64(len(frame)))
				c.reporter.written()
				if !ok {
					c.reporter.countUnsent(frame)
				}
			}
			if !ok {
				return
			}
		case f := <-c.observeOut:
			// What outbound already holds (the registration replay) goes first: local
			// targets exist before the observers that name them.
			for n := len(c.outbound); n > 0; n-- {
				var frame []byte
				select {
				case frame = <-c.outbound:
				default:
				}
				if frame == nil {
					break
				}
				if !write(frame) {
					c.missedObserveFrame(f)
					return
				}
			}
			if f.epoch != epoch {
				c.missedObserveFrame(f) // meant for a connection that is gone
				continue
			}
			if !write(f.data) {
				c.missedObserveFrame(f)
				return
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
		go c.handleInvoke(ctx, c.currentEpoch(), dec.InvokeFunction)
	case MsgInvocationResult:
		c.handleInvocationResult(dec.InvocationResult)
	case MsgPing:
		if frame, err := prepareJSON(&PongMessage{}); err == nil {
			c.enqueueOutboundDirect(frame)
		}
	case MsgWorkerRegistered:
		// Remember the engine-assigned identity so the next reconnect can present
		// it via Reattach, and the engine's features (observe.v1, observe.v2).
		if dec.WorkerRegistered != nil {
			c.onWorkerRegistered(dec.WorkerRegistered)
			c.signalRegistered()
			c.startMetricsReporting() // otel_client.go: worker gauges once the worker id is known
		}
	case MsgFunctionRegistered, MsgObserverAdded, MsgObserverRemoved, MsgObserveResult, MsgObserverStateChanged:
		// Applied in order on the read loop; observe hooks run on per-function queues.
		c.dispatchObserve(dec)
	case MsgRegistrationRejected:
		c.handleRegistrationRejected(dec.RegistrationRejected)
	default:
		// Unknown/unhandled inbound type; ignore.
	}
}

// handleRegistrationRejected processes an engine registration rejection. A rejection
// of one function (FUNCTION_NAMESPACE_CONFLICT, INVALID_ARGS_FORMAT, UNSUPPORTED)
// costs that function and leaves the connection open, so it is logged and the worker
// keeps serving; a Standby function waits for its id instead. Any other code (notably
// WORKER_NAMESPACE_CONFLICT) is terminal: record the typed error, fail pending
// invocations, and stop the supervisor so it does not reconnect into the same
// rejection forever (the default MaxRetries of -1 would otherwise loop).
func (c *Client) handleRegistrationRejected(msg *RegistrationRejectedMessage) {
	if msg == nil {
		return
	}
	if msg.FunctionID != "" {
		c.rejectFunction(msg)
		return
	}

	err := &RegistrationRejectedError{
		Code:          msg.Code,
		Namespace:     msg.Namespace,
		WorkerName:    msg.WorkerName,
		FunctionID:    msg.FunctionID,
		OwnerWorkerID: msg.OwnerWorkerID,
	}
	log.Printf("iii: registration rejected (%s): worker %q in namespace %q already owned by %s; fatal, not reconnecting",
		msg.Code, msg.WorkerName, msg.Namespace, msg.OwnerWorkerID)

	c.mu.Lock()
	c.fatalErr = err
	c.setStateLocked(StateFailed)
	conn := c.conn
	for id, ch := range c.pending {
		ch <- invocationOutcome{err: err}
		delete(c.pending, id)
	}
	c.mu.Unlock()

	// Signal the supervisor to give up, and unblock any Connect caller.
	c.fatalOnce.Do(func() { close(c.fatal) })
	c.failOnce.Do(func() { close(c.failed) })

	// Drop the socket; runConnection returns and the supervisor sees `fatal`.
	if conn != nil {
		_ = conn.CloseNow()
	}
}

// standbyRetryInterval is how often a Standby function is registered again while
// another connection hosts its id.
const standbyRetryInterval = time.Second

// rejectFunction handles the rejection of one function's registration: the engine
// refused it, so no local dispatch, no observers for it here, and its held reports can
// never be confirmed. A Standby function refused with FUNCTION_NAMESPACE_CONFLICT is
// registered again about once a second (±20 %) on this connection until the engine
// accepts it or it is unregistered; a reconnect replays it anyway.
func (c *Client) rejectFunction(msg *RegistrationRejectedMessage) {
	c.mu.Lock()
	fn, ok := c.functions[msg.FunctionID]
	epoch := c.epoch
	c.mu.Unlock()
	if ok {
		fn.observe.onRegistrationRejected(epoch)
		c.reporter.discardFunction(msg.FunctionID)
	}
	if ok && fn.standby && msg.Code == "FUNCTION_NAMESPACE_CONFLICT" {
		if !fn.observe.standingBy.Swap(true) {
			log.Printf("iii: function %q in namespace %q is hosted by worker %s; standing by to take it over when it leaves",
				msg.FunctionID, msg.Namespace, msg.OwnerWorkerID)
		}
		interval := c.standbyInterval
		if interval <= 0 {
			interval = standbyRetryInterval
		}
		time.AfterFunc(time.Duration(float64(interval)*(0.8+0.4*rand.Float64())), func() {
			c.mu.Lock()
			current, ok := c.functions[msg.FunctionID]
			live := ok && current.observe == fn.observe && c.epoch == epoch && c.state == StateConnected
			c.mu.Unlock()
			if live {
				c.sendFunction(current)
			}
		})
		return
	}
	if msg.Code == "FUNCTION_NAMESPACE_CONFLICT" {
		log.Printf("iii: function registration rejected (%s): %q in namespace %q already owned by worker %s; staying connected and serving the rest",
			msg.Code, msg.FunctionID, msg.Namespace, msg.OwnerWorkerID)
		return
	}
	log.Printf("iii: function registration rejected (%s): %q in namespace %q; staying connected and serving the rest",
		msg.Code, msg.FunctionID, msg.Namespace)
}

// failFatally puts the client in the terminal failed state without a
// connection ever being involved: the cause is in the program text, so dialling
// would only produce a second, less honest error.
func (c *Client) failFatally(err error) {
	c.mu.Lock()
	c.fatalErr = err
	c.setStateLocked(StateFailed)
	c.mu.Unlock()
	c.fatalOnce.Do(func() { close(c.fatal) })
	c.failOnce.Do(func() { close(c.failed) })
}

// FatalError returns the terminal registration rejection that stopped this
// worker, or nil while healthy. Mirrors the Node/Python/Rust SDKs' fatal-error
// accessors.
func (c *Client) FatalError() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.fatalErr
}

// enqueueOutboundDirect sends a connection-scoped reply (a pong) on the current
// connection's reply channel. If there is no
// live connection, the reply is dropped: it answers a request that arrived on a socket
// that is now gone, so there is nowhere valid to send it (the engine re-drives on
// reconnect if it still wants the work). This is what keeps a stale reply from leaking
// onto the next connection (iii-hq/iii#1749).
func (c *Client) enqueueOutboundDirect(frame []byte) {
	var err error
	frame, err = prepareFrame(frame)
	if err != nil {
		c.rejectFrame(frame, err)
		return
	}
	c.replyMu.Lock()
	reply, done := c.reply, c.replyDone
	c.replyMu.Unlock()
	if reply == nil {
		return
	}
	select {
	case reply <- frame:
	case <-done:
	case <-c.shutdown:
	}
}

// handleInvoke runs a registered function for an inbound InvokeFunction that arrived on
// connection epoch and, unless the call is fire-and-forget (no invocation_id), replies
// with an InvocationResult. With an engine that advertised observe.v2 this worker
// delivers the call (a void one included: the engine sends it with an invocation_id):
// the report items go out first, then the answer, which lists in observed what was
// delivered with the handler's call.
func (c *Client) handleInvoke(ctx context.Context, epoch uint64, msg *InvokeFunctionMessage) {
	c.mu.Lock()
	fn, ok := c.functions[msg.FunctionID]
	hostDelivers := msg.InvocationID != nil && c.epoch == epoch && c.registeredOnConnection && c.engineFeatures[FeatureObserveV2]
	c.mu.Unlock()

	tc := traceContext{traceparent: msg.Traceparent, baggage: msg.Baggage}

	if !ok {
		c.replyInvocation(epoch, msg, nil, &ErrorBody{
			Code:    "function_not_found",
			Message: fmt.Sprintf("no handler registered for %q", msg.FunctionID),
		}, tc, nil)
		return
	}
	// An HTTP-invoked function (RegisterHTTPFunction) has no handler here: the engine
	// calls its URL. Same code and message as the Node, Python and Rust SDKs.
	if fn.handler == nil {
		c.replyInvocation(epoch, msg, nil, &ErrorBody{
			Code:    "function_not_invokable",
			Message: "Function is HTTP-invoked and cannot be invoked locally",
		}, tc, nil)
		return
	}

	// Attach trace context, per-invocation metadata and the invocation itself to ctx
	// before invoking the handler; they are delivered out-of-band on ctx (see
	// MetadataFromContext, InvocationFromContext) so the Handler signature stays stable.
	inv := &invocation{
		functionID:   msg.FunctionID,
		caller:       msg.Caller,
		activation:   msg.Activation,
		observeDepth: msg.ObserveDepth,
		metadata:     msg.Metadata,
		trace:        tc,
	}
	if msg.InvocationID != nil {
		inv.invocationID = msg.InvocationID.String()
	} else {
		inv.invocationID = uuid.NewString()
	}
	hctx := withInvocation(withMetadata(injectTraceContext(ctx, tc), msg.Metadata), inv)
	f := fn.observe
	// the observers active when the call started
	set := f.compiledSet()
	deliver := f.deliverHook()
	var observed []string
	var items []*reportItem
	started := time.Now()
	out := c.runInvocation(hctx, fn.handler, msg.Data, false, func(ctx context.Context, handler outcome) outcome {
		if !hostDelivers || (deliver == nil && len(set) == 0) {
			return handler
		}
		run := newDeliveryRun(f, handlerCall(msg.FunctionID, inv.invocationID, msg.Data, handler, msg.Caller, msg.Metadata), deliver != nil, true)
		run.startedAt, run.duration, run.observeDepth, run.trace = started, time.Since(started), msg.ObserveDepth, tc
		answer := outcomeOf(run.deliver(ctx, deliver, set))
		observed, items = run.settleRouted(answer)
		return answer
	})
	if len(items) > 0 {
		c.reporter.sendItems(epoch, true, items)
	}
	c.replyInvocation(epoch, msg, out.result, out.errBody, tc, observed)
}

// outcome is a call's outcome as the wire carries it, its encoded result or its error
// body, and the error it came from.
type outcome struct {
	result  json.RawMessage
	errBody *ErrorBody
	err     error
}

// outcomeOf encodes what a handler or a deliver hook returned: a json.RawMessage is
// used as given once it is checked, any other value is marshaled.
func outcomeOf(value any, err error) outcome {
	if err != nil {
		return outcome{errBody: errorBodyFromHandlerError(err), err: err}
	}
	encoded, err := encodeResult(value)
	if err != nil {
		return outcome{errBody: &ErrorBody{Code: "invocation_failed", Message: err.Error()}, err: err}
	}
	return outcome{result: encoded}
}

// runInvocation runs handler h and then deliver, with the handler's outcome, inside the
// invocation's span; deliver's outcome is the call's. A handler panic is recovered into
// an error, so a caller gets an InvocationResult.error instead of a timeout. clone
// copies a json.RawMessage the handler returns.
func (c *Client) runInvocation(ctx context.Context, h Handler, data json.RawMessage, clone bool, deliver func(ctx context.Context, handler outcome) outcome) outcome {
	ctx, endSpan := c.startInvocationSpan(ctx, data) // otel_client.go: "execute <fn>" span
	handler := outcomeOf(callHandler(ctx, h, data))
	if clone && handler.result != nil {
		handler.result = bytes.Clone(handler.result)
	}
	out := deliver(ctx, handler)
	if out.err != nil {
		endSpan(nil, out.err)
	} else if out.errBody != nil {
		endSpan(nil, newInvocationError(out.errBody, ""))
	} else {
		endSpan(out.result, nil)
	}
	return out
}

// callHandler invokes h, recovering a panic into an error.
func callHandler(ctx context.Context, h Handler, data json.RawMessage) (result any, err error) {
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

// replyInvocation sends the InvocationResult for an invocation on the connection it
// arrived on, unless it was fire-and-forget (no invocation_id), in which case there is
// nowhere to reply. Echoes the incoming trace context back on the result, as the engine
// expects, and lists the observers delivered with the handler's call in observed.
func (c *Client) replyInvocation(epoch uint64, msg *InvokeFunctionMessage, result json.RawMessage, errBody *ErrorBody, tc traceContext, observed []string) {
	if msg.InvocationID == nil {
		return // fire-and-forget: no reply expected
	}
	frame, err := prepareJSON(&InvocationResultMessage{
		InvocationID: *msg.InvocationID,
		FunctionID:   msg.FunctionID,
		Result:       result,
		Error:        errBody,
		Traceparent:  tc.traceparent,
		Baggage:      tc.baggage,
		Observed:     observed,
	})
	if err != nil {
		return
	}
	c.sendOnEpoch(epoch, frame)
}

// handleInvocationResult resolves the pending Call keyed by the result's
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

// Close shuts the client down at once: it stops the reconnect loop, gives buffered
// observe reports and telemetry a short, fixed time to go out (250ms for the reports, at
// most 10s for the telemetry flush), cancels all pending calls with
// ErrNotConnected, and drops the socket without a closing handshake. It does not send
// unregister frames (matching the reference SDKs). Use [Client.Shutdown] to wait for the
// flushes under a deadline of your own and close the socket gracefully.
//
// Close is idempotent and implements io.Closer; it always returns nil. Called while a
// Shutdown runs, it cuts that Shutdown short (as http.Server.Close does) and returns
// once it has finished.
func (c *Client) Close() error {
	ctx, cancel := context.WithTimeout(context.Background(), otelShutdownTimeout)
	defer cancel()
	_ = c.terminate(ctx, false)
	return nil
}

// Shutdown shuts the client down gracefully, as the Node SDK's shutdown does, bounded
// by ctx like http.Server.Shutdown: it stops the reconnect loop, waits until the
// buffered observe reports and counters are written, stops the worker gauges, flushes
// and stops the telemetry exporters, cancels all pending calls with
// ErrNotConnected, and closes the socket with a normal-closure handshake.
//
// When ctx is done before those steps finish, the remaining work is cut short (the
// socket is closed at once, also when the engine never answers the close handshake)
// and ctx.Err() is returned; the client is shut down either way and nil means
// everything was delivered. A [Client.Close] called meanwhile cuts it short the same
// way, and Shutdown then returns context.Canceled. Like Close it does not send
// unregister frames. Shutdown is idempotent: a later Shutdown returns once the first
// shutdown has finished, or ctx.Err() if its own ctx ends first.
func (c *Client) Shutdown(ctx context.Context) error {
	return c.terminate(ctx, true)
}

// terminate is Close (graceful false) and Shutdown (graceful true). Close bounds the
// observe flush to closeFlushTimeout and the telemetry flush to ctx; Shutdown bounds
// everything by ctx and closes the socket with a handshake.
func (c *Client) terminate(ctx context.Context, graceful bool) error {
	first := false
	var stop context.CancelFunc
	c.shutOnce.Do(func() {
		first = true
		if graceful {
			// A Close called while this Shutdown runs cancels ctx, which cuts every
			// remaining step short. Set inside Do, so a Close that loses the race sees it.
			ctx, stop = context.WithCancel(ctx)
			c.mu.Lock()
			c.shutForce = stop
			c.mu.Unlock()
		}
	})
	if !first {
		if !graceful {
			// Close keeps its "at once" contract: it cuts a running Shutdown short, then
			// waits for it to finish (http.Server.Close next to Shutdown).
			c.mu.Lock()
			force := c.shutForce
			c.mu.Unlock()
			if force != nil {
				force()
			}
			<-c.shutDone
			return nil
		}
		select {
		case <-c.shutDone:
			return nil
		case <-ctx.Done():
			return ctx.Err()
		}
	}
	if stop != nil {
		defer stop()
	}
	defer close(c.shutDone)

	// Last chance for buffered reports and counters to reach the engine. A writer
	// that does not drain (a peer that stopped reading) cannot hold the shutdown up:
	// closing shutdown unblocks the flush.
	flushCtx := ctx
	if !graceful {
		var cancel context.CancelFunc
		flushCtx, cancel = context.WithTimeout(ctx, closeFlushTimeout)
		defer cancel()
	}
	flushed := make(chan struct{})
	go func() {
		defer close(flushed)
		c.flushObserve(flushCtx)
	}()
	select {
	case <-flushed:
	case <-flushCtx.Done():
	}
	c.reporter.shutdown()
	c.stopTelemetry(ctx) // otel_client.go: stop the worker gauges, flush and stop exporters
	close(c.shutdown)
	c.lifeCancel()

	// Cancel all pending invocations.
	c.mu.Lock()
	for id, ch := range c.pending {
		ch <- invocationOutcome{err: ErrNotConnected}
		delete(c.pending, id)
	}
	conn, connCancel := c.conn, c.connCancel
	c.setStateLocked(StateDisconnected)
	c.mu.Unlock()

	if conn != nil {
		if graceful && ctx.Err() == nil {
			closed := make(chan struct{})
			go func() {
				defer close(closed)
				_ = conn.Close(websocket.StatusNormalClosure, "shutdown")
			}()
			select {
			case <-closed:
			case <-ctx.Done():
				// Out of time, or a Close cut the shutdown short. A peer that never
				// answers the close handshake keeps coder/websocket waiting up to its own
				// 5s, and CloseNow would only wait for that same handshake. Cancelling the
				// connection's context closes the connection under the pending read,
				// which ends the handshake at once.
				if connCancel != nil {
					connCancel()
				}
			}
		} else {
			_ = conn.CloseNow()
		}
	}
	if graceful {
		return ctx.Err()
	}
	return nil
}
