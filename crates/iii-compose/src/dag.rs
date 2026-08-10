// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Dependency graph validation and ordering.
//!
//! Traversal always follows declaration order, so the same file always reports
//! the same cycle and produces the same start order — a diagnostic that moves
//! between runs is a diagnostic nobody trusts.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::{
    config::ComposeFile,
    error::{ComposeError, Result},
};

/// Rejects dependencies on undeclared containers and any dependency cycle.
pub fn validate_dependencies(file: &ComposeFile) -> Result<()> {
    for (key, container) in &file.containers {
        for dependency in &container.depends_on {
            if !file.containers.contains_key(dependency) {
                return Err(ComposeError::UnknownDependency {
                    container: key.clone(),
                    dependency: dependency.clone(),
                });
            }
        }
    }
    detect_cycle(file)
}

fn detect_cycle(file: &ComposeFile) -> Result<()> {
    let mut settled: HashSet<&str> = HashSet::new();
    let mut on_path: Vec<&str> = Vec::new();

    for key in file.containers.keys() {
        visit(file, key.as_str(), &mut settled, &mut on_path)?;
    }
    Ok(())
}

fn visit<'a>(
    file: &'a ComposeFile,
    key: &'a str,
    settled: &mut HashSet<&'a str>,
    on_path: &mut Vec<&'a str>,
) -> Result<()> {
    if settled.contains(key) {
        return Ok(());
    }
    if let Some(start) = on_path.iter().position(|entry| *entry == key) {
        let mut cycle: Vec<&str> = on_path[start..].to_vec();
        cycle.push(key);
        return Err(ComposeError::DependencyCycle {
            path: cycle.join(" -> "),
        });
    }

    on_path.push(key);
    // `validate_dependencies` rejects unknown edges before we get here, so a
    // missing entry can only mean the graph was mutated between the two.
    if let Some(container) = file.containers.get(key) {
        for dependency in &container.depends_on {
            let dependency = file
                .containers
                .get_key_value(dependency)
                .map(|(stored, _)| stored.as_str())
                .unwrap_or(dependency.as_str());
            visit(file, dependency, settled, on_path)?;
        }
    }
    on_path.pop();
    settled.insert(key);
    Ok(())
}

/// Start order: every dependency precedes its dependents. Ready containers are
/// emitted in declaration order.
pub fn topo_order(file: &ComposeFile) -> Result<Vec<String>> {
    let mut pending: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

    for (key, container) in &file.containers {
        pending.insert(key.as_str(), container.depends_on.len());
        for dependency in &container.depends_on {
            dependents
                .entry(dependency.as_str())
                .or_default()
                .push(key.as_str());
        }
    }

    let mut ready: VecDeque<&str> = file
        .containers
        .keys()
        .filter(|key| pending.get(key.as_str()).copied() == Some(0))
        .map(|key| key.as_str())
        .collect();

    let mut order = Vec::with_capacity(file.containers.len());
    while let Some(key) = ready.pop_front() {
        order.push(key.to_string());
        // Declaration order again: dependents were collected in that order, so
        // draining them in sequence keeps the output reproducible.
        for dependent in dependents.get(key).cloned().unwrap_or_default() {
            if let Some(count) = pending.get_mut(dependent) {
                *count -= 1;
                if *count == 0 {
                    ready.push_back(dependent);
                }
            }
        }
    }

    if order.len() != file.containers.len() {
        // Unreachable through `ComposeFile::parse`, which rejects cycles first.
        return Err(ComposeError::DependencyCycle {
            path: "unresolved dependencies".to_string(),
        });
    }
    Ok(order)
}

/// `key` plus everything it transitively depends on — what `up <key>` has to
/// start for `key` to be usable.
pub fn dependency_closure(file: &ComposeFile, key: &str) -> HashSet<String> {
    let mut closure = HashSet::new();
    let mut queue = VecDeque::from([key.to_string()]);

    while let Some(current) = queue.pop_front() {
        if !closure.insert(current.clone()) {
            continue;
        }
        if let Some(container) = file.containers.get(&current) {
            for dependency in &container.depends_on {
                queue.push_back(dependency.clone());
            }
        }
    }
    closure
}

/// Local containers that must stop before `key` can stop: its transitive
/// dependents, in the order they have to stop in.
///
/// Discovery order is not stop order. A breadth-first walk of `db <- api <-
/// web` reaches `api` before `web`, and stopping in that order leaves `web`
/// calling a dependency that is already gone — the state teardown exists to
/// prevent. Reversing the start order applies the rule a whole-project
/// teardown already follows to this subgraph, instead of maintaining a second
/// ordering rule beside it.
pub fn transitive_dependents(file: &ComposeFile, key: &str) -> Vec<String> {
    let mut discovered = HashSet::new();
    let mut queue = VecDeque::from([key.to_string()]);

    while let Some(current) = queue.pop_front() {
        for (candidate, container) in &file.containers {
            if !container.depends_on.iter().any(|dep| dep == &current) {
                continue;
            }
            if discovered.insert(candidate.clone()) {
                queue.push_back(candidate.clone());
            }
        }
    }

    // `ComposeFile::parse` rejects cycles, so the sort cannot fail here. The
    // fallback keeps this function infallible rather than describing a path
    // callers can reach; declaration order is at least reproducible.
    let Ok(start_order) = topo_order(file) else {
        return file
            .containers
            .keys()
            .filter(|candidate| discovered.contains(*candidate))
            .cloned()
            .collect();
    };

    start_order
        .into_iter()
        .rev()
        .filter(|candidate| discovered.contains(candidate))
        .collect()
}
