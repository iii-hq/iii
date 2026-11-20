// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Editing containers in a compose file the operator wrote.
//!
//! The file is edited as text, not parsed and re-serialised. A round trip
//! through serde would return valid YAML and lose everything that is not data:
//! the comments explaining why a container is pinned where it is, the blank
//! lines grouping it, the quoting style. That file belongs to whoever wrote it,
//! and compose edit operations are not entitled to reformat it.
//!
//! So each block is spliced in or out and matched to the indentation already in
//! use. A removal also drops references to the removed key from surviving
//! `start_after` fields, because a dangling edge would make the file impossible
//! to start. Every caller parses the result before it is written.

use serde::Deserialize;
use serde_yaml::{Mapping, Value};

use crate::error::{ComposeError, Result};

/// An add request accepts the short worker spec or a container declaration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum WorkerInput {
    Spec(String),
    Definition(
        #[serde(deserialize_with = "deserialize_definition")]
        #[schemars(with = "crate::config::RawContainer")]
        serde_json::Map<String, serde_json::Value>,
    ),
}

/// Validate container field types while retaining explicit empty and null values.
fn deserialize_definition<'de, D>(
    deserializer: D,
) -> std::result::Result<serde_json::Map<String, serde_json::Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    crate::config::RawContainer::deserialize(&value).map_err(serde::de::Error::custom)?;
    // Keep field presence: an omitted map must not clear an existing map.
    match value {
        serde_json::Value::Object(fields) => Ok(fields),
        _ => Err(serde::de::Error::custom("expected a container object")),
    }
}

impl WorkerInput {
    /// Validates the declaration with the same field types as the compose file.
    pub fn parse(&self) -> Result<NewContainer> {
        let fields = match self {
            Self::Spec(spec) => return parse_worker(spec),
            Self::Definition(fields) => fields,
        };
        let raw: crate::config::RawContainer =
            serde_json::from_value(serde_json::Value::Object(fields.clone()))
                .map_err(|err| invalid("container", &err.to_string()))?;
        let mut new = parse_worker(&raw.worker)?;
        if let Some(version) = raw.version {
            match &mut new.source {
                Source::Package {
                    version: pinned, ..
                } => {
                    if pinned.as_ref().is_some_and(|pinned| pinned != &version) {
                        return Err(invalid(
                            &raw.worker,
                            "worker and version specify different versions",
                        ));
                    }
                    *pinned = Some(version);
                }
                Source::Path { .. } => {
                    new.fields
                        .insert(Value::from("version"), Value::from(version));
                }
            }
        }
        new.start_after = raw.start_after;
        new.start_after.sort();
        new.start_after.dedup();
        for (key, value) in fields {
            if key != "worker" && key != "version" {
                new.fields.insert(
                    Value::from(key.clone()),
                    serde_yaml::to_value(value)
                        .map_err(|err| invalid(&raw.worker, &err.to_string()))?,
                );
            }
        }
        if fields.contains_key("version") && matches!(new.source, Source::Path { .. }) {
            new.fields
                .entry(Value::from("version"))
                .or_insert(Value::Null);
        }
        if fields.contains_key("start_after") {
            new.fields.insert(
                Value::from("start_after"),
                Value::Sequence(new.start_after.iter().cloned().map(Value::from).collect()),
            );
        }
        Ok(new)
    }
}

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
    /// Declaration text for persistence, including unexpanded/escaped references on explicit
    /// requests. Resolve package identity from the effective parsed container, never this text.
    /// Generated declarations use canonical references instead.
    pub source: Source,
    /// Containers this one calls. Two workers may need the same one, so the
    /// shared worker is declared once and named here by both.
    pub start_after: Vec<String>,
    /// Explicit fields to replace. Omitted fields keep their existing values.
    pub fields: Mapping,
}

/// What the edit did, so the caller can report it and decide whether to restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Added(String),
    /// The source, version, dependencies, or explicit settings changed.
    Replaced {
        text: String,
        from: String,
        to: String,
    },
    /// The declaration already has the requested source and settings.
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
    let explicit_path = spec.starts_with("path://");
    let explicit_package = spec.starts_with("package://");
    let spec = spec
        .strip_prefix("path://")
        .or_else(|| spec.strip_prefix("package://"))
        .unwrap_or(spec);
    if spec.is_empty() {
        return Err(invalid(spec, "it is empty"));
    }

    if explicit_path || (!explicit_package && (spec.starts_with('.') || spec.starts_with('/'))) {
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
            start_after: Vec::new(),
            fields: Mapping::new(),
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
        start_after: Vec::new(),
        fields: Mapping::new(),
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
/// not affect anything — start order comes from `start_after` — so the end is
/// where it disturbs the smallest part of the file and of its diff.
pub fn upsert_container(text: &str, new: &NewContainer) -> Result<Outcome> {
    let normalized = normalize_inline_empty_containers(text);
    let text = normalized.as_deref().unwrap_or(text);
    let lines: Vec<&str> = text.lines().collect();
    let containers = find_containers(&lines)?;

    let indent = entry_indent(&lines, &containers);
    let block = render(new, &indent)?;

    match find_entry(&lines, &containers, &indent, &new.key) {
        Some(entry) => {
            // A package and a directory are not two versions of one worker.
            // Without this the version comparison below reads `unpinned` for a
            // `path://` entry, calls a resolved package a version change, and
            // rewrites the operator's local worker away.
            let existing_fields = entry_fields(text, &new.key)?;
            let declared_source = existing_fields
                .get("worker")
                .and_then(Value::as_str)
                .map(|worker| {
                    // Compose already supplies the container key. Unlike a new CLI
                    // request, a declared path URI need not end in a directory name
                    // (`path://.` and `path://..` are valid Compose sources).
                    if let Some(path) = worker.strip_prefix("path://") {
                        if path.is_empty() {
                            return Err(invalid(worker, "the declared path is empty"));
                        }
                        Ok(Source::Path {
                            path: path.to_string(),
                        })
                    } else {
                        parse_worker(worker).map(|declared| declared.source)
                    }
                })
                .transpose()?;
            let declared = declared_source.as_ref().map(Source::kind);
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

            let existing = existing_fields
                .get("version")
                .and_then(Value::as_str)
                .map(str::to_string);
            let wanted = wanted_version(new);
            let mut changes = declaration_fields(new, Some(&existing_fields))?;
            if declared_source
                .as_ref()
                .is_some_and(|declared| match (declared, &new.source) {
                    (
                        Source::Package {
                            reference: from, ..
                        },
                        Source::Package { reference: to, .. },
                    ) => from == to,
                    (Source::Path { path: from }, Source::Path { path: to }) => from == to,
                    _ => false,
                })
            {
                changes.remove("worker");
            }
            changes.retain(|key, value| existing_fields.get(key) != Some(value));
            if changes.is_empty() {
                return Ok(Outcome::Unchanged);
            }
            let mut out: Vec<String> = lines[..entry.start].iter().map(|l| l.to_string()).collect();
            out.extend(
                rewrite(&lines[entry.clone()], &changes, &indent)?
                    .lines()
                    .map(str::to_string),
            );
            out.extend(lines[entry.end..].iter().map(|l| l.to_string()));
            let candidate = join(&out, text);
            let candidate_fields = entry_fields(&candidate, &new.key)
                .map_err(|err| invalid(&new.key, &format!("edit leaves invalid YAML: {err}")))?;
            // A replaced field may have defined an anchor. Do not attach that
            // anchor to its new value or silently change an untouched alias.
            for (field, value) in &existing_fields {
                if !changes.contains_key(field) && candidate_fields.get(field) != Some(value) {
                    return Err(invalid(
                        &new.key,
                        "edit changes an unrequested field through an alias",
                    ));
                }
            }
            Ok(Outcome::Replaced {
                text: candidate,
                from: existing.unwrap_or_else(|| "unpinned".to_string()),
                to: wanted.unwrap_or_else(|| "unpinned".to_string()),
            })
        }
        None => {
            // The text editor's block locator does not recognize `key: {...}`.
            // Normalize that existing entry before deciding this is an append;
            // otherwise a normal update writes a duplicate container key.
            if let Some(block) = inline_entry_as_block(&lines, &containers, &indent, &new.key)? {
                return upsert_container(&join(&block, text), new);
            }
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

/// Removes one container entry and every dependency edge that points to it.
///
/// Unrelated text stays byte-for-byte as the operator wrote it. Dependency
/// edits preserve block or inline form, quoting, comments, and surviving list
/// items where possible. An empty `start_after` field is removed with its last
/// item so the result remains a valid compose declaration.
pub fn remove_container(text: &str, key: &str) -> Result<Option<String>> {
    // Keep terminators in the slices. Rebuilding from `str::lines()` would
    // silently turn every surviving CRLF into LF.
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let containers = find_containers(&lines)?;
    let indent = entry_indent(&lines, &containers);
    if find_complete_entry(&lines, &containers, &indent, key).is_none() {
        return Ok(None);
    }
    remove_containers(text, &[key]).map(Some)
}

/// Apply a requested batch against one original document and validate only the
/// final candidate. Intermediate removal order must not orphan anchors that are
/// used exclusively by other entries in the same batch. No file is written here.
pub(crate) fn remove_containers(text: &str, keys: &[&str]) -> Result<String> {
    let Some(&key) = keys.first() else {
        return Ok(text.to_string());
    };
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let containers = find_containers(&lines)?;
    let indent = entry_indent(&lines, &containers);
    let mut entries = Vec::new();
    for requested in keys {
        let (_, entry) =
            find_complete_entry(&lines, &containers, &indent, requested).ok_or_else(|| {
                ComposeError::UnknownContainer {
                    container: (*requested).into(),
                }
            })?;
        if !entries.contains(&entry) {
            entries.push(entry);
        }
    }
    // A comment immediately before the next entry can also be included in the
    // preceding block. Apply its removal once, using the union of line spans,
    // before translating any range into offsets in the original document.
    entries.sort_by_key(|entry| entry.start);
    let mut merged: Vec<Entry> = Vec::new();
    for entry in entries {
        if let Some(last) = merged.last_mut()
            && entry.start < last.end
        {
            last.end = last.end.max(entry.end);
        } else {
            merged.push(entry);
        }
    }
    let entries = merged;
    let document: Value =
        serde_yaml::from_str(text).map_err(|err| invalid(key, &err.to_string()))?;
    let offsets = line_offsets(&lines);
    let mut edits = dependency_removals(
        text,
        &document,
        &lines,
        &offsets,
        &containers,
        &indent,
        &entries,
        keys,
    )?;
    edits.extend(
        entries
            .iter()
            .map(|entry| (offsets[entry.start]..offsets[entry.end], String::new())),
    );
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.0.start));

    let mut out = text.to_string();
    for (range, replacement) in edits {
        out.replace_range(range, &replacement);
    }
    // Removing an anchor can invalidate aliases outside the removed entry.
    // Never return such a candidate to the caller that will persist it.
    let candidate: Value = serde_yaml::from_str(&out)
        .map_err(|err| invalid(key, &format!("removal leaves invalid YAML: {err}")))?;
    if candidate
        .get("containers")
        .and_then(Value::as_mapping)
        .is_some_and(|entries| {
            entries.values().any(|entry| {
                entry
                    .get("start_after")
                    .and_then(Value::as_sequence)
                    .is_some_and(|needs| {
                        needs
                            .iter()
                            .any(|need| need.as_str().is_some_and(|name| keys.contains(&name)))
                    })
            })
        })
    {
        return Err(invalid(
            key,
            "removal leaves a dependency on the removed container",
        ));
    }
    Ok(out)
}

/// True when Compose created this container block and may remove it as a stale dependency.
pub(crate) fn is_generated_container(text: &str, key: &str) -> Result<bool> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let containers = find_containers(&lines)?;
    let indent = entry_indent(&lines, &containers);
    let Some((head, _)) = find_complete_entry(&lines, &containers, &indent, key) else {
        return Ok(false);
    };
    Ok(head > containers.start && lines[head - 1].trim() == MARKER)
}

/// What [`explicit_vm_mode`] wrote for one container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VmRewrite {
    /// The whole file, rewritten.
    pub text: String,
    /// Whether a command of the container's own became `iii sandbox run --command`.
    pub wrapped: bool,
}

/// The `scripts.run` that runs `command` inside container `worker`'s VM, where 0.26 ran it
/// (spec 7.1): `iii call sandbox::start worker=<worker> 'command=<command>'`.
pub(crate) fn sandbox_run_command(worker: &str, command: &str) -> String {
    crate::sandbox::start_line(worker, Some(command))
}

/// Decodes only the canonical line [`sandbox_run_command`] writes, not shell syntax: its worker
/// and its guest command.
pub(crate) fn sandbox_guest_command(run: &str) -> Option<(String, String)> {
    let worker = crate::sandbox::call_argument(run, "worker")?;
    let command = crate::sandbox::call_argument(run, "command")?;
    (sandbox_run_command(&worker, &command) == run).then_some((worker, command))
}

/// Writes the explicit VM mode of container `key` (spec 7.1): `adapter` in `start_after`,
/// `scripts.build: iii sandbox build` when it has no `scripts.build`, and `scripts.run: iii
/// sandbox run` when it has none, or `iii sandbox run --command` around a command of its own.
/// Values are read as written, before interpolation, so no variable is expanded into the file.
/// Every other key, the comments and the rest of the file stay as they are, except a comment on
/// an inline `scripts: {...}` line, which is rewritten as a block. `None` when nothing is missing.
pub(crate) fn explicit_vm_mode(text: &str, key: &str, adapter: &str) -> Result<Option<VmRewrite>> {
    let fields = entry_fields(text, key)?;
    let scripts = fields.get("scripts").and_then(Value::as_mapping);
    let declared = |name: &str| {
        scripts
            .and_then(|scripts| scripts.get(name))
            .filter(|value| !value.is_null())
    };
    let declared_run = declared("run").map(scalar_text);
    let (run, wrapped) = match &declared_run {
        None => (crate::sandbox::start_line(key, None), false),
        // The form written before `iii call` streamed, with its guest command if it had one.
        Some(run) if crate::sandbox::is_legacy_sandbox(run) => (
            crate::sandbox::start_line(key, crate::sandbox::legacy_guest_command(run).as_deref()),
            false,
        ),
        Some(run) if crate::sandbox::is_sandbox_run(run) => (run.clone(), false),
        Some(run) => (sandbox_run_command(key, run), true),
    };
    let set_run = declared_run.as_deref().is_none_or(|run| {
        !crate::sandbox::is_sandbox_run(run) || crate::sandbox::is_legacy_sandbox(run)
    });
    let set_build = declared("build")
        .map(scalar_text)
        .is_none_or(|build| crate::sandbox::is_legacy_sandbox(&build));
    let build_script = crate::sandbox::build_line(key);
    let add_adapter = !fields
        .get("start_after")
        .and_then(Value::as_sequence)
        .is_some_and(|dependencies| {
            dependencies
                .iter()
                .any(|dependency| dependency.as_str() == Some(adapter))
        });
    if !set_run && !set_build && !add_adapter {
        return Ok(None);
    }

    let normalized = normalize_inline_empty_containers(text);
    let text = normalized.as_deref().unwrap_or(text);
    let lines: Vec<&str> = text.lines().collect();
    let containers = find_containers(&lines)?;
    let indent = entry_indent(&lines, &containers);
    let Some(entry) = find_entry(&lines, &containers, &indent, key) else {
        // An inline `key: {...}` entry is written as a block first.
        let block = inline_entry_as_block(&lines, &containers, &indent, key)?
            .ok_or_else(|| invalid(key, "no such container to rewrite"))?;
        return explicit_vm_mode(&join(&block, text), key, adapter);
    };
    let entry_lines = &lines[entry.clone()];
    let inner = entry_lines
        .iter()
        .find_map(|line| {
            let trimmed = line.trim_start();
            let width = line.len() - trimmed.len();
            (width > indent.len() && !trimmed.is_empty() && !trimmed.starts_with('#'))
                .then(|| line[..width].to_string())
        })
        .unwrap_or_else(|| format!("{indent}{indent}"));
    // (line index in the file, lines removed, lines put there)
    let mut edits: Vec<(usize, usize, Vec<String>)> = Vec::new();
    let render = |name: &str, value: &Value, at: &str| -> Result<Vec<String>> {
        Ok(render_field(name, value, at)?
            .lines()
            .map(str::to_string)
            .collect())
    };

    let mut wanted = Mapping::new();
    if set_build {
        wanted.insert(Value::from("build"), Value::from(build_script.clone()));
    }
    if set_run {
        wanted.insert(Value::from("run"), Value::from(run.clone()));
    }
    match field_span(entry_lines, &inner, "scripts") {
        None if !wanted.is_empty() => {
            edits.push((
                entry.end,
                0,
                render("scripts", &Value::Mapping(wanted), &inner)?,
            ));
        }
        None => {}
        // Nothing in them to write: only `start_after` gains the adapter.
        Some(_) if wanted.is_empty() => {}
        Some((head, end)) => {
            // Edited in place, scripts another container aliases would hand it this container's
            // VM lines: they are split by hand first.
            if let Some(anchor) = shared_anchor(&lines, entry.start + head, entry.start + end) {
                return Err(invalid(
                    key,
                    &format!(
                        "its scripts are shared with another container through the YAML anchor \
                         &{anchor}; give each its own scripts, then run this again"
                    ),
                ));
            }
            let value = without_comment(
                entry_lines[head][inner.len()..]
                    .split_once(':')
                    .map_or("", |(_, value)| value),
            );
            let children = &entry_lines[head + 1..end];
            let child = children
                .iter()
                .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
                .map(|line| leading_whitespace(line).to_string());
            // A flow map that starts on the next line has no keys of its own lines: it is written
            // again whole, as an inline one is.
            let flow = flow_span(entry_lines, head).is_some();
            match child.filter(|_| opens_block(value) && !flow) {
                // A block mapping: its other keys and comments stay line for line.
                Some(child) => {
                    let run_line = field_span(children, &child, "run");
                    let build_line = field_span(children, &child, "build");
                    let last = entry.start + head + 1 + children.len();
                    let at = |index: usize| entry.start + head + 1 + index;
                    if set_run {
                        let line = render("run", &Value::from(run.clone()), &child)?;
                        match run_line {
                            Some((from, to)) => edits.push((at(from), to - from, line)),
                            None => edits.push((last, 0, line)),
                        }
                    }
                    if set_build {
                        let line = render("build", &Value::from(build_script.clone()), &child)?;
                        match (build_line, run_line) {
                            (Some((from, to)), _) => edits.push((at(from), to - from, line)),
                            (None, Some((from, _))) => edits.push((at(from), 0, line)),
                            (None, None) => edits.push((last, 0, line)),
                        }
                    }
                }
                // Inline, empty or null: written again as a block, its comment kept on the
                // `scripts:` line.
                None => {
                    let mut merged = scripts.cloned().unwrap_or_default();
                    for (name, value) in wanted {
                        merged.insert(name, value);
                    }
                    let mut block = render("scripts", &Value::Mapping(merged), &inner)?;
                    let head_value = entry_lines[head][inner.len()..]
                        .split_once(':')
                        .map_or("", |(_, value)| value);
                    block[0].push_str(&trailing_comment(head_value, value));
                    edits.push((entry.start + head, end - head, block));
                }
            }
        }
    }

    if add_adapter {
        let item = serde_yaml::to_string(adapter)
            .map_err(|err| invalid(key, &err.to_string()))?
            .trim_end()
            .to_string();
        match field_span(entry_lines, &inner, "start_after") {
            None => {
                let after = field_span(entry_lines, &inner, "version")
                    .or_else(|| field_span(entry_lines, &inner, "worker"))
                    .map_or(entry_lines.len(), |(_, end)| end);
                edits.push((
                    entry.start + after,
                    0,
                    vec![format!("{inner}start_after: [{item}]")],
                ));
            }
            Some((head, end)) => {
                if let Some(flow) = flow_span(entry_lines, head) {
                    let (close_line, close_column) = flow.close;
                    let line = entry_lines[close_line];
                    if close_line > head && line[..close_column].trim().is_empty() {
                        let item_indent = entry_lines[flow.open.0 + 1..close_line]
                            .iter()
                            .find(|line| {
                                !line.trim().is_empty() && !line.trim_start().starts_with('#')
                            })
                            .map_or_else(
                                || format!("{}  ", leading_whitespace(entry_lines[flow.open.0])),
                                |line| leading_whitespace(line).to_string(),
                            );
                        if let Some((last_line, last_column)) = flow.last
                            && !entry_lines[last_line][last_column..].starts_with(',')
                        {
                            let mut rewritten = entry_lines[last_line].to_string();
                            let token_len = rewritten[last_column..]
                                .chars()
                                .next()
                                .map_or(0, char::len_utf8);
                            rewritten.insert(last_column + token_len, ',');
                            edits.push((entry.start + last_line, 1, vec![rewritten]));
                        }
                        edits.push((
                            entry.start + close_line,
                            0,
                            vec![format!("{item_indent}{item},")],
                        ));
                    } else {
                        let separator = match flow.last {
                            None => "",
                            Some((line, column))
                                if entry_lines[line][column..].starts_with(',') =>
                            {
                                " "
                            }
                            Some(_) => ", ",
                        };
                        let mut rewritten = line.to_string();
                        rewritten.insert_str(close_column, &format!("{separator}{item}"));
                        edits.push((entry.start + close_line, 1, vec![rewritten]));
                    }
                } else if let Some(last_item) = entry_lines[head + 1..end]
                    .iter()
                    .rev()
                    .find(|line| line.trim_start().starts_with("- "))
                {
                    edits.push((
                        entry.start + end,
                        0,
                        vec![format!("{}- {item}", leading_whitespace(last_item))],
                    ));
                } else {
                    let mut dependencies = fields
                        .get("start_after")
                        .and_then(Value::as_sequence)
                        .cloned()
                        .unwrap_or_default();
                    dependencies.push(Value::from(adapter));
                    let flow = dependencies
                        .iter()
                        .map(scalar_text)
                        .collect::<Vec<_>>()
                        .join(", ");
                    edits.push((
                        entry.start + head,
                        end - head,
                        vec![format!("{inner}start_after: [{flow}]")],
                    ));
                }
            }
        }
    }

    let mut out: Vec<String> = lines.iter().map(|line| (*line).to_string()).collect();
    edits.sort_by_key(|(at, ..)| std::cmp::Reverse(*at));
    for (at, removed, replacement) in edits {
        out.splice(at..at + removed, replacement);
    }
    Ok(Some(VmRewrite {
        text: join(&out, text),
        wrapped,
    }))
}

/// Expand a flow-mapping entry without serializing its values: anchors, aliases,
/// scalar spelling and nested flow collections stay connected to the document.
fn inline_entry_as_block(
    lines: &[&str],
    containers: &Block,
    indent: &str,
    key: &str,
) -> Result<Option<Vec<String>>> {
    let Some(at) = (containers.start..containers.end).find(|index| {
        lines[*index]
            .strip_prefix(indent)
            .filter(|body| !body.starts_with(char::is_whitespace))
            .and_then(|body| body.split_once(':'))
            .and_then(|(name, _)| serde_yaml::from_str::<String>(name).ok())
            .is_some_and(|name| name == key)
    }) else {
        return Ok(None);
    };
    // Parse the complete document, not a detached value whose aliases would
    // have no anchor definitions. Also reject duplicate/invalid input first.
    let original: Value =
        serde_yaml::from_str(&lines.join("\n")).map_err(|err| invalid(key, &err.to_string()))?;
    if !original
        .get("containers")
        .and_then(|v| v.get(key))
        .is_some_and(Value::is_mapping)
    {
        return Err(invalid(key, "expected a container mapping"));
    }
    let span = flow_span(&lines[..containers.end], at)
        .ok_or_else(|| invalid(key, "expected a flow mapping to normalize"))?;
    let (open_row, open_column) = span.open;
    let (close_row, close_column) = span.close;
    if !lines[open_row][open_column..].starts_with('{') {
        return Err(invalid(key, "expected a flow mapping to normalize"));
    }
    let (name, _) = lines[at]
        .strip_prefix(indent)
        .and_then(|body| body.split_once(':'))
        .ok_or_else(|| invalid(key, "missing container key"))?;
    let colon = lines[at]
        .find(':')
        .ok_or_else(|| invalid(key, "missing container key"))?;
    let mut properties = Vec::new();
    let mut comments = Vec::new();
    for (row, line) in lines.iter().enumerate().take(open_row + 1).skip(at) {
        let start = if row == at { colon + 1 } else { 0 };
        let end = if row == open_row {
            open_column
        } else {
            line.len()
        };
        let prefix = &line[start..end];
        let data = without_comment(prefix);
        if !data.trim().is_empty() {
            properties.push(data.trim());
        }
        let comment = prefix[data.len()..].trim();
        if !comment.is_empty() {
            comments.push(comment);
        }
    }
    let properties = properties.join(" ");
    if !opens_block(&properties) {
        return Err(invalid(key, "invalid flow mapping properties"));
    }
    let mut body = Vec::new();
    for (row, line) in lines.iter().enumerate().take(close_row + 1).skip(open_row) {
        let start = if row == open_row { open_column + 1 } else { 0 };
        let end = if row == close_row {
            close_column
        } else {
            line.len()
        };
        body.push(&line[start..end]);
    }
    let body = body.join("\n");
    let inner = format!("{indent}{indent}");
    let mut out: Vec<String> = lines[..at].iter().map(|line| (*line).to_string()).collect();
    let properties = if properties.is_empty() {
        properties
    } else {
        format!(" {properties}")
    };
    out.push(format!(
        "{indent}{name}:{properties}{}",
        trailing_comment(lines[close_row], &lines[close_row][..close_column + 1])
    ));
    out.extend(
        comments
            .into_iter()
            .map(|comment| format!("{inner}{comment}")),
    );
    for field in
        flow_mapping_parts(&body).ok_or_else(|| invalid(key, "invalid flow mapping span"))?
    {
        let mut leading = true;
        for line in field.lines() {
            let trimmed = line.trim_start();
            // Comments between pairs are retained. Only the first actual field
            // line is aligned; continuation text is never folded or reserialized.
            if leading {
                if trimmed.is_empty() {
                    continue;
                }
                if trimmed.starts_with('#') {
                    out.push(format!("{inner}{trimmed}"));
                } else {
                    out.push(format!("{inner}{}", block_flow_pair(trimmed)));
                    leading = false;
                }
            } else {
                out.push(format!("{inner}{line}"));
            }
        }
    }
    out.extend(
        lines[close_row + 1..]
            .iter()
            .map(|line| (*line).to_string()),
    );
    // This edit changes presentation only. Refuse rather than silently changing
    // scalar folding, anchor resolution, tags or any unrelated document value.
    let normalized: Value =
        serde_yaml::from_str(&out.join("\n")).map_err(|err| invalid(key, &err.to_string()))?;
    if normalized != original {
        return Err(invalid(
            key,
            "flow mapping normalization changed document values",
        ));
    }
    Ok(Some(out))
}

/// Flow mappings permit JSON-style `"key":"value"` pairs. Block mappings
/// require separation after the key colon. Find that colon outside a quoted key
/// and insert only the missing space, never rewriting the value or nested pairs.
fn block_flow_pair(line: &str) -> String {
    // A colon in a trailing comment is not a mapping separator. Keep the
    // comment and its whitespace untouched when a null key needs a colon.
    let data = without_comment(line);
    let mut quote = None;
    let mut chars = data.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        match quote {
            Some('"') => match c {
                '\\' => {
                    chars.next();
                }
                '"' => quote = None,
                _ => {}
            },
            Some(_) => {
                if c == '\'' {
                    if chars.peek().is_some_and(|(_, next)| *next == '\'') {
                        chars.next();
                    } else {
                        quote = None;
                    }
                }
            }
            None => match c {
                '\'' | '"' => quote = Some(c),
                ':' => {
                    let end = at + 1;
                    if line[end..]
                        .chars()
                        .next()
                        .is_some_and(|next| !next.is_whitespace())
                    {
                        return format!("{} {}", &line[..end], &line[end..]);
                    }
                    return line.to_string();
                }
                _ => {}
            },
        }
    }
    if quote.is_none() && !data.trim().is_empty() {
        let end = data.trim_end().len();
        return format!("{}:{}", &line[..end], &line[end..]);
    }
    line.to_string()
}

/// Split only outer mapping pairs, retaining raw field text and nested syntax.
/// Semantic validation belongs to the full-document YAML parser above.
fn flow_mapping_parts(body: &str) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut delimiters = Vec::new();
    let mut quote = None;
    let mut comment = false;
    let mut verbatim_tag = false;
    // Whitespace can continue a plain scalar; it does not itself permit node
    // properties. Only collection/value boundaries start a new YAML node.
    let mut node_start = true;
    let mut plain_scalar = false;
    let mut property = false;
    let mut previous: Option<char> = None;
    let mut chars = body.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if comment {
            if c == '\n' {
                comment = false;
            }
            previous = Some(c);
            continue;
        }
        if verbatim_tag {
            if c == '>' {
                verbatim_tag = false;
            } else if c.is_whitespace() {
                return None;
            }
            previous = Some(c);
            continue;
        }
        if property {
            if c.is_whitespace() {
                property = false;
                previous = Some(c);
                continue;
            }
            if !",[]{}".contains(c) {
                previous = Some(c);
                continue;
            }
            property = false;
        }
        match quote {
            Some('"') => match c {
                '\\' => {
                    chars.next();
                }
                '"' => quote = None,
                _ => {}
            },
            Some(_) => {
                if c == '\'' {
                    if chars.peek().is_some_and(|(_, next)| *next == '\'') {
                        chars.next();
                    } else {
                        quote = None;
                    }
                }
            }
            None => match c {
                '!' if node_start && chars.peek().is_some_and(|(_, next)| *next == '<') => {
                    chars.next();
                    verbatim_tag = true;
                }
                '!' | '&' if node_start => property = true,
                '#' if previous.is_none_or(|p| p.is_whitespace() || ",[{".contains(p)) => {
                    comment = true
                }
                '\'' | '"' if node_start => {
                    quote = Some(c);
                    node_start = false;
                    plain_scalar = false;
                }
                '[' | '{' => {
                    delimiters.push(if c == '[' { ']' } else { '}' });
                    node_start = true;
                    plain_scalar = false;
                }
                ']' | '}' => {
                    if delimiters.pop() != Some(c) {
                        return None;
                    }
                    node_start = false;
                    plain_scalar = false;
                }
                ',' => {
                    if delimiters.is_empty() {
                        parts.push(&body[start..at]);
                        start = at + 1;
                    }
                    node_start = true;
                    plain_scalar = false;
                }
                ':' if !plain_scalar
                    || chars.peek().is_none_or(|(_, next)| {
                        next.is_whitespace() || ",[]{}".contains(*next)
                    }) =>
                {
                    node_start = true;
                    plain_scalar = false;
                }
                '?' if node_start && chars.peek().is_none_or(|(_, next)| next.is_whitespace()) => {}
                c if c.is_whitespace() => {}
                _ => {
                    node_start = false;
                    plain_scalar = true;
                }
            },
        }
        previous = Some(c);
    }
    if quote.is_some() || verbatim_tag || !delimiters.is_empty() {
        return None;
    }
    parts.push(&body[start..]);
    Some(parts)
}

/// The comment `line` carries after `data`, its part without the comment, as ` # ...`; empty when
/// there is none.
fn trailing_comment(line: &str, data: &str) -> String {
    let comment = line[data.len()..].trim();
    if comment.is_empty() {
        String::new()
    } else {
        format!(" {comment}")
    }
}

/// Whether the text after `key:` opens a block mapping: nothing, or only an anchor and a tag
/// (`&shared`, `!!map`), which then stay on that line.
fn opens_block(value: &str) -> bool {
    value
        .split_whitespace()
        .all(|token| token.starts_with('&') || token.starts_with('!'))
}

/// An anchor that lines `from..to` of `lines` define and a line outside them aliases: the node
/// is shared, so an edit of it in place edits the other too.
fn shared_anchor(lines: &[&str], from: usize, to: usize) -> Option<String> {
    let anchors: Vec<&str> = lines[from..to]
        .iter()
        .flat_map(|line| {
            let body = without_comment(line).trim_start();
            let body = body.strip_prefix("- ").unwrap_or(body);
            let mut value = body
                .split_once(':')
                .map_or("", |(_, value)| value)
                .trim_start();
            let mut found = Vec::new();
            while value.starts_with(['&', '!']) {
                let end = value.find(char::is_whitespace).unwrap_or(value.len());
                found.extend(value[..end].strip_prefix('&'));
                value = value[end..].trim_start();
            }
            found
        })
        .collect();
    anchors
        .into_iter()
        .find(|name| {
            lines
                .iter()
                .enumerate()
                .filter(|(at, _)| !(from..to).contains(at))
                .any(|(_, line)| aliases(without_comment(line), name))
        })
        .map(str::to_string)
}

/// Whether `line` holds the alias node `*name`: a `*` outside quotes where a node starts, at the
/// line's start or after `: `, `- `, `? `, `[`, `{` or `,`. One inside a scalar, as in
/// `run: echo *name`, is text.
fn aliases(line: &str, name: &str) -> bool {
    let alias = format!("*{name}");
    // The last character outside quotes that is not whitespace, and whether whitespace followed.
    let (mut previous, mut spaced): (Option<char>, bool) = (None, false);
    let starts_node = |previous: Option<char>, spaced: bool| match previous {
        None => true,
        Some(c) if "[{,".contains(c) => true,
        Some(c) => ":-?".contains(c) && spaced,
    };
    let mut chars = line.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '\'' | '"' if starts_node(previous, spaced) => {
                let mut escaped = false;
                for (_, inner) in chars.by_ref() {
                    match inner {
                        '\\' if c == '"' && !escaped => escaped = true,
                        inner if inner == c && !escaped => break,
                        _ => escaped = false,
                    }
                }
                (previous, spaced) = (Some(c), false);
                continue;
            }
            '*' if starts_node(previous, spaced) && line[at..].starts_with(&alias) => {
                let after = line[at + alias.len()..].chars().next();
                if after.is_none_or(|c| c.is_whitespace() || ",]}".contains(c)) {
                    return true;
                }
            }
            _ => {}
        }
        if c.is_whitespace() {
            spaced = true;
        } else {
            (previous, spaced) = (Some(c), false);
        }
    }
    false
}

/// A YAML scalar as the text it holds; another value as its YAML.
fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => serde_yaml::to_string(other)
            .unwrap_or_default()
            .trim_end()
            .to_string(),
    }
}

/// Positions in a flow collection, excluding comments and quoted delimiters.
struct FlowSpan {
    open: (usize, usize),
    close: (usize, usize),
    last: Option<(usize, usize)>,
}

/// Finds the matching flow delimiter across lines without rewriting any scalar text.
fn flow_span(lines: &[&str], head: usize) -> Option<FlowSpan> {
    let line = lines[head];
    let indent = leading_whitespace(line).len();
    let colon = line.find(':')?;
    let mut open_row = head;
    let mut value = line[colon + 1..].trim_start();
    loop {
        while value.starts_with(['&', '!']) {
            let end = value.find(char::is_whitespace).unwrap_or(value.len());
            value = value[end..].trim_start();
        }
        if value.starts_with(['[', '{']) {
            break;
        }
        if !value.is_empty() && !value.starts_with('#') {
            return None;
        }
        open_row += 1;
        let line = *lines.get(open_row)?;
        value = line.trim_start();
        // A following field is not this field's value; comments have no indentation.
        if !value.is_empty() && !value.starts_with('#') && leading_whitespace(line).len() <= indent
        {
            return None;
        }
    }
    let open_column = lines[open_row].len() - value.len();
    let mut delimiters = Vec::new();
    let mut quote = None;
    let mut last = None;
    for (row, line) in lines.iter().enumerate().skip(open_row) {
        let mut chars = line.char_indices().peekable();
        let mut previous: Option<char> = None;
        while let Some((column, c)) = chars.next() {
            if row == open_row && column < open_column {
                continue;
            }
            match quote {
                Some('"') => match c {
                    '\\' => {
                        chars.next();
                    }
                    '"' => quote = None,
                    _ => {}
                },
                Some(_) => {
                    if c == '\'' {
                        if chars.peek().is_some_and(|(_, next)| *next == '\'') {
                            chars.next();
                        } else {
                            quote = None;
                        }
                    }
                }
                None => match c {
                    '#' if previous.is_none_or(|c| c.is_whitespace() || ",[{".contains(c)) => break,
                    '\'' | '"'
                        if previous.is_none_or(|c| c.is_whitespace() || ":[{,".contains(c)) =>
                    {
                        quote = Some(c);
                    }
                    '[' => delimiters.push(']'),
                    '{' => delimiters.push('}'),
                    ']' | '}' => {
                        if delimiters.pop() != Some(c) {
                            return None;
                        }
                        if delimiters.is_empty() {
                            return Some(FlowSpan {
                                open: (open_row, open_column),
                                close: (row, column),
                                last,
                            });
                        }
                    }
                    _ => {}
                },
            }
            if !c.is_whitespace() && (row != open_row || column != open_column) {
                last = Some((row, column));
            }
            previous = Some(c);
        }
    }
    None
}

/// The line range `[head, end)` in `lines` of the field `name` written at indentation `at`: its
/// key line and what belongs to it (deeper lines, and sequence items or comments at its own
/// indentation), without the blank lines and comments that trail it.
fn field_span(lines: &[&str], at: &str, name: &str) -> Option<(usize, usize)> {
    let head = lines.iter().position(|line| {
        line.strip_prefix(at)
            .filter(|body| !body.starts_with(char::is_whitespace))
            .and_then(|body| body.split_once(':'))
            .and_then(|(key, _)| serde_yaml::from_str::<String>(key).ok())
            .is_some_and(|key| key == name)
    })?;
    if let Some(flow) = flow_span(lines, head) {
        return Some((head, flow.close.0 + 1));
    }
    let mut end = head + 1;
    while end < lines.len() {
        let body = lines[end].trim_start();
        let width = lines[end].len() - body.len();
        if body.is_empty()
            || width > at.len()
            || (width == at.len() && (body.starts_with("- ") || body.starts_with('#')))
        {
            end += 1;
        } else {
            break;
        }
    }
    while end > head + 1 {
        let body = lines[end - 1].trim_start();
        if body.is_empty()
            || (lines[end - 1].len() - body.len() <= at.len() && body.starts_with('#'))
        {
            end -= 1;
        } else {
            break;
        }
    }
    Some((head, end))
}

/// Byte offset of every line boundary, including the end of the document.
fn line_offsets(lines: &[&str]) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(lines.len() + 1);
    let mut offset = 0;
    offsets.push(offset);
    for line in lines {
        offset += line.len();
        offsets.push(offset);
    }
    offsets
}

/// Decode a mapping field key and retain the byte position of its separator.
/// Quoted keys can contain colons or escapes; neither changes the value slice.
fn mapping_key(line: &str) -> Option<(String, usize)> {
    let data = without_comment(line);
    let mut quote = None;
    let mut chars = data.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        match quote {
            Some('"') => match c {
                '\\' => {
                    chars.next();
                }
                '"' => quote = None,
                _ => {}
            },
            Some(_) => {
                if c == '\'' {
                    if chars.peek().is_some_and(|(_, next)| *next == '\'') {
                        chars.next();
                    } else {
                        quote = None;
                    }
                }
            }
            None => match c {
                '\'' | '"' if data[..at].trim().is_empty() => quote = Some(c),
                ':' => {
                    return serde_yaml::from_str::<String>(data[..at].trim())
                        .ok()
                        .map(|name| (name, at));
                }
                _ => {}
            },
        }
    }
    None
}

/// Text edits that remove `key` from every surviving container's dependency
/// field. Ranges use byte offsets into the original document and therefore can
/// be applied from the end without disturbing one another.
fn dependency_removals(
    text: &str,
    document: &Value,
    lines: &[&str],
    offsets: &[usize],
    containers: &Block,
    indent: &str,
    removed_entries: &[Entry],
    keys: &[&str],
) -> Result<Vec<(std::ops::Range<usize>, String)>> {
    let heads = container_boundaries(lines, containers, indent);
    let mut edits = Vec::new();

    for (position, head) in heads.iter().copied().enumerate() {
        let end = heads.get(position + 1).copied().unwrap_or(containers.end);
        if removed_entries.iter().any(|entry| entry.contains(&head)) {
            continue;
        }
        // Flow entries have an outer dependency field too, but their nested
        // application data must never enter the block-field scan.
        if let Some(span) = flow_span(&lines[..end], head) {
            edits.extend(flow_dependency_removal(
                text, document, lines, offsets, head, &span, keys,
            )?);
            continue;
        }
        if !is_container_head(lines[head], indent) {
            continue;
        }

        let Some(field_indent) = lines[head + 1..end]
            .iter()
            .find(|line| {
                let body = line.trim_start();
                !body.is_empty() && !body.starts_with('#')
            })
            .map(|line| leading_whitespace(line).to_string())
        else {
            continue;
        };

        let Some((start_after_at, colon)) = (head + 1..end).find_map(|index| {
            if leading_whitespace(lines[index]) != field_indent {
                return None;
            }
            let (name, colon) = mapping_key(lines[index])?;
            (name == "start_after").then_some((index, colon))
        }) else {
            continue;
        };

        let line = lines[start_after_at];
        let body = line
            .strip_suffix("\r\n")
            .or_else(|| line.strip_suffix('\n'))
            .unwrap_or(line);
        let value = without_comment(&body[colon + 1..]);

        if let Some(span) = flow_span(&lines[..end], start_after_at)
            .filter(|span| lines[span.open.0][span.open.1..].starts_with('['))
        {
            let (container, _) = mapping_key(lines[head])
                .ok_or_else(|| invalid(keys[0], "invalid container mapping key"))?;
            let needs = document
                .get("containers")
                .and_then(|v| v.get(&container))
                .and_then(|v| v.get("start_after"))
                .and_then(Value::as_sequence)
                .ok_or_else(|| invalid(keys[0], "expected a dependency sequence"))?;
            if !needs
                .iter()
                .any(|v| v.as_str().is_some_and(|name| keys.contains(&name)))
            {
                continue;
            }
            if needs
                .iter()
                .all(|v| v.as_str().is_some_and(|name| keys.contains(&name)))
            {
                edits.push((
                    offsets[start_after_at]..offsets[span.close.0 + 1],
                    String::new(),
                ));
            } else {
                let open = offsets[span.open.0] + span.open.1 + 1;
                let close = offsets[span.close.0] + span.close.1;
                edits.push((
                    open..close,
                    prune_flow_dependency_items(&text[open..close], needs, keys)?,
                ));
            }
            continue;
        }

        if !value.trim().is_empty() {
            continue;
        }

        let mut matching_items = Vec::new();
        let mut surviving_items = 0;
        for (index, candidate) in lines.iter().enumerate().take(end).skip(start_after_at + 1) {
            let candidate = *candidate;
            let trimmed = candidate.trim();
            if !trimmed.is_empty()
                && !trimmed.starts_with('#')
                && (leading_whitespace(candidate).len() < field_indent.len()
                    || (leading_whitespace(candidate).len() == field_indent.len()
                        && !trimmed.starts_with("- ")))
            {
                break;
            }
            let Some(item) = trimmed.strip_prefix("- ") else {
                continue;
            };
            if keys.contains(&dependency_name(item)) {
                matching_items.push(index);
            } else {
                surviving_items += 1;
            }
        }
        if matching_items.is_empty() {
            continue;
        }
        if surviving_items == 0 {
            edits.push((
                offsets[start_after_at]..offsets[start_after_at + 1],
                String::new(),
            ));
        }
        edits.extend(
            matching_items
                .into_iter()
                .map(|index| (offsets[index]..offsets[index + 1], String::new())),
        );
    }
    Ok(edits)
}

/// Edit only the outer flow dependency pair. Resolve values using the complete
/// document, then retain the raw spelling of surviving list items and fields.
fn flow_dependency_removal(
    text: &str,
    document: &Value,
    lines: &[&str],
    offsets: &[usize],
    head: usize,
    span: &FlowSpan,
    removed_keys: &[&str],
) -> Result<Vec<(std::ops::Range<usize>, String)>> {
    let removed = removed_keys[0];
    let name = lines[head]
        .trim_start()
        .split_once(':')
        .and_then(|(name, _)| serde_yaml::from_str::<String>(name).ok())
        .ok_or_else(|| invalid(removed, "invalid flow container key"))?;
    let Some(needs) = document
        .get("containers")
        .and_then(|v| v.get(&name))
        .and_then(|v| v.get("start_after"))
        .and_then(Value::as_sequence)
    else {
        return Ok(vec![]);
    };
    if !needs
        .iter()
        .any(|v| v.as_str().is_some_and(|name| removed_keys.contains(&name)))
    {
        return Ok(vec![]);
    }
    let start = offsets[span.open.0] + span.open.1 + 1;
    let end = offsets[span.close.0] + span.close.1;
    let body = &text[start..end];
    let parts = flow_mapping_parts(body)
        .ok_or_else(|| invalid(removed, "invalid flow dependency mapping"))?;
    let mut at = start;
    for (index, part) in parts.iter().enumerate() {
        let pair_lines: Vec<&str> = part.split_inclusive('\n').collect();
        let first = pair_lines.iter().position(|line| {
            let value = line.trim_start();
            !value.is_empty() && !value.starts_with('#')
        });
        let field = first
            .and_then(|row| pair_lines[row].split_once(':'))
            .and_then(|(field, _)| serde_yaml::from_str::<String>(field.trim()).ok());
        if field.as_deref() == Some("start_after") {
            if needs
                .iter()
                .all(|v| v.as_str().is_some_and(|name| removed_keys.contains(&name)))
            {
                // Delete this pair plus one separator; all other raw pairs stay.
                let range = if index + 1 < parts.len() {
                    at..at + part.len() + 1
                } else if index > 0 {
                    at - 1..at + part.len()
                } else {
                    at..at + part.len()
                };
                return Ok(vec![(range, String::new())]);
            }
            let list = flow_span(&pair_lines, first.unwrap())
                .filter(|s| pair_lines[s.open.0][s.open.1..].starts_with('['))
                .ok_or_else(|| {
                    invalid(removed, "cannot safely edit aliased flow dependency list")
                })?;
            let pair_offsets = line_offsets(&pair_lines);
            let list_start = pair_offsets[list.open.0] + list.open.1 + 1;
            let list_end = pair_offsets[list.close.0] + list.close.1;
            let kept =
                prune_flow_dependency_items(&part[list_start..list_end], needs, removed_keys)?;
            return Ok(vec![(at + list_start..at + list_end, kept)]);
        }
        at += part.len() + 1;
    }
    Err(invalid(
        removed,
        "cannot locate the outer flow dependency field",
    ))
}

/// Prune using values resolved in full-document context, never comma-splitting
/// quoted scalars or reparsing detached aliases. Preserve raw surviving items.
fn prune_flow_dependency_items(
    body: &str,
    needs: &[Value],
    removed_keys: &[&str],
) -> Result<String> {
    let key = removed_keys[0];
    let items =
        flow_mapping_parts(body).ok_or_else(|| invalid(key, "invalid flow dependency list"))?;
    let mut item_index = 0;
    let mut kept = Vec::new();
    for item in items {
        let has_value = item.lines().any(|line| {
            // A full comment can start at column zero inside a flow collection.
            // Quoted '#...' scalars still begin with a quote and remain values.
            !line.trim_start().starts_with('#') && !without_comment(line).trim().is_empty()
        });
        if !has_value {
            kept.push(item);
            continue;
        }
        let value = needs
            .get(item_index)
            .ok_or_else(|| invalid(key, "dependency span/value mismatch"))?;
        item_index += 1;
        if !value
            .as_str()
            .is_some_and(|name| removed_keys.contains(&name))
        {
            kept.push(item);
        }
    }
    if item_index != needs.len() {
        return Err(invalid(key, "dependency span/value mismatch"));
    }
    Ok(kept.join(","))
}

/// Locate container boundaries independently of field-edit representation.
/// A complete flow span is skipped so its nested keys cannot become boundaries.
fn container_boundaries(lines: &[&str], containers: &Block, indent: &str) -> Vec<usize> {
    let mut heads = Vec::new();
    let mut index = containers.start;
    while index < containers.end {
        let is_head = lines[index]
            .strip_prefix(indent)
            .filter(|body| !body.starts_with(char::is_whitespace) && !body.starts_with('#'))
            .and_then(|body| body.split_once(':'))
            .and_then(|(name, _)| serde_yaml::from_str::<String>(name).ok())
            .is_some();
        if is_head {
            heads.push(index);
            if let Some(span) = flow_span(&lines[..containers.end], index) {
                index = span.close.0 + 1;
                continue;
            }
        }
        index += 1;
    }
    heads
}

/// Removal and generated-marker lookup need the complete entry, not a block
/// eligible for field rewriting. Keep find_entry's normalization gate intact.
fn find_complete_entry(
    lines: &[&str],
    containers: &Block,
    indent: &str,
    key: &str,
) -> Option<(usize, Entry)> {
    for head in container_boundaries(lines, containers, indent) {
        let name = lines[head].strip_prefix(indent)?.split_once(':')?.0;
        if serde_yaml::from_str::<String>(name).ok().as_deref() != Some(key) {
            continue;
        }
        if let Some(span) = flow_span(&lines[..containers.end], head) {
            if !lines[span.open.0][span.open.1..].starts_with('{') {
                return None;
            }
            let mut start = head;
            while start > containers.start && lines[start - 1].trim_start().starts_with('#') {
                start -= 1;
            }
            return Some((head, start..span.close.0 + 1));
        }
        return find_entry(lines, containers, indent, key).map(|entry| (head, entry));
    }
    None
}

fn is_container_head(line: &str, indent: &str) -> bool {
    let Some(rest) = line.strip_prefix(indent) else {
        return false;
    };
    !rest.starts_with([' ', '\t', '#'])
        && rest.split_once(':').is_some_and(|(_, value)| {
            // An anchor/tag is a property of this mapping, not its contents.
            // Keep dependency discovery and generated-marker lookup consistent
            // with the block headers accepted by find_entry.
            opens_block(without_comment(value).trim())
        })
}

/// `line` without its trailing YAML comment, whose `#` must follow whitespace:
/// `key:#text` is a plain scalar.
/// A `#` inside a quoted scalar is text, not a comment. A quote opens one only where a scalar
/// starts (at the start of the line, or after whitespace, `:`, `,`, `[` or `{`): the `'` of
/// `echo it's ok` is text. `''` is a quote inside single quotes, and `\` escapes inside double ones.
fn without_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut previous: Option<char> = None;
    let mut chars = line.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        match quote {
            Some('"') => match c {
                '\\' => {
                    chars.next();
                }
                '"' => quote = None,
                _ => {}
            },
            Some(_) => {
                if c == '\'' {
                    if chars.peek().is_some_and(|(_, next)| *next == '\'') {
                        chars.next();
                    } else {
                        quote = None;
                    }
                }
            }
            None => match c {
                '\'' | '"' if previous.is_none_or(|previous| " \t:,[{".contains(previous)) => {
                    quote = Some(c);
                }
                '#' if previous.is_some_and(|previous| previous == ' ' || previous == '\t') => {
                    return &line[..at];
                }
                _ => {}
            },
        }
        previous = Some(c);
    }
    line
}

fn leading_whitespace(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

fn dependency_name(value: &str) -> &str {
    without_comment(value).trim().trim_matches(['"', '\''])
}

/// Preserve uniform CRLF/LF convention and whether a final newline was present.
/// Callers supply logical lines; generated lines must not rewrite a CRLF file.
fn join(lines: &[String], original: &str) -> String {
    let bytes = original.as_bytes();
    let crlf = bytes.contains(&b'\n')
        && bytes
            .iter()
            .enumerate()
            .all(|(at, byte)| *byte != b'\n' || (at > 0 && bytes[at - 1] == b'\r'));
    let newline = if crlf { "\r\n" } else { "\n" };
    let mut text = lines.join(newline);
    if original.ends_with('\n') {
        text.push_str(newline);
    }
    text
}

fn normalize_inline_empty_containers(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let (line_index, comment) = lines.iter().enumerate().find_map(|(index, line)| {
        let rest = line.strip_prefix("containers:")?;
        let (value, comment) = rest
            .split_once('#')
            .map_or((rest, None), |(value, comment)| (value, Some(comment)));
        let inner = value.trim().strip_prefix('{')?.strip_suffix('}')?;
        inner.trim().is_empty().then_some((index, comment))
    })?;

    let mut out: Vec<String> = lines.iter().map(|line| (*line).to_string()).collect();
    out[line_index] = "containers:".to_string();
    if let Some(comment) = comment {
        out.insert(line_index + 1, format!("  #{comment}"));
    }
    Some(join(&out, text))
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
        .position(|line| without_comment(line).trim_end() == "containers:")
        .ok_or_else(|| ComposeError::InvalidWorkerSpec {
            spec: "containers".to_string(),
            reason: "the compose file has no top-level `containers:` mapping to add to".to_string(),
        })?;

    let start = key + 1;
    let mut end = start;
    for (offset, line) in lines[start..].iter().enumerate() {
        // YAML comments have no structural indentation. Only a non-comment
        // line at column zero ends the mapping, not a note between entries.
        if line.trim().is_empty()
            || line.trim_start().starts_with('#')
            || line.starts_with([' ', '\t'])
        {
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
        .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(|line| leading_whitespace(line).to_string())
        .unwrap_or_else(|| "  ".to_string())
}

/// The line range of one container entry: its key line and everything indented
/// under it, plus any comment lines directly above that describe it.
fn find_entry(lines: &[&str], containers: &Block, indent: &str, key: &str) -> Option<Entry> {
    let at = (containers.start..containers.end).find(|i| {
        lines[*i]
            .strip_prefix(indent)
            .filter(|body| !body.starts_with(char::is_whitespace))
            .and_then(|body| body.split_once(':'))
            .filter(|(_, value)| {
                opens_block(without_comment(value).trim())
                    && flow_span(&lines[..containers.end], *i).is_none()
            })
            .and_then(|(name, _)| serde_yaml::from_str::<String>(name).ok())
            .is_some_and(|name| name == key)
    })?;

    let mut end = at + 1;
    for line in &lines[at + 1..containers.end] {
        // Comment indentation has no structural meaning in YAML. A field
        // following a column-zero comment can still belong to this container.
        if line.trim().is_empty()
            || line.trim_start().starts_with('#')
            || line.starts_with(&format!("{indent} "))
        {
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
    // Trailing blank lines and comments at/before container indentation belong
    // to what follows. Only interior comments need the expanded range above;
    // retain the previous ownership of deeper entry comments.
    while end > at + 1
        && (lines[end - 1].trim().is_empty()
            || (lines[end - 1].trim_start().starts_with('#')
                && leading_whitespace(lines[end - 1]).len() <= indent.len()))
    {
        end -= 1;
    }
    Some(Entry { start, end })
}

type Entry = std::ops::Range<usize>;

/// Read values for comparison only. Unchanged text is never serialized.
fn entry_fields(text: &str, key: &str) -> Result<Mapping> {
    // Aliases can refer to anchors in other containers.
    let value: Value = serde_yaml::from_str(text).map_err(|err| invalid(key, &err.to_string()))?;
    value
        .get("containers")
        .and_then(|containers| containers.get(key))
        .and_then(Value::as_mapping)
        .cloned()
        .ok_or_else(|| invalid(key, "expected a container mapping"))
}

fn wanted_version(new: &NewContainer) -> Option<String> {
    match &new.source {
        Source::Package { version, .. } => version.clone(),
        Source::Path { .. } => None,
    }
}

/// Build only the fields owned by this request, including resolved dependencies.
pub(crate) fn declaration_fields(
    new: &NewContainer,
    existing: Option<&Mapping>,
) -> Result<Mapping> {
    let mut fields = Mapping::new();
    let source = match &new.source {
        Source::Package { reference, .. } => format!("package://{reference}"),
        Source::Path { path } => format!("path://{path}"),
    };
    fields.insert(Value::from("worker"), Value::from(source));
    if let Some(version) = wanted_version(new) {
        fields.insert(Value::from("version"), Value::from(version));
    }
    let mut needs = new.start_after.clone();
    if !new.fields.contains_key("start_after")
        && let Some(Value::Sequence(dependencies)) =
            existing.and_then(|fields| fields.get("start_after"))
    {
        needs.extend(
            dependencies
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string),
        );
    }
    needs.sort();
    needs.dedup();
    if !needs.is_empty() || new.fields.contains_key("start_after") {
        let value =
            serde_yaml::to_value(needs).map_err(|err| invalid(&new.key, &err.to_string()))?;
        // Preserve order and comments when the dependency set is unchanged.
        let previous = existing.and_then(|fields| fields.get("start_after"));
        let same = previous.and_then(Value::as_sequence).is_some_and(|old| {
            let mut old = old.clone();
            old.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
            old.dedup();
            value.as_sequence() == Some(&old)
        });
        fields.insert(
            Value::from("start_after"),
            if same {
                previous.cloned().unwrap_or(value)
            } else {
                value
            },
        );
    }
    for (key, value) in &new.fields {
        if key.as_str() != Some("start_after") {
            fields.insert(key.clone(), value.clone());
        }
    }
    Ok(fields)
}

/// Replace complete fields at the container's indentation. Nested keys with the
/// same names (for example config_override.worker) belong to their parent field.
fn rewrite(entry: &[&str], changes: &Mapping, indent: &str) -> Result<String> {
    let inner = entry
        .iter()
        .find_map(|line| {
            let trimmed = line.trim_start();
            let width = line.len() - trimmed.len();
            (width > indent.len() && !trimmed.is_empty() && !trimmed.starts_with('#'))
                .then(|| line[..width].to_string())
        })
        .unwrap_or_else(|| format!("{indent}{indent}"));
    let mut remaining = changes.clone();
    let mut out = String::new();
    let mut index = 0;
    while index < entry.len() {
        let line = entry[index];
        let key = line
            .strip_prefix(&inner)
            .filter(|body| !body.starts_with(char::is_whitespace))
            .and_then(|body| body.split_once(':'))
            .and_then(|(key, _)| serde_yaml::from_str::<String>(key).ok());
        if let Some(key) = key
            && let Some(value) = remaining.remove(Value::from(key.clone()))
        {
            out.push_str(&render_field(&key, &value, &inner)?);
            index += 1;
            let start = index;
            while index < entry.len() {
                let next = entry[index];
                let body = next.trim_start();
                let width = next.len() - body.len();
                // YAML also permits sequence items at the field's indentation.
                if body.is_empty()
                    || width > inner.len()
                    || (width == inner.len() && (body.starts_with("- ") || body.starts_with('#')))
                {
                    index += 1;
                } else {
                    break;
                }
            }
            // Keep trailing blank lines and comments for the next field.
            while index > start {
                let previous = entry[index - 1];
                let body = previous.trim_start();
                if body.is_empty()
                    || (previous.len() - body.len() == inner.len() && body.starts_with('#'))
                {
                    index -= 1;
                } else {
                    break;
                }
            }
        } else {
            out.push_str(line);
            out.push('\n');
            index += 1;
        }
    }
    for (key, value) in remaining {
        if let Some(key) = key.as_str() {
            out.push_str(&render_field(key, &value, &inner)?);
        }
    }
    Ok(out)
}

/// Serialize one field as indented YAML, keeping version strings quoted.
fn render_field(key: &str, value: &Value, indent: &str) -> Result<String> {
    let yaml = if key == "version" && value.is_string() {
        format!(
            "version: {}\n",
            serde_json::to_string(value.as_str().unwrap_or_default())
                .map_err(|err| invalid(key, &err.to_string()))?
        )
    } else {
        let mut field = Mapping::new();
        field.insert(Value::from(key), value.clone());
        serde_yaml::to_string(&field).map_err(|err| invalid(key, &err.to_string()))?
    };
    Ok(yaml
        .lines()
        .map(|line| format!("{indent}{line}\n"))
        .collect())
}

/// The YAML for one entry, indented to match the file.
fn render(new: &NewContainer, indent: &str) -> Result<String> {
    let inner = format!("{indent}{indent}");
    let key = serde_yaml::to_string(&new.key).map_err(|err| invalid(&new.key, &err.to_string()))?;
    let mut out = format!("{indent}{MARKER}\n{indent}{}:\n", key.trim_end());
    for (key, value) in declaration_fields(new, None)? {
        if let Some(key) = key.as_str() {
            out.push_str(&render_field(key, &value, &inner)?);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured(value: serde_json::Value) -> NewContainer {
        serde_json::from_value::<WorkerInput>(value)
            .unwrap()
            .parse()
            .unwrap()
    }

    fn changed(text: &str, new: &NewContainer) -> String {
        match upsert_container(text, new).unwrap() {
            Outcome::Added(text) | Outcome::Replaced { text, .. } => text,
            Outcome::Unchanged => panic!("expected an edit"),
        }
    }

    #[test]
    fn upsert_updates_an_existing_flow_mapping_without_duplicating_the_key() {
        let text = "# keep\ncontainers:\n  tool: {worker: 'package://registry.example/team/tool', version: next, config_name: mine} # tool note\n  local: {worker: path://.}\n";
        let request = parse_worker("package://registry.example/team/tool@1.2.3").unwrap();
        let Outcome::Replaced { text: edited, .. } = upsert_container(text, &request).unwrap()
        else {
            panic!("existing flow entry must be replaced, never appended");
        };
        let parsed = crate::ComposeFile::parse(&edited, "/tmp/worker-compose.yaml").unwrap();
        assert_eq!(parsed.containers.len(), 2);
        assert_eq!(parsed.containers["tool"].version.as_deref(), Some("1.2.3"));
        assert_eq!(
            parsed.containers["tool"].config_name.as_deref(),
            Some("mine")
        );
        assert!(edited.contains("# tool note"));
        assert!(edited.contains("  local: {worker: path://.}\n"));
        assert_eq!(
            upsert_container(&edited, &request).unwrap(),
            Outcome::Unchanged
        );
        let same = parse_worker("package://registry.example/team/tool@next").unwrap();
        assert_eq!(upsert_container(text, &same).unwrap(), Outcome::Unchanged);
    }

    #[test]
    fn flow_mapping_upsert_keeps_source_kind_protection() {
        let text = "containers:\n  tool: {worker: path://.}\n";
        let error = upsert_container(text, &parse_worker("tool@1.2.3").unwrap()).unwrap_err();
        assert_eq!(error.code(), "WORKER_SOURCE_CHANGED");
    }

    #[test]
    fn existing_compose_paths_use_the_declared_key_in_both_flow_and_block_forms() {
        for flow in [false, true] {
            for path in [".", "..", "./workers/tool", "../workers/tool"] {
                let text = if flow {
                    format!("containers:\n  tool: {{worker: path://{path}}}\n")
                } else {
                    format!("containers:\n  tool:\n    worker: path://{path}\n")
                };
                let original = text.clone();
                let compose = crate::ComposeFile::parse(&text, "/tmp/worker-compose.yaml").unwrap();
                assert!(matches!(
                    compose.containers["tool"].worker,
                    crate::WorkerSource::Path { .. }
                ));
                let package = parse_worker("tool@1.2.3").unwrap();
                let error = upsert_container(&text, &package).unwrap_err();
                let ComposeError::WorkerSourceChanged {
                    container,
                    from,
                    to,
                    ..
                } = error
                else {
                    panic!("{flow}/{path}: {error}")
                };
                assert_eq!(
                    (container.as_str(), from.as_str(), to.as_str()),
                    ("tool", "path", "package")
                );
                assert_eq!(text, original);
                let mut same = NewContainer {
                    key: "tool".into(),
                    source: Source::Path { path: path.into() },
                    start_after: vec![],
                    fields: Mapping::new(),
                };
                assert_eq!(upsert_container(&text, &same).unwrap(), Outcome::Unchanged);
                same.fields.insert("config_name".into(), "kept-path".into());
                let Outcome::Replaced { text: edited, .. } =
                    upsert_container(&text, &same).unwrap()
                else {
                    panic!("field edit must replace existing container")
                };
                let parsed =
                    crate::ComposeFile::parse(&edited, "/tmp/worker-compose.yaml").unwrap();
                let crate::WorkerSource::Path { declared, .. } = &parsed.containers["tool"].worker
                else {
                    panic!("path kind changed")
                };
                assert_eq!(declared, &format!("path://{path}"));
                assert_eq!(
                    parsed.containers["tool"].config_name.as_deref(),
                    Some("kept-path")
                );

                let package_text = if flow {
                    "containers:\n  tool: {worker: package://tool, version: 1.2.3}\n"
                } else {
                    "containers:\n  tool:\n    worker: package://tool\n    version: 1.2.3\n"
                };
                let error = upsert_container(package_text, &same).unwrap_err();
                let ComposeError::WorkerSourceChanged { from, to, .. } = error else {
                    panic!("{error}")
                };
                assert_eq!((from.as_str(), to.as_str()), ("package", "path"));
            }
        }
    }

    #[test]
    fn declared_path_handling_does_not_relax_empty_uris_or_cli_key_inference() {
        for spec in ["path://.", "path://..", "path://"] {
            assert!(
                parse_worker(spec).is_err(),
                "a CLI request still needs an inferred key: {spec}"
            );
        }
        let package = parse_worker("tool@1.2.3").unwrap();
        for text in [
            "containers:\n  tool: {worker: 'path://'}\n",
            "containers:\n  tool:\n    worker: 'path://'\n",
        ] {
            assert!(crate::ComposeFile::parse(text, "/tmp/worker-compose.yaml").is_err());
            assert_eq!(
                upsert_container(text, &package).unwrap_err().code(),
                "INVALID_WORKER_SPEC"
            );
        }
    }

    /// `compose::add` writes `runtime.base_image` as given, and the file reads it back.
    #[test]
    fn configured_worker_round_trips_the_runtime_base_image() {
        let worker = configured(serde_json::json!({
            "worker": "package://tool@1.0.0",
            "runtime": {"base_image": "docker.io/library/alpine:3.20"},
            "scripts": {"build": "iii sandbox build", "run": "iii sandbox run"}
        }));
        let out = changed("containers:\n  api:\n    worker: path://./api\n", &worker);
        let file = crate::config::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").unwrap();
        assert_eq!(
            file.containers["tool"].base_image.as_deref(),
            Some("docker.io/library/alpine:3.20")
        );
        // A second add of the same declaration changes nothing.
        assert_eq!(upsert_container(&out, &worker).unwrap(), Outcome::Unchanged);

        let invalid = serde_json::from_value::<WorkerInput>(serde_json::json!({
            "worker": "package://tool@1.0.0",
            "runtime": {"base_image": "docker.io/library/alpine:3.20", "kind": "node"}
        }));
        assert!(invalid.is_err());
    }

    #[test]
    fn configured_worker_round_trips_every_container_field() {
        let request = serde_json::json!({
            "worker": "path://workers/api",
            "version": "local",
            "start_after": ["database"],
            "config_name": "api-config",
            "config_override": {"url": "${API_URL}", "port": 3000, "enabled": true},
            "scripts": {
                "pre_run": "echo 'prepare: #1'\necho ready",
                "pre_run_timeout": "10s",
                "run": "./api --name 'a: b'",
                "post_run": "echo stopped"
            },
            "working_dir": ".",
            "environment": {"MODE": "on", "COUNT": "123"},
            "env_file": ["./api.env"],
            "startup_timeout": "5s"
        });
        let worker = configured(request.clone());
        let out = changed(
            "containers:\n  database:\n    worker: path://./database\n",
            &worker,
        );
        let document: serde_json::Value = serde_yaml::from_str(&out).unwrap();
        assert_eq!(document["containers"]["api"], request);
        crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").unwrap();
        assert_eq!(upsert_container(&out, &worker).unwrap(), Outcome::Unchanged);
    }

    #[test]
    fn null_valued_flow_entries_preserve_null_and_trailing_comments() {
        for field in [
            "config_override",
            "'config_override'",
            "\"config_override\"",
            "config_override # note: null, not a separator\n  ",
            "\"config_override\" # quoted: null\n  ",
        ] {
            let text = format!(
                "containers:\n  tool: {{worker: package://api.workers.iii.dev/tool, version: '1.0.0', {field}}}\n  local: {{worker: path://.}}\n"
            );
            crate::ComposeFile::parse(&text, "/tmp/worker-compose.yaml").unwrap();
            let before: Value = serde_yaml::from_str(&text).unwrap();
            assert_eq!(
                before["containers"]["tool"].get("config_override"),
                Some(&Value::Null)
            );
            assert_eq!(
                upsert_container(&text, &parse_worker("tool@1.0.0").unwrap()).unwrap(),
                Outcome::Unchanged
            );
            let out = changed(&text, &parse_worker("tool@1.1.0").unwrap());
            crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").unwrap();
            let after: Value = serde_yaml::from_str(&out).unwrap();
            assert_eq!(
                after["containers"]["tool"].get("config_override"),
                Some(&Value::Null)
            );
            assert_eq!(after["containers"]["tool"]["version"], "1.1.0");
            assert_eq!(after["containers"].as_mapping().unwrap().len(), 2);
            if let Some((_, comment)) = field.split_once('#') {
                assert!(out.contains(&format!("#{}", comment.trim_end())), "{out}");
            }
            assert!(out.ends_with("  local: {worker: path://.}\n"));
            assert_eq!(
                upsert_container(&out, &parse_worker("tool@1.1.0").unwrap()).unwrap(),
                Outcome::Unchanged
            );
        }
        assert_eq!(
            block_flow_pair("config_override  # note: null"),
            "config_override:  # note: null"
        );
        assert_eq!(
            block_flow_pair("'config:override' # note"),
            "'config:override': # note"
        );
        assert_eq!(
            block_flow_pair("config_override: # already null"),
            "config_override: # already null"
        );
        assert_eq!(
            block_flow_pair("worker: package://host/name"),
            "worker: package://host/name"
        );
    }

    #[test]
    fn json_style_flow_pairs_remain_valid_for_upsert_and_vm_rewrite() {
        for fields in [
            r#"{"worker":"package://api.workers.iii.dev/tool","version":"1.0.0","config_override":{"message":"á: # , }","nested:key":{"quoted":"a\\\"b"}}}"#,
            r#"{"worker": "package://api.workers.iii.dev/tool", "version": "1.0.0", "config_override": {"message": "á: # , }", "nested:key": {"quoted": "a\\\"b"}}}"#,
        ] {
            let text =
                format!("containers:\n  tool: {fields} # keep\n  other: {{worker: path://.}}\n");
            let before: Value = serde_yaml::from_str(&text).unwrap();
            assert_eq!(
                upsert_container(&text, &parse_worker("tool@1.0.0").unwrap()).unwrap(),
                Outcome::Unchanged
            );
            let out = changed(&text, &parse_worker("tool@1.1.0").unwrap());
            crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").unwrap();
            let after: Value = serde_yaml::from_str(&out).unwrap();
            assert_eq!(after["containers"]["tool"]["version"], "1.1.0");
            assert_eq!(
                after["containers"]["tool"]["config_override"],
                before["containers"]["tool"]["config_override"]
            );
            assert!(out.ends_with("  other: {worker: path://.}\n"));
            assert!(out.contains("# keep"));
            assert_eq!(
                upsert_container(&out, &parse_worker("tool@1.1.0").unwrap()).unwrap(),
                Outcome::Unchanged
            );
        }
        let text = "containers:\n  vm: {\"worker\":\"path://./vm\",\"config_override\":{\"message\":\"a:b\"}}\n  sb: {worker: path://./sb}\n";
        let out = explicit_vm_mode(text, "vm", "sb").unwrap().unwrap().text;
        crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").unwrap();
        let parsed: Value = serde_yaml::from_str(&out).unwrap();
        assert_eq!(
            parsed["containers"]["vm"]["config_override"]["message"],
            "a:b"
        );
        assert_eq!(
            parsed["containers"]["vm"]["scripts"]["run"],
            "iii call sandbox::start worker=vm"
        );
        assert!(explicit_vm_mode(&out, "vm", "sb").unwrap().is_none());
    }

    #[test]
    fn flow_mapping_after_properties_header_is_normalized_without_breaking_aliases() {
        for header in ["", "&api", "!!map &api"] {
            let alias = if header.is_empty() {
                ""
            } else {
                "    config_override: *api\n"
            };
            let text = format!(
                "containers:\n  api: {header} # header\n    # before flow\n    {{worker: package://api.workers.iii.dev/api,\n     version: '1.0.0', config_override: {{message: unchanged}}}} # tail\n  consumer:\n    worker: path://./consumer\n{alias}"
            );
            assert_eq!(
                upsert_container(&text, &parse_worker("api@1.0.0").unwrap()).unwrap(),
                Outcome::Unchanged
            );
            let out = changed(&text, &parse_worker("api@1.1.0").unwrap());
            crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").unwrap();
            let parsed: Value = serde_yaml::from_str(&out).unwrap();
            assert_eq!(parsed["containers"].as_mapping().unwrap().len(), 2);
            assert_eq!(parsed["containers"]["api"]["version"], "1.1.0");
            assert_eq!(
                parsed["containers"]["api"]["config_override"]["message"],
                "unchanged"
            );
            if !header.is_empty() {
                assert!(out.contains(&format!("api: {header}")));
                assert!(out.contains("config_override: *api"));
                assert_eq!(
                    parsed["containers"]["consumer"]["config_override"]["version"],
                    "1.1.0"
                );
            }
            assert!(
                out.contains("# header") && out.contains("# before flow") && out.contains("# tail")
            );
            assert_eq!(
                upsert_container(&out, &parse_worker("api@1.1.0").unwrap()).unwrap(),
                Outcome::Unchanged
            );
        }
        // An actual anchored block must still take the direct rewrite path.
        let block = "containers:\n  api: &api # keep\n    worker: package://api.workers.iii.dev/api\n    version: '1.0.0'\n  consumer:\n    worker: path://./consumer\n    config_override: *api\n";
        let out = changed(block, &parse_worker("api@1.1.0").unwrap());
        assert_eq!(out, block.replace("version: '1.0.0'", "version: \"1.1.0\""));
        let parsed: Value = serde_yaml::from_str(&out).unwrap();
        assert_eq!(
            parsed["containers"]["consumer"]["config_override"]["version"],
            "1.1.0"
        );
    }

    #[test]
    fn flow_mapping_update_preserves_external_and_exported_anchors() {
        for fields in [
            "{worker: package://api.workers.iii.dev/api, version: '1.0.0', config_override: *defaults, environment: &env {MODE: 'on'}}",
            "{\n    worker: package://api.workers.iii.dev/api,\n    version: '1.0.0', # old release\n    config_override: *defaults,\n    environment: &env {MODE: 'on'},\n  }",
        ] {
            let text = format!(
                "containers:\n  defaults:\n    worker: path://./defaults\n    config_override: &defaults {{version: nested}}\n  api: &api {fields} # api note\n  consumer:\n    worker: path://./consumer\n    environment: *env\n    config_override: *api\n"
            );
            assert_eq!(
                upsert_container(&text, &parse_worker("api@1.0.0").unwrap()).unwrap(),
                Outcome::Unchanged
            );
            let out = changed(&text, &parse_worker("api@1.1.0").unwrap());
            crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").unwrap();
            assert!(out.contains("  api: &api # api note\n"), "{out}");
            assert!(out.contains("config_override: *defaults"), "{out}");
            assert!(out.contains("environment: &env {MODE: 'on'}"), "{out}");
            assert!(out.ends_with("  consumer:\n    worker: path://./consumer\n    environment: *env\n    config_override: *api\n"));
            let parsed: Value = serde_yaml::from_str(&out).unwrap();
            assert_eq!(parsed["containers"]["api"]["version"], "1.1.0");
            assert_eq!(
                parsed["containers"]["consumer"]["config_override"]["version"],
                "1.1.0"
            );
            let modified: Value =
                serde_yaml::from_str(&out.replace("version: nested", "version: changed")).unwrap();
            assert_eq!(
                modified["containers"]["api"]["config_override"]["version"],
                "changed"
            );
            assert_eq!(
                upsert_container(&out, &parse_worker("api@1.1.0").unwrap()).unwrap(),
                Outcome::Unchanged
            );
        }
    }

    #[test]
    fn multiline_flow_mapping_update_preserves_nested_values_comments_and_neighbors() {
        let text = "# keep\ncontainers:\n  tool: {\n    worker: package://api.workers.iii.dev/tool, # source\n    version: '1.0.0',\n    config_override: {message: 'á, # } [ it''s', nested: [1, {key: value}], url: https://example.test/#fragment},\n  } # tool note\n  local: {worker: path://.}\n";
        let before: Value = serde_yaml::from_str(text).unwrap();
        assert_eq!(
            upsert_container(text, &parse_worker("tool@1.0.0").unwrap()).unwrap(),
            Outcome::Unchanged
        );
        let out = changed(text, &parse_worker("tool@1.1.0").unwrap());
        let after: Value = serde_yaml::from_str(&out).unwrap();
        assert_eq!(after["containers"].as_mapping().unwrap().len(), 2);
        assert_eq!(
            after["containers"]["tool"]["config_override"],
            before["containers"]["tool"]["config_override"]
        );
        assert_eq!(after["containers"]["tool"]["version"], "1.1.0");
        assert!(out.starts_with("# keep\ncontainers:\n"));
        assert!(out.ends_with("  local: {worker: path://.}\n"));
        assert!(out.contains("# source") && out.contains("# tool note"));
        crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").unwrap();
        assert_eq!(
            upsert_container(&out, &parse_worker("tool@1.1.0").unwrap()).unwrap(),
            Outcome::Unchanged
        );
    }

    #[test]
    fn a_version_change_preserves_settings_that_reference_an_external_anchor() {
        let text = "containers:\n  defaults:\n    worker: path://./defaults\n    config_override: &defaults { version: nested }\n  api:\n    worker: package://api.workers.iii.dev/api\n    version: '1.0.0'\n    config_override: *defaults\n";
        let out = changed(text, &parse_worker("api@1.1.0").unwrap());
        assert_eq!(out, text.replace("version: '1.0.0'", "version: \"1.1.0\""));
        crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").unwrap();
    }

    #[test]
    fn configuration_changes_replace_only_explicit_fields() {
        let text = "containers:\n  api:\n    worker: path://./api\n    scripts:\n      run: |\n        echo old\n\n        echo obsolete\n    # keep the environment\n    environment: { MODE: 'on' }\n    config_override:\n      worker: custom\n      version: 'nested'\n      start_after: [nested]\n    env_file: [./api.env]\n";
        let worker = configured(serde_json::json!({
            "worker": "./api", "scripts": {"run": "echo 'new: #value'"}
        }));
        let out = changed(text, &worker);
        assert!(out.ends_with("    # keep the environment\n    environment: { MODE: 'on' }\n    config_override:\n      worker: custom\n      version: 'nested'\n      start_after: [nested]\n    env_file: [./api.env]\n"), "{out}");
        let parsed = crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").unwrap();
        assert_eq!(
            parsed.containers["api"].scripts.run.as_deref(),
            Some("echo 'new: #value'")
        );
        assert_eq!(upsert_container(&out, &worker).unwrap(), Outcome::Unchanged);
    }

    #[test]
    fn explicit_empty_fields_clear_values_and_omitted_fields_preserve_them() {
        let text = "containers:\n  api:\n    worker: path://./api\n    start_after: [database] # keep order\n    environment: { MODE: dev }\n    env_file: [./api.env]\n    config_override: {port: 3000}\n  database:\n    worker: path://./database\n";
        assert_eq!(
            upsert_container(text, &parse_worker("./api").unwrap()).unwrap(),
            Outcome::Unchanged
        );
        let worker = configured(serde_json::json!({
            "worker": "./api", "start_after": [], "environment": {}, "env_file": [], "config_override": null
        }));
        let out = changed(text, &worker);
        let parsed = crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").unwrap();
        let api = &parsed.containers["api"];
        assert!(api.start_after.is_empty());
        assert!(api.environment.is_empty());
        assert!(api.env_file.is_empty());
        assert!(api.config_override.is_none());
        assert_eq!(upsert_container(&out, &worker).unwrap(), Outcome::Unchanged);
    }

    #[test]
    fn configured_package_keeps_resolved_and_explicit_dependencies() {
        let mut worker = configured(serde_json::json!({
            "worker": "api", "version": "1.2.3", "start_after": ["custom"],
            "config_override": {"mode": "production"}
        }));
        worker.start_after.push("database".to_string());
        let out = changed("containers: {}\n", &worker);
        let document: Value = serde_yaml::from_str(&out).unwrap();
        assert_eq!(
            document["containers"]["api"]["start_after"],
            serde_yaml::to_value(["custom", "database"]).unwrap()
        );
        assert_eq!(
            document["containers"]["api"]["version"].as_str(),
            Some("1.2.3")
        );
    }

    #[test]
    fn changing_the_source_or_nested_config_at_the_same_version_is_an_edit() {
        let text = "containers:\n  api:\n    worker: package://old.example/api\n    version: '1.2.3'\n    config_override:\n      worker: nested\n      version: nested\n";
        let worker = configured(serde_json::json!({
            "worker": "new.example/api", "version": "1.2.3", "config_override": {"port": 3001}
        }));
        let out = changed(text, &worker);
        let parsed = crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").unwrap();
        assert_eq!(
            parsed.containers["api"].config_override.as_ref().unwrap()["port"].as_u64(),
            Some(3001)
        );
        assert!(out.contains("worker: package://new.example/api"));
        assert_eq!(upsert_container(&out, &worker).unwrap(), Outcome::Unchanged);
    }

    #[test]
    fn configured_workers_reject_unknown_fields_wrong_types_and_conflicting_versions() {
        for value in [
            serde_json::json!({"worker": "./api", "script": {"run": "./api"}}),
            serde_json::json!({"worker": "./api", "scripts": {"start": "./api"}}),
            serde_json::json!({"worker": "./api", "start_after": "database"}),
            serde_json::json!({"worker": "./api", "environment": {"PORT": 3000}}),
        ] {
            assert!(serde_json::from_value::<WorkerInput>(value).is_err());
        }
        let input: WorkerInput = serde_json::from_value(serde_json::json!({
            "worker": "api@1.0.0", "version": "2.0.0"
        }))
        .unwrap();
        assert_eq!(input.parse().unwrap_err().code(), "INVALID_WORKER_SPEC");
    }

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
    fn generated_marker_distinguishes_compose_dependencies_from_manual_entries() {
        let out = added(FILE);

        assert!(is_generated_container(&out, "state").unwrap());
        assert!(!is_generated_container(&out, "todo").unwrap());
    }

    #[test]
    fn generated_marker_must_be_directly_before_the_container() {
        let text = "containers:\n  # added by compose::add\n  # operator note\n  state:\n    worker: package://state\n    version: '1.0.0'\n";
        assert!(!is_generated_container(text, "state").unwrap());

        let text = "containers:\n  state:\n    worker: package://state\n    version: '1.0.0'\n    # added by compose::add\n";
        assert!(!is_generated_container(text, "state").unwrap());
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
    /// version and no `start_after`. Leaving it alone would start the worker by
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
            start_after: vec!["queue".to_string()],
            fields: serde_yaml::Mapping::new(),
        };

        let once = added(FILE);
        // Same key, same version, but the graph now says it needs something.
        let out = match upsert_container(&once, &with_deps).unwrap() {
            Outcome::Replaced { text, .. } => text,
            other => panic!("expected a rewrite, got {other:?}"),
        };
        assert!(out.contains("start_after:"), "{out}");
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

    /// `start_after: [queue]` is the same declaration as a block list, so it has
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
    start_after: [queue, state]
";
        let mut wanted = parse_worker("api@1.0.0").unwrap();
        wanted.start_after = vec!["queue".to_string(), "state".to_string()];
        assert_eq!(
            upsert_container(file, &wanted).unwrap(),
            Outcome::Unchanged,
            "an inline list should read as the dependencies it declares"
        );
    }

    #[test]
    fn adds_the_first_container_to_a_bare_empty_mapping() {
        let text = "engine:\n  workers: {}\ncontainers:\n";
        let out = match upsert_container(text, &parse_worker("state@0.21.4").unwrap()).unwrap() {
            Outcome::Added(text) => text,
            other => panic!("expected an addition, got {other:?}"),
        };

        assert_eq!(out.matches("state:").count(), 1, "duplicated: {out}");
        assert!(out.contains("# added by compose::add"), "{out}");
        assert!(out.contains("version: \"0.21.4\""), "{out}");
        assert!(out.ends_with('\n'), "lost the trailing newline: {out}");
        crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml")
            .expect("the edited file should load");
    }

    #[test]
    fn adds_the_first_container_to_an_inline_empty_mapping() {
        let text = "engine:\n  workers: {}\ncontainers: {}\n";
        let out = match upsert_container(text, &parse_worker("state@0.21.4").unwrap()).unwrap() {
            Outcome::Added(text) => text,
            other => panic!("expected an addition, got {other:?}"),
        };

        assert!(
            !out.contains("containers: {}"),
            "inline map survived: {out}"
        );
        assert_eq!(out.matches("containers:").count(), 1, "duplicated: {out}");
        assert_eq!(out.matches("state:").count(), 1, "duplicated: {out}");
        assert!(out.contains("worker: package://api.workers.iii.dev/state"));
        assert!(out.contains("version: \"0.21.4\""), "{out}");
        crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml")
            .expect("the edited file should load");
    }

    #[test]
    fn preserves_an_inline_empty_mapping_comment_when_adding() {
        let text = "engine:\n  workers: {}\ncontainers: { } # keep this empty until bootstrap\n";
        let out = match upsert_container(text, &parse_worker("state@0.21.4").unwrap()).unwrap() {
            Outcome::Added(text) => text,
            other => panic!("expected an addition, got {other:?}"),
        };

        assert!(!out.contains("containers: {"), "inline map survived: {out}");
        assert!(
            out.contains("containers:\n  # keep this empty until bootstrap\n"),
            "comment changed: {out}"
        );
        assert!(out.ends_with('\n'), "lost the trailing newline: {out}");
        crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml")
            .expect("the edited file should load");
    }

    #[test]
    fn adds_multiple_containers_after_normalizing_an_inline_empty_mapping() {
        let text = "engine:\n  workers: {}\ncontainers: {}\n";
        let once = match upsert_container(text, &parse_worker("state@0.21.4").unwrap()).unwrap() {
            Outcome::Added(text) => text,
            other => panic!("expected the first addition, got {other:?}"),
        };
        let out = match upsert_container(&once, &parse_worker("queue@1.0.0").unwrap()).unwrap() {
            Outcome::Added(text) => text,
            other => panic!("expected the second addition, got {other:?}"),
        };

        assert!(
            !out.contains("containers: {}"),
            "inline map survived: {out}"
        );
        assert_eq!(out.matches("containers:").count(), 1, "duplicated: {out}");
        assert_eq!(out.matches("state:").count(), 1, "duplicated: {out}");
        assert_eq!(out.matches("queue:").count(), 1, "duplicated: {out}");
        crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml")
            .expect("the edited file should load");
    }

    #[test]
    fn the_containers_key_may_carry_a_comment() {
        for line in [
            "containers:",
            "containers:  ",
            "containers:\r\n",
            "containers: # my workers",
            "containers:   # my workers  ",
            "containers:\t# my workers",
            "containers: #",
            "containers: # my workers\r\n",
        ] {
            let block = find_containers(&["namespace: a\n", line, "  api:\n"]);
            assert_eq!(block.map(|block| block.start).ok(), Some(2), "{line:?}");
        }
        for line in [
            // No whitespace before the `#`: YAML reads a plain scalar.
            "containers:#not a comment",
            "containers: {} # inline",
            "containers: value # comment",
            "  containers: # nested",
            "# containers:",
            "containers_old: # comment",
        ] {
            assert!(find_containers(&[line]).is_err(), "{line:?}");
        }
    }

    #[test]
    fn every_edit_works_under_a_commented_containers_key() {
        let text =
            "namespace: default\ncontainers: # my workers\n  api:\n    worker: path://./api\n";
        let out = added(text);
        assert!(out.starts_with(text), "the file above the end moved: {out}");
        assert!(is_generated_container(&out, "state").unwrap(), "{out}");
        assert!(!is_generated_container(&out, "api").unwrap(), "{out}");
        crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").expect("should load");

        let out = remove_container(&out, "state").unwrap().expect("removed");
        assert_eq!(out, text);
    }

    #[test]
    fn a_commented_container_key_is_read_like_a_bare_one() {
        let text = "containers:\n  # added by compose::add\n  state: # pinned\n    worker: package://state\n    version: '1.0.0'\n";
        assert!(is_generated_container(text, "state").unwrap());
    }

    #[test]
    fn batch_removal_merges_overlapping_comment_spans_without_touching_survivors() {
        for comment in [
            "# x",
            "# This comment is longer than the entire surviving keep entry below.",
        ] {
            for crlf in [false, true] {
                let text = format!(
                    "# retained\ncontainers:\n  a:\n    worker: path://./a\n    {comment}\n  b:\n    worker: path://./b\n    {comment}\n  c:\n    worker: path://./c\n  keep: {{worker: path://.}}\n"
                );
                let text = if crlf {
                    text.replace('\n', "\r\n")
                } else {
                    text
                };
                let expected = "# retained\ncontainers:\n  keep: {worker: path://.}\n";
                let expected = if crlf {
                    expected.replace('\n', "\r\n")
                } else {
                    expected.into()
                };
                for keys in [
                    &["a", "b", "c"][..],
                    &["c", "a", "b"][..],
                    &["b", "c", "a", "b"][..],
                ] {
                    let output = remove_containers(&text, keys).unwrap();
                    assert_eq!(output, expected);
                    crate::ComposeFile::parse(&output, "/tmp/worker-compose.yaml").unwrap();
                }
            }
        }
        // Intersecting ranges must not make separate ranges consume a survivor.
        let text = "containers:\n  a:\n    worker: path://./a\n    # shared\n  b:\n    worker: path://./b\n  keep:\n    worker: path://./keep\n    config_override: {message: untouched}\n  c: {worker: path://./c}\n  last: {worker: path://./last}\n";
        assert_eq!(
            remove_containers(text, &["c", "b", "a"]).unwrap(),
            "containers:\n  keep:\n    worker: path://./keep\n    config_override: {message: untouched}\n  last: {worker: path://./last}\n"
        );
    }

    #[test]
    fn upsert_preserves_uniform_line_endings_and_final_newline() {
        for entry in [
            "  tool:\n    worker: package://api.workers.iii.dev/tool\n    version: '1.0.0'\n",
            "  tool: {worker: package://api.workers.iii.dev/tool, version: '1.0.0'}\n",
        ] {
            for newline in ["\n", "\r\n"] {
                for final_newline in [false, true] {
                    let lf =
                        format!("# heading\ncontainers:\n{entry}  keep: {{worker: path://.}}\n");
                    let text = if final_newline {
                        lf
                    } else {
                        lf.trim_end_matches('\n').to_string()
                    };
                    let text = text.replace('\n', newline);
                    let output = changed(&text, &parse_worker("tool@1.1.0").unwrap());
                    assert_eq!(output.ends_with('\n'), final_newline);
                    let tail = format!(
                        "  keep: {{worker: path://.}}{}",
                        if final_newline { newline } else { "" }
                    );
                    assert!(output.ends_with(&tail), "{output:?}");
                    assert!(output.starts_with(&format!("# heading{newline}containers:{newline}")));
                    if newline == "\r\n" {
                        assert!(!output.replace("\r\n", "").contains('\n'));
                    } else {
                        assert!(!output.contains('\r'));
                    }
                    crate::ComposeFile::parse(&output, "/tmp/worker-compose.yaml").unwrap();
                    assert_eq!(
                        upsert_container(&output, &parse_worker("tool@1.1.0").unwrap()).unwrap(),
                        Outcome::Unchanged
                    );
                }
            }
        }
    }

    #[test]
    fn plain_scalar_tag_text_is_not_a_yaml_property_during_upsert() {
        for settings in [
            "{message: literal !<text}",
            "{message: literal !<text> suffix}",
            "{message: literal !<text:more}",
            "{message: literal !local &anchor !<text}",
            "{message: https://example.test/path !<text}",
            "{message: literal\n      !<text}",
            "{message: literal !<text, other: !<tag:yaml.org,2002:str> queue}",
            "{\"message\":\"literal !<text\", \"other\":!<tag:yaml.org,2002:str> queue}",
            "{message: &message !<tag:yaml.org,2002:str> literal !<text, copy: *message}",
            "{message: !<tag:yaml.org,2002:str> &message literal !<text, copy: *message}",
            "{values: [literal !<text, !<tag:yaml.org,2002:str> queue], message: unchanged}",
        ] {
            let text = format!(
                "containers:\n  tool: {{worker: package://api.workers.iii.dev/tool, version: '1.0.0', config_override: {settings}}}\n  keep: {{worker: path://.}}\n"
            );
            crate::ComposeFile::parse(&text, "/tmp/worker-compose.yaml").unwrap();
            for crlf in [false, true] {
                let input = if crlf {
                    text.replace('\n', "\r\n")
                } else {
                    text.clone()
                };
                let before: Value = serde_yaml::from_str(&input).unwrap();
                assert_eq!(
                    upsert_container(&input, &parse_worker("tool@1.0.0").unwrap()).unwrap(),
                    Outcome::Unchanged
                );
                let output = changed(&input, &parse_worker("tool@1.1.0").unwrap());
                crate::ComposeFile::parse(&output, "/tmp/worker-compose.yaml").unwrap();
                let after: Value = serde_yaml::from_str(&output).unwrap();
                assert_eq!(after["containers"]["tool"]["version"], "1.1.0");
                assert_eq!(
                    after["containers"]["tool"]["config_override"],
                    before["containers"]["tool"]["config_override"]
                );
                assert!(output.contains("!<text"), "{output}");
                assert!(output.ends_with(if crlf {
                    "  keep: {worker: path://.}\r\n"
                } else {
                    "  keep: {worker: path://.}\n"
                }));
                assert_eq!(
                    upsert_container(&output, &parse_worker("tool@1.1.0").unwrap()).unwrap(),
                    Outcome::Unchanged
                );
                if crlf {
                    assert!(!output.replace("\r\n", "").contains('\n'));
                }
            }
        }
        assert_eq!(
            flow_mapping_parts("message: literal !<text, other: value").unwrap(),
            ["message: literal !<text", " other: value"]
        );
        assert_eq!(
            flow_mapping_parts("db, &q !<tag:yaml.org,2002:str> queue").unwrap(),
            ["db", " &q !<tag:yaml.org,2002:str> queue"]
        );
        assert_eq!(
            flow_mapping_parts("db, !<tag:yaml.org,2002:str> &q queue").unwrap(),
            ["db", " !<tag:yaml.org,2002:str> &q queue"]
        );
        assert!(flow_mapping_parts("db, &q !<tag:yaml.org,2002:str queue").is_none());
    }

    #[test]
    fn flow_dependency_pruning_preserves_verbatim_tags() {
        for needs in [
            "[db, !<tag:yaml.org,2002:str> queue]",
            "[!<tag:yaml.org,2002:str> db, !<tag:yaml.org,2002:str> queue]",
            "[\n      db,\n      !<tag:yaml.org,2002:str> queue,\n    ]",
        ] {
            for flow in [false, true] {
                let api = if flow {
                    format!(
                        "  api: {{worker: package://api.workers.iii.dev/api, version: '1.0.0', start_after: {needs}, config_override: {{message: '!<plain,scalar>'}}}}\n"
                    )
                } else {
                    format!(
                        "  api:\n    worker: package://api.workers.iii.dev/api\n    version: '1.0.0'\n    start_after: {needs}\n    config_override: {{message: '!<plain,scalar>'}}\n"
                    )
                };
                let text = format!(
                    "containers:\n  db: {{worker: path://./db}}\n  queue: {{worker: path://./queue}}\n{api}"
                );
                crate::ComposeFile::parse(&text, "/tmp/worker-compose.yaml").unwrap();
                let updated = changed(&text, &parse_worker("api@1.1.0").unwrap());
                for original in [&text, &updated] {
                    for crlf in [false, true] {
                        let input = if crlf {
                            original.replace('\n', "\r\n")
                        } else {
                            original.clone()
                        };
                        let output = remove_container(&input, "db").unwrap().unwrap();
                        let parsed =
                            crate::ComposeFile::parse(&output, "/tmp/worker-compose.yaml").unwrap();
                        assert_eq!(parsed.containers["api"].start_after, ["queue"]);
                        assert!(
                            output.contains("!<tag:yaml.org,2002:str> queue"),
                            "{output}"
                        );
                        assert!(output.contains("config_override: {message: '!<plain,scalar>'}"));
                        assert!(remove_container(&output, "db").unwrap().is_none());
                        if crlf {
                            assert!(!output.replace("\r\n", "").contains('\n'));
                        }
                    }
                }
            }
        }
        assert_eq!(
            flow_mapping_parts("db, !<tag:yaml.org,2002:str> queue").unwrap(),
            ["db", " !<tag:yaml.org,2002:str> queue"]
        );
        assert_eq!(
            flow_mapping_parts("'!<literal,not-tag>', queue")
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            flow_mapping_parts("plain!<not-a-tag, queue").unwrap().len(),
            2
        );
        assert!(flow_mapping_parts("db, !<tag:yaml.org,2002:str queue").is_none());
    }

    #[test]
    fn flow_dependency_comments_are_not_values_but_quoted_hash_names_are() {
        for (needs, expected_needs) in [
            (
                "[db, queue,\n# retained comment\n    ]",
                "[ queue,\n# retained comment\n    ]",
            ),
            (
                "[db, queue,\n  # indented retained comment\n    ]",
                "[ queue,\n  # indented retained comment\n    ]",
            ),
            (
                "[db, \"#queue\",\n# retained comment\n    ]",
                "[ \"#queue\",\n# retained comment\n    ]",
            ),
            (
                "[db, '#queue',\n# retained comment\n    ]",
                "[ '#queue',\n# retained comment\n    ]",
            ),
        ] {
            for flow in [false, true] {
                let api = if flow {
                    format!(
                        "  api: {{worker: path://./api, start_after: {needs}, config_override: {{start_after: [db]}}}}\n"
                    )
                } else {
                    format!(
                        "  api:\n    worker: path://./api\n    start_after: {needs}\n    config_override: {{start_after: [db]}}\n"
                    )
                };
                let prefix = "containers:\n  db: {worker: path://./db}\n  queue: {worker: path://./queue}\n  '#queue': {worker: path://./hash-queue}\n";
                let text = format!("{prefix}{api}");
                crate::ComposeFile::parse(&text, "/tmp/worker-compose.yaml").unwrap();
                let wanted = text
                    .replace("  db: {worker: path://./db}\n", "")
                    .replace(needs, expected_needs);
                for crlf in [false, true] {
                    let input = if crlf {
                        text.replace('\n', "\r\n")
                    } else {
                        text.clone()
                    };
                    let expected = if crlf {
                        wanted.replace('\n', "\r\n")
                    } else {
                        wanted.clone()
                    };
                    let output = remove_container(&input, "db").unwrap().unwrap();
                    assert_eq!(output, expected);
                    let parsed =
                        crate::ComposeFile::parse(&output, "/tmp/worker-compose.yaml").unwrap();
                    assert_eq!(
                        parsed.containers["api"].start_after,
                        [if needs.contains("#queue") {
                            "#queue"
                        } else {
                            "queue"
                        }]
                    );
                    assert!(output.contains("config_override: {start_after: [db]}"));
                }
            }
        }
    }

    #[test]
    fn multiline_flow_dependencies_survive_upsert_then_remove() {
        for key in [
            "start_after",
            "\"start_after\"",
            "'start_after'",
            r#""start_\u0061fter""#,
        ] {
            for (needs, remaining) in [
                ("[db, queue]", vec!["queue"]),
                ("[\n      db,\n      queue\n    ]", vec!["queue"]),
                (
                    "[\n      \"\\u0064b\",\n      # keep survivor\n      'queue',\n      db,\n    ]",
                    vec!["queue"],
                ),
                ("[\n      db\n    ]", vec![]),
            ] {
                let text = format!(
                    "containers:\n  db: {{worker: path://./db}}\n  api: {{worker: package://api.workers.iii.dev/api, version: '1.0.0', {key}: {needs}, config_override: {{start_after: [db], message: 'keep, # ]'}}}}\n  queue: {{worker: path://./queue}}\n"
                );
                crate::ComposeFile::parse(&text, "/tmp/worker-compose.yaml").unwrap();
                let updated = changed(&text, &parse_worker("api@1.1.0").unwrap());
                for crlf in [false, true] {
                    let input = if crlf {
                        updated.replace('\n', "\r\n")
                    } else {
                        updated.clone()
                    };
                    let output = remove_container(&input, "db").unwrap().unwrap();
                    let parsed =
                        crate::ComposeFile::parse(&output, "/tmp/worker-compose.yaml").unwrap();
                    assert_eq!(parsed.containers["api"].version.as_deref(), Some("1.1.0"));
                    assert_eq!(parsed.containers["api"].start_after, remaining);
                    let before: Value = serde_yaml::from_str(&input).unwrap();
                    let after: Value = serde_yaml::from_str(&output).unwrap();
                    assert_eq!(
                        after["containers"]["api"]["config_override"],
                        before["containers"]["api"]["config_override"]
                    );
                    if remaining.is_empty() {
                        assert!(after["containers"]["api"].get("start_after").is_none());
                    }
                    if needs.contains("# keep survivor") {
                        assert!(output.contains("# keep survivor"));
                    }
                    assert!(output.ends_with(if crlf {
                        "  queue: {worker: path://./queue}\r\n"
                    } else {
                        "  queue: {worker: path://./queue}\n"
                    }));
                    if crlf {
                        assert!(!output.replace("\r\n", "").contains('\n'));
                    }
                    assert!(remove_container(&output, "db").unwrap().is_none());
                }
            }
        }
    }

    #[test]
    fn block_flow_dependencies_use_document_values_and_preserve_neighbors() {
        for (field, expected_field) in [
            (
                "    start_after:\n      [db,\n       queue] # close\n",
                "    start_after:\n      [\n       queue] # close\n",
            ),
            (
                "    'start_after': [\n      db,\n      queue\n    ] # close\n",
                "    'start_after': [\n      queue\n    ] # close\n",
            ),
            ("    \"start_after\": [\n      db\n    ] # close\n", ""),
            (
                "    start_after: [db, queue] # close\n",
                "    start_after: [ queue] # close\n",
            ),
        ] {
            let prefix =
                "containers:\n  db: {worker: path://./db}\n  api:\n    worker: path://./api\n";
            let suffix = "    env_file: [db]\n    config_override: {start_after: [db]}\n  queue: {worker: path://./queue}\n";
            let text = format!("{prefix}{field}{suffix}");
            let wanted =
                format!("containers:\n  api:\n    worker: path://./api\n{expected_field}{suffix}");
            for crlf in [false, true] {
                let input = if crlf {
                    text.replace('\n', "\r\n")
                } else {
                    text.clone()
                };
                let expected = if crlf {
                    wanted.replace('\n', "\r\n")
                } else {
                    wanted.clone()
                };
                let output = remove_container(&input, "db").unwrap().unwrap();
                assert_eq!(output, expected);
                crate::ComposeFile::parse(&output, "/tmp/worker-compose.yaml").unwrap();
            }
        }
        // Alias values are resolved using the original document, not the slice.
        let text = "containers:\n  db: {worker: path://./db}\n  queue:\n    worker: path://./queue\n    config_override: &queue_name queue\n  api:\n    worker: path://./api\n    start_after: [\n      db,\n      *queue_name\n    ]\n";
        let output = remove_container(text, "db").unwrap().unwrap();
        assert!(output.contains("*queue_name"));
        let parsed = crate::ComposeFile::parse(&output, "/tmp/worker-compose.yaml").unwrap();
        assert_eq!(parsed.containers["api"].start_after, ["queue"]);
    }

    #[test]
    fn removal_recognizes_quoted_dependency_keys_after_flow_normalization() {
        for key in [
            "start_after",
            "\"start_after\"",
            "'start_after'",
            r#""start_\u0061fter""#,
        ] {
            for keep in [false, true] {
                let needs = if keep {
                    "[\"db\", \"queue\"]"
                } else {
                    "[\"db\"]"
                };
                // YAML requires separation for a plain key; JSON-style quoted
                // keys intentionally retain their adjacent value delimiter.
                let separator = if key == "start_after" { " " } else { "" };
                let text = format!(
                    "containers:\n  db: {{worker: path://./db}}\n  tool: {{\"worker\":\"package://api.workers.iii.dev/tool\",\"version\":\"1.0.0\",{key}:{separator}{needs},\"config_override\":{{\"start_after\":[\"db\"]}}}}\n  queue: {{worker: path://./queue}}\n"
                );
                crate::ComposeFile::parse(&text, "/tmp/worker-compose.yaml")
                    .expect("regression fixture must be a valid Compose declaration");
                let updated = changed(&text, &parse_worker("tool@1.1.0").unwrap());
                assert!(updated.contains(&format!("    {key}:")), "{updated}");
                for crlf in [false, true] {
                    let input = if crlf {
                        updated.replace('\n', "\r\n")
                    } else {
                        updated.clone()
                    };
                    let output = remove_container(&input, "db").unwrap().unwrap();
                    let parsed =
                        crate::ComposeFile::parse(&output, "/tmp/worker-compose.yaml").unwrap();
                    assert_eq!(parsed.containers["tool"].version.as_deref(), Some("1.1.0"));
                    let expected: Vec<String> = if keep { vec!["queue".into()] } else { vec![] };
                    assert_eq!(parsed.containers["tool"].start_after, expected);
                    let value: Value = serde_yaml::from_str(&output).unwrap();
                    assert_eq!(
                        value["containers"]["tool"]["config_override"]["start_after"][0],
                        "db"
                    );
                    let expected = input.replace("  db: {worker: path://./db}", "");
                    let newline = if crlf { "\r\n" } else { "\n" };
                    let expected = expected.replacen(
                        &format!("containers:{newline}{newline}"),
                        &format!("containers:{newline}"),
                        1,
                    );
                    let expected = if keep {
                        expected.replace(needs, "[ \"queue\"]")
                    } else {
                        expected.replace(&format!("    {key}: {needs}{newline}"), "")
                    };
                    assert_eq!(output, expected);
                }
            }
        }
    }

    #[test]
    fn quoted_block_dependency_keys_preserve_nested_and_neighbor_fields() {
        for key in [
            "start_after",
            "\"start_after\"",
            "'start_after'",
            r#""start_\u0061fter""#,
        ] {
            let text = format!(
                "containers:\n  db: {{worker: path://./db}}\n  api: &api\n    config_override:\n      'start_after': [db]\n    \"worker\": path://./api\n    {key}: # edge\n    - db\n    - queue # keep\n    env_file: [db]\n  queue: {{worker: path://./queue}}\n"
            );
            let output = remove_container(&text, "db").unwrap().unwrap();
            assert_eq!(
                output,
                text.replace("  db: {worker: path://./db}\n", "")
                    .replace("    - db\n", "")
            );
            crate::ComposeFile::parse(&output, "/tmp/worker-compose.yaml").unwrap();
        }
        assert_eq!(
            mapping_key("    \"start:after\": [db]").unwrap().0,
            "start:after"
        );
        assert_eq!(
            mapping_key("    'start_after': [db] # comment: kept")
                .unwrap()
                .0,
            "start_after"
        );
    }

    #[test]
    fn removal_uses_outer_field_indentation_even_when_settings_precede_worker() {
        for header in ["", "&api", "!!map &api"] {
            for outer_dependencies in ["", "    start_after: [db, queue] # outer\n"] {
                let nested = "    config_override:\n      worker: application-data\n      start_after: [db]\n      deeper:\n        start_after: [db]\n";
                let text = format!(
                    "containers:\n  db: {{worker: path://./db}}\n  api: {header}\n{nested}    worker: path://./api\n{outer_dependencies}  queue: {{worker: path://./queue}}\n"
                );
                let remaining = if outer_dependencies.is_empty() {
                    ""
                } else {
                    "    start_after: [ queue] # outer\n"
                };
                let expected = format!(
                    "containers:\n  api: {header}\n{nested}    worker: path://./api\n{remaining}  queue: {{worker: path://./queue}}\n"
                );
                for crlf in [false, true] {
                    let input = if crlf {
                        text.replace('\n', "\r\n")
                    } else {
                        text.clone()
                    };
                    let wanted = if crlf {
                        expected.replace('\n', "\r\n")
                    } else {
                        expected.clone()
                    };
                    let output = remove_container(&input, "db").unwrap().unwrap();
                    assert_eq!(output, wanted);
                    let before: Value = serde_yaml::from_str(&input).unwrap();
                    let after: Value = serde_yaml::from_str(&output).unwrap();
                    assert_eq!(
                        before["containers"]["api"]["config_override"],
                        after["containers"]["api"]["config_override"]
                    );
                    crate::ComposeFile::parse(&output, "/tmp/worker-compose.yaml").unwrap();
                }
            }
        }
    }

    #[test]
    fn upsert_rejects_replacing_fields_whose_anchors_are_still_referenced() {
        for fields in [
            "    worker: package://api.workers.iii.dev/api\n    version: &release '1.0.0'\n    config_override: {release: *release}\n",
            "    {worker: package://api.workers.iii.dev/api, version: &release '1.0.0', config_override: {release: *release}}\n",
        ] {
            let text = format!("containers:\n  api:\n{fields}");
            let before: Value = serde_yaml::from_str(&text).unwrap();
            assert_eq!(
                upsert_container(&text, &parse_worker("api@1.0.0").unwrap()).unwrap(),
                Outcome::Unchanged
            );
            let error = upsert_container(&text, &parse_worker("api@1.1.0").unwrap()).unwrap_err();
            assert!(
                error.to_string().contains("edit leaves invalid YAML"),
                "{error}"
            );
            assert_eq!(
                before["containers"]["api"]["config_override"]["release"],
                "1.0.0"
            );
            let unreferenced = text.replace(
                "config_override: {release: *release}",
                "config_override: {release: '1.0.0'}",
            );
            let output = changed(&unreferenced, &parse_worker("api@1.1.0").unwrap());
            let parsed: Value = serde_yaml::from_str(&output).unwrap();
            assert_eq!(parsed["containers"]["api"]["version"], "1.1.0");
            assert_eq!(
                parsed["containers"]["api"]["config_override"]["release"],
                "1.0.0"
            );
        }
        let text = "containers:\n  api:\n    worker: package://api.workers.iii.dev/api\n    version: &release '1.0.0'\n  other:\n    worker: path://./other\n    config_override: {release: *release}\n";
        assert!(upsert_container(text, &parse_worker("api@1.1.0").unwrap()).is_err());
    }

    #[test]
    fn batch_removal_validates_final_document_independently_of_request_order() {
        for flow in [false, true] {
            let definitions = if flow {
                "  db: &db {worker: path://./db}\n  api: {worker: path://./api, config_override: *db}\n"
            } else {
                "  db: &db\n    worker: path://./db\n  api:\n    worker: path://./api\n    config_override: *db\n"
            };
            for dependent in [
                "  keep: {worker: path://./keep, start_after: [db, api, queue], config_override: {start_after: [db]}}\n",
                "  keep:\n    worker: path://./keep\n    start_after:\n    - db\n    - api\n    - queue\n    config_override: {start_after: [db]}\n",
            ] {
                let text = format!(
                    "containers:\n{definitions}{dependent}  queue: {{worker: path://./queue}}\n"
                );
                assert!(remove_container(&text, "db").is_err());
                let first = remove_containers(&text, &["db", "api"]).unwrap();
                for order in [&["api", "db"][..], &["db", "api", "db"][..]] {
                    assert_eq!(remove_containers(&text, order).unwrap(), first);
                }
                let parsed = crate::ComposeFile::parse(&first, "/tmp/worker-compose.yaml").unwrap();
                assert_eq!(parsed.containers.len(), 2);
                assert_eq!(parsed.containers["keep"].start_after, ["queue"]);
                let yaml: Value = serde_yaml::from_str(&first).unwrap();
                assert_eq!(
                    yaml["containers"]["keep"]["config_override"]["start_after"][0],
                    "db"
                );
                assert!(matches!(
                    remove_containers(&text, &["db", "missing"]),
                    Err(ComposeError::UnknownContainer { .. })
                ));
                assert_eq!(remove_containers(&text, &[]).unwrap(), text);
            }
        }
        // An alias surviving the requested batch still makes the final edit invalid.
        let text = "containers:\n  db: &db {worker: path://./db}\n  api: {worker: path://./api}\n  keep: {worker: path://./keep, config_override: *db}\n";
        assert!(remove_containers(text, &["db", "api"]).is_err());
    }

    #[test]
    fn removal_accepts_indentless_sequences_without_crossing_sibling_fields() {
        for item_indent in ["    ", "      "] {
            for keep in [false, true] {
                let surviving = if keep {
                    format!("{item_indent}- queue # keep\n")
                } else {
                    String::new()
                };
                let dependencies =
                    format!("    start_after:\n{item_indent}- db # removed\n{surviving}");
                let kept = if keep {
                    format!("    start_after:\n{surviving}")
                } else {
                    String::new()
                };
                let suffix = "    env_file:\n    - db\n    config_override:\n      start_after: [db]\n  queue: {worker: path://./queue}\n";
                let text = format!(
                    "containers:\n  db: {{worker: path://./db}}\n  api: &api\n    worker: path://./api\n{dependencies}{suffix}"
                );
                let expected =
                    format!("containers:\n  api: &api\n    worker: path://./api\n{kept}{suffix}");
                for crlf in [false, true] {
                    let input = if crlf {
                        text.replace('\n', "\r\n")
                    } else {
                        text.clone()
                    };
                    let wanted = if crlf {
                        expected.replace('\n', "\r\n")
                    } else {
                        expected.clone()
                    };
                    let output = remove_container(&input, "db").unwrap().unwrap();
                    assert_eq!(output, wanted);
                    let parsed =
                        crate::ComposeFile::parse(&output, "/tmp/worker-compose.yaml").unwrap();
                    let needs: Vec<String> = if keep { vec!["queue".into()] } else { vec![] };
                    assert_eq!(parsed.containers["api"].start_after, needs);
                    assert!(output.contains(if crlf {
                        "    env_file:\r\n    - db\r\n"
                    } else {
                        "    env_file:\n    - db\n"
                    }));
                }
            }
        }
    }

    #[test]
    fn removal_prunes_only_outer_dependencies_in_flow_containers() {
        let nested =
            "config_override: {worker: application-data, start_after: [db], message: 'db, # ]'}";
        for needs in [
            "[db]",
            "[queue, db]",
            "[db, queue, db]",
            "[\n      db, # removed\n      'queue', # retained\n    ]",
        ] {
            for position in 0..3 {
                let field = format!("\"start_after\":{needs}");
                let mut fields = vec!["worker: path://./api".to_string(), nested.to_string()];
                fields.insert(position, field);
                for header in ["  api: {", "  api: &api\n    {"] {
                    let text = format!(
                        "containers:\n  db: {{worker: path://./db}}\n{header}{} }} # tail\n  queue: {{worker: path://./queue}}\n",
                        fields.join(", ")
                    );
                    for crlf in [false, true] {
                        let input = if crlf {
                            text.replace('\n', "\r\n")
                        } else {
                            text.clone()
                        };
                        let original: Value = serde_yaml::from_str(&input).unwrap();
                        let output = remove_container(&input, "db").unwrap().unwrap();
                        let parsed =
                            crate::ComposeFile::parse(&output, "/tmp/worker-compose.yaml").unwrap();
                        assert!(!parsed.containers.contains_key("db"));
                        let expected: Vec<String> = if needs.contains("queue") {
                            vec!["queue".into()]
                        } else {
                            vec![]
                        };
                        assert_eq!(parsed.containers["api"].start_after, expected);
                        let current: Value = serde_yaml::from_str(&output).unwrap();
                        assert_eq!(
                            current["containers"]["api"]["config_override"],
                            original["containers"]["api"]["config_override"]
                        );
                        assert!(output.contains(nested), "{output}");
                        assert!(output.ends_with(if crlf {
                            "  queue: {worker: path://./queue}\r\n"
                        } else {
                            "  queue: {worker: path://./queue}\n"
                        }));
                        if !needs.contains("queue") {
                            assert!(current["containers"]["api"].get("start_after").is_none());
                        }
                        assert!(remove_container(&output, "db").unwrap().is_none());
                    }
                }
            }
        }
    }

    #[test]
    fn removal_refuses_candidates_with_dangling_aliases() {
        for entry in [
            "  db: &db {worker: path://./db}\n",
            "  db: &db\n    worker: path://./db\n",
            "  db:\n    worker: path://./db\n    config_override: &db {nested: value}\n",
        ] {
            let text = format!(
                "containers:\n{entry}  api:\n    worker: path://./api\n    config_override: *db\n"
            );
            crate::ComposeFile::parse(&text, "/tmp/worker-compose.yaml").unwrap();
            let before = text.clone();
            let error = remove_container(&text, "db").unwrap_err();
            assert!(
                error.to_string().contains("removal leaves invalid YAML"),
                "{error}"
            );
            assert_eq!(text, before);
            let unreferenced = text.replace("    config_override: *db\n", "");
            let output = remove_container(&unreferenced, "db").unwrap().unwrap();
            assert_eq!(output, "containers:\n  api:\n    worker: path://./api\n");
            crate::ComposeFile::parse(&output, "/tmp/worker-compose.yaml").unwrap();
        }
        // A reference to an unrelated, surviving anchor must remain valid.
        let text = "containers:\n  defaults:\n    worker: path://./defaults\n    config_override: &settings {value: kept}\n  db: &db {worker: path://./db}\n  api: {worker: path://./api, config_override: *settings, start_after: [db]}\n";
        let output = remove_container(text, "db").unwrap().unwrap();
        let parsed: Value = serde_yaml::from_str(&output).unwrap();
        assert_eq!(
            parsed["containers"]["api"]["config_override"]["value"],
            "kept"
        );
        assert!(output.contains("config_override: *settings"));
    }

    #[test]
    fn removing_and_marker_lookup_locate_complete_flow_entries() {
        for entry in [
            "  api: {worker: path://./api}\n",
            "  api:\n    {worker: path://./api}\n",
            "  api: &api # heading\n    # inside entry\n    {worker: path://./api,\n     config_override: {nested: true}} # end\n",
            "  api: !!map &api {\n    worker: path://./api,\n    config_override: {nested: true}\n  } # end\n",
        ] {
            for generated in [false, true] {
                let marker = if generated {
                    "  # added by compose::add\n"
                } else {
                    ""
                };
                let prefix = "# untouched\ncontainers:\n  before:\n    worker: path://./before\n";
                let suffix =
                    "  after:\n    worker: path://./after\n    start_after: [before, api] # keep\n";
                let text = format!("{prefix}{marker}{entry}{suffix}");
                let expected = format!(
                    "{prefix}  after:\n    worker: path://./after\n    start_after: [before] # keep\n"
                );
                for crlf in [false, true] {
                    let input = if crlf {
                        text.replace('\n', "\r\n")
                    } else {
                        text.clone()
                    };
                    let wanted = if crlf {
                        expected.replace('\n', "\r\n")
                    } else {
                        expected.clone()
                    };
                    assert_eq!(is_generated_container(&input, "api").unwrap(), generated);
                    assert!(!is_generated_container(&input, "after").unwrap());
                    let output = remove_container(&input, "api").unwrap().unwrap();
                    assert_eq!(output, wanted);
                    let parsed =
                        crate::ComposeFile::parse(&output, "/tmp/worker-compose.yaml").unwrap();
                    assert_eq!(parsed.containers.len(), 2);
                    assert_eq!(parsed.containers["after"].start_after, ["before"]);
                    assert!(remove_container(&output, "api").unwrap().is_none());
                    assert!(!is_generated_container(&output, "api").unwrap());
                }
            }
        }
    }

    #[test]
    fn removal_never_scans_flow_application_data_as_block_dependencies() {
        for flow in [
            "  api: &api\n    {worker: path://./api,\n     config_override: {\n       worker: application-data,\n       start_after: [db]\n     }}\n",
            "  api: {worker: path://./api,\n    config_override: {\n      worker: application-data,\n      start_after: [db]\n    }}\n",
        ] {
            // Also guard the preceding block: flow entries must remain actual
            // boundaries even though their contents are excluded from scanning.
            let prefix = "containers:\n  db:\n    worker: path://./db\n  before:\n    worker: path://./before\n    config_override:\n      worker: application-data\n      start_after: [db]\n";
            let suffix = "  after: &after\n    worker: path://./after\n    start_after: [db, before] # real edges\n";
            let text = format!("{prefix}{flow}{suffix}");
            let expected = format!(
                "containers:\n  before:\n    worker: path://./before\n    config_override:\n      worker: application-data\n      start_after: [db]\n{flow}  after: &after\n    worker: path://./after\n    start_after: [ before] # real edges\n"
            );
            for crlf in [false, true] {
                let input = if crlf {
                    text.replace('\n', "\r\n")
                } else {
                    text.clone()
                };
                let wanted = if crlf {
                    expected.replace('\n', "\r\n")
                } else {
                    expected.clone()
                };
                let output = remove_container(&input, "db").unwrap().unwrap();
                assert_eq!(output, wanted);
                let parsed =
                    crate::ComposeFile::parse(&output, "/tmp/worker-compose.yaml").unwrap();
                assert_eq!(parsed.containers["after"].start_after, ["before"]);
                let original: Value = serde_yaml::from_str(&input).unwrap();
                let current: Value = serde_yaml::from_str(&output).unwrap();
                for name in ["before", "api"] {
                    assert_eq!(
                        current["containers"][name]["config_override"],
                        original["containers"][name]["config_override"]
                    );
                }
            }
        }
    }

    #[test]
    fn removing_prunes_dependencies_from_anchored_block_entries() {
        for header in ["&api", "!!map &api", "&api !!map"] {
            for (dependencies, expected) in [
                (
                    "    start_after: [db, queue] # keep edge comment\n",
                    "    start_after: [ queue] # keep edge comment\n",
                ),
                (
                    "    start_after:\n      - db\n      - queue # keep edge comment\n",
                    "    start_after:\n      - queue # keep edge comment\n",
                ),
                ("    start_after: [db]\n", ""),
                ("    start_after:\n      - db\n", ""),
            ] {
                let text = format!(
                    "# untouched\ncontainers:\n  db: &db # remove\n    worker: path://./db\n  api: {header} # keep header\n    worker: path://./api\n{dependencies}    config_override: {{unchanged: true}}\n  queue:\n    worker: path://./queue\n  consumer:\n    worker: path://./consumer\n    start_after: [queue]\n    config_override: *api\n"
                );
                let expected_text = format!(
                    "# untouched\ncontainers:\n  api: {header} # keep header\n    worker: path://./api\n{expected}    config_override: {{unchanged: true}}\n  queue:\n    worker: path://./queue\n  consumer:\n    worker: path://./consumer\n    start_after: [queue]\n    config_override: *api\n"
                );
                for crlf in [false, true] {
                    let input = if crlf {
                        text.replace('\n', "\r\n")
                    } else {
                        text.clone()
                    };
                    let wanted = if crlf {
                        expected_text.replace('\n', "\r\n")
                    } else {
                        expected_text.clone()
                    };
                    let output = remove_container(&input, "db").unwrap().unwrap();
                    assert_eq!(output, wanted);
                    let parsed =
                        crate::ComposeFile::parse(&output, "/tmp/worker-compose.yaml").unwrap();
                    assert!(!parsed.containers.contains_key("db"));
                    let expected_dependencies: Vec<String> = if expected.is_empty() {
                        vec![]
                    } else {
                        vec!["queue".into()]
                    };
                    assert_eq!(parsed.containers["api"].start_after, expected_dependencies);
                    assert_eq!(parsed.containers["consumer"].start_after, ["queue"]);
                    let yaml: Value = serde_yaml::from_str(&output).unwrap();
                    assert_eq!(
                        yaml["containers"]["consumer"]["config_override"],
                        yaml["containers"]["api"]
                    );
                    assert!(remove_container(&output, "db").unwrap().is_none());
                }
            }
        }
    }

    #[test]
    fn generated_marker_lookup_recognizes_anchored_block_entries() {
        let text = "containers:\n  # added by compose::add\n  generated: &generated # preserve\n    worker: package://generated\n    version: '1.0.0'\n  manual: !!map &manual\n    worker: path://./manual\n";
        assert!(is_generated_container(text, "generated").unwrap());
        assert!(!is_generated_container(text, "manual").unwrap());
        assert!(!is_generated_container(text, "missing").unwrap());
        assert!(!is_container_head("  api: {worker: path://./api}", "  "));
        assert!(!is_container_head("    worker: path://./api", "  "));
        assert!(!is_container_head("  # api: &api", "  "));
    }

    #[test]
    fn removing_rewrites_dependency_edges_under_commented_keys() {
        let text = "containers:\n  database:\n    worker: path://./workers/database\n  api: # the front door\n    worker: path://./workers/api\n    start_after: [database]\n  queue:\t# drained on stop\n    worker: path://./workers/queue\n    start_after: # [in boot order]\n      - database\n      - api\n";

        let out = remove_container(text, "database")
            .unwrap()
            .expect("database should be removed");

        assert_eq!(
            out,
            "containers:\n  api: # the front door\n    worker: path://./workers/api\n  queue:\t# drained on stop\n    worker: path://./workers/queue\n    start_after: # [in boot order]\n      - api\n"
        );
        crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").expect("should still load");
    }

    #[test]
    fn removing_reads_a_tab_before_a_dependency_comment_as_a_comment() {
        let text = "containers:\n  database:\n    worker: path://./workers/database\n  api:\n    worker: path://./workers/api\n    start_after:\n      - database\t# first\n";

        let out = remove_container(text, "database")
            .unwrap()
            .expect("database should be removed");

        assert_eq!(
            out,
            "containers:\n  api:\n    worker: path://./workers/api\n"
        );
        crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").expect("should still load");
    }

    #[test]
    fn still_refuses_a_file_without_the_containers_key() {
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
    fn removal_includes_fields_after_comments_without_reassigning_settings() {
        for header in ["", "&gone", "!!map &gone"] {
            for comment in [
                "# settings for gone",
                " # settings for gone",
                "  # settings for gone",
                "    # settings for gone",
            ] {
                for keep_settings in ["", "    config_override: {mode: keep}\n"] {
                    for tail in [
                        "",
                        "# next container\n  after:\n    worker: path://./after\n    config_override: {mode: after}\n",
                        "# root settings\nnamespace: default\n",
                    ] {
                        let prefix = format!(
                            "# heading\ncontainers:\n  keep:\n    worker: path://./keep\n{keep_settings}"
                        );
                        let gone = format!(
                            "  gone: {header}\n    worker: path://./gone\n{comment}\n    config_override: {{mode: gone}}\n    environment:\n# still part of gone\n      MODE: gone\n"
                        );
                        let text = format!("{prefix}{gone}{tail}");
                        let expected = format!("{prefix}{tail}");
                        crate::ComposeFile::parse(&text, "/tmp/worker-compose.yaml").unwrap();
                        for crlf in [false, true] {
                            let input = if crlf {
                                text.replace('\n', "\r\n")
                            } else {
                                text.clone()
                            };
                            let wanted = if crlf {
                                expected.replace('\n', "\r\n")
                            } else {
                                expected.clone()
                            };
                            let before: Value = serde_yaml::from_str(&input).unwrap();
                            let output = remove_container(&input, "gone").unwrap().unwrap();
                            assert_eq!(output, wanted);
                            assert_eq!(
                                remove_containers(&input, &["gone", "gone"]).unwrap(),
                                wanted
                            );
                            let after: Value = serde_yaml::from_str(&output).unwrap();
                            assert_eq!(after["containers"]["keep"], before["containers"]["keep"]);
                            if let Some(previous) = before["containers"].get("after") {
                                assert_eq!(&after["containers"]["after"], previous);
                            }
                            assert!(after["containers"].get("gone").is_none());
                            crate::ComposeFile::parse(&output, "/tmp/worker-compose.yaml").unwrap();
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn upsert_crosses_interior_comments_but_keeps_following_entry_comments() {
        for header in ["", "&api"] {
            let text = format!(
                "containers:\n  api: {header}\n    worker: package://api.workers.iii.dev/api\n# release belongs to api\n    version: '1.0.0'\n# settings also belong to api\n    config_override: {{mode: preserved}}\n# describes keep, not api\n  keep: {{worker: path://./keep}}\n"
            );
            let expected = text.replace("version: '1.0.0'", "version: \"1.1.0\"");
            for crlf in [false, true] {
                let input = if crlf {
                    text.replace('\n', "\r\n")
                } else {
                    text.clone()
                };
                let wanted = if crlf {
                    expected.replace('\n', "\r\n")
                } else {
                    expected.clone()
                };
                assert_eq!(
                    upsert_container(&input, &parse_worker("api@1.0.0").unwrap()).unwrap(),
                    Outcome::Unchanged
                );
                let output = changed(&input, &parse_worker("api@1.1.0").unwrap());
                assert_eq!(output, wanted);
                let before: Value = serde_yaml::from_str(&input).unwrap();
                let after: Value = serde_yaml::from_str(&output).unwrap();
                assert_eq!(after["containers"]["keep"], before["containers"]["keep"]);
                assert_eq!(
                    after["containers"]["api"]["config_override"],
                    before["containers"]["api"]["config_override"]
                );
                let tail =
                    "containers:\n# describes keep, not api\n  keep: {worker: path://./keep}\n";
                let tail = if crlf {
                    tail.replace('\n', "\r\n")
                } else {
                    tail.to_string()
                };
                assert_eq!(remove_container(&output, "api").unwrap().unwrap(), tail);
            }
        }
    }

    #[test]
    fn removing_finds_workers_after_unindented_comments() {
        let text = concat!(
            "containers:\n",
            "# The guided tour.\n",
            "  # Opens beside the chat.\n",
            "  onboarding:\n",
            "    worker: package://onboarding\n",
            "    version: \"latest\"\n",
            "    start_after: [state]\n",
            "\n",
            "  state:\n",
            "    worker: package://state\n",
            "    version: \"latest\"\n",
        );
        let parsed = crate::ComposeFile::parse(text, "/tmp/worker-compose.yaml").unwrap();
        assert!(parsed.containers.contains_key("onboarding"));

        let out = remove_container(text, "onboarding")
            .unwrap()
            .expect("a YAML comment must not hide a declared worker");
        assert_eq!(
            out,
            "containers:\n\n  state:\n    worker: package://state\n    version: \"latest\"\n"
        );
        crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").unwrap();
    }

    #[test]
    fn removing_ignores_comment_indentation_when_finding_entries() {
        let text = concat!(
            "containers:\n",
            "      # A comment is not a container key.\n",
            "  onboarding:\n",
            "    worker: path://./onboarding\n",
            "  state:\n",
            "    worker: path://./state\n",
        );
        let out = remove_container(text, "onboarding")
            .unwrap()
            .expect("entry indentation must come from the first container key");
        assert_eq!(out, "containers:\n  state:\n    worker: path://./state\n");
    }

    #[test]
    fn removing_finds_workers_after_comments_between_entries() {
        let text = concat!(
            "containers:\n",
            "  state:\n",
            "    worker: path://./state\n",
            "# Optional workers\n",
            "  onboarding:\n",
            "    worker: path://./onboarding\n",
            "namespace: default\n",
        );
        let out = remove_container(text, "onboarding")
            .unwrap()
            .expect("a comment between entries must not end the mapping");
        assert_eq!(
            out,
            "containers:\n  state:\n    worker: path://./state\nnamespace: default\n"
        );
        crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").unwrap();
    }

    #[test]
    fn upserting_after_unindented_comments_does_not_duplicate_existing_workers() {
        let text = concat!(
            "containers:\n",
            "# The guided tour.\n",
            "  onboarding:\n",
            "    worker: package://api.workers.iii.dev/onboarding\n",
            "    version: \"1.0.0\"\n",
        );
        assert_eq!(
            upsert_container(text, &parse_worker("onboarding@1.0.0").unwrap()).unwrap(),
            Outcome::Unchanged
        );
    }

    #[test]
    fn removing_an_entry_keeps_the_rest_of_the_file() {
        let out = remove_container(FILE, "todo")
            .unwrap()
            .expect("todo should be removed");

        assert!(!out.contains("todo:"), "todo survived: {out}");
        assert!(
            !out.contains("# the local one"),
            "its comment survived: {out}"
        );
        assert!(
            !out.contains("run: ./bin/todo-worker"),
            "its body survived: {out}"
        );
        assert!(out.contains("pi:"), "the other worker was removed: {out}");
        assert!(
            out.contains("version: \"0.1.12\""),
            "the other worker changed: {out}"
        );
    }

    #[test]
    fn removing_rewrites_inline_dependency_edges() {
        let text = r#"containers:
  database:
    worker: path://./workers/database
  api:
    worker: path://./workers/api
    start_after: ['queue', database, "state"] # keep this style
  queue:
    worker: path://./workers/queue
  state:
    worker: path://./workers/state
"#;

        let out = remove_container(text, "database")
            .unwrap()
            .expect("database should be removed");

        assert!(!out.contains("database:"), "database survived: {out}");
        assert!(
            out.contains("start_after: ['queue', \"state\"] # keep this style"),
            "surviving dependencies changed style: {out}"
        );
        crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").expect("should still load");
    }

    #[test]
    fn removing_rewrites_block_dependency_edges() {
        let text = r#"containers:
  database:
    worker: path://./workers/database
  api:
    worker: path://./workers/api
    start_after:
      - queue
      - database
      - state # keep this comment
  queue:
    worker: path://./workers/queue
  state:
    worker: path://./workers/state
"#;

        let out = remove_container(text, "database")
            .unwrap()
            .expect("database should be removed");

        assert!(!out.contains("      - database"), "edge survived: {out}");
        assert!(out.contains("      - queue"), "queue edge changed: {out}");
        assert!(
            out.contains("      - state # keep this comment"),
            "state edge changed: {out}"
        );
        crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").expect("should still load");
    }

    #[test]
    fn removing_the_last_dependency_removes_the_field() {
        let text = r#"containers:
  database:
    worker: path://./workers/database
  api:
    worker: path://./workers/api
    start_after:
      - database
    environment:
      RUST_LOG: info
"#;

        let out = remove_container(text, "database")
            .unwrap()
            .expect("database should be removed");

        assert!(!out.contains("start_after:"), "empty field survived: {out}");
        assert!(out.contains("environment:"), "next field changed: {out}");
        crate::ComposeFile::parse(&out, "/tmp/worker-compose.yaml").expect("should still load");
    }

    #[test]
    fn removing_an_unknown_entry_changes_nothing() {
        assert_eq!(remove_container(FILE, "missing").unwrap(), None);
    }

    #[test]
    fn removing_preserves_crlf_in_the_surviving_bytes() {
        let text = "containers:\r\n  keep:\r\n    worker: path://./keep\r\n  remove:\r\n    worker: path://./remove\r\n  after:\r\n    worker: path://./after\r\n";
        let expected = "containers:\r\n  keep:\r\n    worker: path://./keep\r\n  after:\r\n    worker: path://./after\r\n";

        assert_eq!(
            remove_container(text, "remove").unwrap().as_deref(),
            Some(expected)
        );
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
            start_after: vec![],
            fields: serde_yaml::Mapping::new(),
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
            start_after: vec![],
            fields: serde_yaml::Mapping::new(),
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
            start_after: vec![],
            fields: serde_yaml::Mapping::new(),
        };

        assert!(matches!(
            upsert_container(text, &new).unwrap(),
            Outcome::Replaced { .. }
        ));
    }

    #[test]
    fn explicit_vm_preserves_next_line_flow_opener_and_comments() {
        for opening in [
            "\n      &deps [",
            " # keep field comment\n      # ignore [ in a comment\n      &deps [",
            " !!seq # keep tag comment\n      &deps\n      [",
            "\n      !!seq &deps [",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("worker-compose.yaml");
            let text = format!(
                "containers:\n  db: {{worker: path://./db}}\n  queue: {{worker: path://./queue}}\n  sb: {{worker: path://./sb}}\n  link:\n    worker: path://./link\n    start_after:{opening}\n        db, # keep dependency\n    ] # keep closing\n    environment: {{TOKEN: '${{TOKEN:-review}}'}}\n    scripts:\n      build: iii call sandbox::build worker=link\n      run: iii call sandbox::start worker=link\n"
            );
            let before = crate::ComposeFile::parse(&text, &path).expect("valid input Compose");
            assert_eq!(before.containers["link"].start_after, ["db"]);

            let rewrite = explicit_vm_mode(&text, "link", "sb").unwrap().unwrap();

            let after = crate::ComposeFile::parse(&rewrite.text, &path)
                .expect("migration must stay valid Compose");
            assert_eq!(after.containers["link"].start_after, ["db", "sb"]);
            assert_eq!(
                rewrite.text,
                text.replace("    ] # keep closing", "        sb,\n    ] # keep closing")
            );
            assert_eq!(
                rewrite
                    .text
                    .lines()
                    .filter(|line| line.trim_start().starts_with(']'))
                    .count(),
                1
            );
            assert_eq!(explicit_vm_mode(&rewrite.text, "link", "sb").unwrap(), None);
        }
    }

    #[test]
    fn explicit_vm_keeps_flow_separator_comments_outside_dependencies() {
        for (value, dependencies) in [
            ("[db,# comment ]\n      queue,\n    ]", vec!["db", "queue"]),
            (
                "[# comment ]\n      db,\n      queue,\n    ]",
                vec!["db", "queue"],
            ),
            (
                "[db#suffix, '#notcomment',# comment ]\n      queue,\n    ]",
                vec!["db#suffix", "#notcomment", "queue"],
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("worker-compose.yaml");
            let text = format!(
                "containers:\n  db: {{worker: path://./db}}\n  'db#suffix': {{worker: path://./db}}\n  '#notcomment': {{worker: path://./db}}\n  queue: {{worker: path://./queue}}\n  sb: {{worker: path://./sb}}\n  link:\n    worker: path://./link\n    start_after: {value}\n    scripts:\n      build: iii call sandbox::build worker=link\n      run: iii call sandbox::start worker=link\n"
            );
            let before = crate::ComposeFile::parse(&text, &path).expect("valid input Compose");
            assert_eq!(before.containers["link"].start_after, dependencies);

            let rewrite = explicit_vm_mode(&text, "link", "sb").unwrap().unwrap();

            let after =
                crate::ComposeFile::parse(&rewrite.text, &path).expect("valid output Compose");
            let mut expected = dependencies;
            expected.push("sb");
            assert_eq!(
                after.containers["link"].start_after, expected,
                "adapter must not be inserted inside a comment"
            );
            assert_eq!(rewrite.text, text.replace("    ]\n", "      sb,\n    ]\n"));
            assert_eq!(explicit_vm_mode(&rewrite.text, "link", "sb").unwrap(), None);
        }
    }

    #[test]
    fn field_span_includes_nested_flow_values_and_ignores_quoted_delimiters() {
        let lines = [
            "    config_override: {",
            "      values: [\"quoted]#\", {inner: [nested]}], # ] }",
            "    } # end",
            "    environment: {KEEP: '${VAR}'}",
        ];
        assert_eq!(field_span(&lines, "    ", "config_override"), Some((0, 3)));
    }

    #[test]
    fn explicit_vm_preserves_multiline_flow_dependencies_and_raw_fields() {
        for (dependencies, expected) in [
            ("[\n      db,\n    ]", "[\n      db,\n      sb,\n    ]"),
            (
                "[ # opening ] is a comment\n      'db', # keep this ]\n    ] # closing",
                "[ # opening ] is a comment\n      'db', # keep this ]\n      sb,\n    ] # closing",
            ),
            (
                "[\n      \"db]#\", # quoted delimiter\n    ]",
                "[\n      \"db]#\", # quoted delimiter\n      sb,\n    ]",
            ),
            (
                "[\n      'db' # no trailing comma\n    ]",
                "[\n      'db', # no trailing comma\n      sb,\n    ]",
            ),
            (
                "[\n      'db', \"queue\"] # inline close",
                "[\n      'db', \"queue\", sb] # inline close",
            ),
            ("['db',] # trailing comma", "['db', sb] # trailing comma"),
            ("[\n    ] # empty", "[\n      sb,\n    ] # empty"),
            (
                "[\n      'db]''#',\n    ]",
                "[\n      'db]''#',\n      sb,\n    ]",
            ),
            (
                "[\n      \"db\\\"]#\",\n    ]",
                "[\n      \"db\\\"]#\",\n      sb,\n    ]",
            ),
            (
                "[\n      '${DEPENDENCY}',\n    ]",
                "[\n      '${DEPENDENCY}',\n      sb,\n    ]",
            ),
            (
                "&deps [\n      db,\n    ]",
                "&deps [\n      db,\n      sb,\n    ]",
            ),
            (
                "!!seq &deps [\n      db,\n    ]",
                "!!seq &deps [\n      db,\n      sb,\n    ]",
            ),
            ("[] # empty", "[sb] # empty"),
        ] {
            let prefix = "containers:\n  link:\n    worker: path://${APP_DIR:-./link}\n    ";
            let suffix = "\n    config_override:\n      start_after: [nested]\n      data: {items: [\"raw]#\", {key: '${VAR:-value}'}]}\n    environment: {TOKEN: '${TOKEN}'}\n    scripts: {run: iii call sandbox::start worker=link}\n";
            let text = format!("{prefix}\"start_after\": {dependencies}{suffix}");
            let rewrite = explicit_vm_mode(&text, "link", "sb").unwrap().unwrap();
            let expected = format!("{prefix}\"start_after\": {expected}{suffix}").replace(
                "    scripts: {run: iii call sandbox::start worker=link}\n",
                "    scripts:\n      run: iii call sandbox::start worker=link\n      build: iii call sandbox::build worker=link\n",
            );
            assert_eq!(rewrite.text, expected, "input: {dependencies}");
            let parsed: Value = serde_yaml::from_str(&rewrite.text).unwrap();
            let mut wanted = entry_fields(&text, "link").unwrap()["start_after"]
                .as_sequence()
                .unwrap()
                .clone();
            wanted.push(Value::from("sb"));
            assert_eq!(
                parsed["containers"]["link"]["start_after"],
                Value::Sequence(wanted)
            );
            assert_eq!(explicit_vm_mode(&rewrite.text, "link", "sb").unwrap(), None);
        }
    }

    /// Spec 7.1: the adapter in `start_after` (after `version`), then `scripts.build` and
    /// `scripts.run`; every other line and comment of the file as it was.
    #[test]
    fn the_explicit_vm_mode_is_written_into_its_container_alone() {
        let text = "# my project\ncontainers:\n  # the adapter\n  sb:\n    worker: path://./sb\n  # what onboards\n  onboarding:\n    worker: package://api.workers.iii.dev/onboarding\n    version: \"0.2.0\" # pinned\n    environment:\n      KEY: ${KEY}\n\n  api: {worker: path://./api}\n";
        let rewrite = explicit_vm_mode(text, "onboarding", "sb").unwrap().unwrap();
        assert_eq!(
            rewrite.text,
            "# my project\ncontainers:\n  # the adapter\n  sb:\n    worker: path://./sb\n  # what onboards\n  onboarding:\n    worker: package://api.workers.iii.dev/onboarding\n    version: \"0.2.0\" # pinned\n    start_after: [sb]\n    environment:\n      KEY: ${KEY}\n    scripts:\n      build: iii call sandbox::build worker=onboarding\n      run: iii call sandbox::start worker=onboarding\n\n  api: {worker: path://./api}\n"
        );
        assert!(!rewrite.wrapped);
        assert_eq!(
            explicit_vm_mode(&rewrite.text, "onboarding", "sb").unwrap(),
            None
        );
    }

    /// A command 0.26 ran inside the VM keeps running there, quoted for `sh`; the rest of a
    /// block `scripts` and `start_after`, comments included, stays line for line.
    #[test]
    fn a_command_of_its_own_moves_into_its_sandbox_start_line() {
        let text = "containers:\n  link:\n    worker: path://./link\n    start_after:\n      - state  # needs state\n    scripts:\n      # dev server\n      run: python src/dev.py --name 'x'\n      post_run: echo bye\n  state: {worker: path://./state}\n";
        let rewrite = explicit_vm_mode(text, "link", "sb").unwrap().unwrap();
        assert!(rewrite.wrapped);
        let run =
            "iii call sandbox::start worker=link 'command=python src/dev.py --name '\\''x'\\'''";
        let expected_start = "containers:\n  link:\n    worker: path://./link\n    start_after:\n      - state  # needs state\n      - sb\n    scripts:\n      # dev server\n      build: iii call sandbox::build worker=link\n";
        assert!(rewrite.text.starts_with(expected_start), "{}", rewrite.text);
        assert!(
            rewrite
                .text
                .ends_with("      post_run: echo bye\n  state: {worker: path://./state}\n"),
            "{}",
            rewrite.text
        );
        let parsed: Value = serde_yaml::from_str(&rewrite.text).unwrap();
        let scripts = &parsed["containers"]["link"]["scripts"];
        assert_eq!(scripts["run"].as_str(), Some(run));
        assert_eq!(scripts["post_run"].as_str(), Some("echo bye"));
        assert_eq!(explicit_vm_mode(&rewrite.text, "link", "sb").unwrap(), None);
    }

    /// Scripts another container aliases are not edited: that container would run this one's
    /// VM. Nothing is written, and the operator is told to split them.
    #[test]
    fn scripts_another_container_aliases_are_refused() {
        for (other, scripts) in [
            (
                "scripts: *shared",
                "scripts: &shared # shared\n      run: python main.py",
            ),
            (
                "scripts: {<<: *shared, post_run: x}",
                "scripts: &shared\n      run: python main.py",
            ),
            (
                "scripts: {run: *cmd}",
                "scripts:\n      run: &cmd python main.py",
            ),
        ] {
            let text = format!(
                "containers:\n  vm:\n    worker: path://./vm\n    {scripts}\n  other:\n    worker: path://./other\n    {other}\n"
            );
            let error = explicit_vm_mode(&text, "vm", "sb").unwrap_err();
            let anchor = if other.contains("*cmd") {
                "&cmd"
            } else {
                "&shared"
            };
            assert!(
                error
                    .to_string()
                    .contains(&format!("YAML anchor {anchor};")),
                "{other}: {error}"
            );
        }
    }

    /// Scripts that need nothing written are not checked for aliases: only `start_after` gains
    /// the adapter, and what aliases the scripts keeps them as they are.
    #[test]
    fn aliased_scripts_that_need_nothing_written_are_left_alone() {
        let text = "containers:\n  vm:\n    worker: path://./vm\n    scripts: &shared\n      build: iii call sandbox::build worker=vm\n      run: iii call sandbox::start worker=vm\n  other:\n    worker: path://./other\n    config_override: *shared\n";
        let rewrite = explicit_vm_mode(text, "vm", "sb").unwrap().unwrap();
        let parsed: Value = serde_yaml::from_str(&rewrite.text).unwrap();
        assert_eq!(
            parsed["containers"]["vm"]["start_after"],
            serde_yaml::from_str::<Value>("[sb]").unwrap()
        );
        assert!(
            rewrite.text.contains("    scripts: &shared\n"),
            "{}",
            rewrite.text
        );
    }

    /// An anchor on a block `scripts` that nothing aliases stays where it was; a `*` inside a
    /// scalar, quoted or not, is no alias.
    #[test]
    fn an_anchored_block_of_scripts_keeps_its_anchor() {
        let text = "containers:\n  vm:\n    worker: path://./vm\n    scripts: &shared # shared\n      run: python main.py\n  other:\n    worker: path://./other\n    scripts:\n      run: x\n      note: '*shared'\n      post_run: echo *shared\n";
        let rewrite = explicit_vm_mode(text, "vm", "sb").unwrap().unwrap();
        assert!(
            rewrite.text.contains("    scripts: &shared # shared\n"),
            "{}",
            rewrite.text
        );
        let parsed: Value = serde_yaml::from_str(&rewrite.text).unwrap();
        let scripts = &parsed["containers"]["vm"]["scripts"];
        assert_eq!(
            scripts["build"].as_str(),
            Some("iii call sandbox::build worker=vm")
        );
        assert_eq!(
            scripts["run"].as_str(),
            Some("iii call sandbox::start worker=vm 'command=python main.py'")
        );
        assert_eq!(
            parsed["containers"]["other"]["scripts"]["run"].as_str(),
            Some("x")
        );
    }

    /// A flow `scripts` map that starts on the line after `scripts:` is written again whole, as
    /// an inline one is: `build` lands in the map, not after it.
    #[test]
    fn a_flow_map_of_scripts_on_the_next_line_gets_its_build() {
        let text = "containers:\n  vm:\n    worker: path://./vm\n    start_after: [sb]\n    scripts:\n      {run: iii call sandbox::start worker=vm}\n";
        let rewrite = explicit_vm_mode(text, "vm", "sb").unwrap().unwrap();
        let parsed: Value = serde_yaml::from_str(&rewrite.text).unwrap();
        let scripts = &parsed["containers"]["vm"]["scripts"];
        assert_eq!(
            scripts["build"].as_str(),
            Some("iii call sandbox::build worker=vm")
        );
        assert_eq!(
            scripts["run"].as_str(),
            Some("iii call sandbox::start worker=vm")
        );
        assert_eq!(explicit_vm_mode(&rewrite.text, "vm", "sb").unwrap(), None);
    }

    /// What `sh -c` reads back is the command as it was written.
    #[cfg(unix)]
    #[test]
    fn the_wrapped_command_is_single_quoted_for_sh() {
        for command in ["python main.py", "echo 'hi' && echo \"$HOME\"", "a'b''c"] {
            let wrapped = sandbox_run_command("api", command);
            let quoted = wrapped
                .strip_prefix("iii call sandbox::start worker=api ")
                .unwrap();
            let output = std::process::Command::new("sh")
                .args(["-c", &format!("printf %s {quoted}")])
                .output()
                .unwrap();
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                format!("command={command}")
            );
            assert_eq!(
                sandbox_guest_command(&wrapped),
                Some(("api".to_string(), command.to_string()))
            );
        }
    }

    /// The 0.26 opt-in only gains `scripts.build`; an inline `scripts` becomes a block, and a
    /// `build:` without a value counts as absent.
    #[test]
    fn only_the_missing_pieces_are_added() {
        let text = "containers:\n  opt:\n    worker: path://./opt\n    start_after: [sb]\n    scripts: {run: iii sandbox run}\n";
        let rewrite = explicit_vm_mode(text, "opt", "sb").unwrap().unwrap();
        assert_eq!(
            rewrite.text,
            "containers:\n  opt:\n    worker: path://./opt\n    start_after: [sb]\n    scripts:\n      run: iii call sandbox::start worker=opt\n      build: iii call sandbox::build worker=opt\n"
        );
        let text = "containers:\n  opt:\n    worker: path://./opt\n    start_after: []\n    scripts:\n      build:\n      run: iii sandbox run --command 'x'\n";
        let rewrite = explicit_vm_mode(text, "opt", "sb").unwrap().unwrap();
        assert_eq!(
            rewrite.text,
            "containers:\n  opt:\n    worker: path://./opt\n    start_after: [sb]\n    scripts:\n      build: iii call sandbox::build worker=opt\n      run: iii call sandbox::start worker=opt command=x\n"
        );
        assert!(!rewrite.wrapped);
    }

    /// Spec 7.3 keeps the comments: an inline entry's, and an inline `scripts`', stay on their
    /// lines once written as blocks.
    #[test]
    fn the_comment_of_an_inline_entry_or_scripts_stays() {
        let text = "containers:\n  link: {worker: path://./link}  # the link\n";
        let rewrite = explicit_vm_mode(text, "link", "sb").unwrap().unwrap();
        assert_eq!(
            rewrite.text,
            "containers:\n  link: # the link\n    worker: path://./link\n    start_after: [sb]\n    scripts:\n      build: iii call sandbox::build worker=link\n      run: iii call sandbox::start worker=link\n"
        );

        let text = "containers:\n  opt:\n    worker: path://./opt\n    start_after: [sb]\n    scripts: {run: iii sandbox run} # opt-in\n";
        let rewrite = explicit_vm_mode(text, "opt", "sb").unwrap().unwrap();
        assert_eq!(
            rewrite.text,
            "containers:\n  opt:\n    worker: path://./opt\n    start_after: [sb]\n    scripts: # opt-in\n      run: iii call sandbox::start worker=opt\n      build: iii call sandbox::build worker=opt\n"
        );
        let parsed: Value = serde_yaml::from_str(&rewrite.text).unwrap();
        assert_eq!(
            parsed["containers"]["opt"]["scripts"]["build"].as_str(),
            Some("iii call sandbox::build worker=opt")
        );
    }

    /// A `#` inside quotes is text: the entry parses, and no comment is invented from it.
    #[test]
    fn a_hash_inside_quotes_is_not_a_comment() {
        assert_eq!(
            without_comment(r#"run: "echo #1"  # real"#),
            r#"run: "echo #1"  "#
        );
        assert_eq!(without_comment("run: 'it''s #1'"), "run: 'it''s #1'");
        assert_eq!(
            without_comment("run: {a: 'b #1', c: d}  # e"),
            "run: {a: 'b #1', c: d}  "
        );
        // An apostrophe inside a plain scalar opens no quote.
        let line = "  api: {worker: path://./api, scripts: {run: echo it's ok}}  # api";
        assert_eq!(
            without_comment(line),
            "  api: {worker: path://./api, scripts: {run: echo it's ok}}  "
        );
        let rewrite = explicit_vm_mode(&format!("containers:\n{line}\n"), "api", "sb")
            .unwrap()
            .unwrap();
        assert!(rewrite.text.contains("# api"), "{}", rewrite.text);
        let parsed: Value = serde_yaml::from_str(&rewrite.text).unwrap();
        assert_eq!(
            parsed["containers"]["api"]["scripts"]["run"].as_str(),
            Some("iii call sandbox::start worker=api 'command=echo it'\\''s ok'")
        );
        let text = "containers:\n  api: {worker: path://./api, scripts: {run: \"echo #1\"}}\n";
        let rewrite = explicit_vm_mode(text, "api", "sb").unwrap().unwrap();
        let parsed: Value = serde_yaml::from_str(&rewrite.text).unwrap();
        let scripts = &parsed["containers"]["api"]["scripts"];
        assert_eq!(
            scripts["run"].as_str(),
            Some("iii call sandbox::start worker=api 'command=echo #1'")
        );
        assert!(
            rewrite.text.starts_with("containers:\n  api:\n"),
            "{}",
            rewrite.text
        );

        let text = "containers:\n  opt:\n    worker: path://./opt\n    start_after: [sb]\n    scripts: {run: \"iii sandbox run --command 'a #1'\"}\n";
        let rewrite = explicit_vm_mode(text, "opt", "sb").unwrap().unwrap();
        assert!(rewrite.text.contains("    scripts:\n"), "{}", rewrite.text);
        assert!(!rewrite.text.contains("#1\"}"), "{}", rewrite.text);
    }

    /// Expand in the daemon, then cross the host shell with one opaque guest argument.
    #[cfg(unix)]
    #[test]
    fn f10_wrapped_command_survives_the_expansion() {
        let path = std::path::Path::new("worker-compose.yaml");
        let commands = [
            r#"node app.js --name "${APP_NAME:-shop}""#,
            "printf '%s\\n' \"${APP_NAME}\" '${OTHER}' $${DEFERRED} $$HOME",
            "printf '%s' \"${APP_NAME}\" \\\n  \"${OTHER}\"\n# guest comment: café 🦀",
            r#"printf '%s' "${APP_NAME}" \\"${OTHER}""#,
        ];
        let values = [
            "Bob's shop",
            "x'; echo INJECTED-ON-HOST; '",
            "double\"quote \\\\ café 🦀\nline",
            "$(echo INJECTED-ON-HOST) `echo INJECTED-ON-HOST` $HOME ${OTHER}",
        ];
        for command in commands {
            let mut scripts = Mapping::new();
            scripts.insert(Value::from("run"), Value::from(command));
            let text = format!(
                "containers:\n  dev:\n    worker: path://./dev\n{}",
                render_field("scripts", &Value::Mapping(scripts), "    ").unwrap()
            );
            let rewrite = explicit_vm_mode(&text, "dev", "sb").unwrap().unwrap();
            // The declaration, not expanded secrets, is what migration persists.
            let raw: Value = serde_yaml::from_str(&rewrite.text).unwrap();
            assert!(
                raw["containers"]["dev"]["scripts"]["run"]
                    .as_str()
                    .unwrap()
                    .contains("${APP_NAME")
            );
            assert_eq!(explicit_vm_mode(&rewrite.text, "dev", "sb").unwrap(), None);
            for value in values {
                let lookup = |name: &str| match name {
                    "APP_NAME" => Some(value.to_string()),
                    "OTHER" => Some("second ' \" \\ $${literal}".into()),
                    _ => None,
                };
                let expected = crate::interpolate::expand(command, path, &lookup).unwrap();
                let mut expanded: Value = serde_yaml::from_str(&rewrite.text).unwrap();
                crate::interpolate::expand_tree(&mut expanded, path, &lookup).unwrap();
                let run = expanded["containers"]["dev"]["scripts"]["run"]
                    .as_str()
                    .unwrap();
                // A function stub only records argv. It never invokes an installed iii binary.
                let output = std::process::Command::new("sh")
                    .args([
                        "-c",
                        &format!("iii() {{ printf '%s\\000' \"$@\"; }}; {run}"),
                    ])
                    .output()
                    .unwrap();
                let expected = [
                    b"call\0sandbox::start\0worker=dev\0command=".as_slice(),
                    expected.as_bytes(),
                    b"\0",
                ]
                .concat();
                assert!(
                    output.status.success(),
                    "{run}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(
                    output.stdout, expected,
                    "guest argument changed or host executed an injection: {run}"
                );
            }
        }
    }

    /// The forms written before `iii call` streamed become their `iii call` lines, the guest
    /// command of a `path://` worker kept; the rewrite then has nothing left to do.
    #[test]
    fn the_iii_sandbox_forms_become_iii_call_lines() {
        let text = "containers:\n  link:\n    worker: path://./link\n    start_after: [sb]\n    scripts:\n      build: iii sandbox build\n      run: iii sandbox run --command 'node .'\n";
        let rewrite = explicit_vm_mode(text, "link", "sb").unwrap().unwrap();
        assert!(!rewrite.wrapped);
        assert_eq!(
            rewrite.text,
            "containers:\n  link:\n    worker: path://./link\n    start_after: [sb]\n    scripts:\n      build: iii call sandbox::build worker=link\n      run: iii call sandbox::start worker=link 'command=node .'\n"
        );
        assert_eq!(explicit_vm_mode(&rewrite.text, "link", "sb").unwrap(), None);
    }
}
