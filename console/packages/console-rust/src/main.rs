//! Tombstone for the removed legacy `iii-console` binary (MOT-3619).
//!
//! The legacy console was removed; the iii console now ships as the ADE
//! worker. This placeholder is still released as the `iii-console` asset so
//! that older `iii` CLIs (`iii update`, `iii console`) keep finding it. Every
//! invocation, whatever its arguments, prints the removal notice to stderr and
//! exits with status 1. It never starts a server or touches the network.
//!
//! TODO(MOT-3619): stop shipping the iii-console tombstone after N releases (TBD).

use std::process::ExitCode;

/// Removal notice. Keep in sync with `CONSOLE_REMOVED_NOTICE` in
/// `engine/src/cli/mod.rs` (the `iii console` stub).
const NOTICE: &str = "\
iii-console has been removed. Use the iii console in ADE instead: from your iii project, \
run `iii trigger compose::add worker=ade`, then open http://127.0.0.1:3113 \
(docs: https://iii.dev/docs/using-iii/console).
This placeholder binary will stop shipping in a future release (TBD).";

fn main() -> ExitCode {
    eprintln!("{NOTICE}");
    ExitCode::FAILURE
}
