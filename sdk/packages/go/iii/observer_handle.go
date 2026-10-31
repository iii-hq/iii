package iii

import (
	"context"
	"fmt"
	"log"
	"sync"
)

// ObserverHandle is what [Client.Observe] returns: the observing side of one observer.
// It is kept for replay, with the same id, on every connection until it is disposed.
// Safe for concurrent use.
type ObserverHandle struct {
	id     string
	client *Client
	// frame is the observe frame, sent on every connection.
	frame []byte
	seq   uint64

	mu        sync.Mutex
	state     ObserverState
	reason    *ObserveReason
	listeners []stateListener
	nextID    int
	// changed is closed and replaced on every state change (Ready waits on it).
	changed chan struct{}
	// sent: its observe went out and no refusal answered it, so the engine may hold
	// it and disposing it needs an unobserve.
	sent bool
	// events are state changes waiting for their listeners, delivered in order on a
	// goroutine of their own: the SDK's read loop never runs user code.
	events   []stateEvent
	draining bool
}

type stateEvent struct {
	state     ObserverState
	reason    *ObserveReason
	listeners []stateListener
}

type stateListener struct {
	id int
	fn func(ObserverState, *ObserveReason)
}

func newObserverHandle(c *Client, id string, frame []byte, seq uint64) *ObserverHandle {
	return &ObserverHandle{id: id, client: c, frame: frame, seq: seq, state: ObserverPending, changed: make(chan struct{})}
}

// ID is the observer's id, stable across replays.
func (h *ObserverHandle) ID() string { return h.id }

// State is the observer's current state.
func (h *ObserverHandle) State() ObserverState {
	h.mu.Lock()
	defer h.mu.Unlock()
	return h.state
}

// Reason is why the observer is in its state (error, suspended, disposed), or nil.
func (h *ObserverHandle) Reason() *ObserveReason {
	h.mu.Lock()
	defer h.mu.Unlock()
	if h.reason == nil {
		return nil
	}
	r := *h.reason
	return &r
}

// Ready waits until the observer is active. It returns an *ObserveRejectionError
// carrying the code when the observer turns error or disposed first (INVALID_ARGS,
// INVALID_FILTER, FORBIDDEN, SELF_TARGET, an owner rejection, UNSUPPORTED against an
// engine without observe), and ctx's error when ctx is done first.
func (h *ObserverHandle) Ready(ctx context.Context) error {
	for {
		h.mu.Lock()
		state, reason, changed := h.state, h.reason, h.changed
		h.mu.Unlock()
		switch state {
		case ObserverActive:
			return nil
		case ObserverError, ObserverDisposed:
			if reason == nil {
				code := "OBSERVE_FAILED"
				if state == ObserverDisposed {
					code = "UNOBSERVED"
				}
				return &ObserveRejectionError{Code: code, Message: fmt.Sprintf("observer %s is %s", h.id, state)}
			}
			return &ObserveRejectionError{Code: reason.Code, Message: reason.Message}
		}
		select {
		case <-changed:
		case <-ctx.Done():
			return ctx.Err()
		}
	}
}

// OnStateChange calls listener on every state change, in order of the changes and of
// registration, on a goroutine of the handle's own (one change at a time; never on the
// SDK's read loop, so a listener may call the client). It returns the function that
// unsubscribes it; a change already queued is not delivered to it afterwards.
func (h *ObserverHandle) OnStateChange(listener func(state ObserverState, reason *ObserveReason)) (unsubscribe func()) {
	h.mu.Lock()
	h.nextID++
	id := h.nextID
	h.listeners = append(h.listeners, stateListener{id: id, fn: listener})
	h.mu.Unlock()
	return func() {
		h.mu.Lock()
		defer h.mu.Unlock()
		for i, l := range h.listeners {
			if l.id == id {
				h.listeners = append(h.listeners[:i:i], h.listeners[i+1:]...)
				return
			}
		}
	}
}

// Dispose removes the observer (unobserve). It is not replayed on reconnect. An
// observer disposed while disconnected is unobserved on the next connection, before
// any observe is replayed, so a Reattach does not keep it alive. When the engine
// refused the observer's last observe (an observeresult in state error), Dispose
// sends nothing. Its id is free at once: an Observe under it gets a new handle, which
// hears the engine's answers to its own observe only. ctx is reserved; Dispose does
// not wait for the engine.
func (h *ObserverHandle) Dispose(ctx context.Context) error {
	_ = ctx
	if h.State() == ObserverDisposed {
		return nil
	}
	h.client.disposeObserver(h)
	h.setState(ObserverDisposed, &ObserveReason{Code: "UNOBSERVED", Message: "disposed by the observing worker"})
	return nil
}

func (h *ObserverHandle) markSent() {
	h.mu.Lock()
	h.sent = true
	h.mu.Unlock()
}

// markRefused: the engine refused the observe it last got for h and stores nothing
// for it.
func (h *ObserverHandle) markRefused() {
	h.mu.Lock()
	h.sent = false
	h.mu.Unlock()
}

func (h *ObserverHandle) wasSent() bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	return h.sent
}

// setState applies a state from the engine (observeresult, observerstatechanged) or
// from the SDK. A disposed observer stays disposed.
func (h *ObserverHandle) setState(state ObserverState, reason *ObserveReason) {
	h.mu.Lock()
	if h.state == ObserverDisposed {
		h.mu.Unlock()
		return
	}
	changed := state != h.state || !sameReason(reason, h.reason)
	h.state = state
	if reason != nil {
		r := *reason
		h.reason = &r
		reason = &r
	} else {
		h.reason = nil
	}
	if !changed {
		h.mu.Unlock()
		return
	}
	close(h.changed)
	h.changed = make(chan struct{})
	if len(h.listeners) > 0 {
		h.events = append(h.events, stateEvent{state: state, reason: reason, listeners: append([]stateListener(nil), h.listeners...)})
		if !h.draining {
			h.draining = true
			go h.deliver()
		}
	}
	h.mu.Unlock()
}

// deliver calls the listeners of queued state changes, in order.
func (h *ObserverHandle) deliver() {
	for {
		h.mu.Lock()
		if len(h.events) == 0 {
			h.draining = false
			h.mu.Unlock()
			return
		}
		ev := h.events[0]
		h.events[0] = stateEvent{}
		h.events = h.events[1:]
		h.mu.Unlock()
		for _, l := range ev.listeners {
			if h.subscribed(l.id) {
				callListener(h.id, l.fn, ev.state, ev.reason)
			}
		}
	}
}

func (h *ObserverHandle) subscribed(id int) bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	for _, l := range h.listeners {
		if l.id == id {
			return true
		}
	}
	return false
}

func callListener(id string, fn func(ObserverState, *ObserveReason), state ObserverState, reason *ObserveReason) {
	defer func() {
		if r := recover(); r != nil {
			log.Printf("iii: observer %s state listener panicked: %v", id, r)
		}
	}()
	var arg *ObserveReason
	if reason != nil {
		r := *reason
		arg = &r
	}
	fn(state, arg)
}

func sameReason(a, b *ObserveReason) bool {
	if a == nil || b == nil {
		return a == b
	}
	return *a == *b
}

// toObserverState reads a wire state; one this SDK does not know (a newer engine)
// reads as pending.
func toObserverState(s string) ObserverState {
	switch st := ObserverState(s); st {
	case ObserverPending, ObserverActive, ObserverSuspended, ObserverError, ObserverDisposed:
		return st
	}
	return ObserverPending
}
