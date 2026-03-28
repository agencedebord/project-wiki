use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::init::patch_claude;
use crate::ui;

const INDEX_TEMPLATE: &str = include_str!("../templates/index.md");
const GRAPH_TEMPLATE: &str = include_str!("../templates/graph.md");
const NEEDS_REVIEW_TEMPLATE: &str = include_str!("../templates/needs_review.md");
const ENV_EXAMPLE: &str = include_str!("../templates/env_example");
const DOMAIN_OVERVIEW_TEMPLATE: &str = include_str!("../templates/domain_overview.md");
const DECISION_TEMPLATE: &str = include_str!("../templates/decision.md");

const WIKI_CONSULT_CMD: &str = include_str!("../templates/commands/wiki_consult.md");
const WIKI_UPDATE_CMD: &str = include_str!("../templates/commands/wiki_update.md");
const WIKI_ADD_CONTEXT_CMD: &str = include_str!("../templates/commands/wiki_add_context.md");
const WIKI_ADD_DECISION_CMD: &str = include_str!("../templates/commands/wiki_add_decision.md");

pub fn run() -> Result<()> {
    let wiki_dir = Path::new(".wiki");

    if wiki_dir.exists() {
        bail!(
            ".wiki/ already exists. Use `project-wiki rebuild` to regenerate, \
             or delete .wiki/ manually to start fresh."
        );
    }

    ui::app_header(env!("CARGO_PKG_VERSION"));
    ui::action("Initializing project wiki");
    eprintln!();

    // Create directory structure
    ui::step("Creating .wiki/ directory structure...");
    fs::create_dir_all(".wiki/_templates")
        .context("Failed to create .wiki/_templates directory")?;
    fs::create_dir_all(".wiki/domains").context("Failed to create .wiki/domains directory")?;
    fs::create_dir_all(".wiki/decisions").context("Failed to create .wiki/decisions directory")?;

    // Write template files
    ui::step("Writing wiki templates...");
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let index_content = INDEX_TEMPLATE.replace("{date}", &date);
    fs::write(".wiki/_index.md", index_content).context("Failed to write _index.md")?;
    fs::write(".wiki/_graph.md", GRAPH_TEMPLATE).context("Failed to write _graph.md")?;
    fs::write(".wiki/_needs-review.md", NEEDS_REVIEW_TEMPLATE)
        .context("Failed to write _needs-review.md")?;
    fs::write(".wiki/.env.example", ENV_EXAMPLE).context("Failed to write .env.example")?;
    fs::write(
        ".wiki/_templates/domain-overview.md",
        DOMAIN_OVERVIEW_TEMPLATE,
    )
    .context("Failed to write domain-overview.md template")?;
    fs::write(".wiki/_templates/decision.md", DECISION_TEMPLATE)
        .context("Failed to write decision.md template")?;

    // Write default config
    fs::write(
        ".wiki/config.toml",
        "# project-wiki configuration\n# staleness_days = 30\n# auto_index = true\n",
    )
    .context("Failed to write config.toml")?;

    // Gitkeep files for empty directories
    fs::write(".wiki/domains/.gitkeep", "").context("Failed to write domains/.gitkeep")?;
    fs::write(".wiki/decisions/.gitkeep", "").context("Failed to write decisions/.gitkeep")?;

    // Create .claude/commands/ slash command files
    ui::step("Installing Claude slash commands...");
    fs::create_dir_all(".claude/commands")
        .context("Failed to create .claude/commands directory")?;
    fs::write(".claude/commands/wiki-consult.md", WIKI_CONSULT_CMD)
        .context("Failed to write wiki-consult.md")?;
    fs::write(".claude/commands/wiki-update.md", WIKI_UPDATE_CMD)
        .context("Failed to write wiki-update.md")?;
    fs::write(".claude/commands/wiki-add-context.md", WIKI_ADD_CONTEXT_CMD)
        .context("Failed to write wiki-add-context.md")?;
    fs::write(
        ".claude/commands/wiki-add-decision.md",
        WIKI_ADD_DECISION_CMD,
    )
    .context("Failed to write wiki-add-decision.md")?;

    // Patch CLAUDE.md
    ui::step("Patching CLAUDE.md...");
    patch_claude::run().context("Failed to patch CLAUDE.md")?;

    // Update .gitignore
    ui::step("Updating .gitignore...");
    update_gitignore().context("Failed to update .gitignore")?;

    // Install Claude Code hooks
    ui::step("Installing Claude Code hooks...");
    if let Err(e) = super::hooks::install(Path::new(".")) {
        ui::warn(&format!("Failed to install hooks: {}", e));
    }

    eprintln!();
    ui::done("Wiki initialized successfully.");
    ui::info("Run `project-wiki status` to see the wiki health.");
    ui::info("Run `/wiki-consult` in Claude Code to read the wiki before a task.");
    eprintln!();

    Ok(())
}

fn update_gitignore() -> Result<()> {
    let gitignore_path = Path::new(".gitignore");
    let mut content = if gitignore_path.exists() {
        fs::read_to_string(gitignore_path).context("Failed to read .gitignore")?
    } else {
        String::new()
    };

    let entries = [".wiki/.env", ".wiki/.file-index.json"];
    let mut added = false;

    for entry in &entries {
        if !content.lines().any(|line| line.trim() == *entry) {
            if !content.ends_with('\n') && !content.is_empty() {
                content.push('\n');
            }
            if !added {
                content.push_str("\n# Project Wiki\n");
            }
            content.push_str(entry);
            content.push('\n');
            added = true;
        }
    }

    if added {
        fs::write(gitignore_path, content).context("Failed to write .gitignore")?;
    }

    Ok(())
}
