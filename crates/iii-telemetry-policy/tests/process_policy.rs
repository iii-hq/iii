use std::process::Command;

#[test]
fn process_policy() {
    const EXPECTED: &str = "III_POLICY_TEST_EXPECTED_DISABLED";
    if let Ok(expected) = std::env::var(EXPECTED) {
        assert_eq!(
            iii_telemetry_policy::is_telemetry_disabled(),
            expected == "true"
        );
        return;
    }
    for (enabled, ci, dev, marker, expected) in [
        (None, None, None, false, false),
        (Some(""), None, None, false, false),
        (Some("true"), None, None, false, false),
        (Some(" FALSE "), None, None, false, true),
        (Some("off"), None, None, false, true),
        (Some("true"), Some("false"), None, false, true),
        (Some("true"), None, Some("true"), false, true),
        (Some("true"), None, None, true, true),
    ] {
        let home = tempfile::tempdir().unwrap();
        if marker {
            std::fs::create_dir(home.path().join(".iii")).unwrap();
            std::fs::write(home.path().join(".iii/telemetry_dev_optout"), "").unwrap();
        }
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args(["--exact", "process_policy"])
            .env(EXPECTED, expected.to_string())
            .env("HOME", home.path())
            .env("USERPROFILE", home.path());
        for key in iii_telemetry_policy::CI_ENV_VARS {
            child.env_remove(key);
        }
        for (key, value) in [
            ("III_TELEMETRY_ENABLED", enabled),
            ("CI", ci),
            ("III_TELEMETRY_DEV", dev),
        ] {
            if let Some(value) = value {
                child.env(key, value);
            } else {
                child.env_remove(key);
            }
        }
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "{enabled:?}/{ci:?}/{dev:?}/{marker}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
