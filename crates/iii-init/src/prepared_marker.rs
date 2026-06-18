//! Prepared-marker invalidation policy for local worker boots.
//!
//! Kept platform-agnostic so the subtle timing rules are covered by the
//! normal host test suite, even though `supervisor.rs` itself is Linux-only.

use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

/// True when `path` was modified within `window`.
///
/// A missing marker, unreadable metadata, or a marker timestamp in the future
/// is not considered recent. The future case is deliberately conservative:
/// otherwise clock skew could make every later runtime crash look like an
/// install-adjacent boot failure forever.
pub(crate) fn marker_is_recent(path: &Path, window: Duration) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age < window)
}

/// Decide whether a worker exit should clear the prepared marker.
///
/// `marker_existed_at_spawn` distinguishes a marker inherited from an earlier
/// successful boot from one created by this boot script after setup/install.
/// That matters for slow installs: the worker can crash immediately after a
/// newly-created marker, even when the process has been alive longer than the
/// raw boot-crash window.
pub(crate) fn should_invalidate(
    was_exit: bool,
    code: i32,
    spawned_at: Instant,
    window: Duration,
    marker_existed_at_spawn: bool,
    marker_is_recent: bool,
) -> bool {
    if !was_exit || code == 0 {
        return false;
    }

    if spawned_at.elapsed() < window {
        return true;
    }

    !marker_existed_at_spawn && marker_is_recent
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_invalidate_classifies_exits() {
        let now = Instant::now();
        let old = now
            .checked_sub(Duration::from_secs(35))
            .expect("small Instant subtraction must work");
        let window = Duration::from_secs(30);

        // Real non-zero exit, just after spawn -> boot failure.
        assert!(should_invalidate(true, 1, now, window, false, false));

        // Clean exit -> not a failure.
        assert!(!should_invalidate(true, 0, now, window, false, false));

        // Signal death (e.g. SIGTERM shutdown) -> never a boot failure,
        // even with a non-zero "128 + sig" code.
        assert!(!should_invalidate(false, 143, now, window, false, false));

        // Non-zero exit long after spawn with no fresh marker -> runtime crash.
        assert!(!should_invalidate(true, 1, old, window, false, false));

        // Long setup/install can exceed the raw spawn window. If this boot
        // created the marker recently, an immediate worker crash still
        // implicates the install/deps and should force re-prep.
        assert!(should_invalidate(true, 1, old, window, false, true));

        // A marker inherited from an earlier boot should not be invalidated
        // solely because its timestamp is recent.
        assert!(!should_invalidate(true, 1, old, window, true, true));
    }

    #[test]
    fn marker_is_recent_handles_missing_and_recent_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".iii-prepared");
        let window = Duration::from_secs(30);

        assert!(!marker_is_recent(&path, window));

        std::fs::write(&path, b"prepared").unwrap();
        assert!(marker_is_recent(&path, window));
    }
}
