use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use notify::{
    Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{CreateKind, DataChange, ModifyKind, RemoveKind, RenameMode},
};
use serde_json::Value;
use tokio::sync::mpsc;

use super::config::TypeGenConfig;
use super::generators::typescript::TypeScriptGenerator;
use super::generators::TypeGenerator;
use super::parsers::typescript::TypeScriptParser;
use super::parsers::LanguageParser;
use crate::engine::Engine;
use crate::modules::core_module::CoreModule;
use crate::modules::shell::glob_exec::GlobExec;

#[derive(Clone)]
pub struct TypeGenCoreModule {
    config: TypeGenConfig,
}

#[async_trait::async_trait]
impl CoreModule for TypeGenCoreModule {
    async fn create(
        _engine: Arc<Engine>,
        config: Option<Value>,
    ) -> Result<Box<dyn CoreModule>> {
        let config: TypeGenConfig = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        Ok(Box::new(Self { config }))
    }

    fn register_functions(&self, _engine: Arc<Engine>) {}

    async fn initialize(&self) -> Result<()> {
        let config = self.config.clone();

        tokio::spawn(async move {
            if let Err(err) = Self::run_type_generation_with_watch(config).await {
                tracing::error!("Type generation failed: {:?}", err);
            }
        });

        Ok(())
    }
}

const MAX_WATCH_EVENTS: usize = 100;

impl TypeGenCoreModule {
    async fn run_type_generation_with_watch(config: TypeGenConfig) -> Result<()> {
        let (tx, mut rx) = mpsc::channel::<Event>(MAX_WATCH_EVENTS);
        
        let glob_exec = GlobExec::new(config.watch.clone());
        
        let mut watcher: RecommendedWatcher = Watcher::new(
            move |res| {
                if let Ok(event) = res {
                    let _ = tx.blocking_send(event);
                }
            },
            Config::default(),
        )?;

        for root in glob_exec.watch_roots() {
            watcher.watch(
                Path::new(&root.path),
                if root.recursive {
                    RecursiveMode::Recursive
                } else {
                    RecursiveMode::NonRecursive
                },
            )?;
        }

        tracing::info!("Type generator watching for changes...");
        
        Self::run_type_generation(&config).await?;

        let cwd = std::env::current_dir().unwrap_or_default();

        while let Some(event) = rx.recv().await {
            if !Self::should_regenerate(&event, &glob_exec, &cwd) {
                continue;
            }

            tracing::info!(
                "File change detected {} → regenerating types",
                event
                    .paths
                    .iter()
                    .map(|p| {
                        p.strip_prefix(&cwd)
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|_| p.to_string_lossy().to_string())
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
                    .purple()
            );

            if let Err(err) = Self::run_type_generation(&config).await {
                tracing::error!("Type generation failed: {:?}", err);
            }
        }

        Ok(())
    }

    fn should_regenerate(event: &Event, glob_exec: &GlobExec, cwd: &Path) -> bool {
        let is_valid_event = matches!(
            event.kind,
            EventKind::Create(CreateKind::File)
                | EventKind::Modify(ModifyKind::Data(DataChange::Content))
                | EventKind::Modify(ModifyKind::Name(RenameMode::Any))
                | EventKind::Remove(RemoveKind::File)
        );

        if !is_valid_event {
            return false;
        }

        event
            .paths
            .iter()
            .any(|path| glob_exec.should_trigger(path.strip_prefix(cwd).unwrap_or(path)))
    }

    async fn run_type_generation(config: &TypeGenConfig) -> Result<()> {
        tracing::debug!("Generating types with config: {:?}", config);

        let mut all_steps = Vec::new();
        let mut all_streams = Vec::new();

        let parser: Box<dyn LanguageParser> = match config.language.as_str() {
            "typescript" => Box::new(TypeScriptParser),
            _ => {
                tracing::warn!("Unsupported language: {}", config.language);
                return Ok(());
            }
        };

        for pattern in &config.watch {
            let files = Self::glob_files(pattern)?;

            for file_path in files {
                if !parser.supports_extension(
                    file_path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or(""),
                ) {
                    continue;
                }

                match std::fs::read_to_string(&file_path) {
                    Ok(content) => match parser.parse_file(&file_path, &content) {
                        Ok(parsed) => {
                            all_steps.extend(parsed.steps);
                            all_streams.extend(parsed.streams);
                        }
                        Err(err) => {
                            tracing::warn!("Failed to parse {:?}: {:?}", file_path, err);
                        }
                    },
                    Err(err) => {
                        tracing::warn!("Failed to read {:?}: {:?}", file_path, err);
                    }
                }
            }
        }

        tracing::info!(
            "Parsed {} steps and {} streams",
            all_steps.len(),
            all_streams.len()
        );

        for stream in &all_streams {
            tracing::debug!("Stream '{}': {:?}", stream.name, stream.schema);
        }

        let generator: Box<dyn TypeGenerator> = match config.language.as_str() {
            "typescript" => Box::new(TypeScriptGenerator),
            _ => {
                tracing::warn!("Unsupported language: {}", config.language);
                return Ok(());
            }
        };

        let output = generator.generate(&all_steps, &all_streams);

        std::fs::write(&config.output, output)?;
        tracing::info!("Type definitions written to {}", config.output);

        Ok(())
    }

    fn glob_files(pattern: &str) -> Result<Vec<std::path::PathBuf>> {
        let mut files = Vec::new();

        for entry in glob::glob(pattern)? {
            match entry {
                Ok(path) => {
                    if path.is_file() {
                        files.push(path);
                    }
                }
                Err(e) => {
                    tracing::warn!("Glob error: {:?}", e);
                }
            }
        }

        Ok(files)
    }
}

