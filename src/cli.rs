use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use crate::init;
use crate::ui;
use crate::wiki;

#[derive(Parser)]
#[command(
    name = "project-wiki",
    about = "Auto-managed project knowledge wiki for AI-assisted development",
    version
)]
struct Cli {
    /// Increase verbosity (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a .wiki/ folder in the current project
    Init {
        /// Skip automatic codebase scan
        #[arg(long)]
        no_scan: bool,

        /// Import structure from a Notion database
        #[arg(long)]
        from_notion: Option<String>,

        /// Resume a previously interrupted init
        #[arg(long)]
        resume: bool,
    },

    /// Show wiki status and health summary
    Status,

    /// Validate wiki notes for consistency
    Validate,

    /// Consult wiki notes for a domain or all domains
    Consult {
        /// Domain name to consult
        domain: Option<String>,

        /// Show all domains
        #[arg(long)]
        all: bool,
    },

    /// Display the dependency graph
    Graph,

    /// Search wiki notes
    Search {
        /// Search term
        term: String,
    },

    /// Add a domain, context, or decision
    Add {
        #[command(subcommand)]
        what: AddCommands,
    },

    /// Rebuild the entire wiki from source
    Rebuild,

    /// Regenerate the wiki index
    Index,

    /// Set a note's confidence to confirmed
    Confirm {
        /// Domain name or path to note (e.g., "billing" or "billing/payments.md")
        target: String,
    },

    /// Mark a domain or note as deprecated
    Deprecate {
        /// Domain name or path to note
        target: String,
    },

    /// Rename a domain and update all references
    RenameDomain {
        /// Current domain name
        old: String,
        /// New domain name
        new: String,
    },

    /// Import markdown files into the wiki
    Import {
        /// Path to folder containing .md files
        folder: String,
        /// Target domain (optional, auto-detected from folder name)
        #[arg(long)]
        domain: Option<String>,
    },

    /// Get wiki context for a file (used by Claude Code hooks)
    Context {
        /// File path to look up
        #[arg(long)]
        file: Option<String>,

        /// Read hook JSON from stdin (for PreToolUse hook)
        #[arg(long)]
        hook: bool,
    },

    /// Detect wiki drift after a file change (used by Claude Code hooks)
    DetectDrift {
        /// File path to check
        #[arg(long)]
        file: Option<String>,

        /// Read hook JSON from stdin (for PostToolUse hook)
        #[arg(long)]
        hook: bool,
    },

    /// Install Claude Code hooks for automatic wiki integration
    InstallHooks,

    /// Remove Claude Code hooks
    UninstallHooks,
}

#[derive(Subcommand)]
enum AddCommands {
    /// Add a new domain
    Domain {
        /// Name of the domain
        name: String,
    },

    /// Add context to a domain
    Context {
        /// Target domain
        #[arg(long)]
        domain: Option<String>,

        /// Context text
        text: String,
    },

    /// Add a business decision
    Decision {
        /// Decision text
        text: String,
    },
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    crate::verbosity::set(cli.verbose);

    match cli.command {
        Commands::Init {
            no_scan,
            from_notion,
            resume,
        } => init::run(no_scan, from_notion, resume).await,

        Commands::Status => wiki::status::run(),

        Commands::Validate => wiki::validate::run(),

        Commands::Consult { domain, all } => wiki::consult::run(domain.as_deref(), all),

        Commands::Graph => wiki::graph::run(),

        Commands::Search { term } => wiki::search::run(&term),

        Commands::Add { what } => match what {
            AddCommands::Domain { name } => wiki::add::domain(&name),
            AddCommands::Context { domain, text } => wiki::add::context(&text, domain.as_deref()),
            AddCommands::Decision { text } => wiki::add::decision(&text),
        },

        Commands::Rebuild => {
            wiki::graph::run()?;
            wiki::index::run()?;
            ui::done("Rebuild complete.");
            Ok(())
        }

        Commands::Index => wiki::index::run(),

        Commands::Confirm { target } => wiki::manage::confirm(&target),
        Commands::Deprecate { target } => wiki::manage::deprecate(&target),
        Commands::RenameDomain { old, new } => wiki::manage::rename_domain(&old, &new),
        Commands::Import { folder, domain } => {
            wiki::manage::import_folder(&folder, domain.as_deref())
        }

        Commands::Context { file, hook } => {
            if hook {
                wiki::context::run_from_stdin()
            } else if let Some(f) = file {
                wiki::context::run(&f)
            } else {
                bail!("Provide --file <path> or --hook")
            }
        }

        Commands::DetectDrift { file, hook } => {
            if hook {
                wiki::drift::run_from_stdin()
            } else if let Some(f) = file {
                wiki::drift::run(&f)
            } else {
                bail!("Provide --file <path> or --hook")
            }
        }

        Commands::InstallHooks => init::hooks::install(&std::env::current_dir()?),
        Commands::UninstallHooks => init::hooks::uninstall(&std::env::current_dir()?),
    }
}
