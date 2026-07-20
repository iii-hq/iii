package iii

import (
	"context"
	"encoding/json"
	"testing"
	"time"
)

// TestReconnectSendsReattach is the Go half of iii-hq/iii#1975: on reconnect the
// client must present the previous engine-assigned worker id via a reattach frame
// BEFORE replaying its registrations, and must send no reattach on the first connect.
func TestReconnectSendsReattach(t *testing.T) {
	m := newMockEngine(t)
	c := New(m.url, WithReconnectConfig(ReconnectConfig{
		InitialDelay:      10 * time.Millisecond,
		MaxDelay:          50 * time.Millisecond,
		BackoffMultiplier: 2,
		JitterFactor:      0,
		MaxRetries:        -1,
	}))
	_ = c.RegisterFunction("reattach::fn", func(ctx context.Context, _ json.RawMessage) (any, error) {
		return nil, nil
	})
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatalf("Connect: %v", err)
	}
	t.Cleanup(func() { _ = c.Close() })

	// First connect: the function registers, and no reattach is sent yet.
	first := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return countRegister(msgs, string(MsgRegisterFunction), "reattach::fn") >= 1
	}, 2*time.Second)
	if countType(first, string(MsgReattach)) != 0 {
		t.Fatalf("no reattach must be sent on the first connect, got %d", countType(first, string(MsgReattach)))
	}

	// The engine assigns this connection an identity.
	m.mu.Lock()
	conn := m.active
	m.mu.Unlock()
	if err := m.send(ctx, conn, &WorkerRegisteredMessage{WorkerID: "w-old", ReattachToken: "tok-old"}); err != nil {
		t.Fatalf("send workerregistered: %v", err)
	}
	// Give the dispatch loop time to store the id before the socket drops.
	time.Sleep(200 * time.Millisecond)

	m.clear()
	m.closeActiveConnection()

	// After reconnect, a reattach carrying the old id must precede the replay.
	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return countRegister(msgs, string(MsgRegisterFunction), "reattach::fn") >= 1
	}, 3*time.Second)

	reattachIdx := indexOfType(got, string(MsgReattach))
	registerIdx := indexOfType(got, string(MsgRegisterFunction))
	if reattachIdx < 0 {
		t.Fatalf("reattach frame must be sent on reconnect; frames: %v", got)
	}
	if reattachIdx >= registerIdx {
		t.Errorf("reattach (idx %d) must precede the registration replay (idx %d)", reattachIdx, registerIdx)
	}
	if pid := stringField(got[reattachIdx], "previous_worker_id"); pid != "w-old" {
		t.Errorf("reattach previous_worker_id = %q, want %q", pid, "w-old")
	}
	if tok := stringField(got[reattachIdx], "reattach_token"); tok != "tok-old" {
		t.Errorf("reattach reattach_token = %q, want %q", tok, "tok-old")
	}
}
