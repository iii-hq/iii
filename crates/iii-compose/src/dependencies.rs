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
    /// The worker a request named, rather than a node reached through the
    /// graph of another request.
    root: bool,
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
    /// Exact registry selections keyed by the final container name.
    pub selected_versions: BTreeMap<String, String>,
}

/// How a plan treats dependencies the compose file already declares.
#[derive(Debug, Clone, Default)]
pub(crate) enum Policy {
    /// `compose::add`: a declared dependency keeps its declaration and pin;
    /// the new worker only reuses its container.
    #[default]
    Add,
    /// `compose::update`: declared dependencies follow the resolved graph.
    /// Containers written by `compose::add` (`generated`) move to the exact
    /// resolved version; an operator's selector is kept while the lock moves.
    Update { generated: BTreeSet<String> },
}

/// The declarations an update may move, as written in the compose file.
struct UpdateScope {
    declared: BTreeMap<String, Source>,
    generated: BTreeSet<String>,
}

impl UpdateScope {
    /// How a declared dependency follows the release its graph selected: the
    /// declaration to write and the exact version to lock, if the lock moves.
    fn follow(
        &self,
        key: &str,
        current: &Package,
        resolved: &Package,
    ) -> Result<(Source, Option<String>)> {
        let version = &resolved.node.version;
        let Some(Source::Package {
            reference,
            version: selector,
        }) = self.declared.get(key)
        else {
            return Err(conflict(
                key,
                "the declared dependency is not a registry package",
            ));
        };
        let selector_text = selector.as_deref().unwrap_or("*");
        let exact = semver::Version::parse(selector_text).is_ok();
        // compose::add pins every dependency it writes to an exact version. A
        // generated container whose selector now reads as a tag or a range
        // was changed by the operator, and that choice is kept below.
        if exact && self.generated.contains(key) {
            return Ok((
                Source::Package {
                    reference: reference.clone(),
                    version: Some(version.clone()),
                },
                Some(version.clone()),
            ));
        }

        let declared = Source::Package {
            reference: reference.clone(),
            version: selector.clone(),
        };
        let unchanged = current.node.same_release(&resolved.node);
        let selector = selector_text;
        if exact {
            if unchanged {
                return Ok((declared, None));
            }
            let reason = if current.node.version == *version {
                format!(
                    "the compose file pins '{key}' to {selector}, but the updated dependency graph needs \
                     another artifact or default configuration of {version}. Run compose::update worker={key} first"
                )
            } else {
                format!(
                    "the compose file pins '{key}' to {selector}, but the updated dependency graph needs {version}. \
                     Run compose::update worker={key}@{version} first"
                )
            };
            return Err(conflict(key, &reason));
        }
        let requirement = semver::VersionReq::parse(selector).ok();
        // A prerelease is compared as the release it leads to, so `^1.0`
        // accepts `1.1.0-rc.1` and still refuses `2.0.0-rc.1`.
        if let Some(requirement) = &requirement
            && let Ok(parsed) = semver::Version::parse(version)
            && !requirement.matches(&semver::Version::new(
                parsed.major,
                parsed.minor,
                parsed.patch,
            ))
        {
            return Err(conflict(
                key,
                &format!(
                    "the compose file declares '{key}' as {selector}, but the updated dependency graph needs {version}. \
                     Change the selector or run compose::update worker={key}@{version} first"
                ),
            ));
        }
        if unchanged {
            return Ok((declared, None));
        }
        if requirement.is_none() {
            crate::report::daemon_line(
                &format!(
                    "{key}: locked to {version}, the release the updated dependency graph needs; \
                     its declared selector '{selector}' is kept"
                ),
                true,
            );
        }
        Ok((declared, Some(version.clone())))
    }
}

/// The package a reference names: its last path segment.
fn package_name(reference: &str) -> &str {
    reference
        .rsplit_once('/')
        .map_or(reference, |(_, name)| name)
}

/// Resolve before editing or downloading. Existing package declarations must be
/// inspected too: an old alias may be present only in the compose file.
pub(crate) async fn plan(
    file: &ComposeFile,
    asked: &[NewContainer],
    policy: &Policy,
) -> Result<Plan> {
    plan_with(
        file,
        asked,
        policy,
        |key, reference, range| async move {
            registry::resolve_graph(&key, &reference, &range).await
        },
        |key, reference, range| async move {
            registry::resolve_node(&key, &reference, &range).await
        },
    )
    .await
}

/// [`plan`] with injectable registry lookups, so the planning rules can be
/// exercised without a registry. Both lookups take `(container, reference,
/// range)`.
pub(crate) async fn plan_with<G, GFut, N, NFut>(
    file: &ComposeFile,
    asked: &[NewContainer],
    policy: &Policy,
    resolve_graph: G,
    resolve_node: N,
) -> Result<Plan>
where
    G: Fn(String, String, String) -> GFut,
    GFut: std::future::Future<Output = Result<registry::Graph>>,
    N: Fn(String, String, String) -> NFut,
    NFut: std::future::Future<Output = Result<Node>>,
{
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
    let resolve_graph = &resolve_graph;
    let resolve_node = &resolve_node;
    let expansions = futures::stream::iter(asked.iter().cloned().map(|worker| {
        let paths = paths.clone();
        async move {
            let Source::Package { reference, version } = &worker.source else {
                return Ok(vec![Candidate {
                    declaration: worker.clone(),
                    package: None,
                    root: true,
                }]);
            };
            let graph = resolve_graph(
                worker.key.clone(),
                reference.clone(),
                version.clone().unwrap_or_else(|| "*".to_string()),
            )
            .await?;
            let nodes: BTreeMap<_, _> = graph
                .nodes
                .iter()
                .map(|node| (node.name.clone(), node.clone()))
                .collect();
            // A container may be named apart from the package it runs
            // (`database: package://…/state`). The graph names the package,
            // so expand under that name and give the root its key back.
            let graph_root = if nodes.contains_key(package_name(reference)) {
                package_name(reference).to_string()
            } else {
                worker.key.clone()
            };
            let mut root = worker.clone();
            root.key = graph_root.clone();
            root.fields.clear();
            root.start_after.clear();
            let expanded = crate::daemon::expand_graph(&root, reference, graph, &paths)?;
            let (registry, _) = registry::split_reference(reference);
            expanded
                .into_iter()
                .map(|mut declaration| {
                    let node = nodes.get(&declaration.key).cloned().ok_or_else(|| {
                        conflict(
                            &declaration.key,
                            "the registry graph does not contain the requested worker",
                        )
                    })?;
                    let root = declaration.key == graph_root;
                    if root {
                        declaration.key = worker.key.clone();
                    }
                    Ok(Candidate {
                        declaration,
                        package: Some(Package {
                            registry: registry.clone(),
                            node,
                            exact_version: true,
                        }),
                        root,
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
                container.resolved_package.clone(),
            ))
        })
        .collect();
    let existing = futures::stream::iter(existing_requests.into_iter().map(
        |(key, reference, range, registry, locked)| async move {
            let exact_version = locked.is_some();
            let node = match locked {
                Some(locked) => Node::from(&locked),
                None => resolve_node(key.clone(), reference, range.clone()).await?,
            };
            let exact_version = exact_version || range == node.version;
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
    let update = match policy {
        Policy::Add => None,
        Policy::Update { generated } => Some(UpdateScope {
            declared: file
                .containers
                .iter()
                .filter_map(|(key, container)| match &container.worker {
                    WorkerSource::Package { reference } => Some((
                        key.clone(),
                        Source::Package {
                            reference: reference.clone(),
                            version: container.version.clone(),
                        },
                    )),
                    WorkerSource::Path { .. } => None,
                })
                .collect(),
            generated: generated.clone(),
        }),
    };
    plan_resolved(
        asked,
        expanded,
        &existing,
        &file.containers.keys().cloned().collect(),
        update.as_ref(),
    )
}

fn conflict(name: &str, reason: &str) -> ComposeError {
    ComposeError::InvalidWorkerSpec {
        spec: name.to_string(),
        reason: reason.to_string(),
    }
}

/// Rewrites registry dependency names to the container keys chosen for them.
fn retarget(
    start_after: &mut [String],
    registry: &str,
    targets: &BTreeMap<(String, String), String>,
    key: &str,
    original_key: &str,
) -> Result<()> {
    for dependency in start_after.iter_mut() {
        if let Some(target) = targets.get(&(registry.to_string(), dependency.clone())) {
            *dependency = target.clone();
        }
        if dependency == key {
            return Err(ComposeError::DependencyCycle {
                path: format!("{original_key} -> {key}"),
            });
        }
    }
    Ok(())
}

fn merge_edges(start_after: &mut Vec<String>, edges: impl IntoIterator<Item = String>) {
    start_after.extend(edges);
    start_after.sort();
    start_after.dedup();
}

fn plan_resolved(
    asked: &[NewContainer],
    expanded: Vec<Candidate>,
    existing: &BTreeMap<String, Package>,
    existing_keys: &BTreeSet<String>,
    update: Option<&UpdateScope>,
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
    // A requested worker owns its container key. Every root is registered
    // before any dependency copy is inspected, so argument order and graph
    // order cannot decide which declaration wins.
    let mut roots: BTreeMap<&str, &Package> = BTreeMap::new();
    for candidate in expanded.iter().filter(|candidate| candidate.root) {
        let Some(package) = &candidate.package else {
            continue;
        };
        let key = candidate.declaration.key.as_str();
        roots.insert(key, package);
        owners
            .entry(package.identity())
            .or_default()
            .insert(key, package);
    }
    let mut automatic: BTreeMap<Identity, Vec<&Package>> = BTreeMap::new();
    for candidate in expanded.iter().filter(|candidate| !candidate.root) {
        let Some(package) = &candidate.package else {
            continue;
        };
        let key = candidate.declaration.key.as_str();
        if !requested.contains_key(key) {
            automatic
                .entry(package.identity())
                .or_default()
                .push(package);
            continue;
        }
        // Another request's graph reaches a worker the caller also named.
        // The named declaration wins, so the copy must be the same package
        // at the same release; it only contributes its edges.
        match roots.get(key) {
            Some(root)
                if root.identity() == package.identity()
                    && root.node.same_release(&package.node) => {}
            Some(root) if root.identity() == package.identity() => {
                let versions = if root.node.version == package.node.version {
                    String::new()
                } else {
                    format!(" ({} and {})", root.node.version, package.node.version)
                };
                return Err(conflict(
                    key,
                    &format!(
                        "the requested container resolves to different versions, artifacts, or default configurations in the dependency graphs{versions}"
                    ),
                ));
            }
            _ => {
                return Err(conflict(
                    key,
                    "the requested container and a dependency with the same name resolve to different packages",
                ));
            }
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
                 Align the dependency versions before adding or updating these workers",
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
        // An update moves a declared dependency to the release its graph
        // needs, so a declared instance qualifies even while it runs another
        // release. Requested workers never do: they chose their own release.
        let compatible = if compatible.is_empty() && update.is_some() {
            choices
                .into_iter()
                .flat_map(|choices| choices.keys())
                .filter(|key| !requested.contains_key(**key))
                .copied()
                .collect()
        } else {
            compatible
        };
        let key = match compatible.as_slice() {
            [key] => (*key).to_string(),
            [] if choices.is_some_and(|choices| !choices.is_empty()) => {
                return Err(conflict(
                    &identity.1,
                    &format!(
                        "dependency '{}' at version '{}' conflicts with the declared instances. \
                 Pin matching exact versions and artifacts before adding or updating this worker",
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
                 Keep their settings and select a single dependency instance before adding or updating this worker",
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
    // Edges from dependency copies of requested workers, merged into the
    // requested declaration whichever of the two arrives first.
    let mut copied_edges: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut order = Vec::new();
    let mut aliases = Vec::new();
    let mut selected_versions = BTreeMap::new();
    for Candidate {
        mut declaration,
        package,
        root,
    } in expanded
    {
        let explicit = if root {
            requested.get(declaration.key.as_str()).copied()
        } else {
            None
        };
        if let Some(package) = package {
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
            if !root && requested.contains_key(key.as_str()) {
                retarget(
                    &mut declaration.start_after,
                    &package.registry,
                    &targets,
                    &key,
                    &original_key,
                )?;
                let edges = std::mem::take(&mut declaration.start_after);
                match containers.get_mut(&key) {
                    Some(named) => merge_edges(&mut named.start_after, edges),
                    None => copied_edges.entry(key).or_default().extend(edges),
                }
                continue;
            }
            let mut lock_version = Some(package.node.version.clone());
            let mut declared_source = false;
            if explicit.is_none() && existing.contains_key(&key) {
                let Some(update) = update else {
                    // Reuse an owner without changing its configuration,
                    // source, version, or startup dependencies.
                    continue;
                };
                let (source, lock) = update.follow(&key, &existing[&key], &package)?;
                declaration.source = source;
                lock_version = lock;
                declared_source = true;
            }
            declaration.key = key;
            if let Some(version) = lock_version
                && let Some(previous) =
                    selected_versions.insert(declaration.key.clone(), version.clone())
                && previous != version
            {
                return Err(conflict(
                    &declaration.key,
                    "the dependency resolves to conflicting exact versions",
                ));
            }
            if explicit.is_none() && !declared_source {
                declaration.source = Source::Package {
                    reference: format!(
                        "{}/{}",
                        package.registry.trim_start_matches("https://"),
                        package.node.canonical_name()
                    ),
                    version: Some(package.node.version.clone()),
                };
            }
            retarget(
                &mut declaration.start_after,
                &package.registry,
                &targets,
                &declaration.key,
                &original_key,
            )?;
        }
        if let Some(explicit) = explicit {
            declaration.fields = explicit.fields.clone();
            declaration
                .start_after
                .extend(explicit.start_after.iter().cloned());
        }
        if let Some(edges) = copied_edges.remove(&declaration.key) {
            declaration.start_after.extend(edges);
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
            merge_edges(&mut previous.start_after, declaration.start_after);
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
    selected_versions.retain(|key, _| reachable.contains(key));
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
        selected_versions,
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
            root: false,
        }
    }

    /// A candidate for a worker the request named.
    fn root(name: &str, alias_of: Option<&str>, dependencies: &[&str]) -> Candidate {
        Candidate {
            root: true,
            ..candidate(name, alias_of, dependencies)
        }
    }

    fn request(candidate: &Candidate) -> NewContainer {
        let mut declaration = candidate.declaration.clone();
        declaration.start_after.clear();
        declaration
    }

    #[test]
    fn alias_and_canonical_dependencies_share_one_container() {
        let a = root("api", None, &["console"]);
        let b = root("jobs", None, &["shell"]);
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
            None,
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
    fn plan_keeps_a_tag_in_the_declaration_and_an_exact_version_for_the_lock() {
        let mut state = root("state", None, &[]);
        state.declaration.source = Source::Package {
            reference: "api.workers.iii.dev/state".into(),
            version: Some("next".into()),
        };
        let asked = vec![state.declaration.clone()];

        let plan = plan_resolved(
            &asked,
            vec![state],
            &BTreeMap::new(),
            &BTreeSet::new(),
            None,
        )
        .unwrap();

        assert_eq!(plan.containers[0].source, asked[0].source);
        assert_eq!(plan.selected_versions["state"], "1.0.0");
    }

    #[test]
    fn existing_instance_is_reused_without_editing_its_settings_or_dependencies() {
        let a = root("api", None, &["console"]);
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
            None,
        )
        .unwrap();
        assert_eq!(plan.containers.len(), 1);
        assert_eq!(plan.containers[0].start_after, vec!["my-shell"]);
    }

    #[test]
    fn existing_alias_keeps_its_container_key() {
        let a = root("api", None, &["shell"]);
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
            None,
        )
        .unwrap();
        assert_eq!(plan.containers[0].start_after, vec!["console"]);
    }

    #[test]
    fn explicitly_requested_instances_keep_distinct_configurations() {
        let mut a = root("console", Some("shell"), &[]);
        a.declaration
            .fields
            .insert("config_name".into(), "console-config".into());
        let mut b = root("shell", None, &[]);
        b.declaration
            .fields
            .insert("config_name".into(), "shell-config".into());
        let asked = vec![request(&a), request(&b)];
        let plan =
            plan_resolved(&asked, vec![a, b], &BTreeMap::new(), &BTreeSet::new(), None).unwrap();
        assert_eq!(plan.containers, asked);
    }

    #[test]
    fn generated_alias_uses_an_explicit_canonical_request() {
        let a = root("api", None, &["console"]);
        let shell = root("shell", None, &[]);
        let asked = vec![request(&a), request(&shell)];
        let plan = plan_resolved(
            &asked,
            vec![candidate("console", Some("shell"), &[]), a, shell],
            &BTreeMap::new(),
            &BTreeSet::new(),
            None,
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
        let a = root("api", None, &["console"]);
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
            None,
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
        let a = root("api", None, &["console", "shell"]);
        let asked = vec![request(&a)];
        let mut shell = candidate("shell", None, &[]);
        shell.package.as_mut().unwrap().node.version = "2.0.0".into();
        let error = plan_resolved(
            &asked,
            vec![candidate("console", Some("shell"), &[]), shell, a],
            &BTreeMap::new(),
            &BTreeSet::new(),
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("different versions, artifacts"));
    }

    #[test]
    fn a_matching_version_with_a_different_artifact_is_refused() {
        let a = root("api", None, &["console"]);
        let asked = vec![request(&a)];
        let mut package = candidate("shell", None, &[]).package.unwrap();
        package.node.artifact_digest = Some("b".repeat(64));
        let existing = BTreeMap::from([("shell".into(), package)]);
        let error = plan_resolved(
            &asked,
            vec![candidate("console", Some("shell"), &[]), a],
            &existing,
            &existing.keys().cloned().collect(),
            None,
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
        let a = root("api", None, &["console"]);
        let asked = vec![request(&a)];
        let mut package = candidate("shell", None, &[]).package.unwrap();
        package.exact_version = false;
        let existing = BTreeMap::from([("shell".into(), package)]);
        let error = plan_resolved(
            &asked,
            vec![candidate("console", Some("shell"), &[]), a],
            &existing,
            &existing.keys().cloned().collect(),
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("Pin matching exact versions"));
    }

    #[test]
    fn an_ordinary_declared_dependency_keeps_its_pin_on_add() {
        let a = root("api", None, &["shell"]);
        let asked = vec![request(&a)];
        let mut package = candidate("shell", None, &[]).package.unwrap();
        package.node.version = "0.9.0".into();
        let existing = BTreeMap::from([("shell".into(), package)]);
        let plan = plan_resolved(
            &asked,
            vec![candidate("shell", None, &[]), a],
            &existing,
            &existing.keys().cloned().collect(),
            None,
        )
        .unwrap();
        assert_eq!(plan.containers.len(), 1);
        assert_eq!(plan.containers[0].start_after, vec!["shell"]);
    }

    #[test]
    fn a_requested_root_cannot_hide_conflicting_dependency_artifacts() {
        let a = root("api", None, &["shell"]);
        let shell = root("shell", None, &[]);
        let asked = vec![request(&a), request(&shell)];
        let mut dependency = candidate("shell", None, &[]);
        dependency.package.as_mut().unwrap().node.artifact_digest = Some("b".repeat(64));
        let error = plan_resolved(
            &asked,
            vec![dependency, a, shell],
            &BTreeMap::new(),
            &BTreeSet::new(),
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("different versions, artifacts"));
    }

    #[test]
    fn containers_from_another_registry_do_not_satisfy_a_dependency() {
        let a = root("api", None, &["console"]);
        let asked = vec![request(&a)];
        let mut package = candidate("shell", None, &[]).package.unwrap();
        package.registry = "https://custom.example".into();
        let existing = BTreeMap::from([("custom-shell".into(), package)]);
        let plan = plan_resolved(
            &asked,
            vec![candidate("console", Some("shell"), &[]), a],
            &existing,
            &existing.keys().cloned().collect(),
            None,
        )
        .unwrap();
        assert_eq!(plan.containers[0].key, "shell");
        assert_eq!(plan.containers[1].start_after, vec!["shell"]);
    }

    #[test]
    fn a_generated_canonical_name_cannot_replace_a_local_container() {
        let a = root("api", None, &["console"]);
        let asked = vec![request(&a)];
        let error = plan_resolved(
            &asked,
            vec![candidate("console", Some("shell"), &[]), a],
            &BTreeMap::new(),
            &BTreeSet::from(["shell".into()]),
            None,
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
        let a = root("api", None, &["console"]);
        let asked = vec![request(&a)];
        let plan = plan_resolved(
            &asked,
            vec![a],
            &BTreeMap::new(),
            &BTreeSet::from(["console".into()]),
            None,
        )
        .unwrap();
        assert_eq!(plan.containers[0].start_after, vec!["console"]);
    }

    #[test]
    fn a_cycle_created_by_alias_resolution_is_refused() {
        let a = root("api", None, &["console"]);
        let asked = vec![request(&a)];
        let error = plan_resolved(
            &asked,
            vec![candidate("console", Some("api"), &[]), a],
            &BTreeMap::new(),
            &BTreeSet::new(),
            None,
        )
        .unwrap_err();
        assert_eq!(error.code(), "DEPENDENCY_CYCLE");
    }

    /// Gives a candidate the selector a request wrote instead of an exact version.
    fn tagged(mut candidate: Candidate, tag: &str) -> Candidate {
        let Source::Package { version, .. } = &mut candidate.declaration.source else {
            unreachable!();
        };
        *version = Some(tag.into());
        candidate
    }

    fn keys(plan: &Plan) -> Vec<&str> {
        plan.containers.iter().map(|c| c.key.as_str()).collect()
    }

    fn container<'a>(plan: &'a Plan, key: &str) -> &'a NewContainer {
        plan.containers.iter().find(|c| c.key == key).unwrap()
    }

    fn update_scope(declared: &[(&str, &str)], generated: &[&str]) -> UpdateScope {
        UpdateScope {
            declared: declared
                .iter()
                .map(|(key, selector)| {
                    (
                        (*key).to_string(),
                        Source::Package {
                            reference: format!("api.workers.iii.dev/{key}"),
                            version: Some((*selector).to_string()),
                        },
                    )
                })
                .collect(),
            generated: generated.iter().map(|key| (*key).to_string()).collect(),
        }
    }

    fn existing_at(name: &str, version: &str) -> BTreeMap<String, Package> {
        let mut package = candidate(name, None, &[]).package.unwrap();
        package.node.version = version.into();
        BTreeMap::from([(name.to_string(), package)])
    }

    #[test]
    fn a_requested_tag_absorbs_its_dependency_copy_in_either_order() {
        let asked = vec![
            request(&root("api", None, &["state"])),
            request(&tagged(root("state", None, &["queue"]), "latest")),
        ];
        let orders = [
            vec![
                candidate("queue", None, &[]),
                candidate("state", None, &["queue"]),
                root("api", None, &["state"]),
                candidate("queue", None, &[]),
                tagged(root("state", None, &["queue"]), "latest"),
            ],
            vec![
                candidate("queue", None, &[]),
                tagged(root("state", None, &["queue"]), "latest"),
                candidate("queue", None, &[]),
                candidate("state", None, &["queue"]),
                root("api", None, &["state"]),
            ],
        ];
        for expanded in orders {
            let plan =
                plan_resolved(&asked, expanded, &BTreeMap::new(), &BTreeSet::new(), None).unwrap();

            assert_eq!(keys(&plan), vec!["queue", "state", "api"]);
            let state = container(&plan, "state");
            assert_eq!(
                state.source,
                Source::Package {
                    reference: "api.workers.iii.dev/state".into(),
                    version: Some("latest".into()),
                }
            );
            assert_eq!(state.start_after, vec!["queue"]);
            assert_eq!(plan.selected_versions["state"], "1.0.0");
        }
    }

    #[test]
    fn a_requested_worker_and_its_dependency_copy_must_share_a_release() {
        let api = root("api", None, &["state"]);
        let state = tagged(root("state", None, &[]), "latest");
        let asked = vec![request(&api), request(&state)];
        let mut copy = candidate("state", None, &[]);
        copy.package.as_mut().unwrap().node.version = "0.9.0".into();

        let error = plan_resolved(
            &asked,
            vec![copy, api, state],
            &BTreeMap::new(),
            &BTreeSet::new(),
            None,
        )
        .unwrap_err();

        assert!(
            error.to_string().contains("different versions, artifacts"),
            "{error}"
        );
        assert!(error.to_string().contains("1.0.0 and 0.9.0"), "{error}");
    }

    #[test]
    fn a_dependency_from_another_registry_cannot_merge_into_a_requested_name() {
        let api = root("api", None, &["state"]);
        let state = root("state", None, &[]);
        let asked = vec![request(&api), request(&state)];
        let mut copy = candidate("state", None, &[]);
        copy.package.as_mut().unwrap().registry = "https://custom.example".into();

        let error = plan_resolved(
            &asked,
            vec![copy, api, state],
            &BTreeMap::new(),
            &BTreeSet::new(),
            None,
        )
        .unwrap_err();

        assert!(
            error.to_string().contains("resolve to different packages"),
            "{error}"
        );
    }

    #[test]
    fn update_moves_a_generated_dependency_and_keeps_its_new_dependencies() {
        let api = root("api", None, &["shell"]);
        let asked = vec![request(&api)];
        let existing = existing_at("shell", "0.9.0");
        let scope = update_scope(&[("shell", "0.9.0")], &["shell"]);

        let plan = plan_resolved(
            &asked,
            vec![
                candidate("helper", None, &[]),
                candidate("shell", None, &["helper"]),
                api,
            ],
            &existing,
            &existing.keys().cloned().collect(),
            Some(&scope),
        )
        .unwrap();

        assert_eq!(keys(&plan), vec!["helper", "shell", "api"]);
        let shell = container(&plan, "shell");
        assert_eq!(
            shell.source,
            Source::Package {
                reference: "api.workers.iii.dev/shell".into(),
                version: Some("1.0.0".into()),
            }
        );
        assert_eq!(shell.start_after, vec!["helper"]);
        assert_eq!(plan.selected_versions["shell"], "1.0.0");
    }

    #[test]
    fn update_keeps_an_operator_tag_and_moves_only_the_lock() {
        let api = root("api", None, &["shell"]);
        let asked = vec![request(&api)];
        let existing = existing_at("shell", "0.9.0");
        let scope = update_scope(&[("shell", "latest")], &[]);

        let plan = plan_resolved(
            &asked,
            vec![candidate("shell", None, &[]), api],
            &existing,
            &existing.keys().cloned().collect(),
            Some(&scope),
        )
        .unwrap();

        assert_eq!(
            container(&plan, "shell").source,
            Source::Package {
                reference: "api.workers.iii.dev/shell".into(),
                version: Some("latest".into()),
            }
        );
        assert_eq!(plan.selected_versions["shell"], "1.0.0");
    }

    #[test]
    fn update_leaves_the_lock_alone_when_the_operator_tag_already_has_the_release() {
        let api = root("api", None, &["shell"]);
        let asked = vec![request(&api)];
        let existing = existing_at("shell", "1.0.0");
        let scope = update_scope(&[("shell", "latest")], &[]);

        let plan = plan_resolved(
            &asked,
            vec![candidate("shell", None, &[]), api],
            &existing,
            &existing.keys().cloned().collect(),
            Some(&scope),
        )
        .unwrap();

        assert_eq!(keys(&plan), vec!["shell", "api"]);
        assert!(!plan.selected_versions.contains_key("shell"));
    }

    #[test]
    fn update_refuses_to_move_an_operator_exact_pin() {
        let api = root("api", None, &["shell"]);
        let asked = vec![request(&api)];
        let existing = existing_at("shell", "0.9.0");
        let scope = update_scope(&[("shell", "0.9.0")], &[]);

        let error = plan_resolved(
            &asked,
            vec![candidate("shell", None, &[]), api],
            &existing,
            &existing.keys().cloned().collect(),
            Some(&scope),
        )
        .unwrap_err();

        assert!(
            error.to_string().contains("pins 'shell' to 0.9.0"),
            "{error}"
        );
        assert!(error.to_string().contains("worker=shell@1.0.0"), "{error}");
    }

    #[test]
    fn update_refuses_a_release_outside_an_operator_range() {
        let api = root("api", None, &["shell"]);
        let asked = vec![request(&api)];
        let existing = existing_at("shell", "0.9.0");
        let scope = update_scope(&[("shell", "^0.9")], &[]);

        let error = plan_resolved(
            &asked,
            vec![candidate("shell", None, &[]), api],
            &existing,
            &existing.keys().cloned().collect(),
            Some(&scope),
        )
        .unwrap_err();

        assert!(
            error.to_string().contains("declares 'shell' as ^0.9"),
            "{error}"
        );
    }

    fn node(name: &str, version: &str) -> Node {
        Node {
            name: name.into(),
            version: version.into(),
            kind: "binary".into(),
            artifact_digest: Some("a".repeat(64)),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn a_renamed_container_plans_the_package_it_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let file = ComposeFile::parse(
            "containers:\n  database:\n    worker: package://private.example/team/state\n    version: '1.0.0'\n",
            tmp.path().join("worker-compose.yaml"),
        )
        .unwrap();
        let asked = vec![NewContainer {
            key: "database".into(),
            source: Source::Package {
                reference: "private.example/team/state".into(),
                version: Some("latest".into()),
            },
            start_after: Vec::new(),
            fields: Default::default(),
        }];

        let plan = plan_with(
            &file,
            &asked,
            &Policy::Update {
                generated: BTreeSet::new(),
            },
            |_, _, _| async {
                Ok::<_, ComposeError>(registry::Graph {
                    nodes: vec![node("state", "1.1.0"), node("queue", "2.0.0")],
                    edges: vec![("state".into(), "queue".into())],
                })
            },
            |key, _, _| async move { Err::<Node, _>(conflict(&key, "this test has no registry")) },
        )
        .await
        .unwrap();

        assert_eq!(keys(&plan), vec!["queue", "database"]);
        let database = container(&plan, "database");
        assert_eq!(database.source, asked[0].source);
        assert_eq!(database.start_after, vec!["queue"]);
        assert_eq!(plan.selected_versions["database"], "1.1.0");
        assert_eq!(plan.selected_versions["queue"], "2.0.0");
    }

    fn plan_update(
        existing: &BTreeMap<String, Package>,
        scope: &UpdateScope,
        dependency: Candidate,
    ) -> Result<Plan> {
        let api = root("api", None, &["shell"]);
        let asked = vec![request(&api)];
        plan_resolved(
            &asked,
            vec![dependency, api],
            existing,
            &existing.keys().cloned().collect(),
            Some(scope),
        )
    }

    #[test]
    fn update_keeps_a_tag_the_operator_put_on_a_generated_dependency() {
        let existing = existing_at("shell", "0.9.0");
        let scope = update_scope(&[("shell", "next")], &["shell"]);

        let plan = plan_update(&existing, &scope, candidate("shell", None, &[])).unwrap();

        assert_eq!(
            container(&plan, "shell").source,
            Source::Package {
                reference: "api.workers.iii.dev/shell".into(),
                version: Some("next".into()),
            }
        );
        assert_eq!(plan.selected_versions["shell"], "1.0.0");
    }

    #[test]
    fn update_compares_a_prerelease_with_an_operator_range_as_its_release() {
        let existing = existing_at("shell", "1.0.0");
        let scope = update_scope(&[("shell", "^1.0")], &[]);
        let at = |version: &str| {
            let mut shell = candidate("shell", None, &[]);
            shell.package.as_mut().unwrap().node.version = version.into();
            shell
        };

        let plan = plan_update(&existing, &scope, at("1.1.0-rc.1")).unwrap();
        let error = plan_update(&existing, &scope, at("2.0.0-rc.1")).unwrap_err();

        assert_eq!(plan.selected_versions["shell"], "1.1.0-rc.1");
        assert!(
            error.to_string().contains("declares 'shell' as ^1.0"),
            "{error}"
        );
    }

    #[test]
    fn update_moves_the_lock_of_an_unpinned_operator_dependency() {
        let existing = existing_at("shell", "0.9.0");
        let mut scope = update_scope(&[], &[]);
        scope.declared.insert(
            "shell".into(),
            Source::Package {
                reference: "api.workers.iii.dev/shell".into(),
                version: None,
            },
        );

        let plan = plan_update(&existing, &scope, candidate("shell", None, &[])).unwrap();

        assert_eq!(
            container(&plan, "shell").source,
            Source::Package {
                reference: "api.workers.iii.dev/shell".into(),
                version: None,
            }
        );
        assert_eq!(plan.selected_versions["shell"], "1.0.0");
    }

    #[test]
    fn a_renamed_request_satisfies_a_dependency_on_its_package() {
        let api = root("api", None, &["state"]);
        let mut database = root("state", None, &[]);
        database.declaration.key = "database".into();
        let asked = vec![request(&api), request(&database)];

        let plan = plan_resolved(
            &asked,
            vec![candidate("state", None, &[]), api, database],
            &BTreeMap::new(),
            &BTreeSet::new(),
            None,
        )
        .unwrap();

        assert_eq!(keys(&plan), vec!["database", "api"]);
        assert_eq!(container(&plan, "api").start_after, vec!["database"]);
    }

    #[test]
    fn update_moves_a_single_renamed_declared_instance() {
        let mut package = candidate("shell", None, &[]).package.unwrap();
        package.node.version = "0.9.0".into();
        let existing = BTreeMap::from([("db".to_string(), package.clone())]);
        let scope = update_scope(&[("db", "latest")], &[]);

        let plan = plan_update(&existing, &scope, candidate("shell", None, &[])).unwrap();

        assert_eq!(keys(&plan), vec!["db", "api"]);
        assert_eq!(container(&plan, "api").start_after, vec!["db"]);
        assert_eq!(plan.selected_versions["db"], "1.0.0");

        let existing = BTreeMap::from([
            ("db-one".to_string(), package.clone()),
            ("db-two".to_string(), package),
        ]);
        let scope = update_scope(&[("db-one", "latest"), ("db-two", "latest")], &[]);
        let error = plan_update(&existing, &scope, candidate("shell", None, &[])).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("multiple declared containers: db-one, db-two"),
            "{error}"
        );
    }
}
