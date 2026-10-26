package iii

import "fmt"

// This file removes registrations: each Unregister* drops the registration from the
// in-memory registries, so a reconnect does not replay it, and tells the engine when
// connected. While disconnected nothing is sent: the engine forgets a connection's
// registrations when it goes away, and the next connection's replay no longer includes
// them (the Node SDK's skipIfClosed).

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

// UnregisterFunction removes a function registered with [Client.RegisterFunction],
// [RegisterFunctionTyped] or [Client.RegisterHTTPFunction]: the engine stops routing
// invocations to it, and a reconnect does not register it again. Its observe state is
// cleared as well: its observers are released (through the OnObserverRemoved hook of
// its [ObserveHooks]), calls stop running in-process, and reports it still buffered are
// dropped. It mirrors unregister() on the FunctionRef the Node SDK's registerFunction
// returns. It returns an error when no function with id is registered.
func (c *Client) UnregisterFunction(id string) error {
	c.mu.Lock()
	fn, ok := c.functions[id]
	if ok {
		delete(c.functions, id)
		delete(c.hooksInactiveWarned, id)
	}
	c.mu.Unlock()
	if !ok {
		return fmt.Errorf("iii: UnregisterFunction(%q): function is not registered", id)
	}
	if fn.observe != nil {
		fn.observe.dispose()
	}
	c.sendUnregistration(&UnregisterFunctionMessage{ID: id})
	return nil
}

// UnregisterTrigger removes a trigger registered with [Client.RegisterTrigger] or
// [Client.RegisterTriggerNamespaced]: the engine receives an unregistertrigger frame
// carrying the trigger's id and trigger type, and a reconnect does not register it
// again. It mirrors unregister() on the Trigger the Node SDK's registerTrigger returns.
// It returns an error when no trigger with id is registered.
func (c *Client) UnregisterTrigger(id string) error {
	c.mu.Lock()
	trigger, ok := c.triggers[id]
	if ok {
		delete(c.triggers, id)
	}
	c.mu.Unlock()
	if !ok {
		return fmt.Errorf("iii: UnregisterTrigger(%q): trigger is not registered", id)
	}
	triggerType := trigger.TriggerType
	c.sendUnregistration(&UnregisterTriggerMessage{ID: id, TriggerType: &triggerType})
	return nil
}

// UnregisterTriggerType removes a trigger type registered with
// [Client.RegisterTriggerType]: the engine retires it (when this connection provides
// it), and a reconnect does not register it again. It mirrors the Node SDK's
// unregisterTriggerType. It returns an error when no trigger type with id is
// registered.
//
// Deprecated: trigger types are replaced by functions with an observe descriptor;
// unregister the function with [Client.UnregisterFunction] instead. Migration guide:
// https://iii.dev/docs/upgrading/from-0-24-x
func (c *Client) UnregisterTriggerType(id string) error {
	c.mu.Lock()
	_, ok := c.triggerTypes[id]
	if ok {
		delete(c.triggerTypes, id)
	}
	c.mu.Unlock()
	if !ok {
		return fmt.Errorf("iii: UnregisterTriggerType(%q): trigger type is not registered", id)
	}
	c.sendUnregistration(&UnregisterTriggerTypeMessage{ID: id})
	return nil
}
