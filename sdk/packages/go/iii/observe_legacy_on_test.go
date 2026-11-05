package iii

import (
	"encoding/json"
	"fmt"
	"log"
	"os"
	"slices"
	"strings"
	"sync"
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

var legacyOnCases = []struct {
	capture []Capture
	on      string
}{
	{nil, ""},
	{[]Capture{CaptureInput}, ""},
	{[]Capture{}, ""},
	{[]Capture{CaptureError, CaptureCaller}, "error"},
	{[]Capture{CaptureInput, CaptureError}, "complete"},
	{[]Capture{CaptureInput, CaptureOutput, CaptureError}, "complete"},
}

// observeEach observes once per case on an engine with features and returns the
// source.on of each observe frame, in order, and the log.
func observeEach(t *testing.T, features ...string) ([]string, *lockedLog) {
	t.Helper()
	logs := &lockedLog{}
	log.SetOutput(logs)
	t.Cleanup(func() { log.SetOutput(os.Stderr) })
	fe := newFakeEngine(t)
	fe.setFeatures(features...)
	var mu sync.Mutex
	sent := []string{}
	fe.setOnFrame(func(msg map[string]json.RawMessage) {
		var kind string
		_ = json.Unmarshal(msg["type"], &kind)
		if kind != "observe" {
			return
		}
		var source struct {
			On string `json:"on"`
		}
		_ = json.Unmarshal(msg["source"], &source)
		mu.Lock()
		sent = append(sent, source.On)
		mu.Unlock()
	})
	c := fe.client()
	for i, tc := range legacyOnCases {
		if _, err := c.Observe(Source{FunctionID: "orders::create", Capture: tc.capture}, "audit::record", ObserverOptions{ID: fmt.Sprintf("o%d", i)}); err != nil {
			t.Fatal(err)
		}
	}
	waitUntil(t, "the observes", func() bool {
		mu.Lock()
		defer mu.Unlock()
		return len(sent) == len(legacyOnCases)
	})
	mu.Lock()
	defer mu.Unlock()
	return slices.Clone(sent), logs
}

// An engine without observe.v2 (0.25) decides the outcomes that fire from source.on
// alone: it gets the on the capture implies, and the client logs the deprecation once.
func TestAnEngineWithoutObserveV2GetsTheOnCaptureImplies(t *testing.T) {
	sent, logs := observeEach(t, FeatureObserveV1, FeatureLocalDispatchV1)
	want := []string{}
	for _, tc := range legacyOnCases {
		want = append(want, tc.on)
	}
	if !slices.Equal(sent, want) {
		t.Errorf("source.on sent %q, want %q", sent, want)
	}
	if n := logs.count("`source.on` is deprecated"); n != 1 {
		t.Errorf("the deprecation was logged %d times, want once", n)
	}
}

func TestAnEngineWithObserveV2GetsNoOnAndNoWarning(t *testing.T) {
	sent, logs := observeEach(t, FeatureObserveV1, FeatureObserveV2, FeatureLocalDispatchV1)
	if !slices.Equal(sent, make([]string, len(legacyOnCases))) {
		t.Errorf("source.on sent %q, want none", sent)
	}
	if n := logs.count("source.on"); n != 0 {
		t.Errorf("the deprecation was logged %d times", n)
	}
}
