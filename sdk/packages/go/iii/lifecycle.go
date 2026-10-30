package iii

import (
	"context"
	"log"
	"slices"
	"sync/atomic"
)

// This file is the connection lifecycle a worker can follow: connection-state listeners
// (OnConnectionStateChange) and the wait for the engine's acknowledgement of the worker
// (WaitUntilRegistered).

// connStateListener is one OnConnectionStateChange subscription.
type connStateListener struct {
	fn           func(ConnectionState)
	unsubscribed atomic.Bool
}

// connStateEvent is one state to deliver to the listeners subscribed when it was queued
// (for a subscription's first call: to that listener only).
type connStateEvent struct {
	state     ConnectionState
	listeners []*connStateListener
}

// setStateLocked records a connection-state change and queues it for the listeners. A
// state equal to the current one is not a change. c.mu must be held.
func (c *Client) setStateLocked(s ConnectionState) {
	if c.state == s {
		return
	}
	c.state = s
	if len(c.connStateListeners) > 0 {
		c.queueConnStateLocked(connStateEvent{state: s, listeners: slices.Clone(c.connStateListeners)})
	}
}

// queueConnStateLocked appends ev and starts the delivery goroutine unless one is
// running. c.mu must be held.
func (c *Client) queueConnStateLocked(ev connStateEvent) {
	c.connStateEvents = append(c.connStateEvents, ev)
	if !c.connStateDelivering {
		c.connStateDelivering = true
		go c.deliverConnStates()
	}
}

// deliverConnStates calls the listeners of the queued state changes, in order, one
// call at a time, until the queue is empty.
func (c *Client) deliverConnStates() {
	for {
		c.mu.Lock()
		if len(c.connStateEvents) == 0 {
			c.connStateDelivering = false
			c.mu.Unlock()
			return
		}
		ev := c.connStateEvents[0]
		c.connStateEvents[0] = connStateEvent{}
		c.connStateEvents = c.connStateEvents[1:]
		c.mu.Unlock()
		for _, l := range ev.listeners {
			if !l.unsubscribed.Load() {
				callConnStateListener(l.fn, ev.state)
			}
		}
	}
}

// callConnStateListener calls fn, recovering a panic so one listener cannot stop the
// delivery to the others.
func callConnStateListener(fn func(ConnectionState), state ConnectionState) {
	defer func() {
		if r := recover(); r != nil {
			log.Printf("iii: connection state listener panicked on %q: %v", state, r)
		}
	}()
	fn(state)
}

// OnConnectionStateChange calls listener with the current [ConnectionState], then with
// every later change of state ([StateConnecting], [StateConnected],
// [StateReconnecting], [StateDisconnected], [StateFailed]), in the order the changes
// happen. Calls are made one at a time on a goroutine of the client's own, never on the
// subscribing goroutine or on the SDK's read loop, so a listener may call any Client
// method; it should return promptly, since later changes wait for it. A state is
// reported once per change: the same state is never delivered twice in a row. A
// listener that panics is logged and keeps its subscription.
//
// It returns the function that unsubscribes listener: changes not yet delivered when
// it returns are dropped for this listener (a call already under way completes).
// Calling it more than once does nothing. It mirrors the Python SDK's
// add_connection_state_listener.
func (c *Client) OnConnectionStateChange(listener func(ConnectionState)) (unsubscribe func()) {
	if listener == nil {
		return func() {}
	}
	l := &connStateListener{fn: listener}
	c.mu.Lock()
	c.connStateListeners = append(c.connStateListeners, l)
	// Queued behind the changes already pending (which do not include l), so l sees
	// the current state first and every later change after it, never an older one.
	c.queueConnStateLocked(connStateEvent{state: c.state, listeners: []*connStateListener{l}})
	c.mu.Unlock()
	return func() {
		if !l.unsubscribed.CompareAndSwap(false, true) {
			return
		}
		c.mu.Lock()
		c.connStateListeners = slices.DeleteFunc(c.connStateListeners, func(x *connStateListener) bool { return x == l })
		c.mu.Unlock()
	}
}

// signalRegistered wakes the WaitUntilRegistered callers after a WorkerRegistered.
func (c *Client) signalRegistered() {
	c.mu.Lock()
	close(c.registeredSignal)
	c.registeredSignal = make(chan struct{})
	c.mu.Unlock()
}

// WaitUntilRegistered blocks until the engine has acknowledged this worker on the
// current connection, with the WorkerRegistered frame it sends on each connection, and
// then returns nil. While the client is disconnected it waits for the next connection's
// acknowledgement.
//
// It returns the error of [Client.FatalError] when the worker was terminally rejected
// (for example a WORKER_NAMESPACE_CONFLICT, or a blank [WithNamespace]), and ctx.Err()
// when ctx is done first. Like the Rust SDK's wait_until_registered, which it mirrors
// with ctx in place of a timeout, it returns [ErrNotConnected] when the client was never
// started (call it after [Client.Connect] or [RegisterWorker]), has been closed, or gave
// up reconnecting ([ReconnectConfig].MaxRetries).
func (c *Client) WaitUntilRegistered(ctx context.Context) error {
	for {
		c.mu.Lock()
		fatal, registered, signal := c.fatalErr, c.registeredOnConnection, c.registeredSignal
		c.mu.Unlock()
		switch {
		case fatal != nil:
			return fatal
		case registered:
			return nil
		case !c.started.Load():
			return ErrNotConnected
		}
		select {
		case <-signal:
		case <-c.fatal:
		case <-c.failed:
			if err := c.FatalError(); err != nil {
				return err
			}
			return ErrNotConnected
		case <-c.shutdown:
			return ErrNotConnected
		case <-ctx.Done():
			return ctx.Err()
		}
	}
}
