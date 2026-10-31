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
// [ObserverHandle.Ready]. The engine's refusal of an observe (an observeresult in
// state error) is the handle's final answer: the engine stores nothing for it, the
// handle is not sent again on a later connection, and its Dispose sends nothing. An
// observeresult in state error for a handle whose observer the engine may hold (one of
// its observes was answered pending, active or suspended, or lost its answer to a
// closed connection) is that observer's state: the handle is replayed, and its Dispose
// unobserves it.
// The returned error is for a request that cannot be sent at all: a missing function
// or target, a blank namespace, args or filters that are not JSON, and an id a handle
// of this client holds. A handle holds its id until it is disposed (by
// [ObserverHandle.Dispose] or by the engine); an Observe of a held id sends nothing,
// leaves the holder alone and returns an *ObserveRejectionError with code
// OBSERVER_ID_CONFLICT. Once the holder is disposed the id is free at once.
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
	if _, held := c.observers[id]; held {
		c.mu.Unlock()
		return nil, &ObserveRejectionError{Code: "OBSERVER_ID_CONFLICT", Message: "observer id already in use: " + id}
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
// disposed or refused meanwhile or epoch is not the registered connection (whose own
// replay sends it), and lists h for the engine's answers. The caller holds observeMu,
// which orders observe and unobserve frames, so the list is in the order the frames go
// out.
func (c *Client) sendObserveLocked(h *ObserverHandle, epoch uint64) {
	c.mu.Lock()
	live := c.registeredLocked(epoch) && c.engineFeatures[FeatureObserveV1] && c.observers[h.id] == h && !h.refused
	if live {
		if c.observeSent == nil {
			c.observeSent = map[string][]sentObserve{}
		}
		c.observeSent[h.id] = append(c.observeSent[h.id], sentObserve{handle: h})
	}
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
	refused := h.refused
	c.mu.Unlock()
	if h.wasSent() && !refused {
		// sent on the registered connection, or parked for the next one (a Reattach
		// would otherwise keep the observer alive); the engine stores nothing for a
		// refused observe
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
		// a refused handle has its final answer: it is not sent again
		if !h.refused {
			handles = append(handles, h)
		}
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

// sentObserve is a handle whose observe went out on the current connection, and
// whether its observeresult arrived.
type sentObserve struct {
	handle   *ObserverHandle
	answered bool
}

// The engine reads a connection's messages in order, sends that connection its observe
// answers in the order it makes them, and answers every observe with exactly one
// observeresult (decision 56). So an observeresult for an id answers the oldest listed
// handle of the id still unanswered, and an observerstatechanged is about the newest
// listed one that has its observeresult. A disposed handle stays listed until the
// engine's disposed for it arrives, a newer handle of the id has its observeresult, or
// the connection closes: the late disposed of an earlier observer reaches the earlier
// handle, never the one that observes under its id again. An observeresult in state
// error for a handle that is not held refuses the observe: the engine stores nothing
// for it, so that answer is the handle's final answer, and the handle leaves the list
// when it arrives. For a held handle it is the state of the observer the engine
// holds, and the handle stays listed.

// onObserveResult applies an observeresult to the oldest listed handle of id still
// unanswered; with none it is dropped. A refusal unlists the handle for good, and its
// Dispose sends nothing; one in state disposed frees its id too.
func (c *Client) onObserveResult(id, state string, reason *ErrorBody) {
	st := toObserverState(state)
	c.mu.Lock()
	list := c.observeSent[id]
	i := slices.IndexFunc(list, func(s sentObserve) bool { return !s.answered })
	if i < 0 {
		c.mu.Unlock()
		return
	}
	h := list[i].handle
	// the handles before it are answered, and disposed: a newer one has its answer now
	list = slices.Delete(list, 0, i)
	list[0].answered = true
	c.observeSent[id] = list
	switch {
	case st == ObserverDisposed:
		c.forgetLocked(h)
	case st == ObserverError && !h.held:
		// set before the state, so a Dispose that sees the error sends nothing
		h.refused = true
		c.unlistLocked(h)
	case st != ObserverError:
		h.held = true
	}
	c.mu.Unlock()
	h.setState(st, observeReason(reason))
}

// onObserverStateChanged applies an observerstatechanged to the newest listed handle
// of id that has its observeresult; with none it is dropped (a reattached connection
// hears its observers resume before its replayed observes are answered, and each
// replay's observeresult carries the current state).
func (c *Client) onObserverStateChanged(id, state string, reason *ErrorBody) {
	st := toObserverState(state)
	c.mu.Lock()
	list := c.observeSent[id]
	i := len(list) - 1
	for i >= 0 && !list[i].answered {
		i--
	}
	if i < 0 {
		c.mu.Unlock()
		return
	}
	h := list[i].handle
	if st == ObserverDisposed {
		// disposed by the engine (unobserve, max_fires, expiry): not replayed
		c.forgetLocked(h)
	}
	c.mu.Unlock()
	h.setState(st, observeReason(reason))
}

// forgetLocked drops h, which the engine disposed, from the answer list and frees its
// id. The caller holds mu.
func (c *Client) forgetLocked(h *ObserverHandle) {
	c.unlistLocked(h)
	if c.observers[h.id] == h {
		delete(c.observers, h.id)
	}
}

// dropObserveSentLocked ends the current connection's answer lists. The engine may
// have created the observer of an observe whose answer never arrived, and holds it
// again on a Reattach: that handle is held from here on. The caller holds mu.
func (c *Client) dropObserveSentLocked() {
	for _, list := range c.observeSent {
		for _, s := range list {
			if !s.answered {
				s.handle.held = true
			}
		}
	}
	c.observeSent = nil
}

// unlistLocked drops h from the answer list of its id. The caller holds mu.
func (c *Client) unlistLocked(h *ObserverHandle) {
	list := slices.DeleteFunc(c.observeSent[h.id], func(s sentObserve) bool { return s.handle == h })
	if len(list) == 0 {
		delete(c.observeSent, h.id)
	} else {
		c.observeSent[h.id] = list
	}
}

func observeReason(reason *ErrorBody) *ObserveReason {
	if reason == nil {
		return nil
	}
	return &ObserveReason{Code: reason.Code, Message: reason.Message}
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
		c.onObserveResult(msg.ID, msg.State, msg.Error)
	case MsgObserverStateChanged:
		msg := dec.ObserverStateChanged
		c.onObserverStateChanged(msg.ID, msg.State, msg.Reason)
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
