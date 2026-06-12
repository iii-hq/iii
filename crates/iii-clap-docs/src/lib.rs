// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Render a `clap::Command` tree as a Mintlify MDX reference page.
//!
//! Each user-facing binary (`iii`, `iii-worker`, `iii-console`) exposes a
//! hidden `gen-cli-docs` subcommand that hands its own `Command` tree to
//! [`render_mdx`]. The output is committed under `docs/next/cli-reference/`
//! and CI regenerates + diffs it so the docs can never drift from the CLI.
//!
//! The walker is intentionally dumb: it documents exactly what clap knows.
//! Anything `hide = true` (commands or args) is skipped, the auto `help`
//! command/flag is skipped, and subcommands listed in
//! [`PageMeta::delegated`] are linked to their own page instead of being
//! rendered (the engine's `console`/`cloud`/`worker` passthrough stubs carry
//! no real structure; the real trees live in the other binaries).

use std::collections::BTreeMap;

use clap::Command;

/// A subcommand that is documented elsewhere (or not at all). The walker
/// lists it in the parent's Commands table with `note` as the description,
/// linking to `link` when given, and does not recurse into it.
pub struct Delegated {
    /// Relative doc link (e.g. `./iii-worker`). `None` renders plain text.
    pub link: Option<String>,
    /// Replacement description for the Commands table row.
    pub note: String,
}

/// Page-level metadata supplied by each binary's `gen-cli-docs`
/// implementation.
pub struct PageMeta {
    pub title: String,
    pub description: String,
    pub owner: String,
    /// MDX paragraph(s) inserted after the generated-file banner.
    pub intro: String,
    /// Subcommand name -> external documentation target.
    pub delegated: BTreeMap<String, Delegated>,
    /// Full command path ("iii trigger") -> raw MDX appended to that
    /// command's section. Docs-only prose that has no place in terminal
    /// help; for text that should show in BOTH `--help` and the docs, set
    /// clap's `after_long_help`/`after_help` on the command instead (the
    /// walker renders it as a `<Note>`).
    pub mdx_only_notes: BTreeMap<String, String>,
}

/// Render the full MDX page for `cmd`: frontmatter, generated-file banner,
/// page intro, then the command tree.
pub fn render_mdx(cmd: Command, meta: &PageMeta) -> String {
    let (mut cmd, root_path) = prepare(cmd);

    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("title: \"{}\"\n", yaml_escape(&meta.title)));
    out.push_str(&format!(
        "description: \"{}\"\n",
        yaml_escape(&meta.description)
    ));
    out.push_str(&format!("owner: \"{}\"\n", yaml_escape(&meta.owner)));
    out.push_str("type: \"reference\"\n");
    out.push_str("---\n\n");
    // Single-line MDX comment: multi-line {/* */} blocks get mangled by
    // formatters, and this file must round-trip byte-identical for the CI
    // drift gate.
    out.push_str("{/* AUTO-GENERATED FILE, DO NOT EDIT. Generated from the clap CLI definitions by the hidden `gen-cli-docs` subcommand. Regenerate with `scripts/generate-cli-docs.sh`. */}\n\n");
    if !meta.intro.is_empty() {
        out.push_str(meta.intro.trim_end());
        out.push_str("\n\n");
    }

    render_command(&mut out, &mut cmd, &root_path, 2, meta, None);
    finish(out)
}

/// Render `cmd` as a page fragment: no frontmatter, no banner. The intro is
/// emitted INSIDE the root section (after the about line) as a lead-in
/// instead of above the heading. Fragments are concatenated under another
/// binary's full page to build the combined CLI reference.
pub fn render_mdx_fragment(cmd: Command, meta: &PageMeta) -> String {
    let (mut cmd, root_path) = prepare(cmd);
    let lead_in = if meta.intro.is_empty() {
        None
    } else {
        Some(meta.intro.as_str())
    };
    let mut out = String::new();
    render_command(&mut out, &mut cmd, &root_path, 2, meta, lead_in);
    finish(out)
}

/// Build the command tree and resolve the root display path: prefer an
/// explicit bin_name ("iii worker") over the package name ("iii-worker") so
/// headings show what users actually type.
fn prepare(cmd: Command) -> (Command, String) {
    let mut cmd = cmd;
    cmd.build();
    let root_path = cmd
        .get_bin_name()
        .unwrap_or_else(|| cmd.get_name())
        .to_string();
    validate_value_names(&cmd, &root_path);
    let cmd = set_bin_names(cmd, &root_path);
    (cmd, root_path)
}

/// Enforce the placeholder standard: value names render as `<VALUE>` /
/// `[VALUE]` in usage lines and tables, so they must not contain lowercase
/// ASCII (a lowercase name means someone set `value_name` or `name` by
/// hand). Panics so the gen-cli-docs run, and with it the CLI Docs Built
/// job, fails loudly instead of publishing an inconsistent page.
fn validate_value_names(cmd: &Command, path: &str) {
    for arg in cmd.get_arguments().filter(|a| !a.is_hide_set()) {
        for name in arg.get_value_names().unwrap_or_default() {
            assert!(
                !name.as_str().chars().any(|c| c.is_ascii_lowercase()),
                "value name `{name}` on `{path}` arg `{}` contains lowercase; \
                 placeholders are rendered in caps (e.g. value_name = \"COMMAND\")",
                arg.get_id()
            );
        }
    }
    for sub in cmd.get_subcommands().filter(|s| !s.is_hide_set()) {
        validate_value_names(sub, &format!("{path} {}", sub.get_name()));
    }
}

/// Normalize trailing whitespace to one newline.
fn finish(out: String) -> String {
    let trimmed = out.trim_end().to_string();
    trimmed + "\n"
}

/// Render `cmd` and write it to `out`, or stdout when `out` is `None`.
/// Parent directories are created as needed.
pub fn write_page(
    cmd: Command,
    meta: &PageMeta,
    out: Option<&std::path::Path>,
) -> std::io::Result<()> {
    write_str(render_mdx(cmd, meta), out)
}

/// Like [`write_page`] but renders a concatenable fragment (see
/// [`render_mdx_fragment`]).
pub fn write_fragment(
    cmd: Command,
    meta: &PageMeta,
    out: Option<&std::path::Path>,
) -> std::io::Result<()> {
    write_str(render_mdx_fragment(cmd, meta), out)
}

fn write_str(mdx: String, out: Option<&std::path::Path>) -> std::io::Result<()> {
    match out {
        Some(path) => {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(path, mdx)
        }
        None => {
            use std::io::Write;
            std::io::stdout().write_all(mdx.as_bytes())
        }
    }
}

/// Recursively set each node's bin name to its full command path so
/// `render_usage` prints `Usage: iii worker sandbox run [OPTIONS]` instead
/// of `Usage: run [OPTIONS]`. clap only fills these in lazily while
/// rendering help for a live invocation; for offline walking we do it
/// ourselves.
fn set_bin_names(cmd: Command, path: &str) -> Command {
    let mut cmd = cmd.bin_name(path.to_string());
    let names: Vec<String> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    for name in names {
        let child_path = format!("{path} {name}");
        cmd = cmd.mut_subcommand(name, |sub| set_bin_names(sub, &child_path));
    }
    cmd
}

fn render_command(
    out: &mut String,
    cmd: &mut Command,
    path: &str,
    level: usize,
    meta: &PageMeta,
    lead_in: Option<&str>,
) {
    let hashes = "#".repeat(level.min(5));
    out.push_str(&format!("{hashes} `{path}`\n\n"));

    let about = cmd
        .get_long_about()
        .or_else(|| cmd.get_about())
        .map(|s| s.to_string())
        .unwrap_or_default();
    if !about.is_empty() {
        out.push_str(&esc_mdx(&about));
        out.push_str("\n\n");
    }

    // Fragment lead-in: contextual prose for this binary's section of the
    // combined page (raw MDX, only ever set on the root node).
    if let Some(text) = lead_in {
        out.push_str(text.trim_end());
        out.push_str("\n\n");
    }

    let aliases: Vec<&str> = cmd.get_visible_aliases().collect();
    if !aliases.is_empty() {
        out.push_str(&format!(
            "Alias: {}\n\n",
            aliases
                .iter()
                .map(|a| format!("`{a}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    out.push_str("```bash\n");
    out.push_str(&usage_lines(cmd));
    out.push_str("```\n\n");

    render_args_tables(out, cmd);

    // Subcommands table, sorted alphabetically (clap iterates in
    // declaration order, which reads as arbitrary in a reference). The
    // recursion below shares this vec, so child sections render in the
    // same order as the table rows.
    let mut subs: Vec<(String, String)> = cmd
        .get_subcommands()
        .filter(|s| !s.is_hide_set() && s.get_name() != "help")
        .map(|s| {
            (
                s.get_name().to_string(),
                s.get_about().map(|a| a.to_string()).unwrap_or_default(),
            )
        })
        .collect();
    subs.sort_by(|a, b| a.0.cmp(&b.0));
    // Only root sections (`## iii`, `## iii worker`) get the table: it
    // serves as that binary's index. For nested commands the child
    // sections follow immediately below, so a table would just duplicate
    // their headings.
    if !subs.is_empty() && level == 2 {
        // Bold label rather than a real heading: a `### Subcommands` per
        // section would litter the TOC with duplicate entries and produce
        // unstable -1/-2 anchor suffixes.
        out.push_str("**Subcommands:**\n\n");
        out.push_str("| Command | Description |\n| ------- | ----------- |\n");
        for (name, about) in &subs {
            let row = if let Some(d) = meta.delegated.get(name) {
                let label = match &d.link {
                    Some(link) => format!("[`{name}`]({link})"),
                    None => format!("`{name}`"),
                };
                format!("| {label} | {} |\n", esc_cell(&d.note))
            } else {
                format!(
                    "| [`{name}`](#{}) | {} |\n",
                    anchor(&format!("{path} {name}")),
                    esc_cell(about)
                )
            };
            out.push_str(&row);
        }
        out.push('\n');
    }

    // Trailing notes, mirroring clap's help layout (after_help prints at
    // the end of `--help` output). First the clap-native text, which shows
    // on both surfaces; then any docs-only MDX registered for this path.
    if let Some(after) = cmd.get_after_long_help().or_else(|| cmd.get_after_help()) {
        let text = esc_mdx(&after.to_string());
        out.push_str(&format!("<Note>\n  {}\n</Note>\n\n", text.trim()));
    }
    if let Some(note) = meta.mdx_only_notes.get(path) {
        out.push_str(note.trim_end());
        out.push_str("\n\n");
    }

    // Recurse, skipping delegated and the auto help subcommand.
    for (name, _) in subs {
        if meta.delegated.contains_key(&name) {
            continue;
        }
        let child_path = format!("{path} {name}");
        // mut_subcommand needs ownership; take the child out via clone of
        // the subtree. Command is cheaply clonable (Arc-backed strings).
        let mut child = cmd
            .get_subcommands()
            .find(|s| s.get_name() == name)
            .expect("subcommand listed above")
            .clone();
        render_command(out, &mut child, &child_path, level + 1, meta, None);
    }
}

/// `render_usage` output with the `Usage: ` prefix stripped and every
/// continuation line left-trimmed, newline-terminated.
fn usage_lines(cmd: &mut Command) -> String {
    let usage = cmd.render_usage().to_string();
    let mut out = String::new();
    for line in usage.lines() {
        let line = line.trim().trim_start_matches("Usage:").trim_start();
        if !line.is_empty() {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn render_args_tables(out: &mut String, cmd: &Command) {
    // Positionals.
    let positionals: Vec<_> = cmd.get_positionals().filter(|a| !a.is_hide_set()).collect();
    if !positionals.is_empty() {
        out.push_str("| Argument | Description |\n| -------- | ----------- |\n");
        for arg in positionals {
            let many = arg
                .get_num_args()
                .map(|r| r.max_values() > 1)
                .unwrap_or(false);
            let name = arg
                .get_value_names()
                .and_then(|n| n.first())
                .map(|n| n.to_string())
                .unwrap_or_else(|| arg.get_id().to_string().to_uppercase());
            let wrapped = if arg.is_required_set() {
                format!("<{name}>")
            } else {
                format!("[{name}]")
            };
            let display = if many {
                format!("{wrapped}...")
            } else {
                wrapped
            };
            out.push_str(&format!(
                "| `{display}` | {} |\n",
                esc_cell(&arg_description(arg))
            ));
        }
        out.push('\n');
    }

    // Options. Skip clap's auto help flag everywhere, and skip the auto
    // version flag (recognizable by its canned "Print version" help) while
    // keeping explicitly-declared version args.
    let options: Vec<_> = cmd
        .get_arguments()
        .filter(|a| !a.is_positional() && !a.is_hide_set())
        .filter(|a| a.get_id() != "help")
        .filter(|a| {
            !(a.get_id() == "version"
                && a.get_help().map(|h| h.to_string()).as_deref() == Some("Print version"))
        })
        .collect();
    if !options.is_empty() {
        out.push_str("| Option | Description |\n| ------ | ----------- |\n");
        for arg in options {
            let mut flag = String::new();
            if let Some(short) = arg.get_short() {
                flag.push_str(&format!("-{short}"));
            }
            if let Some(long) = arg.get_long() {
                if !flag.is_empty() {
                    flag.push_str(", ");
                }
                flag.push_str(&format!("--{long}"));
            }
            let takes_value = arg.get_num_args().map(|r| r.takes_values()).unwrap_or(true);
            if takes_value {
                let value_name = arg
                    .get_value_names()
                    .and_then(|n| n.first())
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| arg.get_id().to_string().to_uppercase());
                flag.push_str(&format!(" <{value_name}>"));
            }
            out.push_str(&format!(
                "| `{flag}` | {} |\n",
                esc_cell(&arg_description(arg))
            ));
        }
        out.push('\n');
    }
}

/// Help text plus the bracketed metadata clap appends in its own help:
/// required marker, defaults, env var, accepted values.
fn arg_description(arg: &clap::Arg) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(help) = arg.get_help() {
        parts.push(help.to_string());
    }
    if !arg.is_positional() && arg.is_required_set() {
        parts.push("(required)".to_string());
    }
    // Value-less flags (SetTrue/SetFalse) carry an implicit "false" default;
    // clap's own help suppresses it and so do we.
    let takes_values = arg.get_num_args().map(|r| r.takes_values()).unwrap_or(true);
    let defaults: Vec<String> = arg
        .get_default_values()
        .iter()
        .map(|v| v.to_string_lossy().into_owned())
        .collect();
    if takes_values && !defaults.is_empty() && !arg.is_hide_default_value_set() {
        parts.push(format!("[default: {}]", defaults.join(", ")));
    }
    if let Some(env) = arg.get_env() {
        parts.push(format!("[env: {}]", env.to_string_lossy()));
    }
    if !arg.is_hide_possible_values_set() {
        let values: Vec<String> = arg
            .get_possible_values()
            .iter()
            .filter(|v| !v.is_hide_set())
            .map(|v| v.get_name().to_string())
            .collect();
        // Skip the degenerate boolean flag case.
        if !values.is_empty() && values != ["true", "false"] {
            parts.push(format!("[possible values: {}]", values.join(", ")));
        }
    }
    parts.join(" ")
}

/// Heading anchor the way Mintlify slugs heading text: backticks stripped,
/// lowercased, spaces to hyphens.
fn anchor(path: &str) -> String {
    path.to_lowercase()
        .chars()
        .map(|c| if c == ' ' { '-' } else { c })
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect()
}

/// Escape text destined for MDX flow content: `<` starts JSX, `{`/`}` start
/// expressions. Backslash-escapes keep the source readable in diffs.
///
/// Help strings routinely contain inline-code spans (`` `--json '{"a":1}'` ``)
/// whose contents MDX already treats literally; escaping inside them would
/// render stray backslashes. Splitting on backticks alternates
/// outside/inside-span segments, so only even-indexed segments are escaped.
/// Unbalanced backticks degrade gracefully (the dangling tail is treated as
/// inside a span and left alone).
fn esc_mdx(s: &str) -> String {
    s.split('`')
        .enumerate()
        .map(|(i, seg)| {
            if i % 2 == 0 {
                let escaped = seg
                    .replace('\\', "\\\\")
                    .replace('<', "\\<")
                    .replace('{', "\\{")
                    .replace('}', "\\}");
                backtick_flags(&escaped)
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("`")
}

/// Wrap bare `--flag` mentions in backticks. MDX smart typography turns a
/// plain double hyphen into an en/em dash ("--directory" renders as
/// "–directory"); inside a code span it stays literal, and flags read as
/// code anyway. Only called on text OUTSIDE existing code spans, so
/// already-backticked flags are never double-wrapped.
fn backtick_flags(s: &str) -> String {
    use std::sync::LazyLock;
    // A flag starts at the beginning of the text or after whitespace/an
    // opening paren, and runs while [A-Za-z0-9-]; trailing punctuation
    // (".", ";", ":") naturally falls outside the match.
    static FLAG: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(^|[\s(])(--[A-Za-z][A-Za-z0-9-]*)").expect("static pattern")
    });
    FLAG.replace_all(s, "$1`$2`").into_owned()
}

/// Escape a markdown table cell: MDX escapes plus pipe escaping, newlines
/// collapsed so a multi-line help string cannot split the row.
fn esc_cell(s: &str) -> String {
    esc_mdx(s).replace(['\n', '\t'], " ").replace('|', "\\|")
}

fn yaml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{Arg, ArgAction};

    fn meta() -> PageMeta {
        PageMeta {
            title: "iii CLI reference".to_string(),
            description: "Test page".to_string(),
            owner: "devrel".to_string(),
            intro: "Intro line.".to_string(),
            delegated: BTreeMap::new(),
            mdx_only_notes: BTreeMap::new(),
        }
    }

    fn sample_cli() -> Command {
        Command::new("iii")
            .about("Process communication engine")
            .arg(
                Arg::new("config")
                    .short('c')
                    .long("config")
                    .value_name("FILE")
                    .default_value("config.yaml")
                    .help("Path to the config file"),
            )
            .arg(
                Arg::new("secret")
                    .long("secret")
                    .hide(true)
                    .action(ArgAction::SetTrue),
            )
            .subcommand(
                Command::new("trigger")
                    .about("Invoke a function on a running iii engine")
                    .arg(
                        Arg::new("function")
                            .value_name("FUNCTION")
                            .required(true)
                            .help("Function path like scope::id"),
                    ),
            )
            .subcommand(
                Command::new("project")
                    .about("Manage iii projects")
                    .subcommand(Command::new("init").about("Scaffold a new project")),
            )
            .subcommand(Command::new("ghost").hide(true).about("Hidden daemon"))
    }

    #[test]
    fn renders_frontmatter_banner_and_intro() {
        let mdx = render_mdx(sample_cli(), &meta());
        assert!(mdx.starts_with("---\ntitle: \"iii CLI reference\"\n"));
        assert!(mdx.contains("type: \"reference\"\n"));
        assert!(mdx.contains("{/* AUTO-GENERATED FILE, DO NOT EDIT."));
        assert!(mdx.contains("Intro line."));
        // Banner is a single-line MDX comment (formatters mangle multi-line).
        let banner_line = mdx.lines().find(|l| l.contains("AUTO-GENERATED")).unwrap();
        assert!(banner_line.ends_with("*/}"));
    }

    #[test]
    fn renders_nested_sections_with_full_paths() {
        let mdx = render_mdx(sample_cli(), &meta());
        assert!(mdx.contains("## `iii`\n"));
        assert!(mdx.contains("### `iii trigger`\n"));
        assert!(mdx.contains("### `iii project`\n"));
        assert!(mdx.contains("#### `iii project init`\n"));
        // Usage lines carry the full command path.
        assert!(
            mdx.contains("iii project init"),
            "usage should show full path:\n{mdx}"
        );
    }

    #[test]
    fn skips_hidden_commands_hidden_args_and_help() {
        let mdx = render_mdx(sample_cli(), &meta());
        assert!(!mdx.contains("ghost"));
        assert!(!mdx.contains("--secret"));
        assert!(!mdx.contains("`help`"));
        assert!(!mdx.contains("--help"));
    }

    #[test]
    fn subcommands_labeled_and_sorted_alphabetically() {
        let cmd = Command::new("iii")
            .subcommand(Command::new("update").about("u"))
            .subcommand(Command::new("console").about("c"))
            .subcommand(Command::new("trigger").about("t"))
            .subcommand(Command::new("cloud").about("k"));
        let mdx = render_mdx(cmd, &meta());
        assert!(mdx.contains("**Subcommands:**\n\n| Command |"));
        // Table rows sorted a-z regardless of declaration order.
        let rows: Vec<usize> = ["[`cloud`]", "[`console`]", "[`trigger`]", "[`update`]"]
            .iter()
            .map(|n| mdx.find(*n).unwrap())
            .collect();
        assert!(
            rows.windows(2).all(|w| w[0] < w[1]),
            "rows not sorted:\n{mdx}"
        );
        // Child sections follow the same order.
        let sections: Vec<usize> = [
            "### `iii cloud`",
            "### `iii console`",
            "### `iii trigger`",
            "### `iii update`",
        ]
        .iter()
        .map(|n| mdx.find(*n).unwrap())
        .collect();
        assert!(
            sections.windows(2).all(|w| w[0] < w[1]),
            "sections not sorted:\n{mdx}"
        );
    }

    #[test]
    fn subcommand_table_only_on_root_section() {
        let mdx = render_mdx(sample_cli(), &meta());
        // Root table links to section anchors.
        assert!(mdx.contains("[`trigger`](#iii-trigger)"));
        assert!(mdx.contains("[`project`](#iii-project)"));
        // Nested commands get no table (their child sections follow
        // directly), but the child sections still render.
        assert!(
            !mdx.contains("[`init`](#iii-project-init)"),
            "nested table should be gone:\n{mdx}"
        );
        assert_eq!(mdx.matches("**Subcommands:**").count(), 1);
        assert!(mdx.contains("#### `iii project init`"));
    }

    #[test]
    fn options_table_includes_default_and_value_name() {
        let mdx = render_mdx(sample_cli(), &meta());
        assert!(mdx.contains("`-c, --config <FILE>`"));
        assert!(mdx.contains("[default: config.yaml]"));
    }

    #[test]
    fn respects_explicit_bin_name_for_root_path() {
        let cmd = Command::new("iii-worker")
            .bin_name("iii worker")
            .about("worker runtime")
            .subcommand(Command::new("add").about("Install a worker"));
        let mdx = render_mdx(cmd, &meta());
        assert!(mdx.contains("## `iii worker`\n"));
        assert!(mdx.contains("### `iii worker add`\n"));
        assert!(mdx.contains("[`add`](#iii-worker-add)"));
    }

    #[test]
    fn delegated_subcommands_link_out_and_do_not_recurse() {
        let cmd = Command::new("iii")
            .subcommand(Command::new("worker").about("Manage workers"))
            .subcommand(Command::new("cloud").about("Manage deployments"));
        let mut m = meta();
        m.delegated.insert(
            "worker".to_string(),
            Delegated {
                link: Some("./iii-worker".to_string()),
                note: "Documented on its own page.".to_string(),
            },
        );
        m.delegated.insert(
            "cloud".to_string(),
            Delegated {
                link: None,
                note: "External binary; run `iii cloud --help`.".to_string(),
            },
        );
        let mdx = render_mdx(cmd, &m);
        assert!(mdx.contains("[`worker`](./iii-worker)"));
        assert!(mdx.contains("Documented on its own page."));
        assert!(!mdx.contains("## `iii worker`"), "must not recurse:\n{mdx}");
        assert!(mdx.contains("| `cloud` | External binary; run `iii cloud --help`. |"));
    }

    #[test]
    fn escapes_mdx_hazards_in_about_and_cells() {
        let cmd = Command::new("iii")
            .about("Use <ID> and {braces} carefully")
            .arg(
                Arg::new("filter")
                    .long("filter")
                    .value_name("EXPR")
                    .help("Match a|b pairs"),
            );
        let mdx = render_mdx(cmd, &meta());
        assert!(mdx.contains("Use \\<ID> and \\{braces\\} carefully"));
        assert!(mdx.contains("Match a\\|b pairs"));
    }

    #[test]
    fn keeps_explicit_version_flag_drops_auto_version() {
        let auto = Command::new("iii")
            .version("1.0")
            .subcommand(Command::new("x"));
        let mdx = render_mdx(auto, &meta());
        assert!(!mdx.contains("--version"));

        let explicit = Command::new("iii").arg(
            Arg::new("version")
                .short('v')
                .long("version")
                .action(ArgAction::SetTrue)
                .help("Print version and exit"),
        );
        let mdx = render_mdx(explicit, &meta());
        assert!(mdx.contains("`-v, --version`"));
    }

    #[test]
    fn fragment_has_no_page_chrome_and_inlines_intro_as_lead_in() {
        let cmd = Command::new("iii-worker")
            .bin_name("iii worker")
            .about("iii managed worker runtime")
            .subcommand(Command::new("add").about("Install a worker"));
        let mdx = render_mdx_fragment(cmd, &meta());
        assert!(
            mdx.starts_with("## `iii worker`\n"),
            "fragment must start at the root heading:\n{mdx}"
        );
        assert!(!mdx.contains("---\n"), "no frontmatter in fragments");
        assert!(!mdx.contains("AUTO-GENERATED"), "no banner in fragments");
        // Intro renders inside the section, after the about line.
        let about_pos = mdx.find("iii managed worker runtime").unwrap();
        let intro_pos = mdx.find("Intro line.").unwrap();
        let usage_pos = mdx.find("```bash").unwrap();
        assert!(about_pos < intro_pos && intro_pos < usage_pos);
        // Children render normally.
        assert!(mdx.contains("### `iii worker add`"));
    }

    #[test]
    fn after_help_renders_as_note_on_both_surfaces() {
        let cmd = Command::new("iii").subcommand(
            Command::new("trigger")
                .about("Invoke a function")
                .after_help("Schemas come from <workers>; output varies."),
        );
        let mdx = render_mdx(cmd, &meta());
        // Wrapped in a Note callout, MDX-escaped, inside the right section.
        let section = mdx.split("### `iii trigger`").nth(1).unwrap();
        assert!(
            section.contains("<Note>\n  Schemas come from \\<workers>; output varies.\n</Note>"),
            "after_help missing or mangled:\n{mdx}"
        );
    }

    #[test]
    fn docs_only_notes_insert_raw_mdx_at_the_keyed_path() {
        let cmd = Command::new("iii").subcommand(
            Command::new("project")
                .about("Manage projects")
                .subcommand(Command::new("init").about("Scaffold")),
        );
        let mut m = meta();
        m.mdx_only_notes.insert(
            "iii project init".to_string(),
            "<Warning>\n  Raw MDX, [link](../somewhere) intact.\n</Warning>".to_string(),
        );
        let mdx = render_mdx(cmd, &m);
        let section = mdx.split("#### `iii project init`").nth(1).unwrap();
        assert!(
            section.contains("<Warning>\n  Raw MDX, [link](../somewhere) intact.\n</Warning>"),
            "docs-only note missing:\n{mdx}"
        );
        // Not duplicated into other sections.
        assert_eq!(mdx.matches("<Warning>").count(), 1);
    }

    #[test]
    #[should_panic(expected = "contains lowercase")]
    fn lowercase_value_names_fail_generation() {
        let cmd = Command::new("iii")
            .subcommand(Command::new("update").arg(Arg::new("target").value_name("command")));
        render_mdx(cmd, &meta());
    }

    #[test]
    fn bare_flags_get_backticked_against_smart_dashes() {
        let cmd = Command::new("iii")
            .about("Pass --no-watch for a snapshot. Equivalent to --directory.")
            .arg(
                Arg::new("frozen")
                    .long("frozen")
                    .action(ArgAction::SetTrue)
                    .help("Pass --frozen in CI (see `add --force` and non-empty dirs)"),
            );
        let mdx = render_mdx(cmd, &meta());
        // Bare flags wrapped, trailing punctuation left outside.
        assert!(mdx.contains("Pass `--no-watch` for a snapshot. Equivalent to `--directory`."));
        // Wrapped in cells too; already-backticked spans untouched (no
        // double wrap), hyphenated words untouched.
        assert!(mdx.contains("Pass `--frozen` in CI (see `add --force` and non-empty dirs)"));
        assert!(!mdx.contains("``--"), "double-wrapped flag:\n{mdx}");
    }

    #[test]
    fn no_escaping_inside_inline_code_spans() {
        let cmd = Command::new("iii").arg(
            Arg::new("json")
                .long("json")
                .value_name("JSON")
                .help("JSON payload (`--json '{\"a\":1}'`) with <raw> outside"),
        );
        let mdx = render_mdx(cmd, &meta());
        // Braces inside the code span stay literal; < outside is escaped.
        assert!(
            mdx.contains("`--json '{\"a\":1}'`"),
            "span corrupted:\n{mdx}"
        );
        assert!(mdx.contains("\\<raw>"));
    }

    #[test]
    fn flag_without_value_hides_implicit_false_default() {
        let cmd = Command::new("iii").arg(
            Arg::new("force")
                .long("force")
                .action(ArgAction::SetTrue)
                .help("Force it"),
        );
        let mdx = render_mdx(cmd, &meta());
        assert!(!mdx.contains("[default: false]"), "noise:\n{mdx}");
        assert!(mdx.contains("| `--force` | Force it |"));
    }

    #[test]
    fn env_and_possible_values_render() {
        let cmd = Command::new("iii").arg(
            Arg::new("mode")
                .long("mode")
                .env("III_MODE")
                .value_parser(["fast", "safe"])
                .help("Run mode"),
        );
        let mdx = render_mdx(cmd, &meta());
        assert!(mdx.contains("[env: III_MODE]"));
        assert!(mdx.contains("[possible values: fast, safe]"));
    }
}
