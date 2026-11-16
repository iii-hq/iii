"""unused_deps.py reports unused dependencies as warnings, never as failures."""

from __future__ import annotations

import os
from pathlib import Path
import shlex
import subprocess
import sys

GITHUB = Path(__file__).parents[1]
SCRIPT = Path(__file__).with_name("unused_deps.py")


def run(
    tmp_path: Path,
    tool: str,
    executable: str | None,
    output: str = "",
    exit_code: int = 0,
    *args: str,
) -> subprocess.CompletedProcess[str]:
    """Run the script with `executable` faked to print `output` and exit."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir(exist_ok=True)
    (tmp_path / "project").mkdir(exist_ok=True)
    if executable:
        fake = bin_dir / executable
        fake.write_text(
            "#!/usr/bin/env bash\n"
            f'printf "%s\\n" "$*" > {shlex.quote(str(tmp_path / "argv"))}\n'
            f"printf '%s\\n' {shlex.quote(output)}\n"
            f"exit {exit_code}\n"
        )
        fake.chmod(0o755)
    return subprocess.run(
        [sys.executable, str(SCRIPT), tool, "--directory", "project", *args],
        cwd=tmp_path,
        # Only the fake directory and the basics: a real tool must never run.
        env={**os.environ, "PATH": f"{bin_dir}{os.pathsep}/usr/bin{os.pathsep}/bin"},
        capture_output=True,
        text=True,
        check=False,
    )


def test_error_findings_become_warnings_on_the_repository_path(tmp_path: Path) -> None:
    result = run(
        tmp_path, "cargo-shear", "cargo",
        "::error file=Cargo.toml,line=28,col=1,title=shear/unused_dependency::"
        "unused dependency `tempfile` (remove this dependency)",
        1,
    )

    assert result.returncode == 0
    assert "::error" not in result.stdout
    assert (
        "::warning file=project/Cargo.toml,line=28,col=1,"
        "title=shear/unused_dependency::unused dependency `tempfile`"
    ) in result.stdout
    assert "cargo-shear reported 1 finding in project." in result.stdout
    assert "cargo shear --fix" in result.stdout


def test_warning_findings_with_a_zero_exit_are_kept(tmp_path: Path) -> None:
    # cargo-shear reports unused optional dependencies as ::warning, exit 0.
    result = run(
        tmp_path, "cargo-shear", "cargo",
        "::warning file=Cargo.toml,line=7,col=1,title=shear/unused_optional_dependency::"
        "unused optional dependency `b`",
        0,
    )

    assert result.returncode == 0
    assert "::warning file=project/Cargo.toml,line=7,col=1," in result.stdout
    assert "no unused dependencies" not in result.stdout


def test_knip_findings_and_extra_arguments(tmp_path: Path) -> None:
    result = run(
        tmp_path, "knip", "npx",
        "Unused devDependencies (1)\n"
        "::error file=ui/package.json,line=24,endLine=24,col=6,endColumn=6,"
        "title=✂️ Knip / Unused devDependencies::react-dom in ui/package.json",
        1,
        "--", "--workspace", "./tmux/*",
    )

    assert result.returncode == 0
    assert (
        "::warning file=project/ui/package.json,line=24,endLine=24,col=6,endColumn=6,"
        "title=✂️ Knip / Unused devDependencies::react-dom in ui/package.json"
    ) in result.stdout
    argv = (tmp_path / "argv").read_text()
    assert "--include dependencies" in argv
    assert "--reporter github-actions" in argv
    assert argv.rstrip().endswith("--workspace ./tmux/*")


def test_deptry_findings(tmp_path: Path) -> None:
    result = run(
        tmp_path, "deptry", "deptry",
        "::error file=pyproject.toml,line=1,title=DEP002::"
        "'watchfiles' defined as a dependency but not used in the codebase",
        1,
    )

    assert "::warning file=project/pyproject.toml,line=1,title=DEP002::'watchfiles'" in result.stdout
    argv = (tmp_path / "argv").read_text()
    assert "--pep621-dev-dependency-groups dev" in argv
    assert "--ignore DEP001,DEP003" in argv


GO_DIFF = """\
diff current/go.mod tidy/go.mod
--- current/go.mod
+++ tidy/go.mod
@@ -3,9 +3,8 @@
 go 1.25.0

 require (
-\tgithub.com/a/unused v1.0.0
 \tgithub.com/b/used v1.2.0
-\tgithub.com/c/bumped v1.0.0 // indirect
+\tgithub.com/c/bumped v1.1.0 // indirect
 )
diff current/go.sum tidy/go.sum
--- current/go.sum
+++ tidy/go.sum
@@ -1,2 +1 @@
-github.com/a/unused v1.0.0 h1:x
 github.com/b/used v1.2.0 h1:y
"""


def test_go_reports_each_module_the_tidy_diff_drops(tmp_path: Path) -> None:
    result = run(tmp_path, "go", "go", GO_DIFF, 1)

    assert result.returncode == 0
    assert (
        "::warning file=project/go.mod,line=6,title=go mod tidy::"
        "unused module github.com/a/unused v1.0.0"
    ) in result.stdout
    # A version change removes and re-adds the module: it is not unused.
    assert "github.com/c/bumped" not in result.stdout
    assert "go reported 1 finding in project." in result.stdout
    assert (tmp_path / "argv").read_text().strip() == "mod tidy -diff"


def test_go_reports_a_diff_without_unused_modules_once(tmp_path: Path) -> None:
    diff = GO_DIFF.split("diff current/go.sum")[0].replace(
        "-\tgithub.com/a/unused v1.0.0\n", ""
    )
    result = run(tmp_path, "go", "go", diff, 1)

    assert "::warning file=project/go.mod,title=go mod tidy::go.mod differ from 'go mod tidy'" in result.stdout


def test_a_check_that_cannot_run_warns_with_its_log(tmp_path: Path) -> None:
    result = run(tmp_path, "knip", "npx", "ERROR: Workspace directory not found.", 2)

    assert result.returncode == 0
    assert "ERROR: Workspace directory not found." in result.stdout
    assert "::warning title=project: unused dependencies (knip)::knip could not run in project (exit 2)" in result.stdout


def test_a_missing_tool_warns_instead_of_failing(tmp_path: Path) -> None:
    result = run(tmp_path, "deptry", None)

    assert result.returncode == 0
    assert "deptry could not run in project (exit 127)" in result.stdout


def test_a_clean_project_says_so(tmp_path: Path) -> None:
    result = run(tmp_path, "cargo-shear", "cargo", "", 0)

    assert result.returncode == 0
    assert "::" not in result.stdout.split("\n", 1)[1]
    assert "project: no unused dependencies (cargo-shear)." in result.stdout


def test_invalid_arguments_warn_instead_of_failing(tmp_path: Path) -> None:
    result = subprocess.run(
        [sys.executable, str(SCRIPT), "pip"], cwd=tmp_path, capture_output=True, text=True, check=False
    )

    assert result.returncode == 0
    assert "::warning title=unused dependencies::" in result.stdout


def test_ci_runs_one_warning_only_job_per_language() -> None:
    import yaml

    jobs = yaml.load((GITHUB / "workflows" / "ci.yml").read_text(), Loader=yaml.BaseLoader)["jobs"]
    script = "python3 .github/scripts/unused_deps.py"
    for language, tool in (("rust", "cargo-shear"), ("node", "knip"), ("python", "deptry"), ("go", "go")):
        job = jobs[f"unused-deps-{language}"]
        assert job["permissions"] == {"contents": "read"}
        assert "needs" not in job
        check = job["steps"][-1]
        assert check["name"] == "Check unused dependencies"
        assert f"{script} {tool}" in check["run"]
        # Setup steps that may fail only make the check warn.
        for step in job["steps"]:
            if step.get("name") in {"Install cargo-shear", "Install dependencies"}:
                assert step["continue-on-error"] == "true"

    python = jobs["unused-deps-python"]["steps"][-1]["run"]
    assert "--with deptry==0.25.1" in python
    assert "|| echo \"::warning " in python
    assert "*/testdata/*" in jobs["unused-deps-go"]["steps"][-1]["run"]
