package iii

import "errors"

// This file holds the handle a function registration returns (FunctionRef) and the
// removal of registrations. Removing one drops it from the in-memory registry, so a
// reconnect does not replay it, and tells the engine when connected. While
// disconnected nothing is sent: the engine forgets a connection's registrations when
// it goes away, and the next connection's replay no longer includes them (the Node
// SDK's skipIfClosed).

// errForeignHandle is returned by Unregister on a handle the SDK did not create.
var errForeignHandle = errors.New("iii: Unregister on a handle not returned by the client")

// FunctionRef is the handle of a registered function, returned by
// [Client.RegisterFunction], [Client.RegisterHTTPFunction] and [RegisterFunctionTyped].
// It mirrors the FunctionRef of the Node SDK: its hook methods set the function's hooks
// by its id, and return the handle for chaining.
type FunctionRef struct {
	// ID is the function id given at registration.
	ID string

	client  *Client
	observe *functionObservers // identifies this registration among later ones of the same id
}

// Unregister removes the function: the engine stops routing invocations to it, and a
// reconnect does not register it again. Its observe state is cleared as well: its
// observers are released (through the removed hooks), calls stop running in-process,
// and reports it still buffered are dropped. The hooks set for its id stay, for a
// later registration of the id.
//
// As the Node SDK's FunctionRef, Unregister is idempotent: once the function is removed,
// the handle no longer controls the id, also after the id is registered again, and
// Unregister does nothing and returns nil. It returns an error only for a FunctionRef
// the client did not return.
func (r *FunctionRef) Unregister() error {
	if r == nil || r.client == nil || r.observe == nil {
		return errForeignHandle
	}
	c := r.client
	c.mu.Lock()
	fn, ok := c.functions[r.ID]
	current := ok && fn.observe == r.observe
	if current {
		delete(c.functions, r.ID)
		delete(c.unsupportedWarned, r.ID)
	}
	c.mu.Unlock()
	if !current {
		return nil
	}
	r.observe.dispose()
	c.sendUnregistration(&UnregisterFunctionMessage{ID: r.ID})
	return nil
}

// IsObserved reports whether any observer is active on the function (one load: check
// it before building an event nobody would receive). False once the handle no longer
// controls the registration.
func (r *FunctionRef) IsObserved() bool {
	if r == nil || r.observe == nil {
		return false
	}
	return r.observe.isObserved()
}

// IsObservedFor reports whether call would reach an observer, as
// [Client.IsObservedFor] does: on a function delivered by [Keyed], whether an observer
// is filed under one of the call's keys; otherwise [FunctionRef.IsObserved]. False once
// the handle no longer controls the registration.
func (r *FunctionRef) IsObservedFor(call *Call) bool {
	if r == nil || r.observe == nil {
		return false
	}
	return r.observe.isObservedFor(call)
}

// Observers returns the active observers of the function, as its host sees them.
// Empty once the handle no longer controls the registration.
func (r *FunctionRef) Observers() []*Observer {
	if r == nil || r.observe == nil {
		return nil
	}
	return r.observe.observers()
}

// sendUnregistration hands an unregister frame to the writer when connected and drops
// it otherwise. Unlike enqueueOutbound it never parks the frame in the offline buffer,
// which is flushed after the next connection's replay: there, it could remove a
// registration made again under the same id while disconnected.
func (c *Client) sendUnregistration(msg any) {
	frame, err := prepareJSON(msg)
	if err != nil {
		return
	}
	c.mu.Lock()
	connected := c.state == StateConnected
	c.mu.Unlock()
	if !connected {
		return
	}
	select {
	case c.outbound <- frame:
	case <-c.shutdown:
	}
}
