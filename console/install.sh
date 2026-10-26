#!/bin/sh
# =============================================================================
# iii-console installer (removed)
# =============================================================================
# install.iii.dev/console serves this file from main. The legacy iii-console
# binary was removed in iii 0.25.0 in favour of the iii console in ADE
# (MOT-3619), so this script installs nothing: it only tells anyone who still
# runs the old one-liner where to go, and fails so a pipeline notices.
# It mirrors CONSOLE_REMOVED_NOTICE in engine/src/cli/mod.rs.
# =============================================================================

cat >&2 <<'MSG'
iii-console has been removed. Use the iii console in ADE instead: from your iii project,
run `iii trigger compose::add worker=ade`, then open http://127.0.0.1:3113
(docs: https://iii.dev/docs/using-iii/console).
Nothing was installed.
MSG
exit 1
