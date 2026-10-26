package iii

import (
	"encoding/json"
	"errors"
	"fmt"
)

// This file holds the handles registrations return (FunctionRef, Trigger,
// TriggerTypeRef), the inputs of trigger and trigger-type registration, and the
// removal of registrations. Removing one drops it from the in-memory registries, so a
// reconnect does not replay it, and tells the engine when connected. While
// disconnected nothing is sent: the engine forgets a connection's registrations when
// it goes away, and the next connection's replay no longer includes them (the Node
// SDK's skipIfClosed).

// errForeignHandle is returned by Unregister on a handle the SDK did not create.
var errForeignHandle = errors.New("iii: Unregister on a handle not returned by the client")

// FunctionRef is the handle of a registered function, returned by
// [Client.RegisterFunction], [Client.RegisterHTTPFunction], [RegisterFunctionTyped] and
// [TriggerTypeRef.RegisterFunction]. It mirrors the FunctionRef of the Node SDK.
type FunctionRef struct {
	// ID is the function id given at registration.
	ID string

	client  *Client
	observe *functionObservers // identifies this registration among later ones of the same id
}

// Unregister removes the function: the engine stops routing invocations to it, and a
// reconnect does not register it again. Its observe state is cleared as well: its
// observers are released (through the OnObserverRemoved hook of its [ObserveHooks]),
// calls stop running in-process, and reports it still buffered are dropped.
//
// As in the Node SDK, Unregister is idempotent, and a handle whose function was
// registered again under the same id (or already removed) no longer controls it:
// Unregister then does nothing and returns nil. It returns an error only for a
// FunctionRef the client did not return.
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
		delete(c.hooksInactiveWarned, r.ID)
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

// Observers returns the active observers of the function, as its owner sees them.
// Empty once the handle no longer controls the registration.
func (r *FunctionRef) Observers() []*Observer {
	if r == nil || r.observe == nil {
		return nil
	}
	return r.observe.observers()
}

// RegisterTriggerInput is the input of [Client.RegisterTrigger]: fire FunctionID when a
// trigger of Type matches Config. It mirrors RegisterTriggerInput in the Node SDK.
type RegisterTriggerInput struct {
	// ID identifies the trigger. Empty means the SDK generates one (a random UUID, as
	// the Node and Python SDKs always do); [Trigger.ID] reports it.
	ID string
	// Type is the trigger type, for example "http", "cron" or "durable:subscriber".
	Type string
	// FunctionID is the function the trigger invokes when it fires.
	FunctionID string
	// Config is the trigger-type-specific configuration, as raw JSON. Nil is sent as {}.
	Config json.RawMessage
	// Metadata is arbitrary JSON delivered to the triggered function on every
	// invocation. Nil omits it.
	Metadata json.RawMessage
	// Namespace is the namespace FunctionID resolves in, sent as "namespace". Empty
	// means this worker's namespace (not the engine's default); a namespace that is
	// set but blank is refused.
	Namespace string
	// TriggerNamespace is the namespace to find the trigger type's provider in, sent as
	// "trigger_namespace". Empty asks the engine to resolve it: this worker's namespace
	// first, then the engine's own. A namespace named here is strict.
	TriggerNamespace string
}

// Trigger is the handle of a registered trigger, returned by [Client.RegisterTrigger]
// and [TriggerTypeRef.RegisterTrigger]. It mirrors the Trigger of the Node SDK.
type Trigger struct {
	// ID is the trigger id: the one given in [RegisterTriggerInput], or the one the SDK
	// generated.
	ID string

	client  *Client
	message *RegisterTriggerMessage // identifies this registration among later ones of the same id
}

// Unregister removes the trigger: the engine receives an unregistertrigger frame with
// the trigger's id and type, and a reconnect does not register it again.
//
// Unregister is idempotent: once the trigger is removed, or registered again under the
// same id, the handle no longer controls it and Unregister does nothing and returns nil.
// It returns an error only for a Trigger the client did not return.
func (t *Trigger) Unregister() error {
	if t == nil || t.client == nil || t.message == nil {
		return errForeignHandle
	}
	c := t.client
	c.mu.Lock()
	current := c.triggers[t.ID] == t.message
	if current {
		delete(c.triggers, t.ID)
	}
	c.mu.Unlock()
	if !current {
		return nil
	}
	triggerType := t.message.TriggerType
	c.sendUnregistration(&UnregisterTriggerMessage{ID: t.ID, TriggerType: &triggerType})
	return nil
}

// RegisterTriggerTypeInput is the input of [Client.RegisterTriggerType]: the trigger
// type's id, its description and the JSON Schemas it advertises. It mirrors
// RegisterTriggerTypeInput in the Node SDK. Nonempty schemas must contain valid JSON;
// JSON Schema semantics are not checked, and handlers remain responsible for
// validation.
//
// Deprecated: trigger types are replaced by functions with an observe descriptor:
// TriggerRequestFormat becomes [ObserveHooks.ArgsFormat] and CallRequestFormat the
// request format of the function. Migration guide:
// https://iii.dev/docs/upgrading/from-0-24-x
type RegisterTriggerTypeInput struct {
	// ID is the trigger type id, for example "webhook".
	ID string
	// Description is a human-readable description of the trigger type.
	Description string
	// TriggerRequestFormat is the JSON Schema of a trigger instance's config.
	// engine::triggers::info exposes it as configuration_schema. Nil omits it.
	TriggerRequestFormat json.RawMessage
	// CallRequestFormat is the JSON Schema of the payload sent to the bound function
	// when the trigger fires. engine::triggers::info exposes it as request_schema. Nil
	// omits it.
	CallRequestFormat json.RawMessage
}

// TriggerTypeRef is the handle of a registered trigger type, returned by
// [Client.RegisterTriggerType]. It registers triggers and functions bound to the type
// without repeating its id, and removes the type. It mirrors the TriggerTypeRef of
// the Node SDK.
//
// Deprecated: trigger types are replaced by functions with an observe descriptor, and
// these helpers by [Client.Observe]. Migration guide:
// https://iii.dev/docs/upgrading/from-0-24-x
type TriggerTypeRef struct {
	// ID is the trigger type id.
	ID string

	client  *Client
	message *RegisterTriggerTypeMessage // identifies this registration among later ones of the same id
}

// RegisterTrigger registers a trigger of this type that fires functionID with config
// (raw JSON; nil is sent as {}) and at most one metadata value. The trigger resolves
// functionID in this worker's namespace, as the Node SDK's TriggerTypeRef does, and
// logs the [Client.RegisterTrigger] deprecation warning once per process.
func (r *TriggerTypeRef) RegisterTrigger(functionID string, config json.RawMessage, metadata ...json.RawMessage) (*Trigger, error) {
	if r == nil || r.client == nil {
		return nil, errors.New("iii: RegisterTrigger on a TriggerTypeRef not returned by the client")
	}
	if len(metadata) > 1 {
		return nil, fmt.Errorf("iii: TriggerTypeRef(%q).RegisterTrigger: expected at most one metadata argument, got %d", r.ID, len(metadata))
	}
	in := RegisterTriggerInput{Type: r.ID, FunctionID: functionID, Config: config, Namespace: r.client.namespace}
	if len(metadata) == 1 {
		in.Metadata = metadata[0]
	}
	warnDeprecated(deprecatedRegisterTrigger)
	return r.client.registerTrigger(in, "RegisterTriggerInput.Namespace")
}

// RegisterFunction registers a function with handler and binds a trigger of this type
// to it with config (raw JSON; nil is sent as {}), in this worker's namespace, as the
// Node SDK's TriggerTypeRef.registerFunction does. Pass at most one
// [RegisterFunctionOptions] for the function.
//
// The returned FunctionRef controls the function only: as in the Node SDK, its
// Unregister removes the function and leaves the trigger bound to the type until the
// type itself is removed. When the trigger cannot be registered (for example config is
// not valid JSON), nothing is registered and the error is returned.
func (r *TriggerTypeRef) RegisterFunction(id string, handler Handler, config json.RawMessage, opts ...RegisterFunctionOptions) (*FunctionRef, error) {
	if r == nil || r.client == nil {
		return nil, errors.New("iii: RegisterFunction on a TriggerTypeRef not returned by the client")
	}
	if len(config) > 0 && !json.Valid(config) {
		return nil, fmt.Errorf("iii: TriggerTypeRef(%q).RegisterFunction(%q): config must contain valid JSON", r.ID, id)
	}
	ref, err := r.client.RegisterFunction(id, handler, opts...)
	if err != nil {
		return nil, err
	}
	warnDeprecated(deprecatedRegisterTrigger)
	if _, err := r.client.registerTrigger(RegisterTriggerInput{Type: r.ID, FunctionID: id, Config: config, Namespace: r.client.namespace}, "RegisterTriggerInput.Namespace"); err != nil {
		_ = ref.Unregister()
		return nil, err
	}
	return ref, nil
}

// Unregister removes the trigger type: the engine retires it (when this connection
// provides it), and a reconnect does not register it again. It mirrors unregister()
// on the Node SDK's TriggerTypeRef. Triggers registered through the handle are not
// removed on the worker's side.
//
// Unregister is idempotent: once the type is removed, or registered again under the
// same id, the handle no longer controls it and Unregister does nothing and returns nil.
// It returns an error only for a TriggerTypeRef the client did not return.
func (r *TriggerTypeRef) Unregister() error {
	if r == nil || r.client == nil || r.message == nil {
		return errForeignHandle
	}
	c := r.client
	c.mu.Lock()
	tt, ok := c.triggerTypes[r.ID]
	current := ok && tt.message == r.message
	if current {
		delete(c.triggerTypes, r.ID)
	}
	c.mu.Unlock()
	if !current {
		return nil
	}
	c.sendUnregistration(&UnregisterTriggerTypeMessage{ID: r.ID})
	return nil
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

// UnregisterTriggerType removes the trigger type registered under id with
// [Client.RegisterTriggerType], as [TriggerTypeRef.Unregister] does: the engine retires
// it (when this connection provides it), and a reconnect does not register it again. It
// mirrors the Node SDK's unregisterTriggerType. It returns an error when no trigger type
// with id is registered.
//
// Deprecated: trigger types are replaced by functions with an observe descriptor;
// unregister the function with [FunctionRef.Unregister] instead. Migration guide:
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
