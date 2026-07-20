#!/usr/bin/env bats

setup() {
  ROOT=$(cd "$BATS_TEST_DIRNAME/../.." && pwd)
  VALIDATE="$ROOT/.github/scripts/validate_rust_release_version.sh"
  VERSION=$(awk -F '"' '/^version = "/ { print $2; exit }' "$ROOT/engine/Cargo.toml")
}

@test "accepts a package with a direct version" {
  run bash "$VALIDATE" "$ROOT/engine/Cargo.toml" "iii/v$VERSION" iii

  [ "$status" -eq 0 ]
}

@test "accepts a package with a workspace-inherited version" {
  run bash "$VALIDATE" "$ROOT/crates/iii-worker/Cargo.toml" "iii/v$VERSION" iii-worker

  [ "$status" -eq 0 ]
}

@test "rejects a manifest version that differs from the release tag" {
  run bash "$VALIDATE" "$ROOT/engine/Cargo.toml" "iii/v999.0.0" iii

  [ "$status" -ne 0 ]
  [[ "$output" == *"does not match release tag '999.0.0'"* ]]
}
