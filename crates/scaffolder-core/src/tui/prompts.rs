//! Charm-style CLI prompts using cliclack

use crate::product::ProductConfig;
use crate::runtime::check;
use crate::telemetry;
use crate::templates::manifest::{LanguageFiles, TemplateManifest};
use crate::templates::{copier, fetcher::TemplateFetcher, version};
use anyhow::Result;
use std::path::PathBuf;

/// CLI arguments for the create command
#[derive(Debug, Clone, Default)]
pub struct CreateArgs {
    /// Local directory to use for templates instead of fetching from remote
    pub template_dir: Option<PathBuf>,

    /// Template name to use
    pub template: Option<String>,

    /// Project directory to create
    pub directory: Option<PathBuf>,

    /// Languages to include
    pub languages: Option<Vec<String>>,

    /// Skip tool installation check (e.g., iii)
    pub skip_tool_check: bool,

    /// Auto-confirm all prompts (non-interactive mode)
    pub yes: bool,
}

/// Run the CLI with interactive prompts
pub async fn run<C: ProductConfig>(config: &C, args: CreateArgs, cli_version: &str) -> Result<()> {
    cliclack::intro(config.display_name())?;

    // Step 1: Check tool installation (skip if --skip-tool-check or product doesn't require it)
    if config.requires_iii() && !args.skip_tool_check {
        handle_tool_check(config, &args).await?;
    } else if args.skip_tool_check {
        cliclack::log::info("Skipping tool check")?;
    }

    // Step 2: Setup template fetcher
    let mut fetcher = setup_fetcher(config, &args.template_dir)?;

    // Step 3: Select template (also returns merged language_files)
    let (template_name, manifest, language_files) =
        select_template(&mut fetcher, args.template.as_deref()).await?;

    // Check version compatibility (CLI tools version — advisory)
    if let Some(warning) =
        version::check_compatibility(cli_version, &manifest.version, config.upgrade_command())
    {
        cliclack::log::warning(format!(
            "Version warning: {}",
            warning.lines().next().unwrap_or(&warning)
        ))?;
    }

    // Check iii engine version compatibility (hard block, respects --skip-tool-check)
    if !args.skip_tool_check
        && let Some(min_ver) = &manifest.min_iii_version
    {
        match version::check_iii_engine_version(min_ver) {
            Ok(installed) => {
                cliclack::log::success(format!(
                    "iii engine {} (>= {} required)",
                    installed, min_ver
                ))?;
            }
            Err(msg) => {
                cliclack::log::error(&msg)?;
                anyhow::bail!("{}", msg);
            }
        }
    }

    // Step 4: Select directory
    let project_dir = select_directory(&args)?;

    // Step 5: Select languages
    let selected_languages = select_languages(&manifest, &args)?;

    // Step 6: Check runtimes (advisory = included languages that don't cause hard fail)
    check_runtimes(&manifest, &selected_languages)?;

    // Step 7: Create project
    create_project(
        config.name(),
        cli_version,
        &mut fetcher,
        &template_name,
        &manifest,
        &project_dir,
        &selected_languages,
        &language_files,
    )
    .await?;

    // Step 8: Show next steps
    print_next_steps(&project_dir, &manifest)?;

    Ok(())
}

async fn handle_tool_check<C: ProductConfig>(_config: &C, args: &CreateArgs) -> Result<()> {
    // Create tool manager for iii
    let tool = crate::runtime::tool::iii_tool();

    let installed = tool.is_installed();

    if installed {
        let version = tool.get_version().unwrap_or_else(|| "unknown".to_string());
        cliclack::log::success(format!(
            "{} installed ({})",
            tool.config().display_name,
            version
        ))?;
        return Ok(());
    }

    cliclack::log::warning(format!("{} is not installed", tool.config().display_name))?;

    // In non-interactive mode, just skip
    if args.yes {
        cliclack::log::info(format!(
            "Continuing without {} (--yes mode)",
            tool.config().display_name
        ))?;
        return Ok(());
    }

    let action: &str = cliclack::select("What would you like to do?")
        .item(
            "install",
            format!("Install {} automatically", tool.config().display_name),
            "",
        )
        .item(
            "docs",
            format!("Open documentation ({})", tool.config().docs_url),
            "",
        )
        .item(
            "skip",
            format!("Skip and continue without {}", tool.config().display_name),
            "",
        )
        .interact()?;

    match action {
        "install" => {
            cliclack::log::info(format!("This will execute: {}", tool.install_command()))?;

            let confirm: bool = cliclack::confirm("Proceed with installation?")
                .initial_value(true)
                .interact()?;

            if confirm {
                match tool.install().await {
                    Ok(_) => {
                        cliclack::log::success(format!(
                            "{} installed successfully",
                            tool.config().display_name
                        ))?;
                    }
                    Err(e) => {
                        cliclack::log::error(format!("{}", e))?;

                        let continue_anyway: bool = cliclack::confirm(format!(
                            "Continue without {}?",
                            tool.config().display_name
                        ))
                        .initial_value(false)
                        .interact()?;

                        if !continue_anyway {
                            anyhow::bail!("Setup cancelled.");
                        }
                    }
                }
            } else {
                cliclack::log::info(format!(
                    "Continuing without {}. Refer to the docs for installation instructions: ({})",
                    tool.config().display_name,
                    tool.config().docs_url
                ))?;
            }
        }
        "docs" => {
            tool.open_docs()?;
            cliclack::outro(format!(
                "After installing {}, run this command again.",
                tool.config().display_name
            ))?;
            // Restore the terminal cursor before exiting - without this, the
            // cliclack spinners leave the cursor hidden. The cursor-restore
            // in main() runs only on normal return, not on process::exit.
            let _ = console::Term::stderr().show_cursor();
            std::process::exit(0);
        }
        "skip" => {
            cliclack::log::info(format!(
                "Continuing without {}. Refer to the docs for installation instructions: ({})",
                tool.config().display_name,
                tool.config().docs_url
            ))?;
        }
        _ => {}
    }

    Ok(())
}

fn setup_fetcher<C: ProductConfig>(
    config: &C,
    template_dir: &Option<PathBuf>,
) -> Result<TemplateFetcher> {
    let fetcher = match template_dir {
        Some(path) => {
            cliclack::log::info(format!("Using local templates from {}", path.display()))?;
            TemplateFetcher::from_local(path.clone(), config.user_agent())
        }
        None => {
            cliclack::log::info("Using remote templates")?;
            TemplateFetcher::from_config(config)?
        }
    };

    Ok(fetcher)
}

async fn select_template(
    fetcher: &mut TemplateFetcher,
    specified_template: Option<&str>,
) -> Result<(String, TemplateManifest, LanguageFiles)> {
    let spinner = cliclack::spinner();
    spinner.start("Loading templates...");

    let root_manifest = fetcher.fetch_root_manifest().await?;

    // Helper to merge language files from root and template
    let merge_language_files = |manifest: &TemplateManifest| -> LanguageFiles {
        let mut merged = root_manifest.language_files.clone();
        merged.merge(&manifest.language_files);
        merged
    };

    // If a template was specified via --template flag, use it directly
    if let Some(template_name) = specified_template {
        if !root_manifest.templates.contains(&template_name.to_string()) {
            spinner.stop("Failed to load templates");
            let available = root_manifest.templates.join(", ");
            anyhow::bail!(
                "Template '{}' not found. Available templates: {}",
                template_name,
                available
            );
        }

        let manifest = fetcher.fetch_template_manifest(template_name).await?;
        let language_files = merge_language_files(&manifest);
        spinner.stop(format!(
            "Template: {} - {}",
            manifest.name, manifest.description
        ));
        return Ok((template_name.to_string(), manifest, language_files));
    }

    let mut templates: Vec<(String, TemplateManifest)> = Vec::new();
    for template_name in &root_manifest.templates {
        let manifest = fetcher.fetch_template_manifest(template_name).await?;
        templates.push((template_name.clone(), manifest));
    }

    spinner.stop("Templates loaded");

    if templates.is_empty() {
        anyhow::bail!("No templates found.");
    }

    // If only one template, use it automatically
    if templates.len() == 1 {
        let (name, manifest) = templates.into_iter().next().unwrap();
        let language_files = merge_language_files(&manifest);
        cliclack::log::info(format!(
            "Using template: {} - {}",
            manifest.name, manifest.description
        ))?;
        return Ok((name, manifest, language_files));
    }

    // Build select prompt - use indices to avoid borrow issues
    let mut select = cliclack::select("Select a template");
    for (idx, (_, manifest)) in templates.iter().enumerate() {
        select = select.item(idx, &manifest.name, &manifest.description);
    }

    let selected_idx: usize = select.interact()?;

    let (name, manifest) = templates.into_iter().nth(selected_idx).unwrap();

    let language_files = merge_language_files(&manifest);

    Ok((name, manifest, language_files))
}

fn select_directory(args: &CreateArgs) -> Result<PathBuf> {
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Use --directory flag if provided
    let path = if let Some(dir) = &args.directory {
        let p = if dir.is_absolute() {
            dir.clone()
        } else {
            current_dir.join(dir)
        };
        cliclack::log::info(format!("Using directory: {}", p.display()))?;
        p
    } else {
        let input: String = cliclack::input("Project directory")
            .placeholder(".")
            .default_input(".")
            .interact()?;

        if input.is_empty() || input == "." {
            current_dir
        } else {
            let p = PathBuf::from(&input);
            if p.is_absolute() {
                p
            } else {
                current_dir.join(p)
            }
        }
    };

    // Validate parent directory exists
    if let Some(parent) = path.parent()
        && !parent.exists()
        && parent != std::path::Path::new("")
    {
        anyhow::bail!("Parent directory does not exist: {}", parent.display());
    }

    // Warn if directory exists and has files
    if path.exists()
        && path.is_dir()
        && let Ok(entries) = std::fs::read_dir(&path)
    {
        let count = entries.count();
        if count > 0 {
            cliclack::log::warning(format!("Directory has {} existing items", count))?;

            // Auto-confirm with --yes flag
            let confirm = if args.yes {
                true
            } else {
                cliclack::confirm("Continue anyway?")
                    .initial_value(true)
                    .interact()?
            };

            if !confirm {
                anyhow::bail!("Setup cancelled.");
            }
        }
    }

    Ok(path)
}

/// Parse language string to Language enum
fn parse_language(s: &str) -> Option<check::Language> {
    match s.to_lowercase().as_str() {
        "typescript" | "ts" => Some(check::Language::TypeScript),
        "javascript" | "js" => Some(check::Language::JavaScript),
        "python" | "py" => Some(check::Language::Python),
        "rust" | "rs" => Some(check::Language::Rust),
        _ => None,
    }
}

fn select_languages(
    manifest: &TemplateManifest,
    args: &CreateArgs,
) -> Result<Vec<check::Language>> {
    let mut required_languages: Vec<check::Language> = Vec::new();
    let mut included_languages: Vec<check::Language> = Vec::new();
    let mut optional_languages: Vec<check::Language> = Vec::new();

    let treat_as_included = manifest.treat_required_as_included;

    for (lang_str, lang) in [
        ("typescript", check::Language::TypeScript),
        ("javascript", check::Language::JavaScript),
        ("python", check::Language::Python),
        ("rust", check::Language::Rust),
    ] {
        if manifest.is_required(lang_str) {
            if treat_as_included {
                included_languages.push(lang);
            } else {
                required_languages.push(lang);
            }
        } else if manifest.is_optional(lang_str) {
            optional_languages.push(lang);
        }
    }

    if !required_languages.is_empty() {
        let names: Vec<&str> = required_languages
            .iter()
            .map(|l| l.display_name())
            .collect();
        cliclack::log::info(format!("Required: {}", names.join(", ")))?;
    }

    let mut selected_languages = required_languages.clone();
    selected_languages.extend(included_languages.iter().copied());

    let selectable: Vec<check::Language> = optional_languages.clone();

    if let Some(lang_args) = &args.languages {
        for lang_str in lang_args {
            if let Some(lang) = parse_language(lang_str) {
                if selectable.contains(&lang) && !selected_languages.contains(&lang) {
                    selected_languages.push(lang);
                }
            } else {
                cliclack::log::warning(format!("Unknown language: {}", lang_str))?;
            }
        }
    } else if !selectable.is_empty() {
        if args.yes {
            let to_add: Vec<_> = selectable
                .iter()
                .filter(|l| !selected_languages.contains(l))
                .copied()
                .collect();
            selected_languages.extend(to_add);
        } else {
            // We only reach this arm when `selectable` (= optional_languages)
            // is non-empty, so the prompt is always offering optional additions
            // on top of any required/included languages already selected.
            let mut multi = cliclack::multiselect("Select additional languages (optional)");

            for lang in &selectable {
                multi = multi.item(*lang, lang.display_name(), "");
            }

            let selected: Vec<check::Language> = multi.required(false).interact()?;

            selected_languages = required_languages.clone();
            selected_languages.extend(included_languages.iter().copied());
            for lang in selected {
                if !selected_languages.contains(&lang) {
                    selected_languages.push(lang);
                }
            }
        }
    }

    if selected_languages.is_empty() {
        anyhow::bail!("No languages available for this template.");
    }

    let included_set: std::collections::HashSet<_> = included_languages.iter().collect();
    let selected_set: std::collections::HashSet<_> = selected_languages.iter().collect();
    if !included_languages.is_empty() && included_set != selected_set {
        let names: Vec<&str> = included_languages
            .iter()
            .map(|l| l.display_name())
            .collect();
        cliclack::log::info(format!("Included: {}", names.join(", ")))?;
    }

    let lang_names: Vec<&str> = selected_languages
        .iter()
        .map(|l| l.display_name())
        .collect();
    cliclack::log::success(format!("Project languages: {}", lang_names.join(", ")))?;

    Ok(selected_languages)
}

fn check_runtimes(
    manifest: &TemplateManifest,
    selected_languages: &[check::Language],
) -> Result<()> {
    let advisory: Vec<check::Language> = manifest
        .included_language_names()
        .iter()
        .filter_map(|s| parse_language(s))
        .filter(|l| selected_languages.contains(l))
        .collect();

    let spinner = cliclack::spinner();
    spinner.start("Checking runtimes...");

    match check::check_runtimes_with_advisory(selected_languages, &advisory) {
        Ok(runtimes) => {
            let runtime_info: Vec<String> = runtimes
                .iter()
                .map(|r| {
                    if r.available {
                        format!("{} ({})", r.name, r.version.as_deref().unwrap_or("unknown"))
                    } else {
                        format!("{} (not installed)", r.name)
                    }
                })
                .collect();
            spinner.stop(format!("Detected runtimes: {}", runtime_info.join(", ")));
            Ok(())
        }
        Err(e) => {
            spinner.stop("Missing runtimes");
            cliclack::log::error(format!("{}", e))?;
            anyhow::bail!("Please install the missing runtimes and try again.");
        }
    }
}

async fn create_project(
    product_name: &'static str,
    cli_version: &str,
    fetcher: &mut TemplateFetcher,
    template_name: &str,
    manifest: &TemplateManifest,
    project_dir: &PathBuf,
    selected_languages: &[check::Language],
    language_files: &LanguageFiles,
) -> Result<()> {
    let spinner = cliclack::spinner();
    spinner.start("Creating project...");

    let copied_files = copier::copy_template(
        fetcher,
        template_name,
        manifest,
        project_dir,
        selected_languages,
        language_files,
    )
    .await?;

    let project_id = uuid::Uuid::new_v4().to_string();
    let project_name = project_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    telemetry::write_project_ini(project_dir, &project_id, &project_name, template_name).await?;

    let platform = telemetry::platform_for_product(product_name);
    let tools_version = cli_version.to_string();
    let created_handle = telemetry::spawn_project_event(
        "project_created",
        platform,
        tools_version,
        serde_json::json!({
            "project_id": project_id,
            "project_name": project_name,
            "template": template_name,
            "product": product_name,
        }),
    );

    spinner.stop(format!(
        "Created {} files in {}",
        copied_files.len() + 1,
        project_dir.display()
    ));

    let install_spinner = cliclack::spinner();
    install_spinner.start("Installing dependencies (when applicable)...");
    match telemetry::run_dependency_install(project_dir, selected_languages).await {
        Ok(()) => install_spinner.stop("Dependency step finished"),
        Err(e) => {
            install_spinner.stop("Dependency installation failed");
            return Err(e);
        }
    }

    // Best-effort flush: wait up to 2s for telemetry to complete, then move on
    if let Some(handle) = created_handle {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
    }

    Ok(())
}

fn print_next_steps(
    project_dir: &PathBuf,
    manifest: &crate::templates::manifest::TemplateManifest,
) -> Result<()> {
    let mut steps: Vec<String> = Vec::new();

    if let Ok(current) = std::env::current_dir()
        && current != *project_dir
    {
        steps.push(format!("cd {}", project_dir.display()));
    }

    steps.extend(manifest.next_steps.iter().cloned());

    if !steps.is_empty() {
        println!();
        println!("  Next steps");
        println!();

        for (i, step) in steps.iter().enumerate() {
            println!("  {}.  {}", i + 1, step);
        }
    }

    cliclack::outro("Happy coding!")?;

    Ok(())
}
