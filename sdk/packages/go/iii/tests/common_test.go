//go:build integration

// Package iii_test holds the live-engine integration suite for the iii Go SDK. Unlike
// the in-package unit tests (which use a mock engine), these tests require a real engine
// and are gated behind the "integration" build tag so `go test ./...` stays hermetic:
//
//	go test -tags integration ./tests/...
//
// They mirror the Rust suite under sdk/packages/rust/iii/tests and honor the same
// III_URL / III_HTTP_URL environment variables the sdk-rust-ci job sets. With no env
// vars they target a default local engine (`iii --use-default-config`).
package iii_test

import (
	"context"
	"fmt"
	"net/http"
	"os"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// suffixCounter makes per-call ids unique across `go test -count=N` and concurrent runs
// against the same engine, which keeps a global route/function registry.
var suffixCounter atomic.Uint64

// uniqueSuffix returns a short token unique to this test invocation, derived from the
// test name plus a monotonic counter. Use it to build engine-global identifiers (HTTP
// routes, trigger ids) so retries and repeated runs don't collide.
func uniqueSuffix(t *testing.T) string {
	t.Helper()
	name := strings.NewReplacer("/", "-", " ", "-").Replace(t.Name())
	return fmt.Sprintf("%s-%d", name, suffixCounter.Add(1))
}

// engineWSURL is the worker WebSocket endpoint. Default matches a local
// `iii --use-default-config`; CI overrides it to the test engine on :49199.
func engineWSURL() string {
	if v := os.Getenv("III_URL"); v != "" {
		return v
	}
	return "ws://localhost:49134"
}

// engineHTTPURL is the engine HTTP API base, for exercising HTTP triggers.
func engineHTTPURL() string {
	if v := os.Getenv("III_HTTP_URL"); v != "" {
		return v
	}
	return "http://localhost:3111"
}

// settle waits for registrations to propagate on the engine before a dependent action,
// mirroring common::settle() in the Rust suite (~300ms).
func settle() { time.Sleep(300 * time.Millisecond) }

// connect builds a client against the live engine and connects it, registering cleanup.
// Each test gets its own client so registrations don't leak between tests.
func connect(t *testing.T) *iii.Client {
	return connectNamed(t, "")
}

// connectNamed is connect with an explicit worker name (iii.WithName). A unique name lets
// a test correlate engine::workers::list entries to the worker it actually opened.
func connectNamed(t *testing.T, name string) *iii.Client {
	t.Helper()
	var opts []iii.Option
	if name != "" {
		opts = append(opts, iii.WithName(name))
	}
	c := iii.New(engineWSURL(), opts...)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatalf("connect to engine at %s: %v (is an engine running? `iii --use-default-config`)", engineWSURL(), err)
	}
	t.Cleanup(func() { _ = c.Close() })
	return c
}

// ctxFor returns a context with a sensible per-call timeout and registers its cancel.
func ctxFor(t *testing.T, d time.Duration) context.Context {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), d)
	t.Cleanup(cancel)
	return ctx
}

// httpClient is a plain HTTP client for hitting HTTP triggers on the engine.
func httpClient() *http.Client { return &http.Client{Timeout: 10 * time.Second} }
