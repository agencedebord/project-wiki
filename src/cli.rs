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
        /// Run codebase scan to auto-detect domains
        #[arg(long)]
        scan: bool,

        /// Install Claude Code hooks for automatic wiki integration
        #[arg(long)]
        hooks: bool,

        /// Full setup: scan + hooks + CLAUDE.md patch + slash commands
        #[arg(long)]
        full: bool,

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
    Validate {
        /// Treat warnings as errors (exit code 1 if any warnings)
        #[arg(long)]
        strict: bool,
    },

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

        /// Output as JSON (for programmatic use)
        #[arg(long)]
        json: bool,
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

    /// Check modified files against wiki memory items
    ///
    /// Resolves each file to its wiki domain, then surfaces relevant memory items
    /// (exceptions, decisions, business rules) so you know what to watch for.
    ///
    /// Resolution is file-to-domain only — no semantic diff analysis.
    ///
    /// Examples:
    ///   project-wiki check-diff                          # check unstaged git changes
    ///   project-wiki check-diff src/billing/invoice.ts   # check specific files
    ///   project-wiki check-diff --staged --json          # staged changes, JSON output
    CheckDiff {
        /// Files to check (default: git diff --name-only)
        files: Vec<String>,

        /// Use staged changes only (git diff --cached)
        #[arg(long)]
        staged: bool,

        /// Output as JSON (for programmatic use / hooks)
        #[arg(long, conflicts_with = "pr_comment")]
        json: bool,

        /// Output as GitHub PR comment markdown (silent if low sensitivity)
        #[arg(long, conflicts_with = "json")]
        pr_comment: bool,

        /// Maximum memory items to show per domain
        #[arg(long, default_value = "3")]
        max_items: usize,
    },

    /// Promote a memory candidate to a confirmed memory item
    Promote {
        /// Candidate ID (e.g. billing-001)
        candidate_id: Option<String>,

        /// Auto-promote the highest-priority pending candidate
        #[arg(long)]
        next: bool,

        /// Confidence level (default: confirmed)
        #[arg(long)]
        confidence: Option<String>,

        /// Override candidate text with a reformulation
        #[arg(long)]
        text: Option<String>,
    },

    /// Reject a memory candidate
    Reject {
        /// Candidate ID (e.g. billing-001)
        candidate_id: String,
    },

    /// Generate memory candidates from a codebase scan
    GenerateCandidates,

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
            scan,
            hooks,
            full,
            from_notion,
            resume,
        } => init::run(scan, hooks, full, from_notion, resume).await,

        Commands::Status => wiki::status::run(),

        Commands::Validate { strict } => wiki::validate::run(strict),

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

        Commands::Context { file, hook, json } => {
            if hook {
                wiki::context::run_from_stdin()
            } else if let Some(f) = file {
                wiki::context::run(&f, json)
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

        Commands::CheckDiff {
            files,
            staged,
            json,
            pr_comment,
            max_items,
        } => wiki::check_diff::run(&files, staged, json, pr_comment, max_items),

        Commands::Promote {
            candidate_id,
            next,
            confidence,
            text,
        } => {
            let wiki_dir = std::path::Path::new(".wiki");
            let id = if next {
                let found = wiki::promote::find_next_candidate(wiki_dir)?;
                ui::info(&format!(
                    "Auto-selected: {} [{}]",
                    found.0, found.1
                ));
                found.0
            } else if let Some(id) = candidate_id {
                id
            } else {
                bail!("Provide a candidate ID or use --next to auto-select the highest-priority pending candidate.")
            };
            wiki::promote::promote(
                wiki_dir,
                &id,
                confidence.as_deref(),
                text.as_deref(),
            )
        }

        Commands::Reject { candidate_id } => {
            wiki::promote::reject(std::path::Path::new(".wiki"), &candidate_id)
        }

        Commands::GenerateCandidates => {
            let wiki_dir = std::path::Path::new(".wiki");
            if !wiki_dir.exists() {
                bail!("No .wiki/ found. Run `project-wiki init` first.");
            }
            let scan_result = init::scan::run()?;
            let candidates = init::candidates::generate(&scan_result.domains);
            if candidates.is_empty() {
                ui::info("No memory candidates detected from scan.");
            } else {
                init::candidates::write_candidates_file(wiki_dir, &candidates)?;
                ui::success(&format!(
                    "{} memory candidate(s) written to .wiki/_candidates.md",
                    candidates.len()
                ));
                eprintln!();
                eprintln!("Candidates:");
                for c in &candidates {
                    eprintln!("  {} [{}]  \"{}\"", c.id, c.type_, c.text);
                }
                eprintln!();
                eprintln!("Next: project-wiki promote <id>");
                eprintln!("  or: project-wiki promote --next");
            }
            Ok(())
        }

        Commands::InstallHooks => init::hooks::install(&std::env::current_dir()?),
        Commands::UninstallHooks => init::hooks::uninstall(&std::env::current_dir()?),
    }
}
