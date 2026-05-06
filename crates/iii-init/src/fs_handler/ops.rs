// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! One-shot filesystem operation handlers.
//!
//! Each function maps to an `FsOp` variant except `WriteStart` /
//! `ReadStart`, which are handled by the `streaming` sibling module.
//! All handlers are **sync** (`fn`, not `async fn`) and use only
//! `std::fs` / `std::io` — no tokio anywhere in this file.

use std::io::{BufRead, Read};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use iii_shell_proto::{FsEntry, FsMatch, FsResult, FsSedFileResult};

use super::{FsCallResult, FsError, parse_mode};

// ──────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Build an `FsEntry` from a path and its already-fetched metadata.
/// Uses `symlink_metadata` semantics — callers must pass metadata
/// obtained without following the symlink so `is_symlink` is accurate.
pub(super) fn entry_from_metadata(name: String, _path: &Path, md: &std::fs::Metadata) -> FsEntry {
    let mode_bits = md.mode() & 0o7777;
    let mtime = md.mtime();
    FsEntry {
        name,
        is_dir: md.is_dir(),
        size: md.len(),
        mode: format!("{mode_bits:04o}"),
        mtime,
        is_symlink: md.file_type().is_symlink(),
    }
}

/// Build a sibling path `<path>.iii-tmp-<uuid>` for atomic write
/// patterns. The random suffix keeps concurrent writers from colliding
/// on the same temp name.
pub(super) fn temp_sibling(target: &Path) -> std::path::PathBuf {
    let mut t = target.as_os_str().to_os_string();
    t.push(".iii-tmp-");
    t.push(uuid::Uuid::new_v4().to_string());
    t.into()
}

/// Match a glob `pattern` against a relative path with gitignore-style
/// semantics:
///
/// - If `pattern` contains `/`, match against the full relative path.
///   E.g. `src/*.py` matches `src/main.py` but not `data/foo.py`.
/// - If `pattern` has no `/`, match against the **basename** of
///   `relpath`. E.g. `*.py` matches `src/main.py`, `data/foo.py`, and
///   any `.py` file at any depth.
///
/// A leading `**/` is handled by [`glob_match`]: the pattern `**/*.py`
/// matches at any depth, including the root.
pub(super) fn glob_matches_path(pattern: &str, relpath: &str) -> bool {
    if pattern.contains('/') {
        glob_match(pattern, relpath)
    } else {
        let base = relpath.rsplit('/').next().unwrap_or(relpath);
        glob_match(pattern, base)
    }
}

/// Match a filename against a simple glob. Supports `*` (zero or more
/// of any char **except** `/`) and `?` (single char except `/`).
/// Designed for `*.rs` / `target/*` patterns against the base filename
/// or the relative path under the grep root.
///
/// A leading `**/` is treated as "any depth": `**/*.py` matches both
/// `main.py` (depth 0) and `src/main.py` (depth 1+). The bare pattern
/// `**` matches everything.
pub(super) fn glob_match(pattern: &str, path: &str) -> bool {
    if pattern == "**" {
        return true;
    }
    if let Some(rest) = pattern.strip_prefix("**/") {
        // `**/foo` matches at any depth. Try the basename first
        // (covers depth 0, e.g. `**/*.py` against `main.py`), then
        // fall back to the full path (covers nested cases).
        let base = path.rsplit('/').next().unwrap_or(path);
        if glob_match_simple(rest, base) {
            return true;
        }
        return glob_match_simple(rest, path);
    }
    glob_match_simple(pattern, path)
}

fn glob_match_simple(pattern: &str, path: &str) -> bool {
    let p = pattern.as_bytes();
    let t = path.as_bytes();
    let mut pi = 0usize;
    let mut ti = 0usize;
    let mut star_pi: Option<usize> = None;
    let mut star_ti = 0usize;
    while ti < t.len() {
        let pc = p.get(pi).copied();
        let tc = t[ti];
        match pc {
            Some(b'*') => {
                star_pi = Some(pi);
                star_ti = ti;
                pi += 1;
            }
            Some(b'?') if tc != b'/' => {
                pi += 1;
                ti += 1;
            }
            Some(c) if c == tc => {
                pi += 1;
                ti += 1;
            }
            _ => {
                if let Some(sp) = star_pi {
                    if t[star_ti] == b'/' {
                        return false;
                    }
                    pi = sp + 1;
                    star_ti += 1;
                    ti = star_ti;
                } else {
                    return false;
                }
            }
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

/// Peek up to 8 KiB and check for null bytes — the classic
/// binary-file heuristic. Returns `true` if the file looks binary and
/// should be skipped by grep.
pub(super) fn looks_binary(path: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 8192];
    let n = f.read(&mut buf).unwrap_or(0);
    buf[..n].contains(&0)
}

/// Expand a regex-mode replacement template. Delegates to
/// `regex::Captures::expand` which handles `$1`, `$name`, etc.
fn expand_regex_replacement(caps: &regex::Captures, template: &str) -> String {
    let mut out = String::new();
    caps.expand(template, &mut out);
    out
}

/// Literal per-line replace. Returns `(new_line, replacement_count)`.
///
/// When `case_insensitive` is true we delegate to the regex engine with
/// `(?i)` and an escaped pattern. Hand-rolling case-fold over UTF-8 is
/// unsound: `to_lowercase()` is not byte-length preserving (e.g.
/// `'İ'` U+0130 → `"i\u{0307}"`, 2→3 bytes), so a parallel
/// haystack/line index walk panics or mismatches on the first
/// length-changing codepoint. The case-sensitive path stays a hand-
/// rolled byte search — no regex compile cost on the common path.
fn literal_replace_line(
    line: &str,
    needle: &str,
    case_insensitive: bool,
    replacement: &str,
    first_only: bool,
) -> (String, u64) {
    if line.is_empty() || needle.is_empty() {
        return (line.to_string(), 0);
    }
    if case_insensitive {
        // `(?i)` + literal-escaped needle gives correct Unicode case
        // folding without re-implementing it. `$` in `replacement`
        // would otherwise be expanded as a backref; escape it.
        let escaped_needle = regex::escape(needle);
        let escaped_replacement = replacement.replace('$', "$$");
        let pattern = format!("(?i){escaped_needle}");
        let re = match regex::Regex::new(&pattern) {
            Ok(r) => r,
            // An invalid pattern shouldn't be reachable (escape() always
            // produces a valid regex); fall back to no-op so we never
            // panic on otherwise-good input.
            Err(_) => return (line.to_string(), 0),
        };
        let mut count = 0u64;
        let out = if first_only {
            re.replacen(line, 1, |_caps: &regex::Captures| {
                count += 1;
                escaped_replacement.clone()
            })
            .into_owned()
        } else {
            re.replace_all(line, |_caps: &regex::Captures| {
                count += 1;
                escaped_replacement.clone()
            })
            .into_owned()
        };
        return (out, count);
    }

    // Case-sensitive byte search. Same-byte-length needle and haystack
    // (both `line`), so the byte walk is sound.
    let mut out = String::with_capacity(line.len());
    let mut n = 0u64;
    let mut i = 0usize;
    let bytes = line.as_bytes();
    let needle_bytes = needle.as_bytes();
    while i < line.len() {
        if i + needle_bytes.len() <= bytes.len()
            && &bytes[i..i + needle_bytes.len()] == needle_bytes
        {
            out.push_str(replacement);
            i += needle_bytes.len();
            n += 1;
            if first_only {
                out.push_str(&line[i..]);
                return (out, n);
            }
        } else {
            // Advance by one char boundary.
            let next = line[i..]
                .char_indices()
                .nth(1)
                .map(|(b, _)| i + b)
                .unwrap_or(line.len());
            out.push_str(&line[i..next]);
            i = next;
        }
    }
    (out, n)
}

// ──────────────────────────────────────────────────────────────────────────────
// Public handlers
// ──────────────────────────────────────────────────────────────────────────────

/// List the immediate children of a directory.
///
/// Errors:
/// - `S211` — path does not exist.
/// - `S212` — path exists but is not a directory.
/// - `S216` — I/O error reading the directory.
pub fn ls(path: String) -> FsCallResult {
    let p = Path::new(&path);
    let md = match p.symlink_metadata() {
        Ok(m) => m,
        Err(e) => return Err(FsError::from_io(&path, e)),
    };
    if !md.is_dir() {
        return Err(FsError::new("S212", format!("not a directory: {path}")));
    }
    let rd = match std::fs::read_dir(p) {
        Ok(r) => r,
        Err(e) => return Err(FsError::from_io(&path, e)),
    };
    let mut entries = Vec::new();
    for entry in rd {
        let ent = match entry {
            Ok(e) => e,
            Err(_) => continue, // raced — skip
        };
        let name = ent.file_name().to_string_lossy().into_owned();
        let md = match ent.path().symlink_metadata() {
            Ok(m) => m,
            Err(_) => continue, // raced — skip
        };
        entries.push(entry_from_metadata(name, &ent.path(), &md));
    }
    Ok(FsResult::Ls { entries })
}

/// Stat a single path. Reports the path itself (not its symlink target).
///
/// Errors:
/// - `S211` — path does not exist.
/// - `S216` — I/O error.
pub fn stat(path: String) -> FsCallResult {
    let p = Path::new(&path);
    let md = match p.symlink_metadata() {
        Ok(m) => m,
        Err(e) => return Err(FsError::from_io(&path, e)),
    };
    let name = p
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    Ok(FsResult::Stat(entry_from_metadata(name, p, &md)))
}

/// Create a directory.
///
/// - `parents=true` → `mkdir -p` semantics; existing path is OK.
/// - `parents=false` → fail with `S213` if path already exists;
///   fail with `S211` if the parent does not exist.
///
/// Errors:
/// - `S210` — `mode` is not valid octal.
/// - `S211` — parent dir missing (`parents=false`).
/// - `S213` — path already exists (`parents=false`).
/// - `S215` / `S216` — permission / I/O error.
pub fn mkdir(path: String, mode: String, parents: bool) -> FsCallResult {
    let bits = parse_mode(&mode)?;
    let p = Path::new(&path);

    if p.exists() {
        if parents {
            return Ok(FsResult::Mkdir { created: false });
        }
        return Err(FsError::new("S213", format!("path already exists: {path}")));
    }

    let result = if parents {
        std::fs::create_dir_all(p)
    } else {
        std::fs::create_dir(p)
    };
    result.map_err(|e| FsError::from_io(&path, e))?;

    let perms = std::fs::Permissions::from_mode(bits);
    std::fs::set_permissions(p, perms).map_err(|e| FsError::from_io(&path, e))?;

    Ok(FsResult::Mkdir { created: true })
}

/// Remove a file or directory.
///
/// - For a directory: `recursive=false` requires it to be empty
///   (returns `S214` otherwise); `recursive=true` removes the entire
///   tree.
/// - For a file or symlink: always uses `remove_file` (one operation,
///   no recursion flag needed).
///
/// Errors:
/// - `S211` — path does not exist.
/// - `S214` — directory not empty and `recursive=false`.
/// - `S215` / `S216` — permission / I/O error.
pub fn rm(path: String, recursive: bool) -> FsCallResult {
    let p = Path::new(&path);
    let md = match p.symlink_metadata() {
        Ok(m) => m,
        Err(e) => return Err(FsError::from_io(&path, e)),
    };

    if md.is_dir() && !md.file_type().is_symlink() {
        if recursive {
            std::fs::remove_dir_all(p).map_err(|e| FsError::from_io(&path, e))?;
        } else {
            // Check emptiness first so we return S214 with a clean
            // message rather than the generic ENOTEMPTY io error.
            let mut rd = std::fs::read_dir(p).map_err(|e| FsError::from_io(&path, e))?;
            if rd.next().is_some() {
                return Err(FsError::new("S214", format!("directory not empty: {path}")));
            }
            std::fs::remove_dir(p).map_err(|e| FsError::from_io(&path, e))?;
        }
    } else {
        // Regular file, symlink (including to a dir), or special file.
        std::fs::remove_file(p).map_err(|e| FsError::from_io(&path, e))?;
    }
    Ok(FsResult::Rm { removed: true })
}

/// Change mode (and optionally ownership) of a path.
///
/// - `recursive=true` → walks the entire tree via `walkdir`; the root
///   entry itself counts in the returned `updated` tally.
/// - `uid` / `gid` of `None` means "do not change that field".
///
/// Errors:
/// - `S210` — `mode` is not valid octal.
/// - `S211` — path does not exist.
/// - `S215` / `S216` — permission / I/O error.
pub fn chmod(
    path: String,
    mode: String,
    uid: Option<u32>,
    gid: Option<u32>,
    recursive: bool,
) -> FsCallResult {
    use std::os::unix::fs::chown;

    let bits = parse_mode(&mode)?;
    let p = Path::new(&path);

    if !p.exists() {
        return Err(FsError::new("S211", format!("path not found: {path}")));
    }

    let apply = |target: &Path| -> Result<(), FsError> {
        let perms = std::fs::Permissions::from_mode(bits);
        std::fs::set_permissions(target, perms)
            .map_err(|e| FsError::from_io(&target.to_string_lossy(), e))?;
        if uid.is_some() || gid.is_some() {
            chown(target, uid, gid).map_err(|e| FsError::from_io(&target.to_string_lossy(), e))?;
        }
        Ok(())
    };

    let mut updated: u64 = 0;
    if recursive {
        for entry in walkdir::WalkDir::new(p)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            apply(entry.path())?;
            updated += 1;
        }
    } else {
        apply(p)?;
        updated = 1;
    }
    Ok(FsResult::Chmod { updated })
}

/// Rename / move a path.
///
/// - `overwrite=false` → fail with `S213` if `dst` already exists.
/// - `overwrite=true` → atomically replace `dst` on same-fs moves;
///   copy+unlink on cross-fs moves.
///
/// Errors:
/// - `S211` — `src` does not exist.
/// - `S213` — `dst` exists and `overwrite=false`.
/// - `S215` / `S216` — permission / I/O error.
pub fn mv(src: String, dst: String, overwrite: bool) -> FsCallResult {
    let src_p = Path::new(&src);
    let dst_p = Path::new(&dst);

    if !src_p.exists() {
        return Err(FsError::new("S211", format!("src not found: {src}")));
    }
    if dst_p.exists() && !overwrite {
        return Err(FsError::new("S213", format!("dst already exists: {dst}")));
    }

    match std::fs::rename(src_p, dst_p) {
        Ok(()) => Ok(FsResult::Mv { moved: true }),
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
            // Cross-fs: copy to temp sibling of dst, then rename.
            let tmp = temp_sibling(dst_p);
            std::fs::copy(src_p, &tmp).map_err(|e| FsError::from_io(&dst, e))?;
            if let Err(e) = std::fs::rename(&tmp, dst_p) {
                let _ = std::fs::remove_file(&tmp);
                return Err(FsError::from_io(&dst, e));
            }
            std::fs::remove_file(src_p).map_err(|e| FsError::from_io(&src, e))?;
            Ok(FsResult::Mv { moved: true })
        }
        Err(e) => Err(FsError::from_io(&dst, e)),
    }
}

/// Search files for lines matching `pattern` (a Rust regex).
///
/// - `recursive=true` walks the tree; `false` requires `path` to be a
///   regular file.
/// - `include_glob` / `exclude_glob` filter by filename suffix.
/// - Stops after `max_matches` hits (sets `truncated=true`).
/// - Lines longer than `max_line_bytes` are truncated with `…`.
/// - Binary files (NUL in first 8 KiB) are silently skipped.
///
/// Errors:
/// - `S211` — path does not exist.
/// - `S217` — `pattern` is not a valid regex.
pub fn grep(
    path: String,
    pattern: String,
    recursive: bool,
    ignore_case: bool,
    include_glob: Vec<String>,
    exclude_glob: Vec<String>,
    max_matches: u64,
    max_line_bytes: u64,
) -> FsCallResult {
    let root = Path::new(&path);
    let md = match root.symlink_metadata() {
        Ok(m) => m,
        Err(e) => return Err(FsError::from_io(&path, e)),
    };

    let re = regex::RegexBuilder::new(&pattern)
        .case_insensitive(ignore_case)
        .build()
        .map_err(|e| FsError::new("S217", format!("bad regex: {e}")))?;

    let max_matches_usize = max_matches as usize;
    let max_line_usize = max_line_bytes as usize;

    let mut out: Vec<FsMatch> = Vec::new();
    let mut truncated = false;

    let should_scan = |rel: &str| -> bool {
        if !include_glob.is_empty() && !include_glob.iter().any(|g| glob_matches_path(g, rel)) {
            return false;
        }
        if exclude_glob.iter().any(|g| glob_matches_path(g, rel)) {
            return false;
        }
        true
    };

    // Returns Ok(true) when this scan filled `out` to `max_matches_usize`
    // and the caller should stop walking. Closure mutably borrows `out`
    // only — `truncated` is decided by the caller from the return value
    // so the post-call `if truncated` read doesn't conflict with the
    // closure's lingering capture.
    let mut scan = |file_path: &Path| -> Result<bool, FsError> {
        if looks_binary(file_path) {
            return Ok(false);
        }
        let f = std::fs::File::open(file_path)
            .map_err(|e| FsError::from_io(&file_path.to_string_lossy(), e))?;
        let reader = std::io::BufReader::new(f);
        for (idx, line_res) in reader.lines().enumerate() {
            let Ok(mut line) = line_res else {
                continue; // non-UTF-8 line — skip
            };
            if re.is_match(&line) {
                if max_line_usize > 0 && line.len() > max_line_usize {
                    // String::truncate panics on a non-char-boundary index.
                    // Floor max_line_usize to the nearest boundary so a
                    // multi-byte UTF-8 codepoint straddling the cut doesn't
                    // crash the per-op thread (terminal frame would never
                    // ship and the host trigger would hang to outer timeout).
                    let cut = (0..=max_line_usize)
                        .rev()
                        .find(|&i| line.is_char_boundary(i))
                        .unwrap_or(0);
                    line.truncate(cut);
                    line.push('…');
                }
                out.push(FsMatch {
                    path: file_path.to_string_lossy().into_owned(),
                    line: (idx + 1) as u64,
                    content: line,
                });
                if max_matches_usize > 0 && out.len() >= max_matches_usize {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    };

    if md.is_dir() {
        if !recursive {
            return Err(FsError::new(
                "S210",
                "recursive=false on a directory is unsupported; \
                 pass a file path or set recursive=true",
            ));
        }
        for entry in walkdir::WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let rel = entry
                .path()
                .strip_prefix(root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .into_owned();
            if !should_scan(&rel) {
                continue;
            }
            if scan(entry.path())? {
                truncated = true;
                break;
            }
        }
    } else {
        // Single-file grep — include/exclude globs still apply to the
        // filename so the caller can pre-filter a list.
        let rel = root
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if should_scan(&rel) && scan(root)? {
            truncated = true;
        }
    }

    Ok(FsResult::Grep {
        matches: out,
        truncated,
    })
}

/// Walk `root` and collect every regular-file path that survives
/// `include_glob` / `exclude_glob` filtering. Mirrors the grep walker:
/// `walkdir::WalkDir` with `follow_links(false)` and gitignore-style
/// glob matching against the path **relative to `root`**.
///
/// `root` may be a regular file (returns a single-element list after
/// glob-filtering by basename), a directory (walks it), or a symlink
/// (resolved via `std::fs::metadata`: link-to-file → single-element,
/// link-to-dir → walk the link as if it were the directory using
/// `follow_links(true)` for that one entry-point so `walkdir` will
/// descend into the target).
///
/// Caller is expected to have already validated that `root` exists; a
/// broken symlink here yields an empty result rather than an error
/// (it would have been caught upstream by the metadata probe).
fn collect_files_to_sed(
    root: &Path,
    recursive: bool,
    include_glob: &[String],
    exclude_glob: &[String],
) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let lmd = match root.symlink_metadata() {
        Ok(m) => m,
        Err(_) => return out,
    };

    let passes = |rel: &str| -> bool {
        if !include_glob.is_empty() && !include_glob.iter().any(|g| glob_matches_path(g, rel)) {
            return false;
        }
        if exclude_glob.iter().any(|g| glob_matches_path(g, rel)) {
            return false;
        }
        true
    };

    // Resolve "what is `root` really?" — for a symlink we have to follow
    // it to know whether the caller meant "rewrite this one file" or
    // "walk this directory." `is_symlink()` alone can't tell.
    //
    // - regular file (or symlink → file)  → single-file branch
    // - directory (or symlink → dir)      → walker branch
    // - broken symlink / metadata error   → empty (upstream already
    //   validated existence; treat as nothing-to-do)
    let target_is_dir = if lmd.file_type().is_symlink() {
        match std::fs::metadata(root) {
            Ok(m) => m.is_dir(),
            Err(_) => return out,
        }
    } else {
        lmd.is_dir()
    };

    if !target_is_dir {
        let rel = root
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if passes(&rel) {
            out.push(root.to_path_buf());
        }
        return out;
    }

    // Walker branch. `follow_links` only matters for the *root* entry
    // — we still don't want to follow links discovered mid-walk
    // because they could point outside the tree (mirrors grep's
    // policy). When `root` itself is a symlink-to-dir we'd need
    // `follow_links(true)` so walkdir descends into it; resolve to the
    // canonical target instead, which keeps the inner-walk policy
    // (no-follow) intact.
    let resolved_root: std::path::PathBuf = if lmd.file_type().is_symlink() {
        match std::fs::canonicalize(root) {
            Ok(p) => p,
            Err(_) => return out,
        }
    } else {
        root.to_path_buf()
    };

    let walker = walkdir::WalkDir::new(&resolved_root).follow_links(false);
    let walker = if recursive {
        walker
    } else {
        walker.max_depth(1)
    };
    for entry in walker
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let rel = entry
            .path()
            .strip_prefix(&resolved_root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .into_owned();
        if passes(&rel) {
            out.push(entry.path().to_path_buf());
        }
    }
    out
}

/// Apply a find-and-replace, atomically, to either an explicit list of
/// files (`files`) or every file under `path` that matches the glob
/// filters.
///
/// Caller must supply **exactly one** of `files` (non-empty) or `path`
/// (a directory, a single file, or a symlink to either) — both/neither
/// returns `S210`. The path-form mirrors grep's walker: `walkdir` with
/// `follow_links=false`, gitignore-style glob filtering relative to
/// `path`. Symlink-to-dir is resolved via `canonicalize` so the walker
/// descends into the target.
///
/// `recursive=false` on a directory `path` is rejected with `S210`,
/// matching grep. Use `files: [...]` or `recursive=true` (with a
/// tighter glob) instead.
///
/// - `regex=true` → treat `pattern` as a Rust regex with `$N` / `$name`
///   capture references in `replacement`.
/// - `regex=false` → literal string match.
/// - `first_only=true` → replace only the first match per line.
/// - Per-file failures are isolated; the handler continues to the next
///   file and reports `success=false` in that entry's result.
/// - Writes are atomic: content goes to a temp sibling and is renamed
///   over the original only on success.
///
/// Returns `Err` for: bad regex (S217), empty literal pattern (S210),
/// missing `path` form root (S211), invalid input form (S210), or
/// `recursive=false` on a directory (S210). Per-file I/O errors
/// surface in `FsSedFileResult.success`.
pub fn sed(
    files: Vec<String>,
    path: Option<String>,
    recursive: bool,
    include_glob: Vec<String>,
    exclude_glob: Vec<String>,
    pattern: String,
    replacement: String,
    regex_mode: bool,
    first_only: bool,
    ignore_case: bool,
) -> FsCallResult {
    // Discriminate between the two input forms. We treat an empty
    // `files` vec as "not the files form" so old guests that send only
    // `files` still parse, and new callers can use `path`. Exactly one
    // form must be present.
    let files = match (files.is_empty(), path.as_ref()) {
        (false, None) => files,
        (true, Some(root)) => {
            // Path-form: walk and filter.
            let root_path = Path::new(root);
            // Surface a missing-root error up front so the caller can
            // distinguish it from the "no files matched" case below.
            // We use the symlink-following metadata here so a broken
            // symlink (which only `metadata` notices) also surfaces
            // S211 — same as grep's behaviour on a missing root.
            let _ = root_path
                .symlink_metadata()
                .map_err(|e| FsError::from_io(root, e))?;
            // Resolve target type so we can mirror grep's
            // `recursive=false` rejection on directories. Symlinks
            // here are followed because the caller's intent is "what
            // does this path point at?", not "what is this link".
            let target_is_dir = match std::fs::metadata(root_path) {
                Ok(m) => m.is_dir(),
                Err(e) => return Err(FsError::from_io(root, e)),
            };
            if target_is_dir && !recursive {
                // Mirror grep's policy verbatim. Sed used to silently
                // walk depth-1 here; that diverged from grep without
                // good reason. Realign: callers asking for a depth-1
                // sweep can pass `files: [...]` after enumerating, or
                // pass `recursive: true` with a tighter `include_glob`.
                return Err(FsError::new(
                    "S210",
                    "recursive=false on a directory is unsupported; \
                     pass a file path or set recursive=true",
                ));
            }
            let collected =
                collect_files_to_sed(root_path, recursive, &include_glob, &exclude_glob);
            collected
                .into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect()
        }
        (false, Some(_)) => {
            return Err(FsError::new(
                "S210",
                "sed: provide exactly one of `files` or `path`, not both",
            ));
        }
        (true, None) => {
            return Err(FsError::new(
                "S210",
                "sed: must provide exactly one of `files` or `path`",
            ));
        }
    };

    // Compile the matcher once; a bad pattern aborts the whole call.
    let matcher: Option<regex::Regex> = if regex_mode {
        Some(
            regex::RegexBuilder::new(&pattern)
                .case_insensitive(ignore_case)
                .build()
                .map_err(|e| FsError::new("S217", format!("bad regex: {e}")))?,
        )
    } else if pattern.is_empty() {
        return Err(FsError::new("S210", "pattern is empty"));
    } else {
        None
    };

    let case_fold = ignore_case && !regex_mode;

    let mut results: Vec<FsSedFileResult> = Vec::with_capacity(files.len());
    let mut total: u64 = 0;

    for file in files {
        let p = Path::new(&file);
        let original = match std::fs::read_to_string(p) {
            Ok(s) => s,
            Err(e) => {
                results.push(FsSedFileResult {
                    path: file.clone(),
                    replacements: 0,
                    success: false,
                    error: Some(format!("{e}")),
                });
                continue;
            }
        };

        let mut replacements: u64 = 0;
        let mut output = String::with_capacity(original.len());

        for line in original.split_inclusive('\n') {
            let (new_line, n) = match &matcher {
                Some(re) => {
                    let mut count_here = 0u64;
                    let produced = if first_only {
                        re.replacen(line, 1, |caps: &regex::Captures| {
                            count_here += 1;
                            expand_regex_replacement(caps, &replacement)
                        })
                        .into_owned()
                    } else {
                        re.replace_all(line, |caps: &regex::Captures| {
                            count_here += 1;
                            expand_regex_replacement(caps, &replacement)
                        })
                        .into_owned()
                    };
                    (produced, count_here)
                }
                None => literal_replace_line(line, &pattern, case_fold, &replacement, first_only),
            };
            replacements += n;
            output.push_str(&new_line);
        }

        // Atomic write-to-temp + rename. On any failure we report
        // per-file failure and leave the original intact.
        let tmp = temp_sibling(p);
        let write_result: Result<(), std::io::Error> = (|| {
            let original_md = std::fs::metadata(p)?;
            std::fs::write(&tmp, output.as_bytes())?;
            std::fs::set_permissions(
                &tmp,
                std::fs::Permissions::from_mode(original_md.permissions().mode()),
            )?;
            std::fs::rename(&tmp, p)?;
            Ok(())
        })();

        match write_result {
            Ok(()) => {
                total += replacements;
                results.push(FsSedFileResult {
                    path: file,
                    replacements,
                    success: true,
                    error: None,
                });
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                results.push(FsSedFileResult {
                    path: file,
                    replacements: 0,
                    success: false,
                    error: Some(format!("{e}")),
                });
            }
        }
    }

    Ok(FsResult::Sed {
        results,
        total_replacements: total,
    })
}
