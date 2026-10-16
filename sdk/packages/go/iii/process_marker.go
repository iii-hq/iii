package iii

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sync/atomic"
	"time"
)

var announced atomic.Int64

// announceProcess writes ~/.iii/run/worker-<pid>-<n>.json so `iii ps` can find
// this worker however it was started (or under dir, when given), one file per
// client. Compose already marks its children through their environment; this
// covers a worker run straight from a terminal.
//
// Best effort: a home directory that cannot be written only makes the worker
// harder to find. A process killed before it removes the marker leaves it
// behind, and `iii ps` discards any marker whose process is gone. The returned
// function removes the marker.
func announceProcess(url, name, dir string) func() {
	if dir == "" {
		home, err := os.UserHomeDir()
		if err != nil {
			return func() {}
		}
		dir = filepath.Join(home, ".iii", "run")
	}
	file := filepath.Join(dir, fmt.Sprintf("worker-%d-%d.json", os.Getpid(), announced.Add(1)))
	data, err := json.Marshal(map[string]any{
		"pid":        os.Getpid(),
		"url":        url,
		"name":       name,
		"sdk":        "go",
		"started_at": time.Now().UnixMilli(),
	})
	if err != nil || os.MkdirAll(dir, 0o755) != nil || os.WriteFile(file, data, 0o644) != nil {
		return func() {}
	}
	return func() { _ = os.Remove(file) }
}
