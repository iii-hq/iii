package iii

import (
	"context"
	"encoding/json"
	"log"
	"reflect"
	"slices"
	"sync"
)

// This file is what a worker does when its functions are observed: the three hooks,
// set for one function by id (on the client or on its FunctionRef) or, for added and
// removed, for every function the worker hosts; the Call they read; and Keyed
// delivery.

// AddedHook runs when an observer is added to a function this worker hosts, before the
// SDK acks it. Returning nil accepts the observer; an error built with
// [ObserveRejection] rejects it with that code, any other error with OBSERVE_REJECTED.
// ctx is cancelled after 30 s, when the observer fails with OWNER_TIMEOUT.
type AddedHook func(ctx context.Context, obs *Observer) error

// RemovedHook runs once for every accepted observer when it is removed (unobserve,
// disposal, late rejection), and for each level whose [AddedHook] resolved when another
// level rejects the observer. A panic is logged; the observer is removed either way.
type RemovedHook func(ctx context.Context, obs *Observer)

// DeliverHook decides which observers get each call of one function and what its caller
// gets back. It receives the call, the function's active observers when the call
// started, and next, which is iii's delivery for the observers passed. What it returns
// is the caller's result, encoded as a handler's is; the error it returns is the
// caller's error. It runs inside the function's invocation, so calls made with ctx
// have that invocation as their caller.
type DeliverHook func(ctx context.Context, call *Call, observers []*Observer, next Next) (any, error)

// Next is iii's delivery: it delivers call to each of observers whose on and filters
// match it and that no earlier next of the same call delivered, without waiting for
// the targets, and returns the call's outcome: its Output, or its Error as an
// *[InvocationError]. A call that differs from the handler's is reported as given. The
// engine drops a delivery to an observer that is not active.
type Next func(ctx context.Context, call *Call, observers []*Observer) (any, error)

// Call is one call of a function, as its host sees it in a [DeliverHook], in [Keyed]
// and in [Observer.Matches]. The fields of a call the SDK hands over are read-only: to
// deliver a different call, pass next a copy with other values.
type Call struct {
	FunctionID   string
	InvocationID string
	// Status is "success" or "error". Empty reads as "error" when Error is set, and
	// "success" otherwise.
	Status string
	// Input is the call's input, as the handler received it.
	Input json.RawMessage
	// Output is the handler's return value as JSON: what observers observe. Nil on an
	// error.
	Output json.RawMessage
	// Error is the error the handler returned. Nil on success.
	Error *ErrorBody
	// Caller is who made the call; nil for a call made outside any handler.
	Caller *Caller
	// Metadata is the call's metadata sidecar; nil when it carries none.
	Metadata json.RawMessage
}

// succeeded reports the call's status.
func (call *Call) succeeded() bool {
	switch call.Status {
	case "success":
		return true
	case "error":
		return false
	}
	return call.Error == nil
}

// outcome is what next returns for call: its output, or its error.
func (call *Call) outcome() (any, error) {
	if call.succeeded() {
		return call.Output, nil
	}
	body := call.Error
	if body == nil {
		body = &ErrorBody{Code: "invocation_failed", Message: "the call failed"}
	}
	return nil, newInvocationError(body, call.FunctionID)
}

// doc is the call as filters read it.
func (call *Call) doc() *lazyDoc {
	doc := &lazyDoc{input: call.Input, caller: call.Caller, metadata: call.Metadata}
	if call.succeeded() {
		doc.output = call.Output
	} else {
		doc.errBody = call.Error
	}
	return doc
}

// Identity is a handler that returns the input it is called with. An event source
// registers it: its observers observe each event as both input and output.
func Identity(_ context.Context, data json.RawMessage) (any, error) { return data, nil }

// observerHooks are the hooks of one level: one function, or every function.
type observerHooks struct {
	added AddedHook
	// addedGen counts the settings of added: an observer accepted with an older
	// generation is caught up with the hook (catchUp).
	addedGen uint64
	removed  RemovedHook
	deliver  DeliverHook
}

func (h *observerHooks) empty() bool {
	return h == nil || (h.added == nil && h.removed == nil && h.deliver == nil)
}

// hookLevel names a level of the added and removed hooks.
type hookLevel int

const (
	workerLevel hookLevel = iota
	functionLevel
)

// OnObserverAdded sets the hook that runs when an observer is added to any function
// this worker hosts, before the function's own; nil removes it. A hook set while
// functions have active observers runs for each of them at once (see
// [Client.OnObserverAddedFor]). It returns the client, for chaining.
func (c *Client) OnObserverAdded(hook AddedHook) *Client {
	c.mu.Lock()
	c.hookGen++
	c.workerHooks.added, c.workerHooks.addedGen = hook, c.hookGen
	var functions []*functionObservers
	if hook != nil {
		for _, fn := range c.functions {
			functions = append(functions, fn.observe)
		}
	}
	c.mu.Unlock()
	for _, f := range functions {
		f.catchUp(workerLevel)
	}
	return c
}

// OnObserverAddedFor sets the hook that runs when an observer is added to functionID,
// after the worker's; nil removes it. Hooks are stored by function id: set them before
// RegisterFunction, so they run for every observer, and they stay across
// [FunctionRef.Unregister] and a later registration of the id.
//
// A hook set once the function already has active observers is caught up: it runs for
// each of them at once, an error late-rejects that observer, and the SDK logs one
// warning naming the function and how many observers were active without it. It
// returns the client, for chaining.
func (c *Client) OnObserverAddedFor(functionID string, hook AddedHook) *Client {
	f := c.setFunctionHook(functionID, func(h *observerHooks, _ *functionObservers) {
		c.hookGen++
		h.added, h.addedGen = hook, c.hookGen
	})
	if f != nil && hook != nil {
		f.catchUp(functionLevel)
	}
	return c
}

// OnObserverRemoved sets the hook that runs when an observer is removed from any
// function this worker hosts, after the function's own; nil removes it. It returns the
// client, for chaining.
func (c *Client) OnObserverRemoved(hook RemovedHook) *Client {
	c.mu.Lock()
	c.workerHooks.removed = hook
	c.mu.Unlock()
	return c
}

// OnObserverRemovedFor sets the hook that runs when an observer is removed from
// functionID, before the worker's; nil removes it. It is stored by function id, as
// [Client.OnObserverAddedFor] is. It returns the client, for chaining.
func (c *Client) OnObserverRemovedFor(functionID string, hook RemovedHook) *Client {
	c.setFunctionHook(functionID, func(h *observerHooks, _ *functionObservers) { h.removed = hook })
	return c
}

// OnObserverDeliverFor sets how calls of functionID are delivered; nil restores iii's
// delivery, which delivers each call to every observer whose on and filters match it.
// It is stored by function id, as [Client.OnObserverAddedFor] is. A function with a
// deliver hook needs an engine with observe.v2: against an older one it stays
// unregistered, and the SDK logs UNSUPPORTED once. A deliver hook does not run for a
// function registered with [Client.RegisterHTTPFunction]: the engine delivers its
// calls. It returns the client, for chaining.
func (c *Client) OnObserverDeliverFor(functionID string, hook DeliverHook) *Client {
	c.setFunctionHook(functionID, func(h *observerHooks, f *functionObservers) {
		h.deliver = hook
		if f != nil {
			f.setDeliver(hook)
		}
	})
	c.reconsiderRegistration(functionID)
	return c
}

// setFunctionHook applies set to functionID's hooks, under mu, with the function's
// observe state when it is registered (nil otherwise), and returns that state.
func (c *Client) setFunctionHook(functionID string, set func(*observerHooks, *functionObservers)) *functionObservers {
	c.mu.Lock()
	defer c.mu.Unlock()
	h := c.functionHooks[functionID]
	if h == nil {
		h = &observerHooks{}
		c.functionHooks[functionID] = h
	}
	var f *functionObservers
	if fn, ok := c.functions[functionID]; ok {
		f = fn.observe
	}
	set(h, f)
	if h.empty() {
		delete(c.functionHooks, functionID)
	}
	return f
}

// addedHookGen is the added hook of one level for functionID, and its generation.
func (c *Client) addedHookGen(functionID string, level hookLevel) (AddedHook, uint64) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if level == workerLevel {
		return c.workerHooks.added, c.workerHooks.addedGen
	}
	if h := c.functionHooks[functionID]; h != nil {
		return h.added, h.addedGen
	}
	return nil, 0
}

// removedHook is the removed hook of one level for functionID.
func (c *Client) removedHook(functionID string, level hookLevel) RemovedHook {
	c.mu.Lock()
	defer c.mu.Unlock()
	if level == workerLevel {
		return c.workerHooks.removed
	}
	if h := c.functionHooks[functionID]; h != nil {
		return h.removed
	}
	return nil
}

// deliverHookLocked is functionID's deliver hook. The caller holds mu.
func (c *Client) deliverHookLocked(functionID string) DeliverHook {
	if h := c.functionHooks[functionID]; h != nil {
		return h.deliver
	}
	return nil
}

// OnObserverAdded sets the function's added hook, as [Client.OnObserverAddedFor] does
// for its id, and returns the handle, for chaining.
func (r *FunctionRef) OnObserverAdded(hook AddedHook) *FunctionRef {
	if r != nil && r.client != nil {
		r.client.OnObserverAddedFor(r.ID, hook)
	}
	return r
}

// OnObserverRemoved sets the function's removed hook, as [Client.OnObserverRemovedFor]
// does for its id, and returns the handle, for chaining.
func (r *FunctionRef) OnObserverRemoved(hook RemovedHook) *FunctionRef {
	if r != nil && r.client != nil {
		r.client.OnObserverRemovedFor(r.ID, hook)
	}
	return r
}

// OnObserverDeliver sets the function's deliver hook, as [Client.OnObserverDeliverFor]
// does for its id, and returns the handle, for chaining.
func (r *FunctionRef) OnObserverDeliver(hook DeliverHook) *FunctionRef {
	if r != nil && r.client != nil {
		r.client.OnObserverDeliverFor(r.ID, hook)
	}
	return r
}

// keyed is the spec of a [Keyed] deliver hook.
type keyed struct {
	observerKeys func(*Observer) []string
	callKeys     func(*Call) []string
}

// Keyed is a deliver hook that delivers each call to the observers filed under one of
// its keys, each at most once, through next (so on, filters, policy and deliver
// apply). observerKeys gives the keys an observer is filed under: the SDK computes
// them once, when the observer is accepted, and unfiles the observer when it is
// removed. callKeys gives the keys a call is delivered to. Wildcards are keys too: an
// observer of every table is filed under "primary|*|insert", and a change is delivered
// to "primary|orders|insert", "primary|orders|*", "primary|*|insert" and
// "primary|*|*".
//
// On a function delivered by Keyed, [FunctionRef.IsObservedFor] answers with one
// lookup per call key.
func Keyed(observerKeys func(*Observer) []string, callKeys func(*Call) []string) DeliverHook {
	k := &keyed{observerKeys: observerKeys, callKeys: callKeys}
	return k.deliver
}

// keyedDeliverCode identifies a Keyed hook: every one is a method value of
// (*keyed).deliver, which shares its code.
var keyedDeliverCode = reflect.ValueOf(DeliverHook((*keyed)(nil).deliver)).Pointer()

// keyedProbe is the context key that asks a Keyed hook for its spec.
type keyedProbe struct{}

// asKeyed returns the spec of a hook made by Keyed, or nil for any other hook. Only a
// hook with Keyed's code is called, and it answers the probe without delivering.
func asKeyed(hook DeliverHook) *keyed {
	if hook == nil || reflect.ValueOf(hook).Pointer() != keyedDeliverCode {
		return nil
	}
	var spec *keyed
	_, _ = hook(context.WithValue(context.Background(), keyedProbe{}, &spec), nil, nil, nil)
	return spec
}

func (k *keyed) deliver(ctx context.Context, call *Call, observers []*Observer, next Next) (any, error) {
	if probe, ok := ctx.Value(keyedProbe{}).(**keyed); ok {
		*probe = k
		return nil, nil
	}
	keys := k.keysOfCall(call)
	if run := deliveryFrom(ctx); run != nil {
		if idx := run.f.keyed.Load(); idx != nil && idx.spec == k {
			return next(ctx, call, idx.lookup(keys))
		}
	}
	// not indexed by the SDK (the hook was wrapped, or its index is being built)
	var chosen []*Observer
	for _, obs := range observers {
		if slices.ContainsFunc(k.keysOfObserver(obs), func(key string) bool { return slices.Contains(keys, key) }) {
			chosen = append(chosen, obs)
		}
	}
	return next(ctx, call, chosen)
}

// keysOfObserver runs observerKeys; a panic files the observer under no key.
func (k *keyed) keysOfObserver(obs *Observer) (keys []string) {
	defer func() {
		if r := recover(); r != nil {
			log.Printf("iii: Keyed observer keys failed for observer %s of %q: %v", obs.id, obs.FunctionID, r)
			keys = nil
		}
	}()
	return k.observerKeys(obs)
}

// keysOfCall runs callKeys; a panic delivers the call to no key.
func (k *keyed) keysOfCall(call *Call) (keys []string) {
	defer func() {
		if r := recover(); r != nil {
			log.Printf("iii: Keyed call keys failed for %q: %v", call.FunctionID, r)
			keys = nil
		}
	}()
	return k.callKeys(call)
}

// keyedIndex is one function's index of its active observers by key, for a Keyed
// deliver hook. It holds exactly the active observers: an observer is filed when it is
// activated (on the function's hook queue) and unfiled when it is deactivated.
type keyedIndex struct {
	spec *keyed

	mu    sync.RWMutex
	byKey map[string][]*Observer
	filed map[*Observer][]string
}

func newKeyedIndex(spec *keyed) *keyedIndex {
	return &keyedIndex{spec: spec, byKey: map[string][]*Observer{}, filed: map[*Observer][]string{}}
}

// file files obs under keys while it is active. The caller computed keys outside any
// lock; a deactivation that ran meanwhile left obs inactive, and it is not filed.
func (x *keyedIndex) file(obs *Observer, keys []string) {
	x.mu.Lock()
	defer x.mu.Unlock()
	if _, done := x.filed[obs]; done || !obs.active.Load() {
		return
	}
	var unique []string
	for _, key := range keys {
		if slices.Contains(unique, key) {
			continue
		}
		unique = append(unique, key)
		x.byKey[key] = append(x.byKey[key], obs)
	}
	x.filed[obs] = unique
}

func (x *keyedIndex) unfile(obs *Observer) {
	x.mu.Lock()
	defer x.mu.Unlock()
	keys, ok := x.filed[obs]
	if !ok {
		return
	}
	delete(x.filed, obs)
	for _, key := range keys {
		// lookup copies under the lock, so the bucket is edited in place
		list := slices.DeleteFunc(x.byKey[key], func(o *Observer) bool { return o == obs })
		if len(list) == 0 {
			delete(x.byKey, key)
		} else {
			x.byKey[key] = list
		}
	}
}

// lookup returns the observers filed under any of keys, each once, in key order.
func (x *keyedIndex) lookup(keys []string) []*Observer {
	x.mu.RLock()
	defer x.mu.RUnlock()
	var out []*Observer
	var seen map[*Observer]bool
	for _, key := range keys {
		bucket := x.byKey[key]
		if len(bucket) == 0 {
			continue
		}
		if out == nil {
			out = append([]*Observer(nil), bucket...)
			continue
		}
		if seen == nil {
			seen = make(map[*Observer]bool, len(out)+len(bucket))
			for _, obs := range out {
				seen[obs] = true
			}
		}
		for _, obs := range bucket {
			if !seen[obs] {
				seen[obs] = true
				out = append(out, obs)
			}
		}
	}
	return out
}

// reset unfiles every observer (the function's observers were all released).
func (x *keyedIndex) reset() {
	x.mu.Lock()
	defer x.mu.Unlock()
	x.byKey, x.filed = map[string][]*Observer{}, map[*Observer][]string{}
}

// has reports whether an observer is filed under one of keys.
func (x *keyedIndex) has(keys []string) bool {
	x.mu.RLock()
	defer x.mu.RUnlock()
	for _, key := range keys {
		if len(x.byKey[key]) > 0 {
			return true
		}
	}
	return false
}
