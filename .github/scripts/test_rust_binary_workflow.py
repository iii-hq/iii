"""Regression tests for the reusable Rust binary release workflow."""

from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/_rust-binary.yml"
WORKFLOW_DOCS = ROOT / ".github/workflows/WORKFLOWS.md"


def build_job() -> str:
    workflow = WORKFLOW.read_text()
    return workflow.split("\n  build:\n", 1)[1].split("\n  notify-result:\n", 1)[0]


def test_build_checks_out_release_tag():
    job = build_job()
    checkout = job.split("- uses: actions/checkout@v4", 1)[1].split("\n\n", 1)[0]

    assert "ref: refs/tags/${{ inputs.tag_name }}" in checkout
    assert "persist-credentials: false" in checkout


def test_build_validates_manifest_version_before_upload():
    job = build_job()
    validation = job.index("- name: Validate release version")
    upload = job.index("- name: Build and upload binary")

    assert validation < upload
    assert ".github/scripts/validate_rust_release_version.sh" in job


def test_alpha_install_example_passes_tag_to_shell():
    docs = WORKFLOW_DOCS.read_text()

    assert (
        "curl -fsSL https://iii.dev/install.sh | "
        "III_RELEASE_TAG=iii-alpha/v0.19.2-alpha.1 sh"
    ) in docs
    assert "III_RELEASE_TAG=iii-alpha/v0.19.2-alpha.1 curl" not in docs
