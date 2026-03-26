use std::path::Path;

use anyhow::Result;
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

    /// Manage vector embeddings for semantic search
    Vectors {
        #[command(subcommand)]
        action: Option<VectorsCommands>,
    },

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

#[derive(Subcommand)]
enum VectorsCommands {
    /// Search wiki notes using semantic similarity (future)
    Search {
        /// Search query
        query: String,

        /// Number of results to return
        #[arg(long, default_value = "5")]
        top_k: usize,
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
            AddCommands::Context { domain, text } => {
                wiki::add::context(&text, domain.as_deref())
            }
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
        Commands::Import { folder, domain } => wiki::manage::import_folder(&folder, domain.as_deref()),

        Commands::Vectors { action } => {
            let wiki_dir = Path::new(".wiki");
            match action {
                None => wiki::vectors::index(wiki_dir),
                Some(VectorsCommands::Search { query, top_k }) => {
                    let results = wiki::vectors::search(wiki_dir, &query, top_k)?;
                    if results.is_empty() {
                        ui::coming_soon("semantic search");
                        ui::info("Semantic search will be available in v2.0. Use `project-wiki search` for keyword search.");
                    } else {
                        for result in &results {
                            eprintln!("  {}", result);
                        }
                    }
                    Ok(())
                }
            }
        }
    }
}
