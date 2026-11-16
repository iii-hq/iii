#!/bin/sh
# Installs iii @VERSION@ from the iii registry, with the installer
# iii published for this version.
#   curl -fsSL https://iii.dev/registry/install/iii/@REQUESTED@.sh | sh
# Generated for @REQUESTED@ -> @VERSION@; artefact URLs and SHA-256 digests are pinned.
#
# The registry wrote this preamble: the pinned artefacts and the iii_registry_*
# helpers. The installer iii published follows the line that ends it.

# shellcheck disable=SC2034 # The installer below may not read every value.
{
  III_REGISTRY_URL='https://iii.dev/registry'
  III_REGISTRY_NAME='iii'
  III_REGISTRY_VERSION='@VERSION@'
  III_REGISTRY_REQUESTED='@REQUESTED@'
  III_REGISTRY_TARGETS='aarch64-apple-darwin aarch64-pc-windows-msvc aarch64-unknown-linux-gnu armv7-unknown-linux-gnueabihf i686-pc-windows-msvc x86_64-apple-darwin x86_64-pc-windows-msvc x86_64-unknown-linux-gnu x86_64-unknown-linux-musl'
}

# Sets III_REGISTRY_ARTIFACT_URL, III_REGISTRY_ARTIFACT_SHA256,
# III_REGISTRY_ARTIFACT_BIN (the program inside the archive) and
# III_REGISTRY_ARTIFACT_FORMAT (tar.gz, zip or dmg) for the target $1.
# Returns 1 when this version has no build for it.
iii_registry_artifact() {
  # shellcheck disable=SC2034 # Read by the installer below.
  case "$1" in
    aarch64-apple-darwin)
      III_REGISTRY_ARTIFACT_URL='https://cdn.iii.cloud/workers/iii/@VERSION@/iii-aarch64-apple-darwin.tar.gz'
      III_REGISTRY_ARTIFACT_SHA256='@sha256:aarch64-apple-darwin@'
      III_REGISTRY_ARTIFACT_BIN='iii'
      III_REGISTRY_ARTIFACT_FORMAT='tar.gz'
      ;;
    aarch64-pc-windows-msvc)
      III_REGISTRY_ARTIFACT_URL='https://cdn.iii.cloud/workers/iii/@VERSION@/iii-aarch64-pc-windows-msvc.zip'
      III_REGISTRY_ARTIFACT_SHA256='@sha256:aarch64-pc-windows-msvc@'
      III_REGISTRY_ARTIFACT_BIN='iii'
      III_REGISTRY_ARTIFACT_FORMAT='zip'
      ;;
    aarch64-unknown-linux-gnu)
      III_REGISTRY_ARTIFACT_URL='https://cdn.iii.cloud/workers/iii/@VERSION@/iii-aarch64-unknown-linux-gnu.tar.gz'
      III_REGISTRY_ARTIFACT_SHA256='@sha256:aarch64-unknown-linux-gnu@'
      III_REGISTRY_ARTIFACT_BIN='iii'
      III_REGISTRY_ARTIFACT_FORMAT='tar.gz'
      ;;
    armv7-unknown-linux-gnueabihf)
      III_REGISTRY_ARTIFACT_URL='https://cdn.iii.cloud/workers/iii/@VERSION@/iii-armv7-unknown-linux-gnueabihf.tar.gz'
      III_REGISTRY_ARTIFACT_SHA256='@sha256:armv7-unknown-linux-gnueabihf@'
      III_REGISTRY_ARTIFACT_BIN='iii'
      III_REGISTRY_ARTIFACT_FORMAT='tar.gz'
      ;;
    i686-pc-windows-msvc)
      III_REGISTRY_ARTIFACT_URL='https://cdn.iii.cloud/workers/iii/@VERSION@/iii-i686-pc-windows-msvc.zip'
      III_REGISTRY_ARTIFACT_SHA256='@sha256:i686-pc-windows-msvc@'
      III_REGISTRY_ARTIFACT_BIN='iii'
      III_REGISTRY_ARTIFACT_FORMAT='zip'
      ;;
    x86_64-apple-darwin)
      III_REGISTRY_ARTIFACT_URL='https://cdn.iii.cloud/workers/iii/@VERSION@/iii-x86_64-apple-darwin.tar.gz'
      III_REGISTRY_ARTIFACT_SHA256='@sha256:x86_64-apple-darwin@'
      III_REGISTRY_ARTIFACT_BIN='iii'
      III_REGISTRY_ARTIFACT_FORMAT='tar.gz'
      ;;
    x86_64-pc-windows-msvc)
      III_REGISTRY_ARTIFACT_URL='https://cdn.iii.cloud/workers/iii/@VERSION@/iii-x86_64-pc-windows-msvc.zip'
      III_REGISTRY_ARTIFACT_SHA256='@sha256:x86_64-pc-windows-msvc@'
      III_REGISTRY_ARTIFACT_BIN='iii'
      III_REGISTRY_ARTIFACT_FORMAT='zip'
      ;;
    x86_64-unknown-linux-gnu)
      III_REGISTRY_ARTIFACT_URL='https://cdn.iii.cloud/workers/iii/@VERSION@/iii-x86_64-unknown-linux-gnu.tar.gz'
      III_REGISTRY_ARTIFACT_SHA256='@sha256:x86_64-unknown-linux-gnu@'
      III_REGISTRY_ARTIFACT_BIN='iii'
      III_REGISTRY_ARTIFACT_FORMAT='tar.gz'
      ;;
    x86_64-unknown-linux-musl)
      III_REGISTRY_ARTIFACT_URL='https://cdn.iii.cloud/workers/iii/@VERSION@/iii-x86_64-unknown-linux-musl.tar.gz'
      III_REGISTRY_ARTIFACT_SHA256='@sha256:x86_64-unknown-linux-musl@'
      III_REGISTRY_ARTIFACT_BIN='iii'
      III_REGISTRY_ARTIFACT_FORMAT='tar.gz'
      ;;
    *) return 1 ;;
  esac
}

# Prints the target this machine installs, the best build this version has for
# its uname, or nothing when it has none.
iii_registry_detect_target() {
  case "$(uname -s 2>/dev/null)/$(uname -m 2>/dev/null)" in
    Darwin/arm64 | Darwin/aarch64) echo aarch64-apple-darwin ;;
    Darwin/x86_64) echo x86_64-apple-darwin ;;
    Linux/x86_64 | Linux/amd64) echo x86_64-unknown-linux-musl ;;
    Linux/aarch64 | Linux/arm64) echo aarch64-unknown-linux-gnu ;;
    Linux/armv7l | Linux/armv7) echo armv7-unknown-linux-gnueabihf ;;
    MINGW*/x86_64 | MSYS*/x86_64 | CYGWIN*/x86_64) echo x86_64-pc-windows-msvc ;;
    MINGW*/aarch64 | MINGW*/arm64 | MSYS*/aarch64 | MSYS*/arm64 | CYGWIN*/aarch64 | CYGWIN*/arm64) echo aarch64-pc-windows-msvc ;;
  esac
}

# Downloads the build of the target $1 to the file $2 and checks its SHA-256.
# On failure it says why on stderr, removes $2 and returns 1.
iii_registry_fetch() {
  if ! iii_registry_artifact "$1"; then
    echo "error: $III_REGISTRY_NAME $III_REGISTRY_VERSION has no build for $1 (available: $III_REGISTRY_TARGETS)" >&2
    return 1
  fi
  if command -v curl >/dev/null 2>&1; then
    if [ -t 2 ]; then
      curl -fL --progress-bar --retry 3 --retry-delay 2 --connect-timeout 10 "$III_REGISTRY_ARTIFACT_URL" -o "$2"
    else
      curl -fsSL --retry 3 --retry-delay 2 --connect-timeout 10 "$III_REGISTRY_ARTIFACT_URL" -o "$2"
    fi
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$2" "$III_REGISTRY_ARTIFACT_URL"
  else
    echo "error: curl or wget is required" >&2
    return 1
  fi || {
    rm -f "$2"
    echo "error: could not download $III_REGISTRY_ARTIFACT_URL" >&2
    return 1
  }
  if command -v sha256sum >/dev/null 2>&1; then
    iii_registry_sum=$(sha256sum "$2" | cut -d' ' -f1)
  elif command -v shasum >/dev/null 2>&1; then
    iii_registry_sum=$(shasum -a 256 "$2" | cut -d' ' -f1)
  else
    rm -f "$2"
    echo "error: sha256sum or shasum is required" >&2
    return 1
  fi
  if [ "$iii_registry_sum" != "$III_REGISTRY_ARTIFACT_SHA256" ]; then
    rm -f "$2"
    echo "error: checksum mismatch for $1 (expected $III_REGISTRY_ARTIFACT_SHA256, got $iii_registry_sum)" >&2
    return 1
  fi
}
# --- end of the registry preamble ---
