//go:build !linux

package telemetry

// residentSetSize is unavailable on this platform: the RSS gauge is not reported.
func residentSetSize() (float64, bool) { return 0, false }
