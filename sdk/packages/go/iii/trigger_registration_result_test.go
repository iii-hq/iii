package iii

import (
	"bytes"
	"log"
	"strings"
	"testing"
)

// captureLog redirects the standard logger for the duration of fn and returns
// what it wrote. The logger is global, so the flags and output are restored.
func captureLog(t *testing.T, fn func()) string {
	t.Helper()
	var buf bytes.Buffer
	prevOut := log.Writer()
	prevFlags := log.Flags()
	log.SetOutput(&buf)
	log.SetFlags(0)
	t.Cleanup(func() {
		log.SetOutput(prevOut)
		log.SetFlags(prevFlags)
	})
	fn()
	return buf.String()
}

// TestTriggerRegistrationFailureIsLogged pins down that a failed binding is
// visible. The ack is the only channel the engine has to report it, and the Go
// SDK used to drop the frame, so a worker whose binding died in a boot-order
// race kept running with a handler that never fired.
func TestTriggerRegistrationFailureIsLogged(t *testing.T) {
	c := New("ws://127.0.0.1:0", WithName("memory"))

	out := captureLog(t, func() {
		c.handleTriggerRegistrationResult(&TriggerRegistrationResultMessage{
			ID:          "trig-1",
			TriggerType: "harness::hook::pre-generate",
			FunctionID:  "memory::on-pre-generate",
			Error: &ErrorBody{
				Code:    "trigger_type_not_found",
				Message: "Trigger type not found",
			},
		})
	})

	for _, want := range []string{
		"trig-1",
		"harness::hook::pre-generate",
		"trigger_type_not_found",
		"Trigger type not found",
	} {
		if !strings.Contains(out, want) {
			t.Errorf("log output missing %q; got %q", want, out)
		}
	}
}

// TestTriggerRegistrationErrorIsReadable pins down the programmatic half: a
// retry loop has to branch on the cause, and a log line cannot be branched on.
func TestTriggerRegistrationErrorIsReadable(t *testing.T) {
	c := New("ws://127.0.0.1:0", WithName("memory"))

	if got := c.TriggerRegistrationError("trig-1"); got != nil {
		t.Fatalf("TriggerRegistrationError before any ack = %v, want nil", got)
	}

	captureLog(t, func() {
		c.handleTriggerRegistrationResult(&TriggerRegistrationResultMessage{
			ID:          "trig-1",
			TriggerType: "harness::hook::pre-generate",
			FunctionID:  "memory::on-pre-generate",
			Error:       &ErrorBody{Code: "trigger_type_not_found", Message: "Trigger type not found"},
		})
	})

	got := c.TriggerRegistrationError("trig-1")
	if got == nil {
		t.Fatal("TriggerRegistrationError after the ack = nil, want the cause")
	}
	if got.Code != "trigger_type_not_found" {
		t.Errorf("Code = %q, want trigger_type_not_found", got.Code)
	}

	// Another binding's id is unaffected: the count is per-trigger, which is
	// the whole point of the change.
	if other := c.TriggerRegistrationError("trig-2"); other != nil {
		t.Errorf("TriggerRegistrationError(\"trig-2\") = %v, want nil", other)
	}
}

func TestTriggerRegistrationSuccessIsSilent(t *testing.T) {
	c := New("ws://127.0.0.1:0", WithName("memory"))

	out := captureLog(t, func() {
		c.handleTriggerRegistrationResult(&TriggerRegistrationResultMessage{
			ID:          "trig-2",
			TriggerType: "http",
			FunctionID:  "memory::handler",
		})
		c.handleTriggerRegistrationResult(nil)
	})

	if out != "" {
		t.Errorf("log output = %q, want empty", out)
	}
}
