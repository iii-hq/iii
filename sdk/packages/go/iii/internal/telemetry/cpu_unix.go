//go:build linux || darwin || freebsd || netbsd || openbsd || dragonfly

package telemetry

import "syscall"

// processCPUTimes reads cumulative user and system CPU time (Node process.cpuUsage()).
func processCPUTimes() (cpuTimes, bool) {
	var ru syscall.Rusage
	if err := syscall.Getrusage(syscall.RUSAGE_SELF, &ru); err != nil {
		return cpuTimes{}, false
	}
	return cpuTimes{
		user:   float64(ru.Utime.Sec)*1e6 + float64(ru.Utime.Usec),
		system: float64(ru.Stime.Sec)*1e6 + float64(ru.Stime.Usec),
	}, true
}
