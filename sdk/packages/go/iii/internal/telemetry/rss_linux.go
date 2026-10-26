//go:build linux

package telemetry

import (
	"bytes"
	"os"
	"strconv"
)

// residentSetSize reads the resident set size from /proc/self/statm (Node
// process.memoryUsage().rss).
func residentSetSize() (float64, bool) {
	b, err := os.ReadFile("/proc/self/statm")
	if err != nil {
		return 0, false
	}
	fields := bytes.Fields(b)
	if len(fields) < 2 {
		return 0, false
	}
	pages, err := strconv.ParseUint(string(fields[1]), 10, 64)
	if err != nil {
		return 0, false
	}
	return float64(pages) * float64(os.Getpagesize()), true
}
