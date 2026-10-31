package iii

import (
	"bytes"
	"encoding/json"
	"fmt"
	"maps"
	"net/http"
	"strings"
)

// This file registers HTTP-invoked functions: functions hosted elsewhere (a Lambda, a
// Cloudflare Worker, any HTTP endpoint) that the engine calls over HTTP, as the Node SDK
// does when registerFunction is given an HttpInvocationConfig instead of a handler
// (sdk/packages/node/iii/src/iii.ts).

// RegisterHTTPFunction registers a function that has no handler in this process: the
// engine invokes it by sending an HTTP request as described by invocation, which
// travels as the registration's "invocation" field. invocation.URL is required; an
// empty Method means POST, as in the Node SDK. invocation.Auth, when set, must be valid
// JSON; build it with [HMACAuth], [BearerAuth] or [APIKeyAuth]. Pass at most one
// [RegisterFunctionOptions] for the description, formats, metadata, args_format,
// private, observable or standby; LocalDispatch does not apply, since there is nothing
// to run in-process. The engine delivers the function's calls itself, so a deliver hook
// does not run for it; the added and removed hooks do.
//
// Like every registration it is kept in memory and sent again on each reconnect. An
// empty id, or one this client already registered, is refused as in
// [Client.RegisterFunction]; [FunctionRef.Unregister] on the returned handle removes it.
func (c *Client) RegisterHTTPFunction(id string, invocation HTTPInvocationRef, opts ...RegisterFunctionOptions) (*FunctionRef, error) {
	if strings.TrimSpace(invocation.URL) == "" {
		return nil, fmt.Errorf("iii: RegisterHTTPFunction(%q): invocation URL is empty", id)
	}
	if len(invocation.Auth) > 0 && !json.Valid(invocation.Auth) {
		return nil, fmt.Errorf("iii: RegisterHTTPFunction(%q): invocation Auth must contain valid JSON", id)
	}
	cfg, err := resolveRegisterFunctionOptions("RegisterHTTPFunction", id, opts)
	if err != nil {
		return nil, err
	}
	msg, err := registerFunctionMessage("RegisterHTTPFunction", id, cfg)
	if err != nil {
		return nil, err
	}
	// A copy, so the caller's later changes do not reach the replayed registration.
	ref := HTTPInvocationRef{
		URL:     invocation.URL,
		Method:  invocation.Method,
		Headers: maps.Clone(invocation.Headers),
		Auth:    bytes.Clone(invocation.Auth),
	}
	if ref.Method == "" {
		ref.Method = http.MethodPost
	}
	if invocation.TimeoutMs != nil {
		timeout := *invocation.TimeoutMs
		ref.TimeoutMs = &timeout
	}
	msg.Invocation = &ref
	return c.storeFunction("RegisterHTTPFunction", id, msg, nil, cfg)
}

// HMACAuth returns the [HTTPInvocationRef].Auth that has the engine sign each request
// with HMAC, using the secret held in the engine's environment variable secretKey:
// {"type":"hmac","secret_key":secretKey}. Same shape as the hmac variant of the Node
// SDK's HttpAuthConfig.
func HMACAuth(secretKey string) json.RawMessage {
	return mustMarshalAuth(struct {
		Type      string `json:"type"`
		SecretKey string `json:"secret_key"`
	}{"hmac", secretKey})
}

// BearerAuth returns the [HTTPInvocationRef].Auth that has the engine send
// "Authorization: Bearer <token>", the token being held in the engine's environment
// variable tokenKey: {"type":"bearer","token_key":tokenKey}. Same shape as the bearer
// variant of the Node SDK's HttpAuthConfig.
func BearerAuth(tokenKey string) json.RawMessage {
	return mustMarshalAuth(struct {
		Type     string `json:"type"`
		TokenKey string `json:"token_key"`
	}{"bearer", tokenKey})
}

// APIKeyAuth returns the [HTTPInvocationRef].Auth that has the engine send an API key in
// the request header named header, the key being held in the engine's environment
// variable valueKey: {"type":"api_key","header":header,"value_key":valueKey}. Same
// shape as the api_key variant of the Node SDK's HttpAuthConfig.
func APIKeyAuth(header, valueKey string) json.RawMessage {
	return mustMarshalAuth(struct {
		Type     string `json:"type"`
		Header   string `json:"header"`
		ValueKey string `json:"value_key"`
	}{"api_key", header, valueKey})
}

// mustMarshalAuth encodes an auth variant. Structs of strings always encode.
func mustMarshalAuth(v any) json.RawMessage {
	data, err := json.Marshal(v)
	if err != nil {
		panic(fmt.Sprintf("iii: encoding HTTP auth: %v", err))
	}
	return data
}
