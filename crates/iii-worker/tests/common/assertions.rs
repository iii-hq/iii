// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::path::Path;

/// Assert two paths are equal after canonicalization.
/// On macOS, /tmp -> /private/tmp, so naive string/Path comparison fails.
pub fn assert_paths_eq(left: &Path, right: &Path) {
    let left_canon = std::fs::canonicalize(left)
        .unwrap_or_else(|e| panic!("cannot canonicalize left path {:?}: {}", left, e));
    let right_canon = std::fs::canonicalize(right)
        .unwrap_or_else(|e| panic!("cannot canonicalize right path {:?}: {}", right, e));
    assert_eq!(
        left_canon, right_canon,
        "paths differ after canonicalization:\n  left:  {:?}\n  right: {:?}",
        left_canon, right_canon
    );
}
