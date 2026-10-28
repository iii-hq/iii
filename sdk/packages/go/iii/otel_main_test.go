package iii

import (
	"os"
	"testing"
)

// TestMain runs the package's tests with OTEL_ENABLED=false. Telemetry is on by default
// (as in the Node SDK), which makes every started client dial <engine>/otel; the mock
// engine of the client tests accepts any path, greets every connection and counts
// handshakes, so an extra telemetry socket would disturb them. The otel_*_test.go tests
// enable telemetry explicitly with WithOtel(OtelConfig{Enabled: ...}), which takes
// precedence over the variable, or unset it to exercise the default.
func TestMain(m *testing.M) {
	_ = os.Setenv("OTEL_ENABLED", "false")
	os.Exit(m.Run())
}
