package iii

import (
	"testing"
	"time"
)

// IsObserved by id takes no lock: it answers while another goroutine holds the client's
// mutex (the read loop's invocations, local calls and every registration take it), and
// it follows Unregister and a later registration of the id. Neither form allocates.
func TestIsObservedByIDTakesNoLock(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	ref := registerHost(t, c, "obs::fn", echoHandler)
	addObserver(t, fe, "obs::fn", observerInfo("o1", "x::y", "", ""))

	c.mu.Lock()
	answered := make(chan [2]bool, 1)
	go func() { answered <- [2]bool{c.IsObserved("obs::fn"), c.IsObserved("other::fn")} }()
	select {
	case got := <-answered:
		c.mu.Unlock()
		if got != [2]bool{true, false} {
			t.Errorf("IsObserved %v, want [true false]", got)
		}
	case <-time.After(time.Second):
		c.mu.Unlock()
		<-answered
		t.Fatal("IsObserved(id) waited for the client's mutex")
	}
	if n := testing.AllocsPerRun(1000, func() {
		_ = c.IsObserved("obs::fn")
		_ = ref.IsObserved()
	}); n != 0 {
		t.Errorf("%v allocations per IsObserved", n)
	}

	if err := ref.Unregister(); err != nil {
		t.Fatal(err)
	}
	if c.IsObserved("obs::fn") || ref.IsObserved() {
		t.Error("observed after Unregister")
	}
	// the engine's set still holds o1: the new registration gets it back
	second := registerHost(t, c, "obs::fn", echoHandler)
	waitUntil(t, "the new registration's observer", func() bool { return c.IsObserved("obs::fn") && second.IsObserved() })
	if ref.IsObserved() {
		t.Error("the unregistered handle reports the new registration's observers")
	}
}

// The cost of IsObserved, the check a source makes before building an event: on the
// handle (one atomic load), by id (a lock-free lookup and the load), and by id from
// every core at once.
func BenchmarkIsObserved(b *testing.B) {
	c := New("ws://unused.invalid")
	b.Cleanup(func() { _ = c.Close() })
	ref, err := c.RegisterFunction("bench::fn", echoHandler)
	if err != nil {
		b.Fatal(err)
	}
	b.Run("FunctionRef", func(b *testing.B) {
		b.ReportAllocs()
		for b.Loop() {
			_ = ref.IsObserved()
		}
	})
	b.Run("ByID", func(b *testing.B) {
		b.ReportAllocs()
		for b.Loop() {
			_ = c.IsObserved("bench::fn")
		}
	})
	b.Run("ByIDParallel", func(b *testing.B) {
		b.ReportAllocs()
		b.RunParallel(func(pb *testing.PB) {
			for pb.Next() {
				_ = c.IsObserved("bench::fn")
			}
		})
	})
}
