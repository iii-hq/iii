#!/bin/sh
# Appends its own pid to $RESPAWN_LOG, then blocks. Lets tests observe the
# engine's external-worker supervisor respawning a killed child.
echo "$$" >> "$RESPAWN_LOG"
exec sleep 30
