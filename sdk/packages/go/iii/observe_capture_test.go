package iii

// D1 / D6 of the observe.capture contract (with AMENDMENT 1: the view field is
// fires_on): the default capture, the observe.capture feature, the outcomes an
// observer fires on read from its view's fires_on (or else its legacy on), and the
// legacy source.on sent to an engine without observe.capture.

import (
	"context"
	"encoding/json"
	"fmt"
	"slices"
	"sort"
	"testing"
	"time"
)

// legacyDeprecation is a distinctive part of the one-time deprecation warning.
const legacyDeprecation = "does not support observe.capture"

func TestTheSDKAnnouncesObserveCapture(t *testing.T) {
	if !slices.Contains(sdkFeatures, "observe.capture") {
		t.Fatalf("sdkFeatures %q lacks observe.capture", sdkFeatures)
	}
}

func TestTheDefaultCaptureIsInputOutputCaller(t *testing.T) {
	want := []Capture{CaptureInput, CaptureOutput, CaptureCaller}
	if !slices.Equal(defaultCapture, want) {
		t.Errorf("defaultCapture %q, want %q", defaultCapture, want)
	}
	obs := viewFromJSON(t, `{"id":"o","target":"t"}`)
	if !slices.Equal(obs.Capture, want) {
		t.Errorf("an absent capture reads as %q, want %q", obs.Capture, want)
	}
}

// viewFromJSON is the observer a view announced as js decodes to.
func viewFromJSON(t *testing.T, js string) *Observer {
	t.Helper()
	var info ObserverInfo
	if err := json.Unmarshal([]byte(js), &info); err != nil {
		t.Fatalf("%s: %v", js, err)
	}
	obs, err := observerView(&functionObservers{functionID: "test::f"}, info)
	if err != nil {
		t.Fatalf("%s: %v", js, err)
	}
	return obs
}

func TestAnObserverFiresOnTheOutcomesItsViewAnnounces(t *testing.T) {
	cases := []struct {
		view             string
		success, failure bool
	}{
		// an engine with observe.capture: fires_on is all that counts
		{`"fires_on":["error"],"capture":["input","output"]`, false, true},
		{`"fires_on":["success"],"capture":["error"]`, true, false},
		{`"fires_on":["success","error"],"capture":[]`, true, true},
		{`"fires_on":["error"],"capture":["input"],"on":"complete"`, false, true},
		{`"fires_on":["success"]`, true, false},
		// no fires_on (rc or 0.25 engine): on alone, never the capture
		{`"capture":["input","output","error"]`, true, false},
		{`"capture":["error"]`, true, false},
		{`"capture":["error"],"on":null`, true, false},
		{`"capture":["input"],"on":"success"`, true, false},
		{`"capture":["input"],"on":"error"`, false, true},
		{`"capture":["input"],"on":"complete"`, true, true},
		{`"capture":["error"],"on":"error"`, false, true},
		{`"capture":[],"on":"complete"`, true, true},
		{`"filters":{}`, true, false},
		// fires is not the outcome list (AMENDMENT 1): ignored
		{`"fires":["error"],"capture":["input"]`, true, false},
		{`"fires":3,"fires_on":["error"]`, false, true},
	}
	ok := &Call{Status: "success", Input: json.RawMessage(`{}`), Output: json.RawMessage(`{}`)}
	failed := &Call{Status: "error", Input: json.RawMessage(`{}`), Error: &ErrorBody{Code: "E", Message: "m"}}
	for i, tc := range cases {
		js := fmt.Sprintf(`{"id":"o%d","target":"t",%s}`, i, tc.view)
		obs := viewFromJSON(t, js)
		if got := obs.Matches(ok); got != tc.success {
			t.Errorf("%s: fires on success %v, want %v", js, got, tc.success)
		}
		if got := obs.Matches(failed); got != tc.failure {
			t.Errorf("%s: fires on error %v, want %v", js, got, tc.failure)
		}
	}
}

// A view announced without fires_on warns once per client, whatever carries it; one
// with fires_on never does.
func TestAViewWithoutFiresOnWarnsOnce(t *testing.T) {
	logs := captureLockedLog(t)
	fe := newFakeEngine(t)
	fe.setFeatures(FeatureObserveV1, FeatureObserveV2, "observe.capture", FeatureLocalDispatchV1)
	c := fe.client()
	registerLocal(t, c, "src::fn", Identity)
	fe.sendRaw(`{"type":"observeradded","function_id":"src::fn","namespace":"default","revision":1,` +
		`"observer":{"id":"a","target":"t","capture":["input"],"fires_on":["success"]}}`)
	fe.waitAck("src::fn", "a", 1)
	if n := logs.count(legacyDeprecation); n != 0 {
		t.Fatalf("%d warnings for a view with fires_on", n)
	}
	for i, id := range []string{"b", "c"} {
		fe.sendRaw(fmt.Sprintf(`{"type":"observeradded","function_id":"src::fn","namespace":"default","revision":%d,`+
			`"observer":{"id":%q,"target":"t","capture":["input"]}}`, i+2, id))
		fe.waitAck("src::fn", id, uint64(i+2))
	}
	if n := logs.count(legacyDeprecation); n != 1 {
		t.Fatalf("%d warnings for views without fires_on, want 1", n)
	}
}

// An engine without observe.capture (an rc engine with observe.v2 only) announces
// views without fires_on: the observer fires as its legacy on says (absent: success
// only), never as its capture implies.
func TestAViewWithoutFiresOnFiresAsItsLegacyOnSays(t *testing.T) {
	for _, tc := range []struct {
		on   string
		want []string
	}{
		{"", []string{"success"}},
		{"success", []string{"success"}},
		{"error", []string{"error"}},
		{"complete", []string{"error", "success"}},
	} {
		t.Run("on="+tc.on, func(t *testing.T) {
			fe := newFakeEngine(t)
			fe.setFeatures(FeatureObserveV1, FeatureObserveV2, FeatureLocalDispatchV1)
			c := fe.client()
			registerLocal(t, c, "p::charge", func(_ context.Context, data json.RawMessage) (any, error) {
				var in struct {
					Fail bool `json:"fail"`
				}
				_ = json.Unmarshal(data, &in)
				if in.Fail {
					return nil, &InvocationError{Code: "CARD_DECLINED", Message: "declined"}
				}
				return map[string]bool{"ok": true}, nil
			})
			on := ""
			if tc.on != "" {
				on = fmt.Sprintf(`,"on":%q`, tc.on)
			}
			// the capture alone would fire on both outcomes
			fe.sendRaw(`{"type":"observeradded","function_id":"p::charge","namespace":"default","revision":1,` +
				`"observer":{"id":"obs","args":{},"filters":{},"target":"t::target","capture":["input","error"]` + on + `}}`)
			fe.waitAck("p::charge", "obs", 1)
			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
			defer cancel()
			_, _ = c.Call(ctx, CallRequest{FunctionID: "p::charge", Payload: json.RawMessage(`{"amount":1}`)})
			_, _ = c.Call(ctx, CallRequest{FunctionID: "p::charge", Payload: json.RawMessage(`{"amount":2,"fail":true}`)})
			items := fe.waitItems(len(tc.want))
			time.Sleep(100 * time.Millisecond)
			items = fe.reportItems()
			got := []string{}
			for _, it := range items {
				got = append(got, it.Status)
			}
			sort.Strings(got)
			if !slices.Equal(got, tc.want) {
				t.Errorf("on %q: reported %q, want %q", tc.on, got, tc.want)
			}
		})
	}
}
