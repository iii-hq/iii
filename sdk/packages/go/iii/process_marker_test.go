package iii

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"testing"
)

func readMarkers(t *testing.T, dir string) map[string]map[string]any {
	t.Helper()
	files, err := filepath.Glob(filepath.Join(dir, fmt.Sprintf("worker-%d-*.json", os.Getpid())))
	if err != nil {
		t.Fatal(err)
	}
	markers := map[string]map[string]any{}
	for _, file := range files {
		data, err := os.ReadFile(file)
		if err != nil {
			t.Fatal(err)
		}
		var marker map[string]any
		if err := json.Unmarshal(data, &marker); err != nil {
			t.Fatal(err)
		}
		markers[file] = marker
	}
	return markers
}

func TestAnnounceProcessNamesThisProcessUntilRemoved(t *testing.T) {
	dir := t.TempDir()

	remove := announceProcess("ws://127.0.0.1:49134", "orders", dir)
	markers := readMarkers(t, dir)
	if len(markers) != 1 {
		t.Fatalf("expected one marker, got %v", markers)
	}
	for _, marker := range markers {
		if marker["pid"] != float64(os.Getpid()) || marker["name"] != "orders" ||
			marker["url"] != "ws://127.0.0.1:49134" || marker["sdk"] != "go" {
			t.Fatalf("unexpected marker: %v", marker)
		}
	}

	remove()
	if markers := readMarkers(t, dir); len(markers) != 0 {
		t.Fatalf("marker still present after remove: %v", markers)
	}
}

func TestAnnounceProcessGivesEachClientItsOwnMarker(t *testing.T) {
	dir := t.TempDir()

	orders := announceProcess("ws://127.0.0.1:49134", "orders", dir)
	billing := announceProcess("ws://127.0.0.1:49134", "billing", dir)
	defer billing()
	orders()
	markers := readMarkers(t, dir)
	if len(markers) != 1 {
		t.Fatalf("expected billing's marker alone, got %v", markers)
	}
	for _, marker := range markers {
		if marker["name"] != "billing" {
			t.Fatalf("removing one client's marker removed another's: %v", marker)
		}
	}
}

func TestAnnounceProcessIgnoresAnUnwritableDirectory(t *testing.T) {
	blocker := filepath.Join(t.TempDir(), "file")
	if err := os.WriteFile(blocker, nil, 0o644); err != nil {
		t.Fatal(err)
	}
	announceProcess("ws://x", "y", filepath.Join(blocker, "run"))()
}

// A short-lived program returns from main right after Close, before the
// supervisor goroutine ends, so Close itself must take the marker.
func TestCloseRemovesTheWorkerMarker(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("USERPROFILE", home) // os.UserHomeDir on Windows
	dir := filepath.Join(home, ".iii", "run")

	c := RegisterWorker("ws://127.0.0.1:1")
	if markers := readMarkers(t, dir); len(markers) != 1 {
		t.Fatalf("expected one marker, got %v", markers)
	}
	_ = c.Close()
	if markers := readMarkers(t, dir); len(markers) != 0 {
		t.Fatalf("marker outlived Close: %v", markers)
	}
}

// A Connect that starts the supervisor after Close (or racing it) writes its
// marker after Close took the nil one, so it must remove that marker itself.
func TestStartAfterCloseLeavesNoWorkerMarker(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("USERPROFILE", home) // os.UserHomeDir on Windows
	dir := filepath.Join(home, ".iii", "run")

	c := New("ws://127.0.0.1:1")
	_ = c.Close()
	c.startSupervisor()
	if markers := readMarkers(t, dir); len(markers) != 0 {
		t.Fatalf("marker written after Close: %v", markers)
	}
}
