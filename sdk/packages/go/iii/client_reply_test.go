package iii

import (
	"context"
	"encoding/json"
	"testing"
	"time"
)

// Regression tests for iii-hq/iii#1749: connection-scoped replies must not leak onto a
// later connection. They ride a per-connection reply channel; when there is no live
// connection the reply is dropped.

// TestEnqueueOutboundDirectDropsWhenDisconnected is the deterministic core invariant:
// with no live connection (c.reply == nil), a connection-scoped reply is dropped rather
// than buffered for the next connection. This is what prevents the stale-reply leak.
func TestEnqueueOutboundDirectDropsWhenDisconnected(t *testing.T) {
	c := New("ws://localhost:0")
	// No connection has been established, so reply is nil.
	c.replyMu.Lock()
	if c.reply != nil {
		t.Fatal("reply channel should be nil before any connection")
	}
	c.replyMu.Unlock()

	// Must return immediately without blocking and without queueing onto the shared
	// outbound channel (which the next connection would drain).
	done := make(chan struct{})
	go func() {
		c.enqueueOutboundDirect([]byte(`{"type":"pong"}`))
		close(done)
	}()
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("enqueueOutboundDirect blocked when disconnected; want immediate drop")
	}

	select {
	case f := <-c.outbound:
		t.Fatalf("dropped reply leaked onto the shared outbound channel: %s", f)
	default:
		// good: nothing on the shared channel
	}
}

// TestReplyChannelLifecycle verifies the per-connection reply channel is installed on
// connect and detached on disconnect, so a reply enqueued after the socket drops is
// dropped rather than delivered on the next connection.
func TestReplyChannelLifecycle(t *testing.T) {
	m := newMockEngine(t)
	c := New(m.url, WithReconnectConfig(ReconnectConfig{
		InitialDelay:      10 * time.Millisecond,
		MaxDelay:          50 * time.Millisecond,
		BackoffMultiplier: 2,
		JitterFactor:      0,
		MaxRetries:        -1,
	}))
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatalf("Connect: %v", err)
	}
	t.Cleanup(func() { _ = c.Close() })

	// Connected: a reply channel is installed.
	c.replyMu.Lock()
	haveReply := c.reply != nil
	c.replyMu.Unlock()
	if !haveReply {
		t.Fatal("reply channel should be non-nil while connected")
	}

	// Drop the connection; the teardown detaches the reply channel. Poll for it to go nil
	// (teardown runs on the connection goroutine). Clear the recorded frames before the
	// drop so the post-reconnect assertion sees every frame from the disconnect window
	// onward — clearing later could erase a leaked pong during a fast reconnect.
	m.clear()
	m.closeActiveConnection()
	deadline := time.Now().Add(2 * time.Second)
	detached := false
	for time.Now().Before(deadline) {
		c.replyMu.Lock()
		nilled := c.reply == nil
		c.replyMu.Unlock()
		if nilled {
			detached = true
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	if !detached {
		t.Fatal("reply channel was not detached after disconnect")
	}

	// A reply enqueued in the disconnected window must be dropped (no panic, no block,
	// nothing left to leak onto the reconnected socket).
	c.enqueueOutboundDirect([]byte(`{"type":"pong"}`))

	// After reconnect, the mock must not receive that stale pong. Wait for reconnect
	// (a fresh registration/metadata frame), then assert no pong was recorded. Nothing
	// was cleared since the disconnect, so a leaked pong cannot escape the snapshot.
	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return len(msgs) >= 1 // some frame after reconnect (worker metadata)
	}, 2*time.Second)
	if countType(got, string(MsgPong)) != 0 {
		t.Errorf("stale pong leaked onto the reconnected socket: %d pong frame(s)", countType(got, string(MsgPong)))
	}
}
