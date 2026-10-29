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

	"github.com/iii-hq/iii/sdk/packages/go/iii/filter"
)

// This file is the hosting side of observe: per-function observer state as the engine
// pushed it (functionregistered, observeradded, observerremoved), the added and removed
// hooks at both levels run on the function's hook queue, and the Observer handed to the
// hooks.

// observeHookTimeout is how long an added hook may run before the observer is
// rejected with OWNER_TIMEOUT.
const observeHookTimeout = 30 * time.Second

// defaultCapture is what an absent capture means.
var defaultCapture = []Capture{CaptureInput, CaptureOutput, CaptureCaller}

// errObserverRemoved is what Accepted returns for an observer removed before the SDK
// accepted it.
var errObserverRemoved = errors.New("iii: observer removed before it was accepted")

// dispatchState is whether Call may run a function in-process, and the namespace the
// engine registered it in. Swapped as one pointer: the hot path loads it once.
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
	localDispatchOption bool
	// private: calls to it pass values by reference.
	private bool

	dispatch atomic.Pointer[dispatchState]
	// state is what a call reads when it starts: the active observers, the deliver hook
	// and the match hook. Swapped under mu, loaded once per call.
	state atomic.Pointer[callState]
	// standingBy: a standby registration is waiting for the function's host to leave.
	standingBy atomic.Bool
	// matchPanicLogged: a panic of the match hook was logged.
	matchPanicLogged atomic.Bool

	mu sync.Mutex
	// confirmedEpoch is the connection functionregistered arrived on, or -1.
	confirmedEpoch int64
	// syncedEpoch is the connection whose functionregistered set was applied and
	// acked, or -1: reports wait for it, so none reaches the engine before the acks
	// that make the replayed observers active again.
	syncedEpoch int64
	// releasedEpoch is the connection on which the engine refused the function, or -1.
	// On that connection its reports are gone (INACTIVE), those of a call already
	// running included; a later registration may be accepted, and functionregistered
	// clears it.
	releasedEpoch int64
	// unsupportedEpoch is the connection on which the function was left unregistered
	// because it needs observe.v2 and the engine lacks it, or -1.
	unsupportedEpoch int64
	namespace        string
	// revision is the highest revision received on this connection.
	revision int64
	known    map[string]*Observer
	disposed bool
	// wantMatch is the match hook last set; the hook queue installs it (applyMatch).
	wantMatch *MatchHook

	qmu     sync.Mutex
	tasks   []func()
	running bool
}

func newFunctionObservers(c *Client, functionID string, localDispatch, private bool) *functionObservers {
	f := &functionObservers{
		client:              c,
		functionID:          functionID,
		localDispatchOption: localDispatch,
		private:             private,
		confirmedEpoch:      -1,
		syncedEpoch:         -1,
		releasedEpoch:       -1,
		unsupportedEpoch:    -1,
		revision:            -1,
		known:               map[string]*Observer{},
	}
	f.dispatch.Store(&dispatchState{})
	f.state.Store(&callState{})
	return f
}

// compiledSet is the active observers.
func (f *functionObservers) compiledSet() []*Observer { return f.state.Load().observers }

func (f *functionObservers) isObserved() bool { return len(f.compiledSet()) > 0 }

// isObservedFor: on a function matched by keys, whether an observer is filed under one
// of the call's keys; otherwise whether any observer is active.
func (f *functionObservers) isObservedFor(call *Call) bool {
	st := f.state.Load()
	if len(st.observers) == 0 {
		return false
	}
	if call == nil || !st.match.byKeys() {
		return true
	}
	return st.match.index.has(f.callKeys(st.match.hook, call))
}

func (f *functionObservers) observers() []*Observer { return slices.Clone(f.compiledSet()) }

// installHooks sets the hooks of a function being registered, before its registration
// is sent: no observer is active yet, so the match hook's index starts empty.
func (f *functionObservers) installHooks(h *observerHooks) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.wantMatch = h.match
	f.state.Store(&callState{deliver: h.deliver, match: newMatchState(h.match)})
}

// setDeliver installs the function's deliver hook: calls that start after it use it.
func (f *functionObservers) setDeliver(hook DeliverHook) {
	f.mu.Lock()
	defer f.mu.Unlock()
	st := *f.state.Load()
	st.deliver = hook
	f.state.Store(&st)
}

// syncedOn: functionregistered arrived on connection epoch and its set was applied
// and acked.
func (f *functionObservers) syncedOn(epoch uint64) bool {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.syncedEpoch == int64(epoch)
}

// reportStatus is what a flush on connection epoch does with this function's
// reports: send them, hold them, or drop them as INACTIVE (the engine refused the
// function on that connection, or it stays unregistered there for lack of observe.v2).
func (f *functionObservers) reportStatus(epoch uint64) reportStatus {
	f.mu.Lock()
	defer f.mu.Unlock()
	switch {
	case f.releasedEpoch == int64(epoch), f.unsupportedEpoch == int64(epoch):
		return reportGone
	case f.syncedEpoch == int64(epoch):
		return reportReady
	}
	return reportWait
}

// onFunctionRegistered is always accepted and replaces the set. It runs on the read
// goroutine; the diff and the hooks run on the function's queue.
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
	f.standingBy.Store(false)
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
		// A fresh add of an observer this host rejected starts over: removed, then added.
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

// onRegistrationRejected (the engine refused the registration on connection epoch): no
// local dispatch, no observers; every held observer goes through the removed hooks,
// and the function's reports are gone on that connection.
func (f *functionObservers) onRegistrationRejected(epoch uint64) {
	f.mu.Lock()
	f.releasedEpoch = int64(epoch)
	f.mu.Unlock()
	f.release()
}

// release drops the confirmation, local dispatch and the observer set. The observers
// are removed on the function's queue, after the adds queued before: an add still
// queued or running when the release happens sees it and undoes its own hooks (add).
func (f *functionObservers) release() {
	f.mu.Lock()
	f.confirmedEpoch = -1
	f.syncedEpoch = -1
	st := *f.state.Load()
	for _, obs := range st.observers {
		obs.active.Store(false)
	}
	st.observers = nil
	if st.match != nil {
		st.match = newMatchState(st.match.hook)
	}
	f.state.Store(&st)
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

// dispose: the function was unregistered (a later registration of the same id has
// observe state of its own).
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

// activateLocked puts obs in the active set (before its ack is sent). The caller holds
// mu.
func (f *functionObservers) activateLocked(obs *Observer) {
	if obs.active.Load() {
		return
	}
	obs.active.Store(true)
	st := *f.state.Load()
	st.observers = append(slices.Clone(st.observers), obs)
	f.state.Store(&st)
}

// deactivateLocked takes obs out of the active set and the key index. The caller holds
// mu.
func (f *functionObservers) deactivateLocked(obs *Observer) {
	if !obs.active.Load() {
		return
	}
	obs.active.Store(false)
	st := *f.state.Load()
	st.observers = slices.DeleteFunc(slices.Clone(st.observers), func(o *Observer) bool { return o == obs })
	f.state.Store(&st)
	if st.match.byKeys() {
		st.match.index.unfile(obs)
	}
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
// removed hooks; new -> added hooks; kept -> acked again without the hooks (the engine
// marks a replayed set pending until acked). A kept observer that differs in any field
// is removed, then added.
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

// add runs the added hooks of both levels, the worker's then the function's, computes
// the observer's match keys when the function is matched by keys, then activates the
// observer and acks it. A hook that fails, or keys that panic, reject the observer, and
// the removed hook of each level whose added hook resolved runs. A kept observer is
// acked again (with its late rejection, if the host rejected it).
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
	f.mu.Lock()
	obs.adding = true
	f.mu.Unlock()
	var resolved []hookLevel
	for _, level := range []hookLevel{workerLevel, functionLevel} {
		hook, gen := f.client.addedHookGen(f.functionID, level)
		obs.addedGen[level] = gen
		if hook == nil {
			continue
		}
		outcome := f.runHook(func(ctx context.Context) error { return hook(ctx, obs) })
		if outcome.body == nil {
			resolved = append(resolved, level)
			continue
		}
		f.refuse(obs, revision, namespace, epoch, outcome.body, resolved)
		if outcome.late != nil {
			// It may still finish and set something up: undo that once it does.
			go func() {
				if err := <-outcome.late; err == nil {
					f.enqueue(func() { f.callRemoved(obs, []hookLevel{level}) })
				}
			}()
		}
		return
	}
	// The keys are computed once, for the match state installed now: only this queue
	// replaces it, and a release, which the critical section below sees.
	match := f.state.Load().match
	var keys []string
	if match.byKeys() {
		var rejection *ErrorBody
		if keys, rejection = f.observerKeys(match.hook, obs); rejection != nil {
			f.refuse(obs, revision, namespace, epoch, rejection, resolved)
			return
		}
	}
	// adding ends in the same critical section as the insert: a Reject is either seen
	// below or finds the observer known.
	f.mu.Lock()
	obs.adding = false
	if f.releasedLocked() {
		// released (a conflict, an unregister) while its hooks ran: as removed, no ack
		f.mu.Unlock()
		obs.settle(errObserverRemoved)
		f.callRemoved(obs, resolved)
		return
	}
	f.known[obs.id] = obs
	// Rejected (obs.Reject) while a hook was still running: the rejection wins. It
	// stays known, so the engine's observerremoved runs the removed hooks and undoes
	// what the added hooks set up.
	if rejection := obs.rejection; rejection != nil {
		f.mu.Unlock()
		f.ack(obs.id, revision, namespace, epoch, rejection)
		obs.settle(&ObserveRejectionError{Code: rejection.Code, Message: rejection.Message})
		return
	}
	// In the active set before the ack: the first call after "active" is delivered.
	f.activateLocked(obs)
	if match.byKeys() && f.state.Load().match == match {
		match.index.file(obs, keys)
	}
	f.mu.Unlock()
	f.ack(obs.id, revision, namespace, epoch, nil)
	obs.settle(nil)
}

// refuse rejects an observer whose add failed with body: it is acked with the error,
// Accepted returns it, and the removed hook of each level in resolved runs.
func (f *functionObservers) refuse(obs *Observer, revision uint64, namespace string, epoch uint64, body *ErrorBody, resolved []hookLevel) {
	f.mu.Lock()
	obs.adding = false
	f.mu.Unlock()
	f.ack(obs.id, revision, namespace, epoch, body)
	obs.settle(&ObserveRejectionError{Code: body.Code, Message: body.Message})
	f.callRemoved(obs, resolved)
}

// keep acks an observer the hooks already ran for, again. A late rejection stays in
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
	obs.settle(errObserverRemoved)
	f.callRemoved(obs, []hookLevel{workerLevel, functionLevel})
}

// callRemoved runs the removed hooks of levels, the function's before the worker's.
func (f *functionObservers) callRemoved(obs *Observer, levels []hookLevel) {
	for _, level := range []hookLevel{functionLevel, workerLevel} {
		if !slices.Contains(levels, level) {
			continue
		}
		hook := f.client.removedHook(f.functionID, level)
		if hook == nil {
			continue
		}
		outcome := f.runHook(func(ctx context.Context) error {
			hook(ctx, obs)
			return nil
		})
		if outcome.body != nil {
			log.Printf("iii: OnObserverRemoved failed for observer %s of %q: %s", obs.id, f.functionID, outcome.body.Message)
		}
	}
}

// catchUp runs the added hook of level, on the queue, for each active observer that
// was accepted without it. An error late-rejects that observer. One warning names the
// function and how many observers were active without the hook.
func (f *functionObservers) catchUp(level hookLevel) {
	f.enqueue(func() {
		hook, gen := f.client.addedHookGen(f.functionID, level)
		if hook == nil {
			return
		}
		var behind []*Observer
		for _, obs := range f.compiledSet() {
			if obs.addedGen[level] < gen {
				behind = append(behind, obs)
			}
		}
		if len(behind) == 0 {
			return
		}
		if level == workerLevel {
			log.Printf("iii: OnObserverAdded was set while %d observers of %q were active without it; running it for each of them", len(behind), f.functionID)
		} else {
			log.Printf("iii: OnObserverAddedFor(%q) was set while %d observers were active without it; running it for each of them", f.functionID, len(behind))
		}
		for _, obs := range behind {
			obs.addedGen[level] = gen
			if !obs.active.Load() {
				continue
			}
			if outcome := f.runHook(func(ctx context.Context) error { return hook(ctx, obs) }); outcome.body != nil {
				obs.Reject(outcome.body.Code, outcome.body.Message)
			}
		}
	})
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

func (f *functionObservers) currentNamespace() string {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.namespace
}

// Observer is an observer as the worker that hosts the observed function sees it (in
// its hooks and in [Client.Observers]): its args, filters, target, capture and on.
// Never its metadata, policy or the observing worker. Its fields are set when it is
// added; the SDK keeps its own copy of what it reads, so changing them changes nothing
// but the host's view. The pointer is stable while the observer exists, so hosts keep
// it in their own index (expression -> observers, route table).
type Observer struct {
	ID string
	// FunctionID is the observed function (the one this worker hosts).
	FunctionID string
	// Args are the observer's args, validated against args_format by the engine
	// (numbers decode as float64; see DecodeArgs).
	Args map[string]any
	// Filters are the observer's filters in normal form; iii's delivery and Matches
	// apply them.
	Filters map[string]any
	// Target is the target function id.
	Target  string
	Capture []Capture
	// On selects the calls the observer receives by status.
	On On

	owner *functionObservers
	// The SDK's own copies of what it reads: the exported fields are the host's.
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
	// addedGen is, per level, the generation of the added hook it was accepted or
	// caught up with (read and written on the function's queue only).
	addedGen [2]uint64
	// guarded by owner.mu
	revision  uint64
	rejection *ErrorBody
	// adding: its added hooks are running (a Reject made meanwhile wins over them).
	adding bool

	// settled is closed once the observer is accepted (settleErr nil), or rejected or
	// removed first.
	settled    chan struct{}
	settleOnce sync.Once
	settleErr  error
}

func newObserver(f *functionObservers, info ObserverInfo, revision uint64) (*Observer, error) {
	obs := &Observer{
		ID:         info.ID,
		FunctionID: f.functionID,
		Target:     info.Target,
		Capture:    slices.Clone(info.Capture),
		On:         info.On,
		owner:      f,
		revision:   revision,
		settled:    make(chan struct{}),
	}
	if obs.Capture == nil {
		obs.Capture = slices.Clone(defaultCapture)
	}
	if obs.On == "" {
		obs.On = OnSuccess
	}
	obs.id, obs.target, obs.capture, obs.on = info.ID, info.Target, slices.Clone(obs.Capture), obs.On
	obs.argsRaw = objectOrEmpty(info.Args)
	_ = json.Unmarshal(obs.argsRaw, &obs.Args)
	filtersRaw := objectOrEmpty(info.Filters)
	_ = json.Unmarshal(filtersRaw, &obs.Filters)
	obs.key = canonicalJSON(obs.argsRaw) + "\x00" + canonicalJSON(filtersRaw) + "\x00" + obs.target + "\x00" +
		fmt.Sprint(obs.capture) + "\x00" + string(obs.on)
	// the engine pushes the normal form, compiled without the size limit
	flt, err := filter.ParseNormalized(filtersRaw)
	if err != nil {
		return nil, err
	}
	obs.filter = flt
	return obs, nil
}

func (o *Observer) sameAs(other *Observer) bool { return o.key == other.key }

// settle resolves Accepted, once.
func (o *Observer) settle(err error) {
	o.settleOnce.Do(func() {
		o.settleErr = err
		close(o.settled)
	})
}

// DecodeArgs unmarshals the observer's args into v (a struct, typically).
func (o *Observer) DecodeArgs(v any) error { return json.Unmarshal(o.argsRaw, v) }

// Matches reports whether call reaches the observer: its on accepts the call's status
// and its filters match the call. A deliver hook that calls targets itself checks it
// before calling through the observer; iii's delivery applies it on its own.
func (o *Observer) Matches(call *Call) bool {
	if call == nil || o.filter == nil {
		return false
	}
	return onAccepts(o.on, call.succeeded()) && o.filter.Matches(call.doc())
}

// Accepted waits until the SDK has acked the observer as accepted, and returns nil. A
// call through the observer made after that reaches an active observer, since the
// engine handles a connection's frames in order: a host sends a new observer its
// initial state this way. It returns an *[ObserveRejectionError] when the observer is
// rejected first, an error when it is removed first, and ctx's error when ctx is done
// first. Once accepted, it returns nil at once.
func (o *Observer) Accepted(ctx context.Context) error {
	if o.settled == nil {
		return errors.New("iii: Accepted on an observer not handed out by the client")
	}
	select {
	case <-o.settled:
		return o.settleErr
	case <-ctx.Done():
		return ctx.Err()
	}
}

// Reject sends a late rejection for this observer (e.g. after a config reload removed
// what it needs). It leaves the compiled set at once, so calls stop reaching it; the
// removed hooks run when the engine's observerremoved arrives. The rejection is sent
// again when the engine replays the observer after a reconnect.
func (o *Observer) Reject(code, message string) {
	f := o.owner
	if f == nil {
		return
	}
	f.mu.Lock()
	if o.adding {
		// the add acks with it once the hooks return
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

// lazyDoc is the document of a call, decoded root by root on first use and shared by
// every observer evaluated on it (one goroutine).
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
