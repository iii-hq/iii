"""Tests for validate_release_worker_matrix.py.

Run with: python -m pytest .github/scripts/test_validate_release_worker_matrix.py -v
"""
from __future__ import annotations

import pathlib

from validate_release_worker_matrix import validate

ROOT = pathlib.Path(__file__).resolve().parents[2]

WORKFLOW = """jobs:
  publish-builtin-workers:
    strategy:
      matrix:
        include:
{entries}
    uses: ./.github/workflows/_publish-engine-workers.yml
"""


def make_repo(tmp_path: pathlib.Path, matrix: list[tuple[str, str]], executable: bool = True) -> pathlib.Path:
    """A repo with two engine workers that have skills: stream (`iii-stream`) and engine_fn (`iii`)."""
    for worker_dir, name in (("engine/workers/stream", "iii-stream"), ("engine/workers/engine_fn", "iii")):
        (tmp_path / worker_dir / "skills").mkdir(parents=True)
        (tmp_path / worker_dir / "iii.worker.yaml").write_text(f"name: {name}\ntype: engine\n")
        (tmp_path / worker_dir / "skills" / "SKILL.md").write_text(f"# {name}\n")
    if executable:
        (tmp_path / "engine/registry").mkdir(parents=True)
        (tmp_path / "engine/registry/iii.worker.yaml").write_text("name: iii\ndeploy: executable\nbin: iii\n")
    entries = "\n".join(
        f"          - worker: {worker}\n            worker_dir: {worker_dir}" for worker, worker_dir in matrix
    )
    workflows = tmp_path / ".github/workflows"
    workflows.mkdir(parents=True)
    for name in ("release-iii.yml", "alpha-release.yml"):
        (workflows / name).write_text(WORKFLOW.format(entries=entries))
    return tmp_path


def test_the_repository_matrices_are_valid() -> None:
    assert validate(ROOT) == []


def test_the_engine_executable_needs_no_matrix_entry(tmp_path: pathlib.Path) -> None:
    repo = make_repo(tmp_path, [("iii-stream", "engine/workers/stream")])
    assert validate(repo) == []


def test_a_matrix_entry_for_the_engine_executable_is_refused(tmp_path: pathlib.Path) -> None:
    repo = make_repo(
        tmp_path,
        [("iii-stream", "engine/workers/stream"), ("iii-engine-functions", "engine/workers/engine_fn")],
    )
    errors = validate(repo)
    assert len(errors) == 2
    assert all("'iii' is the engine executable" in error for error in errors)


def test_without_an_engine_executable_every_worker_with_skills_is_published(tmp_path: pathlib.Path) -> None:
    repo = make_repo(tmp_path, [("iii-stream", "engine/workers/stream")], executable=False)
    errors = validate(repo)
    assert errors == [
        "release-iii.yml: missing publishable engine worker 'engine/workers/engine_fn'",
        "alpha-release.yml: missing publishable engine worker 'engine/workers/engine_fn'",
    ]
