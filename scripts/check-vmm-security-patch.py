#!/usr/bin/env python3
"""Verify the narrow vendored VMM patch and its fixed zstd dependency graph."""

import argparse
import hashlib
import io
from pathlib import Path
import tarfile
import tomllib
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
PACKAGE = "msb_krun_vmm-0.1.16"
SHA256 = "bb70c044c60e011e6f43c2e336687850b0910853a0efe37010e9649eff1395fe"
URL = f"https://static.crates.io/crates/msb_krun_vmm/{PACKAGE}.crate"
OLD = b'[target.\'cfg(target_arch = "x86_64")\'.dependencies.zstd]\nversion = "0.13"'
NEW = OLD.replace(b'"0.13"', b'"0.14.3"')


def check(archive):
    if hashlib.sha256(archive).hexdigest() != SHA256:
        raise ValueError("Upstream archive checksum mismatch")
    vendor = ROOT / "vendor/msb_krun_vmm"
    expected_paths = {"README.iii.md", "LICENSE", "THIRD-PARTY"}
    checked = 0
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as source:
        for member in source.getmembers():
            path = Path(member.name).relative_to(PACKAGE)
            if path.is_absolute() or ".." in path.parts:
                raise ValueError(f"Unsafe archive path: {member.name}")
            if member.isdir():
                continue
            if not member.isfile():
                raise ValueError(f"Unexpected archive entry: {member.name}")
            if str(path) == "Cargo.lock":
                continue
            expected_paths.add(path.as_posix())
            expected = source.extractfile(member).read()
            if str(path) == "Cargo.toml":
                if expected.count(OLD) != 1:
                    raise ValueError("Upstream manifest no longer matches the patch")
                expected = expected.replace(OLD, NEW)
            if (vendor / path).read_bytes() != expected:
                raise ValueError(f"Unexpected modification: {path}")
            checked += 1
    actual_paths = {
        path.relative_to(vendor).as_posix()
        for path in vendor.rglob("*")
        if path.is_file()
    }
    if actual_paths != expected_paths:
        raise ValueError(f"Unexpected vendor file set: {actual_paths ^ expected_paths}")

    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
    patch = manifest["patch"]["crates-io"]["msb_krun_vmm"]
    if patch != {"path": "vendor/msb_krun_vmm"}:
        raise ValueError("The workspace must use the verified local VMM patch")
    packages = tomllib.loads((ROOT / "Cargo.lock").read_text())["package"]
    required = {
        "msb_krun_vmm": "0.1.16",
        "zstd": "0.14.3",
        "zstd-safe": "8.2.0",
        "zstd-sys": "2.1.2+zstd.1.5.7",
    }
    for name, version in required.items():
        matches = [package for package in packages if package["name"] == name]
        if len(matches) != 1 or matches[0]["version"] != version:
            raise ValueError(f"Expected only {name} {version} in the lockfile")
        if name == "msb_krun_vmm" and "source" in matches[0]:
            raise ValueError("The lockfile must resolve the VMM to the local patch")
    print(f"Verified {checked} upstream files, the manifest-only patch, and fixed zstd versions.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, help="Use an already downloaded crate archive")
    args = parser.parse_args()
    if args.archive:
        archive = args.archive.read_bytes()
    else:
        with urllib.request.urlopen(URL, timeout=30) as response:
            archive = response.read()
    check(archive)


if __name__ == "__main__":
    main()
