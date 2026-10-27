// Package workerconnection holds the RBAC hook types of the iii worker-manager, the Go
// counterpart of the Node @iii-dev/helpers/worker-connection-manager module and the Rust
// iii_helpers::worker_connection_manager module.
//
// The worker-manager rbac block names functions the engine calls for untrusted
// connections on the RBAC port:
//
//   - auth_function_id receives an [AuthInput] during the WebSocket upgrade and returns
//     an [AuthResult] (or an error to refuse the connection).
//   - on_trigger_type_registration_function_id receives an
//     [OnTriggerTypeRegistrationInput] and returns an [OnTriggerTypeRegistrationResult].
//   - on_trigger_registration_function_id receives an [OnTriggerRegistrationInput] and
//     returns an [OnTriggerRegistrationResult].
//   - on_function_registration_function_id receives an [OnFunctionRegistrationInput]
//     and returns an [OnFunctionRegistrationResult].
//
// A hook returns the (possibly remapped) fields, or an error to deny the registration;
// fields left empty in a result keep the value of the original registration request.
// The middleware input is in the core package: iii.MiddlewareFunctionInput.
//
// The JSON names of every field are the wire contract and match the Node and Rust types
// exactly. Arbitrary JSON values (context, config, metadata) are json.RawMessage, as in
// the core Go SDK.
package workerconnection

import "encoding/json"

// AuthInput is the input passed to the RBAC auth function during the WebSocket upgrade.
// It carries the HTTP headers, query parameters and client IP of the connecting
// worker's upgrade request.
type AuthInput struct {
	// Headers holds the HTTP headers of the WebSocket upgrade request.
	Headers map[string]string `json:"headers"`
	// QueryParams holds the query parameters of the upgrade URL. Each key maps to all of
	// its values, so repeated keys (?a=1&a=2) are preserved.
	QueryParams map[string][]string `json:"query_params"`
	// IPAddress is the IP address of the connecting client.
	IPAddress string `json:"ip_address"`
}

// AuthResult is the value the RBAC auth function returns. It controls which functions
// the authenticated worker can invoke and what context is forwarded to the middleware.
//
// Every field is optional; the zero value of a field means "use the engine default":
// no namespace-scoped grants, no extra allowed or forbidden functions, every trigger
// type allowed, trigger type registration denied, function registration allowed, an
// empty context and no function ID prefix.
type AuthResult struct {
	// Namespaces grants functions scoped to one namespace each, for example
	// {"orders": ["svc::*"]}. A value is an exact function ID or a wildcard, in either
	// the bare (svc::*) or the match("svc::*") spelling.
	//
	// The keys are also the namespaces the session may declare on
	// engine::workers::register. Leave it nil to add no namespace-scoped grants: the
	// session may then declare any namespace, and only AllowedFunctions and the
	// expose_functions config apply.
	Namespaces map[string][]string `json:"namespaces,omitempty"`
	// AllowedFunctions lists additional function IDs to allow beyond the
	// expose_functions config, in the default namespace only. Put per-namespace grants in
	// Namespaces. Empty means none.
	AllowedFunctions []string `json:"allowed_functions,omitempty"`
	// ForbiddenFunctions lists function IDs to deny even if they match
	// expose_functions. It takes precedence over the allowed lists. Empty means none.
	ForbiddenFunctions []string `json:"forbidden_functions,omitempty"`
	// AllowedTriggerTypes lists the trigger type IDs the worker may register triggers
	// for. nil (sent as null) allows every trigger type; a non-nil empty slice (sent as
	// []) allows none.
	AllowedTriggerTypes []string `json:"allowed_trigger_types"`
	// AllowTriggerTypeRegistration reports whether the worker may register new trigger
	// types. The default is false.
	AllowTriggerTypeRegistration bool `json:"allow_trigger_type_registration,omitempty"`
	// AllowFunctionRegistration reports whether the worker may register new functions.
	// nil means the engine default, true; set it to new(bool) to deny function
	// registration.
	AllowFunctionRegistration *bool `json:"allow_function_registration,omitempty"`
	// Context is arbitrary JSON forwarded to the middleware function on every invocation
	// (iii.MiddlewareFunctionInput.Context). nil means an empty object.
	Context json.RawMessage `json:"context,omitempty"`
	// FunctionRegistrationPrefix is an optional prefix applied to every function ID this
	// worker registers (the engine registers prefix::id). Empty means no prefix.
	FunctionRegistrationPrefix string `json:"function_registration_prefix,omitempty"`
}

// OnTriggerTypeRegistrationInput is the input passed to the
// on_trigger_type_registration_function_id hook when a worker attempts to register a
// new trigger type through the RBAC port. Return an [OnTriggerTypeRegistrationResult]
// with the (possibly mapped) fields, or an error to deny the registration.
type OnTriggerTypeRegistrationInput struct {
	// TriggerTypeID is the ID of the trigger type being registered.
	TriggerTypeID string `json:"trigger_type_id"`
	// Description is the human-readable description of the trigger type.
	Description string `json:"description"`
	// Context is the auth context from [AuthResult].Context for this session.
	Context json.RawMessage `json:"context"`
}

// OnTriggerTypeRegistrationResult is the result returned from the
// on_trigger_type_registration_function_id hook. Every field is optional; an omitted
// field keeps the original value from the registration request.
type OnTriggerTypeRegistrationResult struct {
	// TriggerTypeID is the mapped trigger type ID. Empty keeps the original.
	TriggerTypeID string `json:"trigger_type_id,omitempty"`
	// Description is the mapped description. nil keeps the original; a pointer to ""
	// clears it.
	Description *string `json:"description,omitempty"`
}

// OnTriggerRegistrationInput is the input passed to the
// on_trigger_registration_function_id hook when a worker attempts to register a trigger
// through the RBAC port. Return an [OnTriggerRegistrationResult] with the (possibly
// mapped) fields, or an error to deny the registration.
type OnTriggerRegistrationInput struct {
	// TriggerID is the ID of the trigger being registered.
	TriggerID string `json:"trigger_id"`
	// TriggerType is the trigger type identifier.
	TriggerType string `json:"trigger_type"`
	// FunctionID is the ID of the function this trigger is bound to.
	FunctionID string `json:"function_id"`
	// Config is the trigger-specific configuration.
	Config json.RawMessage `json:"config"`
	// Metadata is the arbitrary metadata attached to the trigger, if any (the engine
	// sends null when there is none).
	Metadata json.RawMessage `json:"metadata,omitempty"`
	// Namespace is the namespace the trigger's target resolves in (explicit, or
	// "default" when the registration names none). The same function ID can exist in
	// several namespaces, so the hook needs it to authorize per target namespace. Empty
	// means "default".
	Namespace string `json:"namespace,omitempty"`
	// Context is the auth context from [AuthResult].Context for this session.
	Context json.RawMessage `json:"context"`
}

// OnTriggerRegistrationResult is the result returned from the
// on_trigger_registration_function_id hook. Every field is optional; an omitted field
// keeps the original value from the registration request.
type OnTriggerRegistrationResult struct {
	// TriggerID is the mapped trigger ID. Empty keeps the original.
	TriggerID string `json:"trigger_id,omitempty"`
	// TriggerType is the mapped trigger type. Empty keeps the original.
	TriggerType string `json:"trigger_type,omitempty"`
	// FunctionID is the mapped function ID. Empty keeps the original.
	FunctionID string `json:"function_id,omitempty"`
	// Config is the mapped trigger configuration. nil keeps the original.
	Config json.RawMessage `json:"config,omitempty"`
}

// OnFunctionRegistrationInput is the input passed to the
// on_function_registration_function_id hook when a worker attempts to register a
// function through the RBAC port. Return an [OnFunctionRegistrationResult] with the
// (possibly mapped) fields, or an error to deny the registration.
type OnFunctionRegistrationInput struct {
	// FunctionID is the ID of the function being registered.
	FunctionID string `json:"function_id"`
	// Description is the human-readable description of the function, if any.
	Description string `json:"description,omitempty"`
	// Metadata is the arbitrary metadata attached to the function, if any (the engine
	// sends null when there is none).
	Metadata json.RawMessage `json:"metadata,omitempty"`
	// Namespace is the namespace the function registers in. The same ID can exist in
	// several namespaces, so the hook needs it to authorize per namespace. Empty means
	// "default".
	Namespace string `json:"namespace,omitempty"`
	// Context is the auth context from [AuthResult].Context for this session.
	Context json.RawMessage `json:"context"`
}

// OnFunctionRegistrationResult is the result returned from the
// on_function_registration_function_id hook. Every field is optional; an omitted field
// keeps the original value from the registration request.
type OnFunctionRegistrationResult struct {
	// FunctionID is the mapped function ID. Empty keeps the original.
	FunctionID string `json:"function_id,omitempty"`
	// Description is the mapped description. nil keeps the original; a pointer to ""
	// clears it.
	Description *string `json:"description,omitempty"`
	// Metadata is the mapped metadata. nil keeps the original; any value that is set,
	// including the JSON literal null, replaces it.
	Metadata json.RawMessage `json:"metadata,omitempty"`
}
