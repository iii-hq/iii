// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Adding a container to a compose file the operator wrote.
//!
//! The file is edited as text, not parsed and re-serialised. A round trip
//! through serde would return valid YAML and lose everything that is not data:
//! the comments explaining why a container is pinned where it is, the blank
//! lines grouping it, the quoting style. That file belongs to whoever wrote it,
//! and `compose::add` is not entitled to reformat it.
//!
//! So the block is spliced in, matched to the indentation already in use, and
//! the result is parsed before it is written. A file that would not load is not
//! written at all.

use crate::error::{ComposeError, Result};

/// Registry a bare worker name resolves against, spelled as a reference host.
const DEFAULT_REGISTRY_HOST: &str = "api.workers.iii.dev";

/// Marks the block as machine-written, so the next reader knows why it has no
/// comment of its own and where it came from.
const MARKER: &str = "# added by compose::add";

/// Where a container's worker comes from, once `worker=` has been read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A registry worker. `version` is `None` until it is resolved.
    Package {
        reference: String,
        version: Option<String>,
    },
    /// A directory, relative to the compose file.
    Path { path: String },
}

impl Source {
    /// The word used when a change of kind is refused.
    fn kind(&self) -> &'static str {
        match self {
            Self::Package { .. } => "package",
            Self::Path { .. } => "path",
        }
    }
}

/// A container to splice in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewContainer {
    pub key: String,
    pub source: Source,
    /// Containers this one calls. Two workers may need the same one, so the
    /// shared worker is declared once and named here by both.
    pub depends_on: Vec<String>,
}

/// What the edit did, so the caller can report it and decide whether to restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Added(String),
    /// Same key, different version: an upgrade or a downgrade, both wanted.
    Replaced {
        text: String,
        from: String,
        to: String,
    },
    /// Same key, same version. Nothing to do, and saying so beats a no-op
    /// restart of a project that is already what was asked for.
    Unchanged,
}

/// Reads `worker=` into a key and a source.
///
/// Four spellings, and the leading character separates them: a path starts with
/// `.` or `/`, and everything else is a registry reference — which may carry a
/// host (`api.workers.iii.dev/state`) or a scope, so "contains a slash" would
/// misread those as directories.
pub fn parse_worker(spec: &str) -> Result<NewContainer> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(invalid(spec, "it is empty"));
    }

    if spec.starts_with('.') || spec.starts_with('/') {
        let key = spec
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .filter(|segment| !segment.is_empty() && *segment != "." && *segment != "..")
            .ok_or_else(|| invalid(spec, "the path does not end in a directory name"))?;
        return Ok(NewContainer {
            key: key.to_string(),
            source: Source::Path {
                path: spec.to_string(),
            },
            depends_on: Vec::new(),
        });
    }

    // `name@version`, and the version is optional. Split from the right so a
    // scoped name keeps any `@` of its own.
    let (name, version) = match spec.rsplit_once('@') {
        Some((name, version)) if !name.is_empty() && !version.is_empty() => {
            (name, Some(version.to_string()))
        }
        Some(_) => {
            return Err(invalid(
                spec,
                "it has an empty name or version around the '@'",
            ));
        }
        None => (spec, None),
    };

    let key = name.rsplit('/').next().unwrap_or(name);
    if key.is_empty() {
        return Err(invalid(spec, "it names no worker"));
    }
    let reference = if name.contains('/') {
        name.to_string()
    } else {
        format!("{DEFAULT_REGISTRY_HOST}/{name}")
    };

    Ok(NewContainer {
        key: key.to_string(),
        source: Source::Package { reference, version },
        depends_on: Vec::new(),
    })
}

fn invalid(spec: &str, reason: &str) -> ComposeError {
    ComposeError::InvalidWorkerSpec {
        spec: spec.to_string(),
        reason: reason.to_string(),
    }
}

/// Splices `new` into the `containers:` mapping of `text`.
///
/// Appended rather than inserted anywhere clever: the order of the mapping does
/// not affect anything — start order comes from `depends_on` — so the end is
/// where it disturbs the smallest part of the file and of its diff.
pub fn upsert_container(text: &str, new: &NewContainer) -> Result<Outcome> {
    let lines: Vec<&str> = text.lines().collect();
    let containers = find_containers(&lines)?;

    let indent = entry_indent(&lines, &containers);
    let block = render(new, &indent);

    match find_entry(&lines, &containers, &indent, &new.key) {
        Some(entry) => {
            // A package and a directory are not two versions of one worker.
            // Without this the version comparison below reads `unpinned` for a
            // `path://` entry, calls a resolved package a version change, and
            // rewrites the operator's local worker away.
            let declared = declared_kind(&lines[entry.clone()]);
            let wanted_kind = new.source.kind();
            if let Some(declared) = declared
                && declared != wanted_kind
            {
                return Err(ComposeError::WorkerSourceChanged {
                    container: new.key.clone(),
                    name: new.key.clone(),
                    from: declared.to_string(),
                    to: wanted_kind.to_string(),
                });
            }

            let existing = declared_version(&lines[entry.clone()]);
            let wanted = wanted_version(new);
            // Version alone is not the whole declaration. An entry written
            // before its dependencies were known carries the right version and
            // no `depends_on`, and leaving it would start the worker by where
            // its line happens to sit rather than after what it calls.
            let mut needs = new.depends_on.clone();
            needs.sort();
            if existing == wanted && declared_needs(&lines[entry.clone()]) == needs {
                return Ok(Outcome::Unchanged);
            }
            let mut out: Vec<String> = lines[..entry.start].iter().map(|l| l.to_string()).collect();
            out.extend(
                rewrite(&lines[entry.clone()], new, &indent)
                    .lines()
                    .map(str::to_string),
            );
            out.extend(lines[entry.end..].iter().map(|l| l.to_string()));
            Ok(Outcome::Replaced {
                text: join(&out, text),
                from: existing.unwrap_or_else(|| "unpinned".to_string()),
                to: wanted.unwrap_or_else(|| "unpinned".to_string()),
            })
        }
        None => {
            let mut out: Vec<String> = lines[..containers.end]
                .iter()
                .map(|l| l.to_string())
                .collect();
            // Trailing blank lines inside the mapping belong after the new
            // entry, not between the last one and it.
            while out.last().is_some_and(|l| l.trim().is_empty()) {
                out.pop();
            }
            out.extend(block.lines().map(str::to_string));
            out.extend(lines[containers.end..].iter().map(|l| l.to_string()));
            Ok(Outcome::Added(join(&out, text)))
        }
    }
}

/// Keeps the file's final newline as it was: adding one to a file without it,
/// or dropping one from a file with it, is a diff line nobody asked for.
fn join(lines: &[String], original: &str) -> String {
    let mut text = lines.join("\n");
    if original.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// The half-open line range of the `containers:` mapping body.
#[derive(Debug, Clone)]
struct Block {
    /// First line of the body, after the `containers:` key itself.
    start: usize,
    end: usize,
}

fn find_containers(lines: &[&str]) -> Result<Block> {
    let key = lines
        .iter()
        .position(|line| line.trim_end() == "containers:")
        .ok_or_else(|| ComposeError::InvalidWorkerSpec {
            spec: "containers".to_string(),
            reason: "the compose file has no top-level `containers:` mapping to add to".to_string(),
        })?;

    let start = key + 1;
    let mut end = start;
    for (offset, line) in lines[start..].iter().enumerate() {
        // A blank line may sit between entries, so it does not end the mapping;
        // the first line at column zero does.
        if line.trim().is_empty() || line.starts_with([' ', '\t']) {
            end = start + offset + 1;
        } else {
            break;
        }
    }
    Ok(Block { start, end })
}

/// The indentation the file already gives its container keys, so the new entry
/// matches rather than imposing a house style.
fn entry_indent(lines: &[&str], containers: &Block) -> String {
    lines[containers.start..containers.end]
        .iter()
        .find(|line| !line.trim().is_empty())
        .map(|line| line[..line.len() - line.trim_start().len()].to_string())
        .unwrap_or_else(|| "  ".to_string())
}

/// The line range of one container entry: its key line and everything indented
/// under it, plus any comment lines directly above that describe it.
fn find_entry(lines: &[&str], containers: &Block, indent: &str, key: &str) -> Option<Entry> {
    let head = format!("{indent}{key}:");
    let at =
        (containers.start..containers.end).find(|i| lines[*i].trim_end() == head.trim_end())?;

    let mut end = at + 1;
    for line in &lines[at + 1..containers.end] {
        if line.trim().is_empty() || line.starts_with(&format!("{indent} ")) {
            end += 1;
        } else {
            break;
        }
    }
    // Comments immediately above belong to the entry they describe.
    let mut start = at;
    while start > containers.start && lines[start - 1].trim_start().starts_with('#') {
        start -= 1;
    }
    // And trailing blank lines belong to whatever follows, not to this entry.
    while end > at + 1 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    Some(Entry { start, end })
}

type Entry = std::ops::Range<usize>;

/// The `depends_on:` an entry declares, sorted so two lists compare by content
/// rather than by the order someone wrote them in.
fn declared_needs(entry: &[&str]) -> Vec<String> {
    let mut inside = false;
    let mut needs = Vec::new();
    for line in entry {
        let trimmed = line.trim();
        // `depends_on: [queue, state]` declares the same thing as a block list,
        // and reading it as empty would rewrite an entry that already says what
        // was asked for — and a rewrite is where fields get lost.
        if let Some(inline) = trimmed.strip_prefix("depends_on:")
            && let Some(list) = inline
                .trim()
                .strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
        {
            needs.extend(
                list.split(',')
                    .map(|name| name.trim().trim_matches(['"', '\'']).to_string())
                    .filter(|name| !name.is_empty()),
            );
            continue;
        }
        if trimmed == "depends_on:" {
            inside = true;
            continue;
        }
        if inside {
            match trimmed.strip_prefix("- ") {
                Some(name) => needs.push(name.trim().trim_matches(['"', '\'']).to_string()),
                // The list ended; anything after it belongs to another key.
                None if !trimmed.is_empty() => break,
                None => {}
            }
        }
    }
    needs.sort();
    needs
}

/// The `version:` an entry declares, if it declares one.
/// Whether the entry names a registry package or a directory, read from its
/// `worker:` line. `None` when the entry has none, which a hand-written file
/// may do when the manifest supplies it.
fn declared_kind(entry: &[&str]) -> Option<&'static str> {
    entry.iter().find_map(|line| {
        let value = line.trim().strip_prefix("worker:")?.trim();
        let value = value.trim_matches(['"', '\'']);
        if value.starts_with("path://") || value.starts_with('.') || value.starts_with('/') {
            Some("path")
        } else if value.starts_with("package://") {
            Some("package")
        } else {
            None
        }
    })
}

fn declared_version(entry: &[&str]) -> Option<String> {
    entry.iter().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("version:")
            .map(|value| value.trim().trim_matches(['"', '\'']).to_string())
            .filter(|value| !value.is_empty())
    })
}

fn wanted_version(new: &NewContainer) -> Option<String> {
    match &new.source {
        Source::Package { version, .. } => version.clone(),
        Source::Path { .. } => None,
    }
}

/// Rewrites an existing entry, keeping every line it does not own.
///
/// A replacement changes the version and the dependencies. Everything else in
/// the entry belongs to whoever wrote it — `env_file` naming the credentials a
/// worker needs, `config_name`, `working_dir`, a `scripts.run` — and rendering
/// the block afresh would drop all of it, leaving a project that starts and
/// cannot work.
fn rewrite(entry: &[&str], new: &NewContainer, indent: &str) -> String {
    let inner = format!("{indent}{indent}");
    let mut kept: Vec<&str> = Vec::new();
    let mut skipping_list = false;

    for line in entry
        .iter()
        .skip_while(|line| !line.trim_end().ends_with(':'))
        .skip(1)
    {
        let trimmed = line.trim();
        // The list items under a `depends_on:` we are replacing.
        if skipping_list {
            if trimmed.starts_with("- ") {
                continue;
            }
            skipping_list = false;
        }
        if trimmed.starts_with("worker:") || trimmed.starts_with("version:") {
            continue;
        }
        if trimmed.starts_with("depends_on:") {
            skipping_list = !trimmed.contains('[');
            continue;
        }
        kept.push(line);
    }

    let mut out = String::new();
    // The comments above the key, and the key itself, exactly as they were.
    for line in entry
        .iter()
        .take_while(|line| !line.trim_end().ends_with(':'))
    {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&format!("{indent}{}:\n", new.key));
    match &new.source {
        Source::Package { reference, version } => {
            out.push_str(&format!("{inner}worker: package://{reference}\n"));
            if let Some(version) = version {
                out.push_str(&format!("{inner}version: \"{version}\"\n"));
            }
        }
        Source::Path { path } => {
            out.push_str(&format!("{inner}worker: path://{path}\n"));
        }
    }
    if !new.depends_on.is_empty() {
        out.push_str(&format!("{inner}depends_on:\n"));
        for dependency in &new.depends_on {
            out.push_str(&format!("{inner}{indent}- {dependency}\n"));
        }
    }
    for line in kept {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The YAML for one entry, indented to match the file.
fn render(new: &NewContainer, indent: &str) -> String {
    let inner = format!("{indent}{indent}");
    let mut out = String::new();
    out.push_str(&format!("{indent}{MARKER}\n"));
    out.push_str(&format!("{indent}{}:\n", new.key));
    match &new.source {
        Source::Package { reference, version } => {
            out.push_str(&format!("{inner}worker: package://{reference}\n"));
            if let Some(version) = version {
                out.push_str(&format!("{inner}version: \"{version}\"\n"));
            }
        }
        Source::Path { path } => {
            out.push_str(&format!("{inner}worker: path://{path}\n"));
        }
    }
    if !new.depends_on.is_empty() {
        out.push_str(&format!("{inner}depends_on:\n"));
        for dependency in &new.depends_on {
            out.push_str(&format!("{inner}{indent}- {dependency}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "\
namespace: default

containers:
  # the local one
  todo:
    worker: path://./workers/todo
    scripts:
      run: ./bin/todo-worker

  pi:
    worker: package://pi
    version: \"0.1.12\"
";

    fn added(text: &str) -> String {
        match upsert_container(text, &parse_worker("state@0.21.4").unwrap()).unwrap() {
            Outcome::Added(text) => text,
            other => panic!("expected an addition, got {other:?}"),
        }
    }

    #[test]
    fn a_bare_name_resolves_against_the_default_registry() {
        let parsed = parse_worker("state").unwrap();
        assert_eq!(parsed.key, "state");
        assert_eq!(
            parsed.source,
            Source::Package {
                reference: "api.workers.iii.dev/state".to_string(),
                version: None
            }
        );
    }

    #[test]
    fn a_name_can_pin_its_version() {
        let parsed = parse_worker("state@0.21.4-alpha.4").unwrap();
        assert_eq!(
            parsed.source,
            Source::Package {
                reference: "api.workers.iii.dev/state".to_string(),
                version: Some("0.21.4-alpha.4".to_string())
            }
        );
    }

    /// A reference may carry a host, which has slashes and is not a directory.
    /// The leading character is what separates the two, not the slash.
    #[test]
    fn a_host_is_a_reference_and_a_dot_is_a_path() {
        assert_eq!(
            parse_worker("registry.example/team/thing").unwrap().source,
            Source::Package {
                reference: "registry.example/team/thing".to_string(),
                version: None
            }
        );
        assert_eq!(
            parse_worker("./workers/api").unwrap().source,
            Source::Path {
                path: "./workers/api".to_string()
            }
        );
        assert_eq!(parse_worker("./workers/api").unwrap().key, "api");
    }

    #[test]
    fn the_comments_of_the_file_survive() {
        let out = added(FILE);
        assert!(out.contains("# the local one"), "{out}");
        assert!(out.contains("run: ./bin/todo-worker"), "{out}");
        assert!(out.contains("version: \"0.1.12\""), "{out}");
    }

    #[test]
    fn the_entry_is_appended_and_parses() {
        let out = added(FILE);
        let containers = out.split("containers:").nth(1).unwrap();
        let order: Vec<&str> = ["todo:", "pi:", "state:"]
            .iter()
            .map(|key| {
                assert!(containers.contains(key), "{key} missing from {out}");
                *key
            })
            .collect();
        let positions: Vec<usize> = order
            .iter()
            .map(|key| containers.find(key).unwrap())
            .collect();
        assert!(positions[0] < positions[1], "todo should precede pi");
        assert!(positions[1] < positions[2], "state should be last: {out}");

        crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml")
            .expect("the edited file should still load");
    }

    #[test]
    fn the_indentation_of_the_file_is_matched() {
        let four = FILE
            .replace("\n  ", "\n    ")
            .replace("\n      ", "\n        ");
        let out = match upsert_container(&four, &parse_worker("state@1.0.0").unwrap()).unwrap() {
            Outcome::Added(text) => text,
            other => panic!("{other:?}"),
        };
        assert!(out.contains("\n    state:\n"), "{out}");
        assert!(out.contains("\n        worker: package://"), "{out}");
    }

    #[test]
    fn the_same_version_is_left_alone() {
        let once = added(FILE);
        let again = upsert_container(&once, &parse_worker("state@0.21.4").unwrap()).unwrap();
        assert_eq!(again, Outcome::Unchanged);
    }

    /// The reason a repeat is not simply refused: pinning a different version is
    /// how an upgrade — or a rollback — is asked for.
    #[test]
    fn a_different_version_replaces_in_place() {
        let once = added(FILE);
        let out = match upsert_container(&once, &parse_worker("state@0.22.0").unwrap()).unwrap() {
            Outcome::Replaced { text, from, to } => {
                assert_eq!(from, "0.21.4");
                assert_eq!(to, "0.22.0");
                text
            }
            other => panic!("expected a replacement, got {other:?}"),
        };
        assert_eq!(
            out.matches("state:").count(),
            1,
            "duplicated the key: {out}"
        );
        assert!(out.contains("version: \"0.22.0\""), "{out}");
        assert!(!out.contains("version: \"0.21.4\""), "{out}");
        crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").expect("should still load");
    }

    /// An entry written before its dependencies were known has the right
    /// version and no `depends_on`. Leaving it alone would start the worker by
    /// where its line sits rather than after what it calls — which is how a
    /// worker added by an older build kept starting fourth out of eight.
    #[test]
    fn an_entry_missing_its_dependencies_is_rewritten() {
        let with_deps = NewContainer {
            key: "state".to_string(),
            source: Source::Package {
                reference: "api.workers.iii.dev/state".to_string(),
                version: Some("0.21.4".to_string()),
            },
            depends_on: vec!["queue".to_string()],
        };

        let once = added(FILE);
        // Same key, same version, but the graph now says it needs something.
        let out = match upsert_container(&once, &with_deps).unwrap() {
            Outcome::Replaced { text, .. } => text,
            other => panic!("expected a rewrite, got {other:?}"),
        };
        assert!(out.contains("depends_on:"), "{out}");
        assert!(out.contains("- queue"), "{out}");
        assert_eq!(out.matches("state:").count(), 1, "duplicated: {out}");

        // And with the dependencies already written, it is left alone.
        assert_eq!(
            upsert_container(&out, &with_deps).unwrap(),
            Outcome::Unchanged
        );
    }

    /// A replacement rewrites the version, not the container. Everything else
    /// in the entry is the operator's: `env_file` carries the credentials a
    /// worker needs, and losing it on an upgrade is a project that starts and
    /// cannot work.
    #[test]
    fn a_replacement_keeps_what_it_does_not_own() {
        let file = "\
namespace: default

containers:
  llm-router:
    worker: package://api.workers.iii.dev/llm-router
    version: \"1.4.7\"
    config_name: llm-router
    env_file:
      - ./providers.env
";
        let out = match upsert_container(file, &parse_worker("llm-router@1.5.0").unwrap()).unwrap()
        {
            Outcome::Replaced { text, .. } => text,
            other => panic!("expected a replacement, got {other:?}"),
        };
        assert!(out.contains("version: \"1.5.0\""), "{out}");
        assert!(out.contains("env_file:"), "lost env_file: {out}");
        assert!(
            out.contains("- ./providers.env"),
            "lost the file it named: {out}"
        );
        assert!(
            out.contains("config_name: llm-router"),
            "lost config_name: {out}"
        );
        crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").expect("should still load");
    }

    /// `depends_on: [queue]` is the same declaration as a block list, so it has
    /// to compare equal — otherwise an entry is rewritten for no reason, and a
    /// rewrite is where fields get lost.
    #[test]
    fn an_inline_depends_on_is_read_like_a_block_one() {
        let file = "\
namespace: default

containers:
  api:
    worker: package://api.workers.iii.dev/api
    version: \"1.0.0\"
    depends_on: [queue, state]
";
        let mut wanted = parse_worker("api@1.0.0").unwrap();
        wanted.depends_on = vec!["queue".to_string(), "state".to_string()];
        assert_eq!(
            upsert_container(file, &wanted).unwrap(),
            Outcome::Unchanged,
            "an inline list should read as the dependencies it declares"
        );
    }

    #[test]
    fn a_file_without_containers_is_refused() {
        let err = upsert_container("namespace: default\n", &parse_worker("state").unwrap())
            .expect_err("there is nothing to add to");
        assert!(err.to_string().contains("containers"), "{err}");
    }

    #[test]
    fn an_empty_spec_is_refused() {
        assert!(parse_worker("   ").is_err());
        assert!(parse_worker("state@").is_err());
        assert!(parse_worker("@1.0.0").is_err());
    }

    #[test]
    fn a_file_without_a_trailing_newline_keeps_not_having_one() {
        let out = added(FILE.trim_end());
        assert!(!out.ends_with('\n'), "gained a trailing newline");
    }
    #[test]
    fn a_package_never_replaces_a_local_worker_of_the_same_name() {
        // The hazard is quiet: `path://` carries no version, so comparing
        // versions alone reads this as unpinned -> 1.2.3 and rewrites the
        // entry, dropping whatever the operator kept under it.
        let text =
            "containers:\n  state:\n    worker: path://./workers/state\n    working_dir: .\n";
        let new = NewContainer {
            key: "state".to_string(),
            source: Source::Package {
                reference: "workers.iii.dev/state".to_string(),
                version: Some("1.2.3".to_string()),
            },
            depends_on: vec![],
        };

        let err = upsert_container(text, &new).unwrap_err();
        assert_eq!(err.code(), "WORKER_SOURCE_CHANGED");
    }

    #[test]
    fn a_local_worker_never_replaces_a_package_either() {
        let text = "containers:\n  state:\n    worker: package://workers.iii.dev/state\n    version: \"1.2.3\"\n";
        let new = NewContainer {
            key: "state".to_string(),
            source: Source::Path {
                path: "./workers/state".to_string(),
            },
            depends_on: vec![],
        };

        assert_eq!(
            upsert_container(text, &new).unwrap_err().code(),
            "WORKER_SOURCE_CHANGED"
        );
    }

    #[test]
    fn a_version_change_within_one_kind_is_still_a_replacement() {
        let text = "containers:\n  state:\n    worker: package://workers.iii.dev/state\n    version: \"1.0.0\"\n";
        let new = NewContainer {
            key: "state".to_string(),
            source: Source::Package {
                reference: "workers.iii.dev/state".to_string(),
                version: Some("1.2.3".to_string()),
            },
            depends_on: vec![],
        };

        assert!(matches!(
            upsert_container(text, &new).unwrap(),
            Outcome::Replaced { .. }
        ));
    }
}
