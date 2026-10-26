//go:build race

package iii

// raceEnabled: the race detector slows local calls ~10x; long loops scale down.
const raceEnabled = true
