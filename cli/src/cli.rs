use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "iii-cli",
    about = "Unified CLI dispatcher for iii tools",
    version
)]
pub struct Cli {
    /// Disable background update and advisory checks
    #[arg(long, global = true)]
    pub no_update_check: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Launch the iii web console
    #[command(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        disable_help_flag = true
    )]
    Console {
        /// Arguments passed through to the binary
        #[arg(num_args = 0..)]
        args: Vec<String>,
    },

    /// Create a new iii project from a template
    #[command(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        disable_help_flag = true
    )]
    Create {
        /// Arguments passed through to the binary
        #[arg(num_args = 0..)]
        args: Vec<String>,
    },

    /// Manage SDKs powered by Motia
    #[command(subcommand)]
    Sdk(SdkCommands),

    /// Start the iii process communication engine
    #[command(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        disable_help_flag = true
    )]
    Start {
        /// Arguments passed through to the binary
        #[arg(num_args = 0..)]
        args: Vec<String>,
    },

    /// Install a worker from the registry (or all workers from iii.toml if no name given)
    Install {
        /// Worker name to install, optionally with version (e.g., "pdfkit" or "pdfkit@1.0.0")
        #[arg(value_name = "WORKER[@VERSION]")]
        worker_name: Option<String>,

        /// Overwrite existing config.yaml entries without prompting
        #[arg(long, short)]
        force: bool,
    },

    /// Uninstall a worker (removes binary, manifest entry, and config)
    Uninstall {
        /// Worker name to uninstall (e.g., "pdfkit")
        #[arg(value_name = "WORKER")]
        worker_name: String,
    },

    /// Update iii-cli and managed binaries to their latest versions
    Update {
        /// Specific command or binary to update (e.g., "console", "self").
        /// Use "self" or "iii-cli" to update only iii-cli.
        /// If omitted, updates iii-cli and all installed binaries.
        #[arg(name = "command")]
        target: Option<String>,
    },

    /// List installed workers and their versions
    List,

    /// Show details about a worker from the registry
    Info {
        /// Worker name to inspect (e.g., "pdfkit")
        #[arg(value_name = "WORKER")]
        worker_name: String,
    },
}

/// SDK subcommands
#[derive(Subcommand)]
pub enum SdkCommands {
    /// Motia SDK tools
    #[command(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        disable_help_flag = true
    )]
    Motia {
        /// Arguments passed through to the binary
        #[arg(num_args = 0..)]
        args: Vec<String>,
    },
}

/// Extract the command name and passthrough args from a parsed Commands value.
pub fn extract_command_info(cmd: &Commands) -> CommandInfo<'_> {
    match cmd {
        Commands::Console { args } => CommandInfo::Dispatch {
            command: "console",
            args,
        },
        Commands::Create { args } => CommandInfo::Dispatch {
            command: "create",
            args,
        },
        Commands::Sdk(sdk_cmd) => match sdk_cmd {
            SdkCommands::Motia { args } => CommandInfo::Dispatch {
                command: "motia",
                args,
            },
        },
        Commands::Start { args } => CommandInfo::Dispatch {
            command: "start",
            args,
        },
        Commands::Install { worker_name, force } => CommandInfo::Install {
            worker_name: worker_name.as_deref(),
            force: *force,
        },
        Commands::Uninstall { worker_name } => CommandInfo::Uninstall {
            worker_name: worker_name.as_str(),
        },
        Commands::Update { target } => CommandInfo::Update {
            target: target.as_deref(),
        },
        Commands::List => CommandInfo::List,
        Commands::Info { worker_name } => CommandInfo::Info {
            worker_name: worker_name.as_str(),
        },
    }
}

/// Parsed command information for the main dispatcher.
pub enum CommandInfo<'a> {
    /// Dispatch to a managed binary with passthrough args
    Dispatch {
        command: &'static str,
        args: &'a [String],
    },
    /// Update command
    Update { target: Option<&'a str> },
    /// Install a worker
    Install {
        worker_name: Option<&'a str>,
        force: bool,
    },
    /// Uninstall a worker
    Uninstall { worker_name: &'a str },
    /// List installed binaries
    List,
    /// Inspect worker details
    Info { worker_name: &'a str },
}
