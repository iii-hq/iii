package iii

import (
	"bytes"
	"cmp"
	"context"
	"encoding/json"
	"fmt"
	"log"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/google/uuid"
)

// This file wires observe into the Client: Observe, feature negotiation, the inbound
// observe messages, and what the hosting side and the reporter need from the
// connection.

// Features this SDK declares in its engine::workers::register payload. The engine
// advertises the first three in WorkerRegistered: observe.v2 includes observe.v1, and
// with it this worker runs the match and deliver hooks of the functions it hosts for
// their routed calls, whose observers' on and filters the engine evaluates.
// FeatureLargeMessagesV1 is declared by the SDK only: it receives JSON messages up to
// 64 MiB, so the engine sends it messages of that size whatever version it reports.
const (
	FeatureObserveV1       = "observe.v1"
	FeatureObserveV2       = "observe.v2"
	FeatureLocalDispatchV1 = "local-dispatch.v1"
	FeatureLargeMessagesV1 = "large-messages.v1"
)

var sdkFeatures = []string{FeatureObserveV1, FeatureObserveV2, FeatureLocalDispatchV1, FeatureLargeMessagesV1}

// Engine functions of the observer registry.
const (
	FnListObservers   = "engine::observers::list"
	FnGetObserver     = "engine::observers::get"
	FnDisposeObserver = "engine::observers::dispose"
)

var unsupportedReason = &ObserveReason{
	Code:    "UNSUPPORTED",
	Message: "the engine does not support observe (no observe.v1 in its features); upgrade it to 0.25 or later",
}

// Observe observes a function: when source.FunctionID runs, target runs, for every
// call that the function's worker delivers to the observer and that passes
// source.Filters. target is a function registered by some worker (until it is, the
// observer stays pending). source.Args go to the worker that hosts the function, which
// reads them in its hooks (a cron expression, a route).
//
// It returns the handle at once, also before the connection is up; the observer is
// replayed with the same id on every reconnect. Errors from the engine (INVALID_ARGS,
// INVALID_FILTER, FORBIDDEN, NOT_OBSERVABLE, SELF_TARGET, the host's rejection, and
// UNSUPPORTED against an engine without observe) show up as state error and fail
// [ObserverHandle.Ready].
// The returned error is for a request that cannot be sent at all: a missing function
// or target, a reused id, a blank namespace, args or filters that are not JSON.
//
//	client.RegisterFunction("alerts::payment-failed", notify)
//	failures, err := client.Observe(iii.Source{
//	    FunctionID: "payments::charge",
//	    On:         iii.OnError,
//	    Filters:    iii.Filters{"input.amount": iii.Op{"$gte": 500}},
//	}, "alerts::payment-failed")
//	if err == nil {
//	    err = failures.Ready(ctx)
//	}
func (c *Client) Observe(source Source, target string, opts ...ObserverOptions) (*ObserverHandle, error) {
	if strings.TrimSpace(source.FunctionID) == "" {
		return nil, fmt.Errorf("iii: Observe: Source.FunctionID is required")
	}
	if strings.TrimSpace(target) == "" {
		return nil, fmt.Errorf("iii: Observe: target is required (the id of a registered function)")
	}
	if len(opts) > 1 {
		return nil, fmt.Errorf("iii: Observe(%q): expected at most one ObserverOptions, got %d", source.FunctionID, len(opts))
	}
	var o ObserverOptions
	if len(opts) == 1 {
		o = opts[0]
	}
	if err := refuseBlankCallNamespace(source.Namespace, "Source.Namespace"); err != nil {
		return nil, err
	}
	if err := refuseBlankCallNamespace(o.TargetNamespace, "ObserverOptions.TargetNamespace"); err != nil {
		return nil, err
	}
	id := o.ID
	if id == "" {
		id = uuid.NewString()
	}
	msg := &ObserveMessage{
		ID: id,
		Source: ObserveMessageSource{
			FunctionID: source.FunctionID,
			Namespace:  invocationNamespace(source.FunctionID, source.Namespace, c.namespace),
			Capture:    slices.Clone(source.Capture),
			On:         source.On,
			Deliver:    source.Deliver,
		},
		Target: ObserveMessageTarget{FunctionID: target, Namespace: invocationNamespace(target, o.TargetNamespace, c.namespace)},
	}
	if source.Capture != nil && msg.Source.Capture == nil {
		msg.Source.Capture = []Capture{}
	}
	var err error
	if source.Args != nil {
		if msg.Source.Args, err = json.Marshal(source.Args); err != nil {
			return nil, fmt.Errorf("iii: Observe(%q): args: %w", source.FunctionID, err)
		}
	}
	if source.Filters != nil {
		if msg.Source.Filters, err = json.Marshal(source.Filters); err != nil {
			return nil, fmt.Errorf("iii: Observe(%q): filters: %w", source.FunctionID, err)
		}
	}
	if len(o.Metadata) > 0 {
		if !json.Valid(o.Metadata) {
			return nil, fmt.Errorf("iii: Observe(%q): ObserverOptions.Metadata must contain valid JSON", source.FunctionID)
		}
		msg.Metadata = append(json.RawMessage(nil), o.Metadata...)
	}
	if o.MaxFires > 0 {
		msg.MaxFires = &o.MaxFires
	}
	if o.ExpiresIn > 0 {
		ms := uint64(max(1, o.ExpiresIn/time.Millisecond))
		msg.ExpiresInMs = &ms
	}
	if o.MaxInFlight > 0 {
		msg.MaxInFlight = &o.MaxInFlight
	}
	frame, err := prepareJSON(msg)
	if err != nil {
		return nil, err
	}

	c.mu.Lock()
	if _, dup := c.observers[id]; dup {
		c.mu.Unlock()
		return nil, fmt.Errorf("iii: Observe: observer id already in use: %s", id)
	}
	c.observerSeq++
	h := newObserverHandle(c, id, frame, c.observerSeq)
	c.observers[id] = h
	live := c.state == StateConnected && c.registeredOnConnection
	supported, epoch := c.engineFeatures[FeatureObserveV1], c.epoch
	c.mu.Unlock()
	if live {
		if supported {
			c.sendObserve(h, epoch)
		} else {
			h.setState(ObserverError, unsupportedReason)
		}
	}
	return h, nil
}

// ObserveFunction observes functionID with target, every call and the observer
// defaults: [Client.Observe] with Source{FunctionID: functionID}.
func (c *Client) ObserveFunction(functionID, target string, opts ...ObserverOptions) (*ObserverHandle, error) {
	return c.Observe(Source{FunctionID: functionID}, target, opts...)
}

// IsObserved reports whether functionID, registered by this worker, has active
// observers, so a source can skip building an event nobody wants. It takes no lock: a
// lookup of functionID and one atomic load. [FunctionRef.IsObserved] is the load alone.
func (c *Client) IsObserved(functionID string) bool {
	f := c.functionObservers(functionID)
	return f != nil && f.isObserved()
}

// IsObservedFor reports whether call of functionID, registered by this worker, would
// reach an observer. On a function matched by keys ([MatchKeys]) it is true when an
// observer is filed under one of the call's keys (one lookup per key); on any other
// function call is ignored, and it is [Client.IsObserved]. Set what the keys read
// (Input, Metadata).
func (c *Client) IsObservedFor(functionID string, call *Call) bool {
	f := c.functionObservers(functionID)
	return f != nil && f.isObservedFor(call)
}

// Observers returns the active observers of functionID, registered by this worker, as
// its host sees them.
func (c *Client) Observers(functionID string) []*Observer {
	f := c.functionObservers(functionID)
	if f == nil {
		return nil
	}
	return f.observers()
}

// LocalDispatchActive reports whether Call runs functionID in-process: it is
// registered here, the engine confirmed it with local_dispatch, and
// RegisterFunctionOptions.LocalDispatch did not opt out.
func (c *Client) LocalDispatchActive(functionID string) bool {
	f := c.functionObservers(functionID)
	return f != nil && f.state.Load().local
}

// functionObservers is the observe state of functionID's current registration, or nil;
// it takes no lock.
func (c *Client) functionObservers(functionID string) *functionObservers {
	if f, ok := c.observeByID.Load(functionID); ok {
		return f.(*functionObservers)
	}
	return nil
}

func (c *Client) sendObserve(h *ObserverHandle, epoch uint64) {
	c.observeMu.Lock()
	defer c.observeMu.Unlock()
	c.sendObserveLocked(h, epoch)
}

// sendObserveLocked queues h's observe frame for connection epoch, unless h was
// disposed meanwhile or that connection is no longer the registered one (its own
// replay sends it then). The caller holds observeMu, which orders observe and
// unobserve frames.
func (c *Client) sendObserveLocked(h *ObserverHandle, epoch uint64) {
	c.mu.Lock()
	live := c.registeredLocked(epoch) && c.engineFeatures[FeatureObserveV1] && c.observers[h.id] == h
	c.mu.Unlock()
	if !live {
		return
	}
	h.markSent()
	c.queueObserveFrameLocked(observeFrame{data: h.frame, epoch: epoch})
}

// registeredLocked: connection epoch is the current one, and WorkerRegistered arrived
// on it. The caller holds mu.
func (c *Client) registeredLocked(epoch uint64) bool {
	return c.state == StateConnected && c.registeredOnConnection && c.epoch == epoch
}

// observeFrame is an observe or unobserve frame meant for connection epoch.
type observeFrame struct {
	data        []byte
	epoch       uint64
	unobserveID string
}

// queueObserveFrameLocked hands a frame to the observe pump: one goroutine pushes
// them to the writer in order, so observeMu (which the read loop takes too) is never
// held across a blocking send. The caller holds observeMu.
func (c *Client) queueObserveFrameLocked(f observeFrame) {
	c.observeQueue = append(c.observeQueue, f)
	if !c.observePumping {
		c.observePumping = true
		go c.pumpObserveFrames()
	}
}

func (c *Client) pumpObserveFrames() {
	for {
		c.observeMu.Lock()
		if len(c.observeQueue) == 0 {
			c.observePumping = false
			c.observeMu.Unlock()
			return
		}
		f := c.observeQueue[0]
		c.observeQueue[0] = observeFrame{}
		c.observeQueue = c.observeQueue[1:]
		c.observeMu.Unlock()

		c.mu.Lock()
		send := c.registeredLocked(f.epoch)
		c.mu.Unlock()
		if !send {
			c.missedObserveFrame(f)
			continue
		}
		// the writer checks the epoch again when it writes the frame
		select {
		case c.observeOut <- f:
		case <-c.shutdown:
			return
		}
	}
}

// missedObserveFrame handles a frame whose connection is gone. An observe is dropped:
// the current connection's replay sends every live observer. An unobserve is
// re-routed to the current connection, synchronously, so an observe of the same id
// queued after it cannot overtake it. Called from the writer and the pump: no holder
// of observeMu ever waits on either (they only append to the queue).
func (c *Client) missedObserveFrame(f observeFrame) {
	if f.unobserveID == "" {
		return
	}
	c.observeMu.Lock()
	defer c.observeMu.Unlock()
	c.routeUnobserveLocked(f.unobserveID)
}

// detachReply detaches the current connection's reply channel: no new send can start
// on it. It returns what a teardown needs to finish it.
func (c *Client) detachReply() (reply chan []byte, done chan struct{}, senders *sync.WaitGroup) {
	c.replyMu.Lock()
	defer c.replyMu.Unlock()
	reply, done, senders = c.reply, c.replyDone, c.replySenders
	c.reply, c.replyDone, c.replyQueued, c.replySenders = nil, nil, nil, nil
	return reply, done, senders
}

// routeUnobserveLocked sends an unobserve on the current connection when
// WorkerRegistered arrived on it (and it supports observe), and otherwise parks it
// for the next WorkerRegistered (SDK-PARITY decision 16). The caller holds observeMu,
// as onWorkerRegistered does while it drains the parked ones.
func (c *Client) routeUnobserveLocked(id string) {
	c.mu.Lock()
	if !(c.state == StateConnected && c.registeredOnConnection) {
		c.pendingUnobserve = append(c.pendingUnobserve, id)
		c.mu.Unlock()
		return
	}
	supported, epoch := c.engineFeatures[FeatureObserveV1], c.epoch
	c.mu.Unlock()
	if supported {
		c.queueUnobserveLocked(id, epoch)
	}
}

func (c *Client) disposeObserver(h *ObserverHandle) {
	c.observeMu.Lock()
	defer c.observeMu.Unlock()
	c.mu.Lock()
	if c.observers[h.id] == h {
		delete(c.observers, h.id)
	}
	c.mu.Unlock()
	if h.wasSent() {
		// sent on the registered connection, or parked for the next one (a Reattach
		// would otherwise keep the observer alive)
		c.routeUnobserveLocked(h.id)
	}
}

// discardUnwritten counts the report frames left in a finished connection's reply
// channel as INACTIVE, once every send into it has returned.
func (c *Client) discardUnwritten(reply chan []byte, senders *sync.WaitGroup) {
	senders.Wait()
	for {
		select {
		case frame := <-reply:
			if bytes.HasPrefix(frame, reportFrameHead) {
				c.reporter.countUnsent(frame)
			}
		default:
			return
		}
	}
}

// queueUnobserveLocked queues an unobserve for connection epoch. The caller holds
// observeMu.
func (c *Client) queueUnobserveLocked(id string, epoch uint64) {
	if frame, err := prepareJSON(&UnobserveMessage{ID: id}); err == nil {
		c.queueObserveFrameLocked(observeFrame{data: frame, epoch: epoch, unobserveID: id})
	}
}

// onWorkerRegistered: the engine said what it supports on this connection. With
// observe.v2 the functions that need it, held until WorkerRegistered, are registered;
// without it they stay unregistered (UNSUPPORTED, logged once each) and are released:
// no local dispatch, their observers removed through the removed hooks, their buffered
// reports dropped (INACTIVE). With observe.v1
// the observers are replayed (after the functions) and held reports flushed; without
// it Observe fails with UNSUPPORTED. Hook ids this worker has not registered are
// reported once each, after a grace period (checkHookIDsLater), which catches a
// mistyped id.
func (c *Client) onWorkerRegistered(msg *WorkerRegisteredMessage) {
	// Holding observeMu across the replay keeps a concurrent Dispose (or a re-routed
	// unobserve) either before it (parked, drained here) or after it (sent directly).
	c.observeMu.Lock()
	defer c.observeMu.Unlock()
	c.mu.Lock()
	c.workerID = msg.WorkerID
	c.reattachToken = msg.ReattachToken
	c.engineFeatures = map[string]bool{}
	for _, f := range msg.Features {
		c.engineFeatures[f] = true
	}
	c.registeredOnConnection = true
	supported, v2, epoch := c.engineFeatures[FeatureObserveV1], c.engineFeatures[FeatureObserveV2], c.epoch
	handles := make([]*ObserverHandle, 0, len(c.observers))
	for _, h := range c.observers {
		handles = append(handles, h)
	}
	unobserve := c.pendingUnobserve
	c.pendingUnobserve = nil
	var held []*RegisterFunctionMessage
	var unsupported []registeredFunction
	var warn []string
	for id, fn := range c.functions {
		if !c.needsV2Locked(fn) {
			continue
		}
		if v2 {
			held = append(held, fn.message)
			continue
		}
		unsupported = append(unsupported, fn)
		if c.markUnsupportedLocked(fn) {
			warn = append(warn, id)
		}
	}
	c.checkHookIDsLater(epoch)
	c.mu.Unlock()
	slices.SortFunc(handles, func(a, b *ObserverHandle) int { return cmp.Compare(a.seq, b.seq) })
	slices.SortFunc(held, func(a, b *RegisterFunctionMessage) int { return cmp.Compare(a.ID, b.ID) })
	slices.Sort(warn)

	// The held registrations go through the observe pump, on this connection only and
	// ahead of the observers that may name them as targets.
	for _, m := range held {
		if frame, err := prepareJSON(m); err == nil {
			c.queueObserveFrameLocked(observeFrame{data: frame, epoch: epoch})
		}
	}
	for _, id := range warn {
		logUnsupported(id)
	}
	for _, fn := range unsupported {
		fn.observe.release()
		c.reporter.discardFunction(fn.message.ID)
	}
	if supported {
		for _, id := range unobserve {
			c.queueUnobserveLocked(id, epoch)
		}
		for _, h := range handles {
			c.sendObserveLocked(h, epoch)
		}
		c.reporter.kick()
		return
	}
	for _, h := range handles {
		h.setState(ObserverError, unsupportedReason)
	}
	c.reporter.clear()
}

// hookIDCheckDelay is how long after the connection is registered, or after a hook is
// set for an id this worker has not registered, the SDK checks the hook ids: a
// registration that follows its hooks within it is not reported.
const hookIDCheckDelay = 1500 * time.Millisecond

// checkHookIDsLater checks the hook ids once hookIDCheckDelay has passed, if
// connection epoch is still the registered one then. The caller holds mu.
func (c *Client) checkHookIDsLater(epoch uint64) {
	delay := c.hookIDDelay
	if delay <= 0 {
		delay = hookIDCheckDelay
	}
	time.AfterFunc(delay, func() { c.checkHookIDs(epoch) })
}

// checkHookIDs logs, once per id, each hook id this worker has not registered, which
// catches a mistyped id.
func (c *Client) checkHookIDs(epoch uint64) {
	c.mu.Lock()
	if !c.registeredLocked(epoch) {
		c.mu.Unlock()
		return
	}
	var unknown []string
	for id := range c.functionHooks {
		if _, ok := c.functions[id]; !ok && !c.hookIDWarned[id] {
			c.hookIDWarned[id] = true
			unknown = append(unknown, id)
		}
	}
	c.mu.Unlock()
	slices.Sort(unknown)
	for _, id := range unknown {
		log.Printf("iii: observer hooks are set for %q, which this worker has not registered", id)
	}
}

func (c *Client) onObserverState(id, state string, reason *ErrorBody) {
	c.mu.Lock()
	h := c.observers[id]
	c.mu.Unlock()
	if h == nil {
		return
	}
	st := toObserverState(state)
	var r *ObserveReason
	if reason != nil {
		r = &ObserveReason{Code: reason.Code, Message: reason.Message}
	}
	h.setState(st, r)
	if st == ObserverDisposed {
		// disposed by the engine (max_fires, expiry): not replayed
		c.mu.Lock()
		if c.observers[id] == h {
			delete(c.observers, id)
		}
		c.mu.Unlock()
	}
}

// isObserveFrame reports the observe-family frames (SDK-PARITY decision 16) and, for
// an unobserve, its id.
func isObserveFrame(frame []byte) (family bool, unobserveID string) {
	var head struct {
		Type MessageType `json:"type"`
		ID   string      `json:"id"`
	}
	if json.Unmarshal(frame, &head) != nil {
		return false, ""
	}
	switch head.Type {
	case MsgUnobserve:
		return true, head.ID
	case MsgObserve, MsgObserverAck, MsgInvocationsObserved, MsgLocalInvocationStats:
		return true, ""
	}
	return false, ""
}

// dispatchObserve handles the observe messages from the read loop.
func (c *Client) dispatchObserve(dec *DecodedMessage) {
	switch dec.Type {
	case MsgFunctionRegistered:
		msg := dec.FunctionRegistered
		c.mu.Lock()
		epoch := c.epoch
		fn, ok := c.functions[msg.ID]
		c.mu.Unlock()
		// The answer to a registration withdrawn on this connection (a function that
		// needs observe.v2, on an engine without it) is ignored: it stays unregistered.
		if ok && !fn.observe.heldOn(epoch) {
			// held reports go once the set is applied and acked (applySet kicks)
			fn.observe.onFunctionRegistered(msg, epoch)
		}
	case MsgObserverAdded:
		msg := dec.ObserverAdded
		c.mu.Lock()
		epoch := c.epoch
		fn, ok := c.functions[msg.FunctionID]
		c.mu.Unlock()
		// An observer of a registration withdrawn on this connection: no hook, no ack.
		if ok && !fn.observe.heldOn(epoch) {
			fn.observe.onObserverAdded(msg, epoch)
		}
	case MsgObserverRemoved:
		if f := c.functionObservers(dec.ObserverRemoved.FunctionID); f != nil {
			f.onObserverRemoved(dec.ObserverRemoved)
		}
	case MsgObserveResult:
		msg := dec.ObserveResult
		if len(msg.Warnings) > 0 {
			log.Printf("iii: observer %s: %s", msg.ID, strings.Join(msg.Warnings, "; "))
		}
		c.onObserverState(msg.ID, msg.State, msg.Error)
	case MsgObserverStateChanged:
		msg := dec.ObserverStateChanged
		c.onObserverState(msg.ID, msg.State, msg.Reason)
	}
}

// identity is this worker as the engine knows it (the id is empty before the first
// WorkerRegistered).
func (c *Client) identity() (workerID, workerName string) {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.workerID, c.name
}

func (c *Client) engineSupportsObserve() bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.engineFeatures[FeatureObserveV1]
}

func (c *Client) observeHookTimeout() time.Duration {
	if c.hookTimeout > 0 {
		return c.hookTimeout
	}
	return observeHookTimeout
}

// sendOnEpoch sends a connection-scoped frame (observerack, invocationsobserved,
// localinvocationstats) on connection epoch, and drops it when that connection is
// gone: an ack or a report means nothing on the next one.
func (c *Client) sendOnEpoch(epoch uint64, frame []byte) bool {
	checked, err := prepareFrame(frame)
	if err != nil {
		c.rejectFrame(frame, err)
		return false
	}
	c.replyMu.Lock()
	reply, done := c.reply, c.replyDone
	ok := reply != nil && c.replyEpoch == epoch
	c.replyMu.Unlock()
	if !ok {
		return false
	}
	select {
	case reply <- checked:
		return true
	case <-done:
		return false
	case <-c.shutdown:
		return false
	}
}

// sendReport sends an invocationsobserved or localinvocationstats frame on connection
// epoch, counting a report frame as queued until the writer has written it.
func (c *Client) sendReport(epoch uint64, frame []byte) bool {
	checked, err := prepareFrame(frame)
	if err != nil {
		c.rejectFrame(frame, err)
		return false
	}
	c.replyMu.Lock()
	reply, done, queued, senders := c.reply, c.replyDone, c.replyQueued, c.replySenders
	ok := reply != nil && c.replyEpoch == epoch
	counted := ok && queued != nil && bytes.HasPrefix(checked, reportFrameHead)
	if counted {
		queued.Add(int64(len(checked)))
	}
	if ok && senders != nil {
		senders.Add(1)
		defer senders.Done()
	}
	c.replyMu.Unlock()
	if !ok {
		return false
	}
	select {
	case reply <- checked:
		return true
	case <-done:
	case <-c.shutdown:
	}
	if counted {
		queued.Add(-int64(len(checked)))
	}
	return false
}

// reportQueued implements reporterHost: report bytes handed to the current
// connection's writer and not written yet.
func (c *Client) reportQueued() int {
	c.replyMu.Lock()
	defer c.replyMu.Unlock()
	if c.replyQueued == nil {
		return 0
	}
	return int(c.replyQueued.Load())
}

// reportOnline implements reporterHost.
func (c *Client) reportOnline() (uint64, bool, bool) {
	c.mu.Lock()
	defer c.mu.Unlock()
	online := c.state == StateConnected && c.registeredOnConnection && c.engineFeatures[FeatureObserveV1]
	return c.epoch, online, online && c.engineFeatures[FeatureObserveV2]
}

// reportStatus implements reporterHost.
func (c *Client) reportStatus(functionID string, epoch uint64) reportStatus {
	f := c.functionObservers(functionID)
	if f == nil || f.isDisposed() {
		return reportGone
	}
	return f.reportStatus(epoch)
}

// closeFlushTimeout bounds the final observe flush in Close (Shutdown uses its ctx).
const closeFlushTimeout = 250 * time.Millisecond

// flushObserve gives buffered reports and counters a last chance to reach the engine,
// waiting until the writer drained them or ctx is done (Close: closeFlushTimeout;
// Shutdown: the caller's ctx).
func (c *Client) flushObserve(ctx context.Context) {
	c.reporter.flush()
	c.reporter.sendStats(time.Now())
	for ctx.Err() == nil {
		c.replyMu.Lock()
		pending := 0
		if c.reply != nil {
			pending = len(c.reply)
		}
		c.replyMu.Unlock()
		if pending == 0 {
			return
		}
		time.Sleep(time.Millisecond)
	}
}

func (c *Client) currentEpoch() uint64 {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.epoch
}
