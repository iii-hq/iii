#!/bin/sh
# argv probe: writes the args it was called with to the file named by
# the environment variable ARGV_LOG, then sleeps so the engine's
# spawn path can inspect the file before the probe exits.
printf '%s\n' "$*" > "$ARGV_LOG"
sleep 30
