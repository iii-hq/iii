"""Offline entrypoint checks: never launch an actual iii or iii-worker binary."""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import tomllib

import pytest
import yaml

import collect_engine_worker_interface as collect


ROOT = Path(__file__).resolve().parents[2]
TELEMETRY_ENV = "III_TELEMETRY_ENABLED"
WORKFLOWS = (
    "ci.yml",
    "install-sh.yml",
    "bench-release.yml",
    "release-iii.yml",
    "alpha-release.yml",
    "docker-engine.yml",
    "_publish-engine-workers.yml",
    "_rust-binary.yml",
    "_rust-cargo.yml",
    "_npm.yml",
    "_py.yml",
    "_go.yml",
    "_homebrew.yml",
    "test-scripts.yml",
)


def executable(path: Path, body: str) -> Path:
    path.write_text("#!/bin/sh\nset -eu\n" + body)
    path.chmod(0o755)
    return path


@pytest.fixture(params=[None, "true", "false"], ids=["unset", "enabled", "disabled"])
def parent_env(request: pytest.FixtureRequest, monkeypatch: pytest.MonkeyPatch):
    if request.param is None:
        monkeypatch.delenv(TELEMETRY_ENV, raising=False)
    else:
        monkeypatch.setenv(TELEMETRY_ENV, request.param)
    return request.param


@pytest.mark.parametrize("mode", ["engine", "managed", "compose-file"])
def test_start_launcher_opts_out_child_and_descendant(tmp_path: Path, parent_env, mode: str):
    # Exit before readiness so no socket or long-lived process is needed.
    # Both launch branches still cross their real fork/exec boundary.
    binary = executable(
        tmp_path / "iii",
        'printf "child=%s\\n" "${III_TELEMETRY_ENABLED-unset}"\n'
        "sh -c 'printf \"descendant=%s\\n\" \"${III_TELEMETRY_ENABLED-unset}\"'\n"
        "exit 17\n",
    )
    executable(tmp_path / "nc", "exit 1\n")
    config = tmp_path / "config.yaml"
    config.write_text("engine: {}\n" if mode == "managed" else "workers: []\n")
    pid_file = tmp_path / "engine.pid"
    log_file = tmp_path / "engine.log"
    args = [
        "bash", str(ROOT / "scripts/start-iii.sh"),
        "--binary", str(binary), "--config", str(config), "--port", "1",
        "--pid-file", str(pid_file), "--log-file", str(log_file), "--timeout", "5",
    ]
    if mode == "compose-file":
        compose = tmp_path / "worker-compose.yaml"
        compose.write_text("containers: {}\n")
        args.extend(["--compose-file", str(compose)])
    result = subprocess.run(
        args, capture_output=True, text=True, timeout=15,
        env={**os.environ, "PATH": str(tmp_path) + os.pathsep + os.environ["PATH"]},
    )
    assert result.returncode == 1, result.stdout + result.stderr
    assert log_file.read_text().splitlines() == ["child=false", "descendant=false"]
    assert not pid_file.exists(), "failed startup must clean up its PID file"


def test_interface_collector_opts_out_child_and_descendant(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, parent_env,
):
    executable(
        tmp_path / "iii",
        "printf '{\"telemetry\":\"%s\",\"engine_url\":\"%s\",\"descendant\":\"%s\"}\\n' "
        '"${III_TELEMETRY_ENABLED-unset}" "$III_URL" '
        '"$(sh -c \'printf %s "${III_TELEMETRY_ENABLED-unset}"\')"\n',
    )
    monkeypatch.setenv("PATH", str(tmp_path) + os.pathsep + os.environ["PATH"])
    monkeypatch.setenv("III_URL", "ws://127.0.0.1:19001")
    assert collect.run_iii("engine::workers::list", {}) == {
        "telemetry": "false",
        "engine_url": "ws://127.0.0.1:19001",
        "descendant": "false",
    }
    assert os.environ.get(TELEMETRY_ENV) == parent_env


def test_cli_docs_launcher_opts_out_even_with_explicit_parent_opt_in(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, parent_env,
):
    # Copy the launcher to avoid writing generated docs in the real checkout.
    scripts = tmp_path / "scripts"
    scripts.mkdir()
    launcher = scripts / "generate-cli-docs.sh"
    shutil.copyfile(ROOT / "scripts/generate-cli-docs.sh", launcher)
    executable(
        tmp_path / "cargo",
        'printf "telemetry=%s\\n" "${III_TELEMETRY_ENABLED-unset}"\nexit 42\n',
    )
    monkeypatch.setenv("PATH", str(tmp_path) + os.pathsep + os.environ["PATH"])
    result = subprocess.run(["bash", str(launcher)], capture_output=True, text=True, timeout=10)
    assert result.returncode == 42, result.stdout + result.stderr
    assert "telemetry=false" in result.stdout


@pytest.fixture(scope="module")
def cargo_probe(tmp_path_factory: pytest.TempPathFactory) -> Path:
    # This tiny standalone crate has no III dependency, build script, or network I/O.
    root = tmp_path_factory.mktemp("telemetry-cargo-probe")
    (root / "Cargo.toml").write_text(
        '[package]\nname = "telemetry-env-probe"\nversion = "0.0.0"\n'
        'edition = "2021"\n[workspace]\n'
    )
    (root / "src").mkdir()
    (root / "src/main.rs").write_text(
        'fn main() { println!("{}", std::env::var("III_TELEMETRY_ENABLED")'
        '.unwrap_or_else(|_| "unset".to_owned())); }\n'
    )
    return root


@pytest.mark.skipif(shutil.which("cargo") is None, reason="Cargo is not installed")
@pytest.mark.parametrize("directory", [".", "engine"])
def test_cargo_runtime_default_preserves_explicit_environment(
    cargo_probe: Path, parent_env, directory: str,
):
    result = subprocess.run(
        ["cargo", "run", "--quiet", "--offline", "--manifest-path", str(cargo_probe / "Cargo.toml")],
        cwd=ROOT / directory,
        env={**os.environ, "CARGO_TARGET_DIR": str(cargo_probe / "target")},
        capture_output=True, text=True, timeout=60,
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == (parent_env if parent_env is not None else "false")


def test_cargo_opt_out_is_a_non_forcing_environment_default():
    config = tomllib.loads((ROOT / ".cargo/config.toml").read_text())
    assert config["env"][TELEMETRY_ENV] == {"value": "false", "force": False}


@pytest.mark.parametrize("name", WORKFLOWS)
def test_ci_and_reusable_workflows_explicitly_disable_telemetry(name: str):
    workflow = yaml.safe_load((ROOT / ".github/workflows" / name).read_text())
    assert workflow.get("env", {}).get(TELEMETRY_ENV) == "false", name
    # A narrower environment must not accidentally undo the workflow default.
    for job in workflow["jobs"].values():
        assert job.get("env", {}).get(TELEMETRY_ENV, "false") == "false", name
        for step in job.get("steps", []):
            assert step.get("env", {}).get(TELEMETRY_ENV, "false") == "false", name


@pytest.mark.parametrize(
    ("path", "default"),
    [
        ("engine/docker-compose.yml", "true"),
        ("engine/docker-compose.prod.yml", "true"),
        ("engine/tests/fixtures/templates/docker/docker-compose.yml", "false"),
    ],
)
def test_docker_compose_forwards_runtime_opt_out(path: str, default: str):
    compose = yaml.safe_load((ROOT / path).read_text())
    assert f"{TELEMETRY_ENV}=${{{TELEMETRY_ENV}:-{default}}}" in (
        compose["services"]["iii"]["environment"]
    )


@pytest.mark.skipif(shutil.which("docker") is None, reason="Docker CLI is not installed")
@pytest.mark.parametrize(
    ("path", "default"),
    [
        ("engine/docker-compose.yml", "true"),
        ("engine/docker-compose.prod.yml", "true"),
        ("engine/tests/fixtures/templates/docker/docker-compose.yml", "false"),
    ],
)
def test_compose_interpolation_preserves_explicit_opt_out(
    tmp_path: Path, parent_env, path: str, default: str,
):
    # Render configuration only, without a Docker daemon or running containers.
    env_file = tmp_path / ".env"
    env_file.write_text("")
    result = subprocess.run(
        [
            "docker", "compose", "--project-directory", str(tmp_path),
            "--env-file", str(env_file), "-f", str(ROOT / path),
            "config", "--format", "json",
        ],
        capture_output=True, text=True, timeout=20,
    )
    assert result.returncode == 0, result.stderr
    environment = json.loads(result.stdout)["services"]["iii"]["environment"]
    assert environment[TELEMETRY_ENV] == (parent_env if parent_env is not None else default)
