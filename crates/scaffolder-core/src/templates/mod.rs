//! Template fetching, parsing, and copying
//!
//! This module provides:
//! - Template manifest types (RootManifest, TemplateManifest)
//! - Template fetching from remote URLs or local directories
//! - Template copying with language-based filtering
//! - Version compatibility checking

pub mod copier;
pub mod fetcher;
pub mod manifest;
pub mod version;

use crate::product::ProductConfig;
use anyhow::{Context, Result};
use colored::Colorize;
use std::path::PathBuf;

pub use copier::copy_template;
pub use fetcher::{TemplateFetcher, TemplateSource};
pub use manifest::{LanguageFiles, RootManifest, SharedFile, TemplateManifest};
pub use version::check_compatibility;

/// Build zip files for all templates in a directory
pub async fn build_zips<C: ProductConfig>(
    config: &C,
    template_dir: &Option<PathBuf>,
) -> Result<()> {
    let dir = template_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("templates"));

    if !dir.exists() {
        anyhow::bail!("Template directory not found: {}", dir.display());
    }

    let manifest_path = dir.join("template.yaml");
    if !manifest_path.exists() {
        anyhow::bail!("Root template.yaml not found in {}", dir.display());
    }

    // Read root manifest to get template list
    let manifest_content = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
    let root_manifest: manifest::RootManifest = serde_yaml::from_str(&manifest_content)
        .context("Failed to parse root template.yaml")?;

    println!(
        "{}",
        format!("Building {} template zips...", config.display_name())
            .cyan()
            .bold()
    );
    println!();

    let mut built = 0;
    for template_name in &root_manifest.templates {
        let template_path = dir.join(template_name);
        if !template_path.exists() {
            eprintln!(
                "{} Template directory not found: {}",
                "Warning:".yellow(),
                template_path.display()
            );
            continue;
        }

        print!("  {} {}...", "->".blue(), template_name);

        match fetcher::TemplateFetcher::build_local_zip(
            &dir,
            template_name,
            &root_manifest.shared_files,
        ) {
            Ok(zip_bytes) => {
                let zip_path = dir.join(format!("{}.zip", template_name));
                std::fs::write(&zip_path, &zip_bytes)
                    .with_context(|| format!("Failed to write {}", zip_path.display()))?;
                println!(" {} ({} bytes)", "done".green(), zip_bytes.len());
                built += 1;
            }
            Err(e) => {
                println!(" {}", "failed".red());
                eprintln!("    Error: {}", e);
            }
        }
    }

    println!();
    println!(
        "{} {} template zip(s) in {}",
        "Built".green().bold(),
        built,
        dir.display()
    );

    Ok(())
}
