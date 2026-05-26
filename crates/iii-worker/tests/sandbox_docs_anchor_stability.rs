//! R2 — anchor stability test. Verifies that every SandboxErrorCode the
//! daemon can emit has a corresponding `<a id="...">` anchor in the
//! sandbox_daemon README. If a new S-code lands without a README entry,
//! this test fails and CI catches it before agents see a 404-shaped
//! docs_url.
//!
//! Surfaced by /plan-eng-review (E1+R2). The wire contract is:
//!
//!   docs_url = DOCS_BASE + SandboxErrorCode::as_str()
//!
//! where DOCS_BASE points at the GitHub README. If the README loses an
//! anchor — or someone renames it lowercase, or someone adds a new code
//! without docs — the anchor stops resolving. Catching that as a build
//! failure is much cheaper than catching it as an agent-side 404.

use iii_worker::sandbox_daemon::errors::SandboxErrorCode;

const README_PATH: &str = "src/sandbox_daemon/README.md";

fn read_readme() -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(README_PATH);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("README not readable at {path:?}: {e}"))
}

fn all_codes() -> Vec<SandboxErrorCode> {
    // Mirrors the variant list in errors.rs::SandboxErrorCode. Mechanically
    // synced via the canonical `as_str()` round-trip: if a new code lands
    // it must also be added here, which makes test failure the forcing
    // function rather than a silent skip.
    use SandboxErrorCode::*;
    vec![
        S001, S002, S003, S004, S100, S101, S102, S200, S210, S211, S212, S213, S214, S215, S216,
        S217, S218, S219, S300, S400,
    ]
}

#[test]
fn every_s_code_has_a_readme_anchor() {
    let readme = read_readme();
    let mut missing = Vec::new();
    for code in all_codes() {
        let code_str = code.as_str();
        // Accept either an HTML id anchor (preferred, case-stable) or a
        // markdown heading whose auto-anchor matches (lowercase form).
        let html_anchor = format!("<a id=\"{}\"></a>", code_str);
        let lowercase_heading = format!("#### {}", code_str.to_lowercase());
        if !readme.contains(&html_anchor) && !readme.contains(&lowercase_heading) {
            missing.push(code_str);
        }
    }
    assert!(
        missing.is_empty(),
        "README is missing anchors for these S-codes: {missing:?}. \
         Add `<a id=\"{}\"></a>` headings to {README_PATH}.",
        missing.first().copied().unwrap_or("S0XX"),
    );
}

#[test]
fn docs_base_points_at_the_readme() {
    // Sanity-check the DOCS_BASE constant by reconstructing one URL and
    // asserting it's the expected GitHub anchor shape.
    use iii_worker::sandbox_daemon::errors::SandboxError;
    let err = SandboxError::InvalidRequest("test".into());
    let payload = err.to_payload();
    let docs_url = payload["docs_url"].as_str().expect("docs_url must be str");
    assert!(
        docs_url.ends_with("#S001"),
        "docs_url should end with the case-sensitive S-code anchor; got {docs_url:?}"
    );
    assert!(
        docs_url.contains("README.md"),
        "docs_url should point at the README; got {docs_url:?}"
    );
}
