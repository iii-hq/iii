#!/bin/sh
# argv probe: writes the args it was called with (line 1) and the
# engine-attachment env contract (line 2) to the file named by the
# environment variable ARGV_LOG, then sleeps so the engine's spawn
# path can inspect the file before the probe exits.
{
  printf '%s\n' "$*"
  if [ -e "/dev/fd/${III_LIFELINE_FD:-999}" ]; then lifeline_open=yes; else lifeline_open=no; fi
  printf 'engine_pid=%s lifeline_fd=%s lifeline_open=%s engine_url=%s extra_env=%s\n' \
    "${III_ENGINE_PID:-unset}" "${III_LIFELINE_FD:-unset}" "$lifeline_open" \
    "${III_ENGINE_URL:-unset}" "${III_PROBE_EXTRA_ENV:-unset}"
} > "$ARGV_LOG"
sleep 30
