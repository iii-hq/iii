package iii

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"testing"
	"time"
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

// markerHome points os.UserHomeDir at a temporary directory and returns the run
// directory a started client announces itself in.
func markerHome(t *testing.T) string {
	t.Helper()
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("USERPROFILE", home) // os.UserHomeDir on Windows
	return filepath.Join(home, ".iii", "run")
}

func TestStoppingTheClientRemovesTheWorkerMarker(t *testing.T) {
	for name, stop := range map[string]func(*Client){
		"Close":    func(c *Client) { _ = c.Close() },
		"Shutdown": func(c *Client) { _ = c.Shutdown(context.Background()) },
	} {
		t.Run(name, func(t *testing.T) {
			dir := markerHome(t)
			// Nothing listens there: the marker must not wait for a connection.
			c := RegisterWorker("ws://127.0.0.1:1", WithName("orders"))
			if markers := readMarkers(t, dir); len(markers) != 1 {
				t.Fatalf("expected one marker while running, got %v", markers)
			}
			// A short-lived main returns right after this, without waiting for the
			// supervisor goroutine.
			stop(c)
			if markers := readMarkers(t, dir); len(markers) != 0 {
				t.Fatalf("marker outlived %s: %v", name, markers)
			}
		})
	}
}

func TestConnectAfterCloseWritesNoWorkerMarker(t *testing.T) {
	dir := markerHome(t)
	c := New("ws://127.0.0.1:1")
	_ = c.Close()
	_ = c.Connect(context.Background())
	if markers := readMarkers(t, dir); len(markers) != 0 {
		t.Fatalf("a closed client announced itself: %v", markers)
	}
}

func TestASupervisorThatGivesUpRemovesTheWorkerMarker(t *testing.T) {
	dir := markerHome(t)
	// Nothing listens there, and no retries: the supervisor gives up after one dial.
	c := RegisterWorker("ws://127.0.0.1:1", WithName("orders"),
		WithReconnectConfig(ReconnectConfig{MaxRetries: 0}))
	defer c.Close()
	deadline := time.Now().Add(5 * time.Second)
	for len(readMarkers(t, dir)) != 0 {
		if time.Now().After(deadline) {
			t.Fatal("the marker outlived a supervisor that gave up")
		}
		time.Sleep(10 * time.Millisecond)
	}
}
