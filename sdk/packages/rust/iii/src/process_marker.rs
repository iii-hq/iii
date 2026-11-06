//! `~/.iii/run/worker-<pid>-<n>.json`: how `iii ps` finds a worker however it
//! was started, one file per client. Compose already marks its children
//! through their environment; this covers a worker run straight from a
//! terminal or a service manager.
//!
//! Best effort, and never an error: a home directory that cannot be written
//! only makes the worker harder to find. A process that exits without
//! shutting down leaves its marker behind, and `iii ps` discards any marker
//! whose PID is gone or now belongs to a younger process.

use std::path::PathBuf;

/// Removes the marker when the client that wrote it shuts down.
pub(crate) struct Marker(pub(crate) Option<PathBuf>);

impl Marker {
    pub(crate) fn announce(url: &str, name: Option<&str>) -> Self {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
        Self(home.and_then(|home| write(&PathBuf::from(home).join(".iii").join("run"), url, name)))
    }
}

impl Drop for Marker {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

static ANNOUNCED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn write(dir: &std::path::Path, url: &str, name: Option<&str>) -> Option<PathBuf> {
    std::fs::create_dir_all(dir).ok()?;
    let pid = std::process::id();
    let started_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    let marker = serde_json::json!({
        "pid": pid,
        "url": url,
        "name": name,
        "sdk": "rust",
        "started_at": started_at,
    });
    let n = ANNOUNCED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!("worker-{pid}-{n}.json"));
    std::fs::write(&path, marker.to_string()).ok()?;
    Some(path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_marker_names_this_process_until_it_is_dropped() {
        let dir = std::env::temp_dir().join(format!("iii-marker-{}", uuid::Uuid::new_v4()));
        let path = super::write(&dir, "ws://127.0.0.1:49134", Some("orders")).unwrap();
        let file = path.file_name().unwrap().to_str().unwrap();
        assert!(file.starts_with(&format!("worker-{}-", std::process::id())));
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["pid"], std::process::id());
        assert_eq!(written["name"], "orders");
        assert_eq!(written["url"], "ws://127.0.0.1:49134");
        assert_eq!(written["sdk"], "rust");

        // A second client in the same process keeps its own file.
        let other = super::write(&dir, "ws://127.0.0.1:49134", Some("billing")).unwrap();
        drop(super::Marker(Some(path.clone())));
        assert!(!path.exists());
        assert!(other.exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
