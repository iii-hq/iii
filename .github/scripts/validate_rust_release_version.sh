#!/usr/bin/env bash
set -euo pipefail

manifest_path="$1"
tag_name="$2"
package_name="$3"

expected="${tag_name##*/v}"
actual=$(cargo metadata --format-version 1 --no-deps --manifest-path "$manifest_path" \
  | jq -r --arg package "$package_name" \
    'first(.packages[] | select(.name == $package) | .version) // empty')

if [[ -z "$actual" ]]; then
  echo "error: package '$package_name' not found for manifest '$manifest_path'" >&2
  exit 1
fi

if [[ "$actual" != "$expected" ]]; then
  echo "error: manifest version '$actual' does not match release tag '$expected'" >&2
  exit 1
fi

echo "validated $package_name v$actual against $tag_name"
