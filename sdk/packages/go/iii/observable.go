package iii

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"slices"
	"sync"
	"sync/atomic"
	"time"

	"github.com/google/uuid"

	"github.com/iii-hq/iii/sdk/packages/go/iii/filter"
)

// This file is the owner side of observe (sdk.md §1, §4.1-§4.3): per-function observer
// state as the engine pushed it (functionregistered, observeradded, observerremoved),
// the observe hooks, the Observer handed to them, and the reports of local invocations
// and obs.Emit events.

// observeHookTimeout is how long OnObserverAdded may run before the observer is
// rejected with OWNER_TIMEOUT (CONTRACT D11).
const observeHookTimeout = 30 * time.Second

// defaultCapture is what an absent capture means (CONTRACT D1).
var defaultCapture = []Capture{CaptureInput, CaptureOutput, CaptureCaller}

// dispatchState is whether trigger() may run a function in-process, and the namespace
// the engine registered it in. Swapped as one pointer: the hot path loads it once.
type dispatchState struct {
	enabled   bool
	namespace string
}

// functionObservers is one registered function's observers. Hooks run one at a time,
// in revision order, on a queue drained by its own goroutine. It never calls into the
// client or the reporter while holding mu.
type functionObservers struct {
	client              *Client
	functionID          string
	hooks               *ObserveHooks
	delivery            Delivery
	localDispatchOption bool

	dispatch atomic.Pointer[dispatchState]
	// compiled holds the active observers: swapped on change, read once per call.
	compiled atomic.Pointer[[]*Observer]

	mu sync.Mutex
	// confirmedEpoch is the connection functionregistered arrived on, or -1.
	confirmedEpoch int64
	// syncedEpoch is the connection whose functionregistered set was applied and
	// acked, or -1: reports wait for it, so none reaches the engine before the acks
	// that make the replayed observers active again.
	syncedEpoch int64
	// releasedEpoch is the connection on which the engine refused the function
	// (FUNCTION_NAMESPACE_CONFLICT), or -1. On that connection its reports are gone
	// (INACTIVE), those of a call already running included; a later connection's
	// replayed registration may be accepted, and functionregistered clears it.
	releasedEpoch int64
	namespace     string
	// revision is the highest revision received on this connection.
	revision int64
	known    map[string]*Observer
	disposed bool

	qmu     sync.Mutex
	tasks   []func()
	running bool
}

func newFunctionObservers(c *Client, functionID string, hooks *ObserveHooks, localDispatch bool) *functionObservers {
	f := &functionObservers{
		client:              c,
		functionID:          functionID,
		hooks:               hooks,
		delivery:            DeliveryAuto,
		localDispatchOption: localDispatch,
		confirmedEpoch:      -1,
		syncedEpoch:         -1,
		releasedEpoch:       -1,
		revision:            -1,
		known:               map[string]*Observer{},
	}
	if hooks != nil && hooks.Delivery == DeliveryOwner {
		f.delivery = DeliveryOwner
	}
	f.dispatch.Store(&dispatchState{})
	f.compiled.Store(&[]*Observer{})
	return f
}

// needsObserve: owner delivery or callbacks, which need observe.v1 on both sides.
func (f *functionObservers) needsObserve() bool {
	return f.delivery == DeliveryOwner || (f.hooks != nil && (f.hooks.OnObserverAdded != nil || f.hooks.OnObserverRemoved != nil))
}

func (f *functionObservers) compiledSet() []*Observer { return *f.compiled.Load() }

func (f *functionObservers) isObserved() bool { return len(f.compiledSet()) > 0 }

func (f *functionObservers) observers() []*Observer { return slices.Clone(f.compiledSet()) }

// syncedOn: functionregistered arrived on connection epoch and its set was applied
// and acked.
func (f *functionObservers) syncedOn(epoch uint64) bool {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.syncedEpoch == int64(epoch)
}

// reportStatus is what a flush on connection epoch does with this function's
// reports: send them, hold them, or drop them as INACTIVE.
func (f *functionObservers) reportStatus(epoch uint64) reportStatus {
	f.mu.Lock()
	defer f.mu.Unlock()
	switch {
	case f.releasedEpoch == int64(epoch):
		return reportGone
	case f.syncedEpoch == int64(epoch):
		return reportReady
	}
	return reportWait
}

// onFunctionRegistered is always accepted and replaces the set (§4.2). It runs on the
// read goroutine; the diff and the hooks run on the function's queue.
func (f *functionObservers) onFunctionRegistered(msg *FunctionRegisteredMessage, epoch uint64) {
	f.mu.Lock()
	if f.disposed {
		f.mu.Unlock()
		return
	}
	f.confirmedEpoch = int64(epoch)
	f.releasedEpoch = -1
	f.namespace = msg.Namespace
	f.revision = int64(msg.Revision)
	f.mu.Unlock()
	f.dispatch.Store(&dispatchState{enabled: msg.LocalDispatch && f.localDispatchOption, namespace: msg.Namespace})
	list := slices.Clone(msg.Observers)
	f.enqueue(func() { f.applySet(list, msg.Revision, msg.Namespace, epoch) })
}

func (f *functionObservers) onObserverAdded(msg *ObserverAddedMessage, epoch uint64) {
	f.mu.Lock()
	if f.disposed || int64(msg.Revision) <= f.revision {
		f.mu.Unlock()
		return
	}
	f.revision = int64(msg.Revision)
	f.mu.Unlock()
	info := msg.Observer
	f.enqueue(func() {
		// A fresh add of an observer this owner rejected starts over: removed, then added.
		f.mu.Lock()
		rejected := f.known[info.ID] != nil && f.known[info.ID].rejection != nil
		f.mu.Unlock()
		if rejected {
			f.remove(info.ID)
		}
		f.add(info, msg.Revision, msg.Namespace, epoch)
	})
}

func (f *functionObservers) onObserverRemoved(msg *ObserverRemovedMessage) {
	f.mu.Lock()
	if f.disposed || int64(msg.Revision) <= f.revision {
		f.mu.Unlock()
		return
	}
	f.revision = int64(msg.Revision)
	f.mu.Unlock()
	id := msg.ID
	f.enqueue(func() { f.remove(id) })
}

// onRegistrationRejected (FUNCTION_NAMESPACE_CONFLICT on connection epoch): no local
// dispatch, no observers; every held observer goes through OnObserverRemoved, and the
// function's reports are gone on that connection.
func (f *functionObservers) onRegistrationRejected(epoch uint64) {
	f.mu.Lock()
	f.releasedEpoch = int64(epoch)
	f.mu.Unlock()
	f.release()
}

// release drops the confirmation, local dispatch and the observer set. The observers
// are removed on the function's queue, after the adds queued before: an add still
// queued or running when the release happens sees it and undoes its own hook (add).
func (f *functionObservers) release() {
	f.mu.Lock()
	f.confirmedEpoch = -1
	f.syncedEpoch = -1
	for _, obs := range f.compiledSet() {
		obs.active.Store(false)
	}
	f.compiled.Store(&[]*Observer{})
	f.mu.Unlock()
	f.dispatch.Store(&dispatchState{})
	f.enqueue(func() {
		f.mu.Lock()
		ids := make([]string, 0, len(f.known))
		for id := range f.known {
			ids = append(ids, id)
		}
		f.mu.Unlock()
		slices.Sort(ids)
		for _, id := range ids {
			f.remove(id)
		}
	})
}

// releasedLocked: replaced, or refused / not confirmed by the engine. Its observers
// are not to be activated or acked.
func (f *functionObservers) releasedLocked() bool {
	return f.disposed || f.confirmedEpoch < 0
}

// dispose: the function was replaced by a new registration of the same id (whose
// state now answers for its reports).
func (f *functionObservers) dispose() {
	f.mu.Lock()
	f.disposed = true
	f.mu.Unlock()
	f.release()
}

func (f *functionObservers) isDisposed() bool {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.disposed
}

func (f *functionObservers) enqueue(task func()) {
	f.qmu.Lock()
	f.tasks = append(f.tasks, task)
	if !f.running {
		f.running = true
		go f.drain()
	}
	f.qmu.Unlock()
}

func (f *functionObservers) drain() {
	for {
		f.qmu.Lock()
		if len(f.tasks) == 0 {
			f.running = false
			f.qmu.Unlock()
			return
		}
		task := f.tasks[0]
		f.tasks[0] = nil
		f.tasks = f.tasks[1:]
		f.qmu.Unlock()
		func() {
			defer func() {
				if r := recover(); r != nil {
					log.Printf("iii: observe hook failed for %q: %v", f.functionID, r)
				}
			}()
			task()
		}()
	}
}

// activateLocked puts obs in the compiled set (before its ack is sent).
func (f *functionObservers) activateLocked(obs *Observer) {
	if obs.active.Load() {
		return
	}
	obs.active.Store(true)
	next := append(slices.Clone(f.compiledSet()), obs)
	f.compiled.Store(&next)
}

func (f *functionObservers) deactivateLocked(obs *Observer) {
	if !obs.active.Load() {
		return
	}
	obs.active.Store(false)
	next := slices.DeleteFunc(slices.Clone(f.compiledSet()), func(o *Observer) bool { return o == obs })
	f.compiled.Store(&next)
}

func (f *functionObservers) ack(id string, revision uint64, namespace string, epoch uint64, errBody *ErrorBody) {
	frame, err := prepareJSON(&ObserverAckMessage{FunctionID: f.functionID, Namespace: namespace, ID: id, Revision: revision, Error: errBody})
	if err != nil {
		log.Printf("iii: observerack for %q not encodable: %v", id, err)
		return
	}
	f.client.sendOnEpoch(epoch, frame)
}

// applySet diffs the engine's full set against what this SDK holds: removed ->
// OnObserverRemoved; new -> OnObserverAdded; kept -> acked again without the hook (the
// engine marks a replayed set pending until acked). A kept observer that differs in
// any field is removed, then added.
func (f *functionObservers) applySet(list []ObserverInfo, revision uint64, namespace string, epoch uint64) {
	incoming := make(map[string]bool, len(list))
	for _, info := range list {
		incoming[info.ID] = true
	}
	f.mu.Lock()
	var gone []string
	for id := range f.known {
		if !incoming[id] {
			gone = append(gone, id)
		}
	}
	f.mu.Unlock()
	slices.Sort(gone)
	for _, id := range gone {
		f.remove(id)
	}
	for _, info := range list {
		f.add(info, revision, namespace, epoch)
	}
	f.mu.Lock()
	synced := f.confirmedEpoch == int64(epoch)
	if synced {
		f.syncedEpoch = int64(epoch)
	}
	f.mu.Unlock()
	if synced {
		// reports held for this function can go now, after the acks
		f.client.reporter.kick()
	}
}

// add is handleAdded (§4.2): the hook, then the compiled set, then the ack. A kept
// observer is acked again (with its late rejection, if the owner rejected it:
// SDK-PARITY decision 11).
func (f *functionObservers) add(info ObserverInfo, revision uint64, namespace string, epoch uint64) {
	if f.isDisposed() {
		return
	}
	obs, compileErr := newObserver(f, info, revision)
	f.mu.Lock()
	existing := f.known[info.ID]
	f.mu.Unlock()
	if existing != nil {
		if compileErr == nil && existing.sameAs(obs) {
			f.keep(existing, revision, namespace, epoch)
			return
		}
		f.remove(info.ID)
	}
	if compileErr != nil {
		f.ack(info.ID, revision, namespace, epoch, &ErrorBody{Code: string(filter.CodeInvalidFilter), Message: compileErr.Error()})
		return
	}
	if f.hooks != nil && f.hooks.OnObserverAdded != nil {
		onAdded := f.hooks.OnObserverAdded
		f.mu.Lock()
		obs.adding = true
		f.mu.Unlock()
		outcome := f.runHook(func(ctx context.Context) error { return onAdded(ctx, obs) })
		if outcome.body != nil {
			f.mu.Lock()
			obs.adding = false
			f.mu.Unlock()
			f.ack(info.ID, revision, namespace, epoch, outcome.body)
			if outcome.late != nil {
				// It may still finish and index the observer: undo that once it does.
				go func() {
					if err := <-outcome.late; err == nil {
						f.enqueue(func() { f.callRemoved(obs) })
					}
				}()
			}
			return
		}
	}
	// adding ends in the same critical section as the insert: a Reject is either seen
	// below or finds the observer known.
	f.mu.Lock()
	obs.adding = false
	if f.releasedLocked() {
		// released (a conflict, a replacement) while its hook ran: as removed, no ack
		f.mu.Unlock()
		f.callRemoved(obs)
		return
	}
	f.known[obs.id] = obs
	// Rejected (obs.Reject) while its hook was still running: the rejection wins
	// (SDK-PARITY decision 24). It stays known, so the engine's observerremoved runs
	// OnObserverRemoved and undoes what the hook indexed.
	if rejection := obs.rejection; rejection != nil {
		f.mu.Unlock()
		f.ack(obs.id, revision, namespace, epoch, rejection)
		return
	}
	// In the compiled set before the ack: the first event after "active" is evaluated.
	f.activateLocked(obs)
	f.mu.Unlock()
	f.ack(obs.id, revision, namespace, epoch, nil)
}

// keep acks an observer the hook already ran for, again. A late rejection stays in
// force until a fresh observeradded starts the observer over.
func (f *functionObservers) keep(obs *Observer, revision uint64, namespace string, epoch uint64) {
	f.mu.Lock()
	if f.releasedLocked() {
		f.mu.Unlock()
		return // the release's own task removes it
	}
	obs.revision = revision
	rejection := obs.rejection
	if rejection == nil {
		f.activateLocked(obs)
	}
	f.mu.Unlock()
	f.ack(obs.id, revision, namespace, epoch, rejection)
}

func (f *functionObservers) remove(id string) {
	f.mu.Lock()
	obs := f.known[id]
	if obs == nil {
		f.mu.Unlock()
		return
	}
	delete(f.known, id)
	f.deactivateLocked(obs)
	f.mu.Unlock()
	f.callRemoved(obs)
}

func (f *functionObservers) callRemoved(obs *Observer) {
	if f.hooks == nil || f.hooks.OnObserverRemoved == nil {
		return
	}
	onRemoved := f.hooks.OnObserverRemoved
	outcome := f.runHook(func(ctx context.Context) error {
		onRemoved(ctx, obs)
		return nil
	})
	if outcome.body != nil {
		log.Printf("iii: OnObserverRemoved failed for observer %s of %q: %s", obs.id, f.functionID, outcome.body.Message)
	}
}

type hookOutcome struct {
	// body is the rejection; nil when the hook succeeded.
	body *ErrorBody
	// late delivers the hook's result after a timeout.
	late <-chan error
}

func (f *functionObservers) runHook(fn func(ctx context.Context) error) hookOutcome {
	timeout := f.client.observeHookTimeout()
	ctx, cancel := context.WithTimeout(f.client.lifeCtx, timeout)
	done := make(chan error, 1)
	go func() {
		var err error
		defer func() {
			if r := recover(); r != nil {
				err = fmt.Errorf("observe hook panicked: %v", r)
			}
			done <- err
		}()
		err = fn(ctx)
	}()
	select {
	case err := <-done:
		cancel()
		if err == nil {
			return hookOutcome{}
		}
		return hookOutcome{body: rejectionBody(err)}
	case <-ctx.Done():
		late := make(chan error, 1)
		go func() {
			err := <-done
			cancel()
			late <- err
		}()
		return hookOutcome{
			body: &ErrorBody{Code: "OWNER_TIMEOUT", Message: fmt.Sprintf("observe hook of %s still running after %s", f.functionID, timeout)},
			late: late,
		}
	}
}

func rejectionBody(err error) *ErrorBody {
	var rej *ObserveRejectionError
	if errors.As(err, &rej) {
		return &ErrorBody{Code: rej.Code, Message: rej.Message}
	}
	return &ErrorBody{Code: "OBSERVE_REJECTED", Message: err.Error()}
}

func onAccepts(on On, ok bool) bool {
	switch on {
	case OnComplete:
		return true
	case OnSuccess:
		return ok
	case OnError:
		return !ok
	}
	return false
}

// ── owner delivery ───────────────────────────────────────────────────────────

func (f *functionObservers) notOwnerDelivered(what string) error {
	return &InvocationError{
		Code:       "NOT_OWNER_DELIVERED",
		Message:    fmt.Sprintf("obs.%s() needs an owner-delivered function; %s is auto-delivered (ObserveHooks.Delivery)", what, f.functionID),
		FunctionID: f.functionID,
	}
}

func (f *functionObservers) deliverable(obs *Observer) bool {
	f.mu.Lock()
	defer f.mu.Unlock()
	return obs.active.Load() && f.known[obs.id] == obs
}

// eventCaller is the caller an event carries (sdk.md §1): inside the function's own
// handler, that invocation's caller; inside another handler, that handler; outside any
// handler, this worker only. own reports the first case.
func (f *functionObservers) eventCaller(cur *invocation) (caller *Caller, own bool) {
	if cur != nil && cur.functionID == f.functionID {
		return cloneCaller(cur.caller), true
	}
	workerID, workerName := f.client.identity()
	c := &Caller{WorkerID: workerID, WorkerName: workerName}
	if cur != nil {
		c.FunctionID = cur.functionID
		c.InvocationID = cur.invocationID
		c.Parent = stripCaller(cur.caller)
	}
	return c, false
}

func (f *functionObservers) currentNamespace() string {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.namespace
}

// localCall is a finished local invocation of an auto function.
type localCall struct {
	invocationID string
	ok           bool
	input        json.RawMessage
	output       json.RawMessage
	err          *ErrorBody
	caller       *Caller
	metadata     json.RawMessage
	startedAt    time.Time
	duration     time.Duration
	observeDepth uint32
	trace        traceContext
}

// reportLocal evaluates set (the compiled set captured when the call started) on a
// finished local invocation and reports it when an observer matches. Never panics on
// user data: the evaluator is total.
func (f *functionObservers) reportLocal(set []*Observer, call localCall) {
	if f.delivery != DeliveryAuto || len(set) == 0 {
		return
	}
	doc := &lazyDoc{input: call.input, caller: call.caller, metadata: call.metadata}
	if call.ok {
		doc.output = call.output
	} else {
		doc.errBody = call.err
	}
	var matched []string
	var captures []Capture
	for _, obs := range set {
		if !onAccepts(obs.on, call.ok) || !obs.filter.Matches(doc) {
			continue
		}
		matched = append(matched, obs.id)
		for _, c := range obs.capture {
			if !slices.Contains(captures, c) {
				captures = append(captures, c)
			}
		}
	}
	if len(matched) == 0 {
		return
	}
	values := reportValues{input: call.input, caller: call.caller, metadata: call.metadata}
	status := "success"
	if call.ok {
		values.output = call.output
	} else {
		status = "error"
		// the error as the routed path reports it: the handler's error body
		values.errBody, _ = json.Marshal(call.err)
	}
	f.client.reporter.push(reportInit{
		functionID:   f.functionID,
		namespace:    f.currentNamespace(),
		invocationID: call.invocationID,
		matched:      matched,
		status:       status,
		values:       values,
		startedAtMs:  call.startedAt.UnixMilli(),
		durationMs:   float64(call.duration) / float64(time.Millisecond),
		observeDepth: call.observeDepth,
		trace:        call.trace,
	}, captures)
}

// Observer is an observer as the observed function's owner sees it (in
// [ObserveHooks.OnObserverAdded] and [Client.Observers]): its args, filters, target,
// capture and on. Never its metadata, policy or the observing worker. Its fields are
// set when it is added; the SDK keeps its own copy of what it reads, so changing them
// changes nothing but the owner's view. The pointer is stable while the observer exists,
// so owners keep it in their own index (expression -> observers, route table).
type Observer struct {
	ID string
	// FunctionID is the observed function (the one this worker hosts).
	FunctionID string
	// Args are the observer's args, validated against args_format by the engine
	// (numbers decode as float64; see DecodeArgs).
	Args map[string]any
	// Filters are the observer's filters in normal form; the SDK applies them in
	// Emit and Call.
	Filters map[string]any
	// Target is the target function id.
	Target  string
	Capture []Capture
	// On is for auto functions only.
	On On
	// Legacy is set when the engine's legacy shim created the observer from an old
	// RegisterTrigger: Legacy.Config is the old trigger config.
	Legacy *LegacyTrigger

	owner *functionObservers
	// The SDK's own copies of what it reads: the exported fields are the owner's.
	id      string
	target  string
	capture []Capture
	on      On
	argsRaw json.RawMessage
	filter  *filter.Filter
	// key identifies the engine's view of the observer, to tell a kept one from a
	// changed one.
	key    string
	active atomic.Bool
	// guarded by owner.mu
	revision  uint64
	rejection *ErrorBody
	// adding: its OnObserverAdded hook is running (a Reject now wins over the hook).
	adding bool
}

func newObserver(f *functionObservers, info ObserverInfo, revision uint64) (*Observer, error) {
	obs := &Observer{
		ID:         info.ID,
		FunctionID: f.functionID,
		Target:     info.Target,
		Capture:    slices.Clone(info.Capture),
		On:         info.On,
		Legacy:     info.Legacy,
		owner:      f,
		revision:   revision,
	}
	if obs.Capture == nil {
		obs.Capture = slices.Clone(defaultCapture)
	}
	if obs.On == "" {
		obs.On = OnSuccess
	}
	obs.id, obs.target, obs.capture, obs.on = info.ID, info.Target, slices.Clone(obs.Capture), obs.On
	if info.Legacy != nil {
		legacy := *info.Legacy
		legacy.Config = bytes.Clone(info.Legacy.Config)
		obs.Legacy = &legacy
	}
	obs.argsRaw = objectOrEmpty(info.Args)
	_ = json.Unmarshal(obs.argsRaw, &obs.Args)
	filtersRaw := objectOrEmpty(info.Filters)
	_ = json.Unmarshal(filtersRaw, &obs.Filters)
	legacyKey := ""
	if info.Legacy != nil {
		legacyKey = info.Legacy.TriggerType + "\x00" + canonicalJSON(info.Legacy.Config)
	}
	obs.key = canonicalJSON(obs.argsRaw) + "\x00" + canonicalJSON(filtersRaw) + "\x00" + obs.target + "\x00" +
		fmt.Sprint(obs.capture) + "\x00" + string(obs.on) + "\x00" + legacyKey
	// the engine pushes the normal form, compiled without the size limit
	flt, err := filter.ParseNormalized(filtersRaw)
	if err != nil {
		return nil, err
	}
	obs.filter = flt
	return obs, nil
}

// objectOrEmpty maps an absent or null JSON value to {}.
func objectOrEmpty(raw json.RawMessage) json.RawMessage {
	trimmed := bytes.TrimSpace(raw)
	if len(trimmed) == 0 || bytes.Equal(trimmed, []byte("null")) {
		return json.RawMessage("{}")
	}
	return raw
}

// canonicalJSON re-encodes a JSON value with sorted keys and numbers as written.
func canonicalJSON(raw json.RawMessage) string {
	if len(raw) == 0 {
		return ""
	}
	v, err := decodeJSONValue(raw)
	if err != nil {
		return string(raw)
	}
	out, err := encodeCompact(v)
	if err != nil {
		return string(raw)
	}
	return string(out)
}

func (o *Observer) sameAs(other *Observer) bool { return o.key == other.key }

// DecodeArgs unmarshals the observer's args into v (a struct, typically).
func (o *Observer) DecodeArgs(v any) error { return json.Unmarshal(o.argsRaw, v) }

// Matches evaluates the observer's filters on doc.
func (o *Observer) Matches(doc ObserveDoc) bool { return o.filter.Matches(observeDocView(doc)) }

// Emit hands event to this observer, fire-and-forget (owner delivery). ctx is the
// handler's context: the event's caller follows it (inside the observed function's
// own handler the event is that invocation; inside another handler, that handler is
// the caller; outside any handler, this worker). It returns false and sends nothing
// when the observer's filters reject the event or the observer is no longer active.
// It fails with NOT_OWNER_DELIVERED on an auto function.
func (o *Observer) Emit(ctx context.Context, event any) (bool, error) {
	f := o.owner
	if f.delivery != DeliveryOwner {
		return false, f.notOwnerDelivered("Emit")
	}
	if !f.deliverable(o) {
		return false, nil
	}
	eventJSON, err := marshalPayload(event)
	if err != nil {
		return false, err
	}
	cur := invocationFrom(ctx)
	caller, own := f.eventCaller(cur)
	var metadata json.RawMessage
	var trace traceContext
	var depth uint32
	if cur != nil {
		metadata, trace, depth = cur.metadata, cur.trace, cur.observeDepth
	}
	if !o.filter.Matches(&lazyDoc{input: eventJSON, caller: caller, metadata: metadata}) {
		return false, nil
	}
	invocationID := uuid.NewString()
	if own {
		invocationID = cur.invocationID
	}
	f.client.reporter.pushEmit(reportInit{
		functionID:   f.functionID,
		namespace:    f.currentNamespace(),
		invocationID: invocationID,
		status:       "success",
		values:       reportValues{input: eventJSON, caller: caller, metadata: metadata},
		startedAtMs:  time.Now().UnixMilli(),
		observeDepth: depth,
		trace:        trace,
	}, o.capture, o.id, eventJSON, cur)
	return true, nil
}

// Call invokes the observer's target with payload through the engine and returns its
// result (owner delivery, request/response). ctx bounds the wait and carries the
// caller, as for Emit. It fails with FILTERS_NOT_MATCHED (nothing sent) when the
// observer's filters reject the payload, OBSERVER_NOT_ACTIVE when the observer is no
// longer active, and NOT_OWNER_DELIVERED on an auto function; an error the engine
// answers comes back as an *InvocationError with its code.
func (o *Observer) Call(ctx context.Context, payload any) (json.RawMessage, error) {
	f := o.owner
	if f.delivery != DeliveryOwner {
		return nil, f.notOwnerDelivered("Call")
	}
	if !f.deliverable(o) {
		return nil, &InvocationError{Code: "OBSERVER_NOT_ACTIVE", Message: fmt.Sprintf("observer %s of %s is not active", o.id, f.functionID), FunctionID: f.functionID}
	}
	payloadJSON, err := marshalPayload(payload)
	if err != nil {
		return nil, err
	}
	cur := invocationFrom(ctx)
	caller, _ := f.eventCaller(cur)
	var metadata json.RawMessage
	if cur != nil {
		metadata = cur.metadata
	}
	if !o.filter.Matches(&lazyDoc{input: payloadJSON, caller: caller, metadata: metadata}) {
		return nil, &InvocationError{Code: "FILTERS_NOT_MATCHED", Message: fmt.Sprintf("the filters of observer %s reject this payload", o.id), FunctionID: f.functionID}
	}
	// No namespace: the engine routes an activation call to the observer's stored
	// target (CONTRACT D13).
	return f.client.invokeRemote(ctx, TriggerRequest{FunctionID: o.target, Data: payloadJSON}, "", &Activation{ObserverID: o.id})
}

// Reject sends a late rejection for this observer (e.g. after a config reload removed
// what it needs). It leaves the compiled set at once; OnObserverRemoved runs when the
// engine's observerremoved arrives. The rejection is re-sent when the engine replays
// the observer after a reconnect.
func (o *Observer) Reject(code, message string) {
	f := o.owner
	f.mu.Lock()
	if o.adding {
		// the add acks with it once the hook returns
		o.rejection = &ErrorBody{Code: code, Message: message}
		f.mu.Unlock()
		return
	}
	if f.known[o.id] != o {
		f.mu.Unlock()
		return
	}
	o.rejection = &ErrorBody{Code: code, Message: message}
	f.deactivateLocked(o)
	revision, namespace, epoch := o.revision, f.namespace, f.confirmedEpoch
	f.mu.Unlock()
	if epoch >= 0 {
		f.ack(o.id, revision, namespace, uint64(epoch), &ErrorBody{Code: code, Message: message})
	}
}

// marshalPayload is a value as JSON: a json.RawMessage is validated and copied, any
// other value is encoded with encoding/json.
func marshalPayload(v any) (json.RawMessage, error) {
	if raw, ok := v.(json.RawMessage); ok {
		if raw == nil {
			return json.RawMessage("null"), nil
		}
		if !json.Valid(raw) {
			_, err := json.Marshal(raw)
			return nil, err
		}
		return bytes.Clone(raw), nil
	}
	return json.Marshal(v)
}

// decodeJSONValue decodes JSON keeping numbers as json.Number (the filter evaluator
// compares them as doubles).
func decodeJSONValue(raw []byte) (any, error) {
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	var v any
	if err := dec.Decode(&v); err != nil {
		return nil, err
	}
	return v, nil
}

// lazyDoc is the document of a reported invocation or event, decoded root by root on
// first use and shared by every observer evaluated on it (one goroutine).
type lazyDoc struct {
	input, output, metadata json.RawMessage
	errBody                 *ErrorBody
	caller                  *Caller

	decoded [3]any
	ok      [3]bool
	done    [3]bool
}

func (d *lazyDoc) part(i int, raw json.RawMessage) (any, bool) {
	if raw == nil {
		return nil, false
	}
	if !d.done[i] {
		d.done[i] = true
		v, err := decodeJSONValue(raw)
		d.decoded[i], d.ok[i] = v, err == nil
	}
	return d.decoded[i], d.ok[i]
}

// Root implements filter.Doc.
func (d *lazyDoc) Root(root filter.Root) (any, bool) {
	switch root {
	case filter.RootInput:
		return d.part(0, d.input)
	case filter.RootOutput:
		return d.part(1, d.output)
	case filter.RootMetadata:
		return d.part(2, d.metadata)
	case filter.RootError:
		if d.errBody == nil {
			return nil, false
		}
		m := map[string]any{"code": d.errBody.Code, "message": d.errBody.Message}
		if d.errBody.Stacktrace != nil {
			m["stacktrace"] = *d.errBody.Stacktrace
		}
		return m, true
	case filter.RootCaller:
		if d.caller == nil {
			return nil, false
		}
		return callerDoc(d.caller), true
	}
	return nil, false
}

// observeDocView reads an ObserveDoc as a filter.Doc: nil fields are absent.
type observeDocView ObserveDoc

func (d observeDocView) Root(root filter.Root) (any, bool) {
	switch root {
	case filter.RootInput:
		return d.Input, d.Input != nil
	case filter.RootOutput:
		return d.Output, d.Output != nil
	case filter.RootError:
		return d.Error, d.Error != nil
	case filter.RootMetadata:
		return d.Metadata, d.Metadata != nil
	case filter.RootCaller:
		if d.Caller == nil {
			return nil, false
		}
		return callerDoc(d.Caller), true
	}
	return nil, false
}
