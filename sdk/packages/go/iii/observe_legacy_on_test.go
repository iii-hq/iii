package iii

import (
	"encoding/json"
	"fmt"
	"log"
	"os"
	"reflect"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
)

// lockedLog is a log output safe to read while the client's goroutines write to it.
type lockedLog struct {
	mu  sync.Mutex
	buf strings.Builder
}

func (l *lockedLog) Write(p []byte) (int, error) {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.buf.Write(p)
}

func (l *lockedLog) count(s string) int {
	l.mu.Lock()
	defer l.mu.Unlock()
	return strings.Count(l.buf.String(), s)
}

// captureLockedLog sends the standard logger to a lockedLog for the test.
func captureLockedLog(t *testing.T) *lockedLog {
	t.Helper()
	logs := &lockedLog{}
	log.SetOutput(logs)
	t.Cleanup(func() { log.SetOutput(os.Stderr) })
	return logs
}

var legacyOnCases = []struct {
	capture []Capture
	on      string
}{
	{nil, ""},
	{[]Capture{CaptureInput}, ""},
	{[]Capture{}, ""},
	{[]Capture{CaptureInput, CaptureOutput, CaptureCaller, CaptureMetadata}, ""},
	{[]Capture{CaptureError}, "error"},
	{[]Capture{CaptureError, CaptureCaller}, "error"},
	{[]Capture{CaptureInput, CaptureError}, "complete"},
	{[]Capture{CaptureInput, CaptureOutput, CaptureError}, "complete"},
}

var (
	engine025Features = []string{FeatureObserveV1, FeatureLocalDispatchV1}
	rcEngineFeatures  = []string{FeatureObserveV1, FeatureObserveV2, FeatureLocalDispatchV1}
	captureFeatures   = []string{FeatureObserveV1, FeatureObserveV2, "observe.capture", FeatureLocalDispatchV1}
)

// observeEach observes once per case on an engine with features and returns the
// source.on of each observe frame by observer id, on the first connection and on the
// replay after a reconnect, and the log.
func observeEach(t *testing.T, features ...string) (first, replay map[string]string, logs *lockedLog) {
	t.Helper()
	logs = captureLockedLog(t)
	fe := newFakeEngine(t)
	fe.setFeatures(features...)
	var mu sync.Mutex
	sent := map[string]string{}
	fe.setOnFrame(func(msg map[string]json.RawMessage) {
		if messageType(msg) != "observe" {
			return
		}
		var source map[string]json.RawMessage
		_ = json.Unmarshal(msg["source"], &source)
		on := ""
		if raw, ok := source["on"]; ok {
			_ = json.Unmarshal(raw, &on)
			if on == "" {
				on = "<present>"
			}
		}
		mu.Lock()
		sent[messageID(msg)] = on
		mu.Unlock()
	})
	collect := func() map[string]string {
		waitUntil(t, "the observes", func() bool {
			mu.Lock()
			defer mu.Unlock()
			return len(sent) == len(legacyOnCases)
		})
		mu.Lock()
		defer mu.Unlock()
		out := sent
		sent = map[string]string{}
		return out
	}
	c := fe.client()
	for i, tc := range legacyOnCases {
		if _, err := c.Observe(Source{FunctionID: "orders::create", Capture: tc.capture}, "audit::record", ObserverOptions{ID: fmt.Sprintf("o%d", i)}); err != nil {
			t.Fatal(err)
		}
	}
	first = collect()
	fe.reconnect(c)
	replay = collect()
	return first, replay, logs
}

func legacyOnWant(engineHasCapture bool) map[string]string {
	want := map[string]string{}
	for i, tc := range legacyOnCases {
		want[fmt.Sprintf("o%d", i)] = tc.on
		if engineHasCapture {
			want[fmt.Sprintf("o%d", i)] = ""
		}
	}
	return want
}

// An engine without observe.capture, a 0.25 one (observe.v1) or an rc one (observe.v2
// only), decides the outcomes that fire from source.on alone: it gets the on the
// capture implies, on the first observe and on the replay, and the client logs the
// deprecation once.
func TestAnEngineWithoutObserveCaptureGetsTheOnCaptureImplies(t *testing.T) {
	for name, features := range map[string][]string{"0.25": engine025Features, "rc": rcEngineFeatures} {
		t.Run(name, func(t *testing.T) {
			first, replay, logs := observeEach(t, features...)
			want := legacyOnWant(false)
			if !reflect.DeepEqual(first, want) {
				t.Errorf("source.on sent %q, want %q", first, want)
			}
			if !reflect.DeepEqual(replay, want) {
				t.Errorf("source.on replayed %q, want %q", replay, want)
			}
			if n := logs.count(legacyDeprecation); n != 1 {
				t.Errorf("the deprecation was logged %d times, want once", n)
			}
		})
	}
}

func TestAnEngineWithObserveCaptureGetsNoOnAndNoWarning(t *testing.T) {
	first, replay, logs := observeEach(t, captureFeatures...)
	want := legacyOnWant(true)
	if !reflect.DeepEqual(first, want) || !reflect.DeepEqual(replay, want) {
		t.Errorf("source.on sent %q, replayed %q, want none", first, replay)
	}
	if n := logs.count(legacyDeprecation); n != 0 {
		t.Errorf("the deprecation was logged %d times", n)
	}
}

// The handle keeps one frame: the legacy one (with source.on) is built when sent, and
// only to an engine without observe.capture.
func TestTheHandleKeepsNoLegacyFrame(t *testing.T) {
	if _, ok := reflect.TypeOf(ObserverHandle{}).FieldByName("legacyFrame"); ok {
		t.Error("ObserverHandle keeps a second, legacy frame")
	}
}

func TestTheLegacyFrameIsBuiltOnlyForAnEngineWithoutObserveCapture(t *testing.T) {
	var built atomic.Int64
	orig := buildLegacyObserveFrame
	buildLegacyObserveFrame = func(frame []byte, on string) ([]byte, error) {
		built.Add(1)
		return orig(frame, on)
	}
	t.Cleanup(func() { buildLegacyObserveFrame = orig })

	observeEach(t, captureFeatures...)
	if n := built.Load(); n != 0 {
		t.Errorf("an engine with observe.capture: %d legacy frames built, want none", n)
	}
	observeEach(t, rcEngineFeatures...)
	legacy := 0
	for _, tc := range legacyOnCases {
		if tc.on != "" {
			legacy++
		}
	}
	// one per observer that needs on, on each of the two connections
	if n := built.Load(); n != int64(2*legacy) {
		t.Errorf("an rc engine: %d legacy frames built, want %d", n, 2*legacy)
	}

}
