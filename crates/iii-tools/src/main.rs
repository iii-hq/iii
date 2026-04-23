//! iii CLI - Project scaffolding for iii workflows

use anyhow::Result;
use clap::{Parser, Subcommand};
use scaffolder_core::ProductConfig;
use scaffolder_core::tui::CreateArgs;
use std::path::PathBuf;

/// CLI version
pub const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// iii product configuration
#[derive(Clone)]
pub struct IiiConfig;

impl ProductConfig for IiiConfig {
    fn name(&self) -> &'static str {
        "iii"
    }

    fn display_name(&self) -> &'static str {
        "iii"
    }

    fn default_template_url(&self) -> &'static str {
        "https://github.com/iii-hq/templates.git"
    }

    fn template_url_env(&self) -> &'static str {
        "III_TEMPLATE_URL"
    }

    fn requires_iii(&self) -> bool {
        true
    }

    fn docs_url(&self) -> &'static str {
        "https://iii.dev/docs"
    }

    fn cli_description(&self) -> &'static str {
        "CLI for scaffolding iii projects"
    }

    fn upgrade_command(&self) -> &'static str {
        "cargo install iii-tools --force"
    }
}

#[derive(Parser, Debug)]
#[command(name = "iii", bin_name = "iii")]
#[command(about = "CLI for scaffolding iii projects")]
#[command(version)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Create a new iii project
    Create(CliCreateArgs),
}

#[derive(Parser, Debug)]
pub struct CliCreateArgs {
    /// Local directory to use for templates instead of fetching from remote (for development use)
    #[arg(long = "template-dir")]
    pub template_dir: Option<PathBuf>,

    /// Template name to use
    #[arg(short, long)]
    pub template: Option<String>,

    /// Project directory to create
    #[arg(short, long)]
    pub directory: Option<PathBuf>,

    /// Languages to include (comma-separated: ts,js,py or typescript,javascript,python)
    #[arg(short, long, value_delimiter = ',')]
    pub languages: Option<Vec<String>>,

    /// Skip iii installation check
    #[arg(long = "skip-iii")]
    pub skip_iii: bool,

    /// Auto-confirm all prompts (non-interactive mode)
    #[arg(short, long)]
    pub yes: bool,
}

impl From<CliCreateArgs> for CreateArgs {
    fn from(args: CliCreateArgs) -> Self {
        CreateArgs {
            template_dir: args.template_dir,
            template: args.template,
            directory: args.directory,
            languages: args.languages,
            skip_tool_check: args.skip_iii,
            yes: args.yes,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Ensure terminal cursor is restored on panic
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = console::Term::stderr().show_cursor();
        default_panic(info);
    }));

    // Handle Ctrl+C gracefully
    ctrlc::set_handler(move || {
        let _ = console::Term::stderr().show_cursor();
        std::process::exit(130);
    })
    .ok();

    let args = Args::parse();
    let config = IiiConfig;

    // Handle subcommands
    match args.command {
        Some(Command::Create(create_args)) => {
            // Run the TUI application with the create args
            let result = scaffolder_core::run(&config, create_args.into(), CLI_VERSION).await;

            // Ensure cursor is visible on normal exit
            let _ = console::Term::stderr().show_cursor();

            result
        }
        None => {
            // No subcommand provided, default to create behavior (interactive mode)
            let create_args = CreateArgs::default();
            let result = scaffolder_core::run(&config, create_args, CLI_VERSION).await;

            // Ensure cursor is visible on normal exit
            let _ = console::Term::stderr().show_cursor();

            result
        }
    }
}
