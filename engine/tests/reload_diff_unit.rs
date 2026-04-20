use iii::workers::config::{WorkerEntry, assign_instance_ids};
use iii::workers::reload::diff_entries;
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
        d.changed
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
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
        d.changed
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
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

// =========================================================================
// Multi-instance (duplicate name) tests
// =========================================================================

#[test]
fn duplicate_names_get_instance_ids() {
    let mut entries = vec![
        entry("iii-http", Some(json!({"port": 4112}))),
        entry("iii-http", Some(json!({"port": 3111}))),
        entry("other", None),
    ];
    assign_instance_ids(&mut entries);

    assert_eq!(entries[0].name, "iii-http");
    assert_eq!(entries[1].name, "iii-http#1");
    assert_eq!(entries[2].name, "other");
    // worker_type strips the suffix
    assert_eq!(entries[0].worker_type(), "iii-http");
    assert_eq!(entries[1].worker_type(), "iii-http");
    assert_eq!(entries[2].worker_type(), "other");
}

#[test]
fn changing_one_instance_only_marks_that_instance_changed() {
    let mut old = vec![
        entry("iii-http", Some(json!({"port": 4112}))),
        entry("iii-http", Some(json!({"port": 3111}))),
    ];
    assign_instance_ids(&mut old);

    // Change only the first instance's port
    let mut new = vec![
        entry("iii-http", Some(json!({"port": 5112}))),
        entry("iii-http", Some(json!({"port": 3111}))),
    ];
    assign_instance_ids(&mut new);

    let d = diff_entries(&old, &new);
    assert_eq!(
        d.changed
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        vec!["iii-http"],
        "only the first instance should be marked as changed"
    );
    assert_eq!(
        d.unchanged,
        vec!["iii-http#1".to_string()],
        "the second instance should be unchanged"
    );
    assert!(d.added.is_empty());
    assert!(d.removed.is_empty());
}

#[test]
fn changing_second_instance_only_marks_second_changed() {
    let mut old = vec![
        entry("iii-http", Some(json!({"port": 4112}))),
        entry("iii-http", Some(json!({"port": 3111}))),
    ];
    assign_instance_ids(&mut old);

    // Change only the second instance
    let mut new = vec![
        entry("iii-http", Some(json!({"port": 4112}))),
        entry("iii-http", Some(json!({"port": 9999}))),
    ];
    assign_instance_ids(&mut new);

    let d = diff_entries(&old, &new);
    assert_eq!(
        d.changed
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        vec!["iii-http#1"],
        "only the second instance should be marked as changed"
    );
    assert_eq!(d.unchanged, vec!["iii-http".to_string()]);
}

#[test]
fn adding_third_instance_detected_as_added() {
    let mut old = vec![
        entry("iii-http", Some(json!({"port": 4112}))),
        entry("iii-http", Some(json!({"port": 3111}))),
    ];
    assign_instance_ids(&mut old);

    let mut new = vec![
        entry("iii-http", Some(json!({"port": 4112}))),
        entry("iii-http", Some(json!({"port": 3111}))),
        entry("iii-http", Some(json!({"port": 8080}))),
    ];
    assign_instance_ids(&mut new);

    let d = diff_entries(&old, &new);
    assert_eq!(
        d.added.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        vec!["iii-http#2"],
    );
    assert_eq!(d.unchanged.len(), 2);
    assert!(d.changed.is_empty());
    assert!(d.removed.is_empty());
}

#[test]
fn removing_second_instance_detected_as_removed() {
    let mut old = vec![
        entry("iii-http", Some(json!({"port": 4112}))),
        entry("iii-http", Some(json!({"port": 3111}))),
    ];
    assign_instance_ids(&mut old);

    // Only one iii-http in new config
    let mut new = vec![entry("iii-http", Some(json!({"port": 4112})))];
    assign_instance_ids(&mut new);

    let d = diff_entries(&old, &new);
    assert_eq!(d.removed, vec!["iii-http#1".to_string()]);
    assert_eq!(d.unchanged, vec!["iii-http".to_string()]);
    assert!(d.added.is_empty());
    assert!(d.changed.is_empty());
}
