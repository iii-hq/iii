#!/usr/bin/env bash
# Sources only the function definitions from install.sh without executing
# the imperative body. This allows unit-testing err, iii_telemetry_enabled,
# iii_gen_uuid, iii_read_toml_key, iii_set_toml_key, iii_get_or_create_telemetry_id,
# iii_send_event, iii_detect_from_version, and iii_export_host_user_id in isolation.

_install_sh="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/install.sh}"

# Extract everything from line 3 up to (but not including) the argument
# parsing comment. This gives us variable declarations + all functions.
_func_body=$(sed '/^# --- Argument parsing ---/,$d' "$_install_sh" | tail -n +3)
eval "$_func_body"
