#!/usr/bin/env python3
import argparse
import json
import pathlib
import sys
import tomllib
from typing import Any


def normalize_dependencies(raw_deps: Any) -> list[dict[str, Any]]:
    if raw_deps in (None, ""):
        return []
    if isinstance(raw_deps, dict):
        return [{"name": name, "version": version} for name, version in raw_deps.items()]
    if isinstance(raw_deps, list):
        return raw_deps
    raise ValueError(f"`dependencies` must be a map or list, got {type(raw_deps).__name__}")


def derive_registry_function_name(function_id: str, metadata: dict[str, Any] | None) -> str:
    metadata = metadata or {}
    for key in ("registry_name", "name"):
        value = metadata.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
    return function_id


def _extract_array(payload: dict[str, Any], key: str) -> list[dict[str, Any]]:
    value = payload.get(key, [])
    if value is None:
        return []
    if not isinstance(value, list):
        raise ValueError(f"`{key}` must be an array")
    return value


def _read_yaml(path: pathlib.Path) -> Any:
    import yaml
    return yaml.safe_load(path.read_text(encoding="utf-8"))


def _schema_or_empty(value: Any) -> dict[str, Any]:
    if value is None:
        return {}
    if isinstance(value, dict):
        return value
    raise ValueError("function schema fields must be objects or null")


def _metadata_or_empty(value: Any) -> dict[str, Any]:
    return value if isinstance(value, dict) else {}


def _string_or_empty(value: Any) -> str:
    return value if isinstance(value, str) else ""


def _args_format_or_none(value: Any) -> dict[str, Any] | None:
    if value is None:
        return None
    if isinstance(value, dict):
        return value
    raise ValueError("function args_format must be an object or null")


def _observe_fields(function: dict[str, Any]) -> dict[str, Any]:
    """What a function row says about observing the function.

    `args_format` is the JSON Schema of observers' `args` (`None` when the
    function takes no args), `private` is true when only the hosting worker
    calls it (an event source), and `observable` is false when nobody may
    observe it. The engine leaves the defaults out of its rows
    (`private: false`, `observable: true`); the Registry row states all three.
    """
    return {
        "args_format": _args_format_or_none(function.get("args_format")),
        "private": function.get("private") is True,
        "observable": function.get("observable") is not False,
    }


def normalize_worker_interface(
    *,
    worker_name: str,
    worker_info: dict[str, Any],
    function_details: list[dict[str, Any]],
) -> dict[str, list[dict[str, Any]]]:
    """Normalize the engine's worker surface into the Registry shape.

    The worker's interface is its functions. An event source is a function
    row with `private: true` (and `args_format` when observers pass args).

    Args:
        worker_name: Sanity check that the info envelope is the right worker.
        worker_info: Response from `engine::workers::info { name }`.
        function_details: `engine::functions::info` details, one per function
            id `worker_info` lists. Each carries the schemas, the metadata and
            the observe fields (`args_format`, `private`, `observable`).
    """
    worker = worker_info.get("worker") or {}
    if not isinstance(worker, dict):
        raise ValueError("worker_info `worker` must be an object")
    actual_name = worker.get("name") or worker.get("id")
    if actual_name != worker_name:
        raise ValueError(
            f"worker_info envelope is for {actual_name!r}, expected {worker_name!r}"
        )

    functions = []
    for details in function_details:
        if not isinstance(details, dict):
            continue
        function_id = details.get("function_id")
        if not isinstance(function_id, str):
            continue
        metadata = details.get("metadata") or {}
        functions.append(
            {
                "name": derive_registry_function_name(function_id, metadata),
                "description": _string_or_empty(details.get("description")),
                "request_schema": _schema_or_empty(details.get("request_schema")),
                "response_schema": _schema_or_empty(details.get("response_schema")),
                **_observe_fields(details),
                "metadata": _metadata_or_empty(metadata),
            }
        )
    return {"functions": functions}


def build_payload(
    *,
    repo_root: pathlib.Path,
    worker: str,
    worker_dir: pathlib.Path,
    expected_version: str,
    registry_tag: str,
    repo_url: str,
    interface: dict[str, Any],
) -> dict[str, Any]:
    engine_manifest = repo_root / "engine" / "Cargo.toml"
    if not engine_manifest.exists():
        raise ValueError(f"{engine_manifest} not found")
    try:
        engine_version = tomllib.loads(engine_manifest.read_text(encoding="utf-8"))["package"]["version"]
    except KeyError as exc:
        raise ValueError(f"{engine_manifest}: missing [package].version ({exc})") from exc
    if expected_version and expected_version != engine_version:
        raise ValueError(
            f"engine worker version mismatch: input is {expected_version}, engine/Cargo.toml is {engine_version}"
        )

    manifest_path = repo_root / worker_dir / "iii.worker.yaml"
    if not manifest_path.exists():
        raise ValueError(f"{manifest_path} not found")
    meta = _read_yaml(manifest_path) or {}
    if meta.get("type") != "engine":
        raise ValueError(
            f"{worker}: iii.worker.yaml type must be 'engine' (got {meta.get('type')!r})"
        )

    readme_path = repo_root / worker_dir / "README.md"
    readme = readme_path.read_text(encoding="utf-8") if readme_path.exists() else ""

    manifest_name = meta.get("name")
    worker_name = (
        manifest_name.strip()
        if isinstance(manifest_name, str) and manifest_name.strip()
        else worker
    )

    return {
        "worker_name": worker_name,
        "version": engine_version,
        "tag": registry_tag or "latest",
        "type": "engine",
        "readme": readme,
        "repo": repo_url,
        "description": meta.get("description", ""),
        "dependencies": normalize_dependencies(meta.get("dependencies")),
        "config": meta.get("config") or {},
        "functions": interface.get("functions") or [],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--worker", required=True)
    parser.add_argument("--worker-dir", required=True)
    parser.add_argument("--expected-version", default="")
    parser.add_argument("--registry-tag", default="latest")
    parser.add_argument("--repo-url", required=True)
    parser.add_argument("--interface-json", required=True)
    parser.add_argument("--repo-root", default=".")
    parser.add_argument("--out", default="payload.json")
    args = parser.parse_args()

    interface = json.loads(pathlib.Path(args.interface_json).read_text(encoding="utf-8"))
    payload = build_payload(
        repo_root=pathlib.Path(args.repo_root),
        worker=args.worker,
        worker_dir=pathlib.Path(args.worker_dir),
        expected_version=args.expected_version,
        registry_tag=args.registry_tag,
        repo_url=args.repo_url,
        interface=interface,
    )
    pathlib.Path(args.out).write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({k: v for k, v in payload.items() if k != "readme"}, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
