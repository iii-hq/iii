#!/usr/bin/env python3
import argparse
import json
import pathlib
import re
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
    if "::" in function_id:
        return function_id.rsplit("::", 1)[1]
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


def _slug(value: Any, fallback: str) -> str:
    raw = value if isinstance(value, str) else fallback
    slug = re.sub(r"[^a-z0-9]+", "-", raw.lower()).strip("-")
    return slug or fallback


def _derive_trigger_name(trigger: dict[str, Any]) -> str:
    metadata = _metadata_or_empty(trigger.get("metadata"))
    for key in ("registry_name", "name"):
        value = metadata.get(key)
        if isinstance(value, str) and value.strip():
            return _slug(value, "trigger")
    config = trigger.get("config") if isinstance(trigger.get("config"), dict) else {}
    api_path = config.get("api_path")
    if isinstance(api_path, str) and api_path.strip():
        return _slug(api_path, "trigger")
    function_id = trigger.get("function_id")
    if isinstance(function_id, str) and function_id.strip():
        return _slug(function_id.rsplit("::", 1)[-1], "trigger")
    return _slug(trigger.get("trigger_type") or trigger.get("name"), "trigger")


def _normalize_registry_trigger(trigger: dict[str, Any]) -> dict[str, Any]:
    config = trigger.get("config") if isinstance(trigger.get("config"), dict) else {}
    metadata = _metadata_or_empty(trigger.get("metadata")).copy()
    for source_key, metadata_key in (
        ("id", "engine_id"),
        ("trigger_type", "trigger_type"),
        ("function_id", "function_id"),
    ):
        if trigger.get(source_key) is not None:
            metadata.setdefault(metadata_key, trigger.get(source_key))
    if config:
        metadata.setdefault("config", config)
    return {
        "name": _derive_trigger_name(trigger),
        "description": _string_or_empty(trigger.get("description")),
        "invocation_schema": _schema_or_empty(trigger.get("invocation_schema")),
        "return_schema": _schema_or_empty(trigger.get("return_schema")),
        "metadata": metadata,
    }


def normalize_worker_interface(
    *,
    worker_name: str,
    workers_json: dict[str, Any],
    functions_json: dict[str, Any],
    triggers_json: dict[str, Any] | None = None,
) -> dict[str, list[dict[str, Any]]]:
    workers = _extract_array(workers_json, "workers")
    matches = [w for w in workers if w.get("name") == worker_name or w.get("id") == worker_name]
    if len(matches) != 1:
        raise ValueError(
            f"expected exactly one worker matching {worker_name!r}, found {len(matches)}"
        )
    worker_function_ids = matches[0].get("functions") or []
    if not isinstance(worker_function_ids, list):
        raise ValueError("worker `functions` must be an array")
    functions_by_id = {
        f.get("function_id"): f
        for f in _extract_array(functions_json, "functions")
        if f.get("function_id")
    }
    functions = []
    for function_id in worker_function_ids:
        details = functions_by_id.get(function_id, {})
        metadata = details.get("metadata") or {}
        functions.append(
            {
                "name": derive_registry_function_name(function_id, metadata),
                "description": _string_or_empty(details.get("description")),
                "request_schema": _schema_or_empty(details.get("request_format")),
                "response_schema": _schema_or_empty(details.get("response_format")),
                "metadata": _metadata_or_empty(metadata),
            }
        )
    worker_ids = set(worker_function_ids)
    triggers = []
    if triggers_json:
        for trigger in _extract_array(triggers_json, "triggers"):
            if trigger.get("function_id") not in worker_ids:
                continue
            triggers.append(_normalize_registry_trigger(trigger))
    return {"functions": functions, "triggers": triggers}


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

    return {
        "worker_name": worker,
        "version": engine_version,
        "tag": registry_tag or "latest",
        "type": "engine",
        "readme": readme,
        "repo": repo_url,
        "description": meta.get("description", ""),
        "dependencies": normalize_dependencies(meta.get("dependencies")),
        "config": meta.get("config") or {},
        "functions": interface.get("functions") or [],
        "triggers": interface.get("triggers") or [],
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
