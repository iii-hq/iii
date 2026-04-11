use iii::workers::config::WorkerEntry;
use iii::workers::reload::{ReloadDiff, diff_entries};
use serde_json::json;

fn entry(name: &str, cfg: Option<serde_json::Value>) -> WorkerEntry {
    WorkerEntry {
        name: name.to_string(),
        image: None,
        config: cfg,
    }
}

#[test]
fn identical_configs_produce_all_unchanged() {
    let old = vec![entry("a", None), entry("b", Some(json!({"k": 1})))];
    let new = vec![entry("a", None), entry("b", Some(json!({"k": 1})))];

    let d = diff_entries(&old, &new);
    assert!(d.added.is_empty());
    assert!(d.removed.is_empty());
    assert!(d.changed.is_empty());
    assert_eq!(d.unchanged.len(), 2);
}

#[test]
fn added_worker_detected() {
    let old = vec![entry("a", None)];
    let new = vec![entry("a", None), entry("b", None)];

    let d = diff_entries(&old, &new);
    assert_eq!(
        d.added.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        vec!["b"]
    );
    assert!(d.removed.is_empty());
    assert!(d.changed.is_empty());
    assert_eq!(d.unchanged, vec!["a".to_string()]);
}

#[test]
fn removed_worker_detected() {
    let old = vec![entry("a", None), entry("b", None)];
    let new = vec![entry("a", None)];

    let d = diff_entries(&old, &new);
    assert_eq!(d.removed, vec!["b".to_string()]);
    assert!(d.added.is_empty());
    assert!(d.changed.is_empty());
}

#[test]
fn changed_config_detected() {
    let old = vec![entry("a", Some(json!({"k": 1})))];
    let new = vec![entry("a", Some(json!({"k": 2})))];

    let d = diff_entries(&old, &new);
    assert_eq!(
        d.changed.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        vec!["a"]
    );
    assert!(d.unchanged.is_empty());
}

#[test]
fn field_reordering_in_config_is_not_a_change() {
    let old = vec![entry("a", Some(json!({"k1": 1, "k2": 2})))];
    let new = vec![entry("a", Some(json!({"k2": 2, "k1": 1})))];

    let d = diff_entries(&old, &new);
    assert!(d.changed.is_empty());
    assert_eq!(d.unchanged, vec!["a".to_string()]);
}

#[test]
fn changed_image_detected() {
    let mut a_old = entry("a", None);
    a_old.image = Some("v1".to_string());
    let mut a_new = entry("a", None);
    a_new.image = Some("v2".to_string());

    let d = diff_entries(&[a_old], &[a_new]);
    assert_eq!(
        d.changed.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        vec!["a"]
    );
}

#[test]
fn empty_old_and_new_produces_empty_diff() {
    let d = diff_entries(&[], &[]);
    assert!(d.added.is_empty());
    assert!(d.removed.is_empty());
    assert!(d.changed.is_empty());
    assert!(d.unchanged.is_empty());
}
