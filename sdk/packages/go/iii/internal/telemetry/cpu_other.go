//go:build !(linux || darwin || freebsd || netbsd || openbsd || dragonfly)

package telemetry

// processCPUTimes is unavailable on this platform: the CPU gauges are not reported.
func processCPUTimes() (cpuTimes, bool) { return cpuTimes{}, false }
