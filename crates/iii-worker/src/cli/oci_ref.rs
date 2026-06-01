// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! OCI image-ref helpers shared between the pull path (cache canonicalization,
//! digest sidecar) and the lockfile-writing path (digest normalization,
//! `@sha256:` pinning).
//!
//! Three public functions:
//! - [`canonical_cache_key`] — strip any `@sha256:...` suffix before hashing
//!   so pinned and unpinned variants of the same image share one cache dir.
//! - [`normalize_digest`] — accept `sha256:<hex>` or bare `<hex>`, emit the
//!   64-char lowercase hex payload or error. (Wired in Task 4.)
//! - [`pin_image_ref`] — append `@sha256:{digest}` to a ref unless already
//!   pinned. (Wired in Task 4.)

/// Return the portion of `image_ref` that should be used as the cache-dir
/// hash input. Strips any trailing `@sha256:...` so `foo:latest` and
/// `foo:latest@sha256:abc` resolve to the same cache dir, and so the
/// in-process engine doesn't re-pull an image we just pinned into
/// `config.yaml` / `iii.lock`.
///
/// Examples:
/// - `foo:latest` -> `foo:latest`
/// - `foo:latest@sha256:abc` -> `foo:latest`
/// - `foo@sha256:abc` -> `foo`
/// - `docker.io/foo/bar:tag@sha256:deadbeef...` -> `docker.io/foo/bar:tag`
pub(crate) fn canonical_cache_key(image_ref: &str) -> &str {
    image_ref
        .split_once("@sha256:")
        .map(|(lhs, _)| lhs)
        .unwrap_or(image_ref)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_digest_from_tagged_ref() {
        assert_eq!(
            canonical_cache_key("docker.io/foo/bar:latest@sha256:abc"),
            "docker.io/foo/bar:latest"
        );
    }

    #[test]
    fn strips_digest_from_nameonly_ref() {
        assert_eq!(canonical_cache_key("foo@sha256:abc"), "foo");
    }

    #[test]
    fn unpinned_ref_passes_through() {
        assert_eq!(canonical_cache_key("foo:latest"), "foo:latest");
        assert_eq!(canonical_cache_key("foo"), "foo");
        assert_eq!(
            canonical_cache_key("registry.example.com/path/foo:v1"),
            "registry.example.com/path/foo:v1"
        );
    }

    #[test]
    fn pinned_and_unpinned_canonicalize_equal() {
        // This is the load-bearing invariant: two refs that describe the
        // same image must hash to the same cache dir, so the engine at
        // start time reuses the rootfs we just pulled.
        let unpinned = "docker.io/andersonofl/todo-worker:latest";
        let pinned = "docker.io/andersonofl/todo-worker:latest@sha256:aabbcc";
        assert_eq!(canonical_cache_key(unpinned), canonical_cache_key(pinned));
    }

    #[test]
    fn empty_ref_passes_through() {
        // Degenerate input: empty string. Don't panic, just return it.
        assert_eq!(canonical_cache_key(""), "");
    }

    #[test]
    fn ref_with_only_at_sha256_marker_strips_to_empty_left() {
        // Pathological but deterministic: "@sha256:abc" -> "".
        assert_eq!(canonical_cache_key("@sha256:abc"), "");
    }
}
