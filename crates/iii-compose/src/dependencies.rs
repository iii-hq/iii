// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Select instances for generated dependencies without renaming declarations.

use std::collections::{BTreeMap, BTreeSet};

use futures::StreamExt;

use crate::{
    ComposeFile, WorkerSource,
    edit::{NewContainer, Source},
    error::{ComposeError, Result},
    registry::{self, Node},
};

type Identity = (String, String);

#[derive(Clone)]
struct Package {
    registry: String,
    node: Node,
    /// A fresh resolution of a range cannot identify an already running version.
    exact_version: bool,
}

impl Package {
    fn identity(&self) -> Identity {
        (
            self.registry.clone(),
            self.node.canonical_name().to_string(),
        )
    }
}

struct Candidate {
    declaration: NewContainer,
    package: Option<Package>,
}

#[derive(Debug)]
pub(crate) struct Alias {
    pub container: String,
    pub reference: String,
    pub canonical: String,
}

#[derive(Debug)]
pub(crate) struct Plan {
    pub containers: Vec<NewContainer>,
    pub aliases: Vec<Alias>,
}

/// Resolve before editing or downloading. Existing package declarations must be
/// inspected too: an old alias may be present only in the compose file.
pub(crate) async fn plan(file: &ComposeFile, asked: &[NewContainer]) -> Result<Plan> {
    let paths: BTreeSet<String> = file
        .containers
        .iter()
        .filter(|(_, container)| matches!(container.worker, WorkerSource::Path { .. }))
        .map(|(key, _)| key.clone())
        .chain(
            asked
                .iter()
                .filter(|worker| matches!(worker.source, Source::Path { .. }))
                .map(|worker| worker.key.clone()),
        )
        .collect();
    let expansions = futures::stream::iter(asked.iter().cloned().map(|worker| {
        let paths = paths.clone();
        async move {
            let Source::Package { reference, version } = &worker.source else {
                return Ok(vec![Candidate {
                    declaration: worker.clone(),
                    package: None,
                }]);
            };
            let graph =
                registry::resolve_graph(&worker.key, reference, version.as_deref().unwrap_or("*"))
                    .await?;
            let nodes: BTreeMap<_, _> = graph
                .nodes
                .iter()
                .map(|node| (node.name.clone(), node.clone()))
                .collect();
            let mut root = worker.clone();
            root.fields.clear();
            root.start_after.clear();
            let expanded = crate::daemon::expand_graph(&root, reference, graph, &paths)?;
            let (registry, _) = registry::split_reference(reference);
            expanded
                .into_iter()
                .map(|declaration| {
                    let node = nodes.get(&declaration.key).cloned().ok_or_else(|| {
                        conflict(
                            &declaration.key,
                            "the registry graph does not contain the requested worker",
                        )
                    })?;
                    Ok(Candidate {
                        declaration,
                        package: Some(Package {
                            registry: registry.clone(),
                            node,
                            exact_version: true,
                        }),
                    })
                })
                .collect::<Result<Vec<_>>>()
        }
    }))
    .buffered(4)
    .collect::<Vec<Result<Vec<Candidate>>>>()
    .await;
    let expanded = expansions
        .into_iter()
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let registries: BTreeSet<String> = expanded
        .iter()
        .filter_map(|candidate| candidate.package.as_ref())
        .map(|package| package.registry.clone())
        .collect();
    let requested: BTreeSet<&str> = asked.iter().map(|worker| worker.key.as_str()).collect();
    let existing_requests: Vec<_> = file
        .containers
        .iter()
        .filter_map(|(key, container)| {
            if requested.contains(key.as_str()) {
                return None;
            }
            let WorkerSource::Package { reference } = &container.worker else {
                return None;
            };
            let (registry, _) = registry::split_reference(reference);
            if !registries.contains(registry.as_str()) {
                return None;
            }
            Some((
                key.clone(),
                reference.clone(),
                container.version.clone().unwrap_or_else(|| "*".into()),
                registry,
            ))
        })
        .collect();
    let existing = futures::stream::iter(existing_requests.into_iter().map(
        |(key, reference, range, registry)| async move {
            let node = registry::resolve_package(&key, &reference, &range).await?;
            let exact_version = range == node.version;
            Ok((
                key,
                Package {
                    registry,
                    node,
                    exact_version,
                },
            ))
        },
    ))
    .buffered(4)
    .collect::<Vec<Result<_>>>()
    .await
    .into_iter()
    .collect::<Result<BTreeMap<_, _>>>()?;
    plan_resolved(
        asked,
        expanded,
        &existing,
        &file.containers.keys().cloned().collect(),
    )
}

fn conflict(name: &str, reason: &str) -> ComposeError {
    ComposeError::InvalidWorkerSpec {
        spec: name.to_string(),
        reason: reason.to_string(),
    }
}

fn plan_resolved(
    asked: &[NewContainer],
    expanded: Vec<Candidate>,
    existing: &BTreeMap<String, Package>,
    existing_keys: &BTreeSet<String>,
) -> Result<Plan> {
    let requested: BTreeMap<&str, &NewContainer> = asked
        .iter()
        .map(|worker| (worker.key.as_str(), worker))
        .collect();
    let mut owners: BTreeMap<Identity, BTreeMap<&str, &Package>> = BTreeMap::new();
    for (key, package) in existing {
        owners
            .entry(package.identity())
            .or_default()
            .insert(key, package);
    }
    let mut automatic: BTreeMap<Identity, Vec<&Package>> = BTreeMap::new();
    for candidate in &expanded {
        let Some(package) = &candidate.package else {
            continue;
        };
        if requested.contains_key(candidate.declaration.key.as_str()) {
            let previous = owners
                .entry(package.identity())
                .or_default()
                .insert(&candidate.declaration.key, package);
            if let Some(previous) = previous
                && !previous.node.same_release(&package.node)
            {
                return Err(conflict(
                    &candidate.declaration.key,
                    "the requested container resolves to different versions, artifacts, or default configurations in the dependency graphs",
                ));
            }
        } else {
            automatic
                .entry(package.identity())
                .or_default()
                .push(package);
        }
    }

    let mut selected = BTreeMap::new();
    for (identity, packages) in &automatic {
        let release = &packages[0].node;
        if packages
            .iter()
            .any(|package| !release.same_release(&package.node))
        {
            return Err(conflict(
                &identity.1,
                &format!(
                    "aliases of '{}' resolve to different versions, artifacts, or default configurations. \
                 Align the dependency versions before adding these workers",
                    identity.1,
                ),
            ));
        }
        let choices = owners.get(identity);
        // Preserve the existing add policy for an unchanged package name.
        // Alias convergence needs a release match; an ordinary declared
        // dependency keeps its pin until compose::update changes it.
        let aliases_used = packages
            .iter()
            .any(|package| package.node.alias_of.is_some())
            || choices.is_some_and(|choices| {
                choices
                    .values()
                    .any(|package| package.node.alias_of.is_some())
            });
        if !aliases_used
            && let Some(declared) = existing.get(&release.name)
            && declared.identity() == *identity
        {
            selected.insert(identity.clone(), release.name.clone());
            continue;
        }
        let compatible: Vec<&str> = choices
            .into_iter()
            .flat_map(|choices| choices.iter())
            .filter(|(_, package)| package.exact_version && release.same_release(&package.node))
            .map(|(key, _)| *key)
            .collect();
        let key = match compatible.as_slice() {
            [key] => (*key).to_string(),
            [] if choices.is_some_and(|choices| !choices.is_empty()) => {
                return Err(conflict(
                    &identity.1,
                    &format!(
                        "dependency '{}' at version '{}' conflicts with the declared instances. \
                 Pin matching exact versions and artifacts before adding this worker",
                        identity.1, release.version,
                    ),
                ));
            }
            [] => {
                let key = identity.1.clone();
                if existing_keys.contains(&key) || requested.contains_key(key.as_str()) {
                    return Err(conflict(
                        &key,
                        "the canonical dependency name is already used by another container. Keep that declaration and choose an unambiguous dependency",
                    ));
                }
                key
            }
            _ => {
                return Err(conflict(
                    &identity.1,
                    &format!(
                        "dependency '{}' matches multiple declared containers: {}. \
                 Keep their settings and select a single dependency instance before adding this worker",
                        identity.1,
                        compatible.join(", "),
                    ),
                ));
            }
        };
        selected.insert(identity.clone(), key);
    }

    // Edges use registry names, while start_after uses container keys. The
    // registry is part of this lookup so a custom registry cannot borrow a
    // declaration from another registry with the same worker name.
    let mut targets = BTreeMap::new();
    for candidate in &expanded {
        let Some(package) = &candidate.package else {
            continue;
        };
        let key = if requested.contains_key(candidate.declaration.key.as_str()) {
            candidate.declaration.key.clone()
        } else {
            selected[&package.identity()].clone()
        };
        targets.insert(
            (package.registry.clone(), candidate.declaration.key.clone()),
            key,
        );
    }

    let mut containers: BTreeMap<String, NewContainer> = BTreeMap::new();
    let mut order = Vec::new();
    let mut aliases = Vec::new();
    for candidate in expanded {
        let mut declaration = candidate.declaration;
        let explicit = requested.get(declaration.key.as_str()).copied();
        if let Some(package) = candidate.package {
            let original_key = declaration.key.clone();
            let key = targets[&(package.registry.clone(), original_key.clone())].clone();
            if let Some(canonical) = &package.node.alias_of {
                aliases.push(Alias {
                    container: key.clone(),
                    reference: format!(
                        "{}/{}",
                        package.registry.trim_start_matches("https://"),
                        package.node.name
                    ),
                    canonical: canonical.clone(),
                });
            }
            if explicit.is_none()
                && (existing.contains_key(&key) || requested.contains_key(key.as_str()))
            {
                // Reuse an owner without changing its configuration, source,
                // version, or startup dependencies.
                continue;
            }
            declaration.key = key;
            if explicit.is_none() {
                declaration.source = Source::Package {
                    reference: format!(
                        "{}/{}",
                        package.registry.trim_start_matches("https://"),
                        package.node.canonical_name()
                    ),
                    version: Some(package.node.version.clone()),
                };
            }
            for dependency in &mut declaration.start_after {
                if let Some(target) = targets.get(&(package.registry.clone(), dependency.clone())) {
                    *dependency = target.clone();
                }
                if dependency == &declaration.key {
                    return Err(ComposeError::DependencyCycle {
                        path: format!("{original_key} -> {}", declaration.key),
                    });
                }
            }
        }
        if let Some(explicit) = explicit {
            declaration.fields = explicit.fields.clone();
            declaration
                .start_after
                .extend(explicit.start_after.iter().cloned());
        }
        declaration.start_after.sort();
        declaration.start_after.dedup();
        if let Some(previous) = containers.get_mut(&declaration.key) {
            if previous.source != declaration.source || previous.fields != declaration.fields {
                return Err(conflict(
                    &declaration.key,
                    "the dependency resolves to conflicting sources, versions, or settings",
                ));
            }
            previous.start_after.extend(declaration.start_after);
            previous.start_after.sort();
            previous.start_after.dedup();
        } else {
            order.push(declaration.key.clone());
            containers.insert(declaration.key.clone(), declaration);
        }
    }

    // A reused declaration owns its dependency tree. Do not install nodes that
    // were reachable only through the registry's version of that declaration.
    let mut reachable = BTreeSet::new();
    let mut visit: Vec<String> = asked.iter().map(|worker| worker.key.clone()).collect();
    while let Some(key) = visit.pop() {
        if reachable.insert(key.clone())
            && let Some(container) = containers.get(&key)
        {
            visit.extend(container.start_after.iter().cloned());
        }
    }
    order.retain(|key| reachable.contains(key));
    containers.retain(|key, _| reachable.contains(key));
    aliases.retain(|alias| reachable.contains(&alias.container));
    let mut sorted = Vec::new();
    while !order.is_empty() {
        let Some(index) = order.iter().position(|key| {
            containers[key]
                .start_after
                .iter()
                .all(|dep| !containers.contains_key(dep))
        }) else {
            return Err(ComposeError::DependencyCycle {
                path: order.join(" -> "),
            });
        };
        let key = order.remove(index);
        if let Some(container) = containers.remove(&key) {
            sorted.push(container);
        }
    }
    Ok(Plan {
        containers: sorted,
        aliases,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(name: &str, alias_of: Option<&str>, dependencies: &[&str]) -> Candidate {
        Candidate {
            declaration: NewContainer {
                key: name.into(),
                source: Source::Package {
                    reference: format!("api.workers.iii.dev/{name}"),
                    version: Some("1.0.0".into()),
                },
                start_after: dependencies.iter().map(|name| (*name).into()).collect(),
                fields: Default::default(),
            },
            package: Some(Package {
                registry: registry::DEFAULT_REGISTRY.into(),
                exact_version: true,
                node: Node {
                    name: name.into(),
                    alias_of: alias_of.map(str::to_string),
                    version: "1.0.0".into(),
                    kind: "binary".into(),
                    artifact_digest: Some("a".repeat(64)),
                    ..Default::default()
                },
            }),
        }
    }

    fn request(candidate: &Candidate) -> NewContainer {
        let mut declaration = candidate.declaration.clone();
        declaration.start_after.clear();
        declaration
    }

    #[test]
    fn alias_and_canonical_dependencies_share_one_container() {
        let a = candidate("api", None, &["console"]);
        let b = candidate("jobs", None, &["shell"]);
        let asked = vec![request(&a), request(&b)];
        let plan = plan_resolved(
            &asked,
            vec![
                candidate("console", Some("shell"), &[]),
                a,
                candidate("shell", None, &[]),
                b,
            ],
            &BTreeMap::new(),
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            plan.containers
                .iter()
                .map(|c| (c.key.as_str(), c.start_after.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("shell", vec![]),
                ("api", vec!["shell".into()]),
                ("jobs", vec!["shell".into()]),
            ]
        );
        assert_eq!(plan.aliases[0].container, "shell");
    }

    #[test]
    fn existing_instance_is_reused_without_editing_its_settings_or_dependencies() {
        let a = candidate("api", None, &["console"]);
        let asked = vec![request(&a)];
        let existing = BTreeMap::from([(
            "my-shell".into(),
            candidate("shell", None, &[]).package.unwrap(),
        )]);
        let plan = plan_resolved(
            &asked,
            vec![
                candidate("helper", None, &[]),
                candidate("console", Some("shell"), &["helper"]),
                a,
            ],
            &existing,
            &existing.keys().cloned().collect(),
        )
        .unwrap();
        assert_eq!(plan.containers.len(), 1);
        assert_eq!(plan.containers[0].start_after, vec!["my-shell"]);
    }

    #[test]
    fn existing_alias_keeps_its_container_key() {
        let a = candidate("api", None, &["shell"]);
        let asked = vec![request(&a)];
        let existing = BTreeMap::from([(
            "console".into(),
            candidate("console", Some("shell"), &[]).package.unwrap(),
        )]);
        let plan = plan_resolved(
            &asked,
            vec![candidate("shell", None, &[]), a],
            &existing,
            &existing.keys().cloned().collect(),
        )
        .unwrap();
        assert_eq!(plan.containers[0].start_after, vec!["console"]);
    }

    #[test]
    fn explicitly_requested_instances_keep_distinct_configurations() {
        let mut a = candidate("console", Some("shell"), &[]);
        a.declaration
            .fields
            .insert("config_name".into(), "console-config".into());
        let mut b = candidate("shell", None, &[]);
        b.declaration
            .fields
            .insert("config_name".into(), "shell-config".into());
        let asked = vec![request(&a), request(&b)];
        let plan = plan_resolved(&asked, vec![a, b], &BTreeMap::new(), &BTreeSet::new()).unwrap();
        assert_eq!(plan.containers, asked);
    }

    #[test]
    fn generated_alias_uses_an_explicit_canonical_request() {
        let a = candidate("api", None, &["console"]);
        let shell = candidate("shell", None, &[]);
        let asked = vec![request(&a), request(&shell)];
        let plan = plan_resolved(
            &asked,
            vec![candidate("console", Some("shell"), &[]), a, shell],
            &BTreeMap::new(),
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            plan.containers
                .iter()
                .map(|c| c.key.as_str())
                .collect::<Vec<_>>(),
            vec!["shell", "api"]
        );
    }

    #[test]
    fn ambiguous_existing_instances_are_not_merged() {
        let a = candidate("api", None, &["console"]);
        let asked = vec![request(&a)];
        let package = candidate("shell", None, &[]).package.unwrap();
        let existing = BTreeMap::from([
            ("shell-one".into(), package.clone()),
            ("shell-two".into(), package),
        ]);
        let error = plan_resolved(
            &asked,
            vec![candidate("console", Some("shell"), &[]), a],
            &existing,
            &existing.keys().cloned().collect(),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("multiple declared containers: shell-one, shell-two")
        );
    }

    #[test]
    fn different_alias_versions_are_refused() {
        let a = candidate("api", None, &["console", "shell"]);
        let asked = vec![request(&a)];
        let mut shell = candidate("shell", None, &[]);
        shell.package.as_mut().unwrap().node.version = "2.0.0".into();
        let error = plan_resolved(
            &asked,
            vec![candidate("console", Some("shell"), &[]), shell, a],
            &BTreeMap::new(),
            &BTreeSet::new(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("different versions, artifacts"));
    }

    #[test]
    fn a_matching_version_with_a_different_artifact_is_refused() {
        let a = candidate("api", None, &["console"]);
        let asked = vec![request(&a)];
        let mut package = candidate("shell", None, &[]).package.unwrap();
        package.node.artifact_digest = Some("b".repeat(64));
        let existing = BTreeMap::from([("shell".into(), package)]);
        let error = plan_resolved(
            &asked,
            vec![candidate("console", Some("shell"), &[]), a],
            &existing,
            &existing.keys().cloned().collect(),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("conflicts with the declared instances")
        );
    }

    #[test]
    fn an_existing_version_range_is_not_treated_as_a_known_running_version() {
        let a = candidate("api", None, &["console"]);
        let asked = vec![request(&a)];
        let mut package = candidate("shell", None, &[]).package.unwrap();
        package.exact_version = false;
        let existing = BTreeMap::from([("shell".into(), package)]);
        let error = plan_resolved(
            &asked,
            vec![candidate("console", Some("shell"), &[]), a],
            &existing,
            &existing.keys().cloned().collect(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("Pin matching exact versions"));
    }

    #[test]
    fn an_ordinary_declared_dependency_keeps_its_pin_on_add() {
        let a = candidate("api", None, &["shell"]);
        let asked = vec![request(&a)];
        let mut package = candidate("shell", None, &[]).package.unwrap();
        package.node.version = "0.9.0".into();
        let existing = BTreeMap::from([("shell".into(), package)]);
        let plan = plan_resolved(
            &asked,
            vec![candidate("shell", None, &[]), a],
            &existing,
            &existing.keys().cloned().collect(),
        )
        .unwrap();
        assert_eq!(plan.containers.len(), 1);
        assert_eq!(plan.containers[0].start_after, vec!["shell"]);
    }

    #[test]
    fn a_requested_root_cannot_hide_conflicting_dependency_artifacts() {
        let a = candidate("api", None, &["shell"]);
        let shell = candidate("shell", None, &[]);
        let asked = vec![request(&a), request(&shell)];
        let mut dependency = candidate("shell", None, &[]);
        dependency.package.as_mut().unwrap().node.artifact_digest = Some("b".repeat(64));
        let error = plan_resolved(
            &asked,
            vec![dependency, a, shell],
            &BTreeMap::new(),
            &BTreeSet::new(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("different versions, artifacts"));
    }

    #[test]
    fn containers_from_another_registry_do_not_satisfy_a_dependency() {
        let a = candidate("api", None, &["console"]);
        let asked = vec![request(&a)];
        let mut package = candidate("shell", None, &[]).package.unwrap();
        package.registry = "https://custom.example".into();
        let existing = BTreeMap::from([("custom-shell".into(), package)]);
        let plan = plan_resolved(
            &asked,
            vec![candidate("console", Some("shell"), &[]), a],
            &existing,
            &existing.keys().cloned().collect(),
        )
        .unwrap();
        assert_eq!(plan.containers[0].key, "shell");
        assert_eq!(plan.containers[1].start_after, vec!["shell"]);
    }

    #[test]
    fn a_generated_canonical_name_cannot_replace_a_local_container() {
        let a = candidate("api", None, &["console"]);
        let asked = vec![request(&a)];
        let error = plan_resolved(
            &asked,
            vec![candidate("console", Some("shell"), &[]), a],
            &BTreeMap::new(),
            &BTreeSet::from(["shell".into()]),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("already used by another container")
        );
    }

    #[test]
    fn local_dependencies_keep_their_original_edges() {
        let a = candidate("api", None, &["console"]);
        let asked = vec![request(&a)];
        let plan = plan_resolved(
            &asked,
            vec![a],
            &BTreeMap::new(),
            &BTreeSet::from(["console".into()]),
        )
        .unwrap();
        assert_eq!(plan.containers[0].start_after, vec!["console"]);
    }

    #[test]
    fn a_cycle_created_by_alias_resolution_is_refused() {
        let a = candidate("api", None, &["console"]);
        let asked = vec![request(&a)];
        let error = plan_resolved(
            &asked,
            vec![candidate("console", Some("api"), &[]), a],
            &BTreeMap::new(),
            &BTreeSet::new(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "DEPENDENCY_CYCLE");
    }
}
