pub mod candidates;
#[cfg(feature = "enrich")]
pub mod enrich;
pub mod hooks;
#[cfg(feature = "notion")]
pub mod notion;
pub mod patch_claude;
pub mod scaffold;
pub mod scan;

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::ui;

pub async fn run(
    scan: bool,
    hooks: bool,
    full: bool,
    from_notion: Option<String>,
    resume: bool,
    do_enrich: bool,
) -> Result<()> {
    // --full enables all opt-in steps (but NOT --enrich, which requires an API key)
    let do_scan = scan || full || from_notion.is_some();
    let do_hooks = hooks || full;
    let do_claude_integration = full;

    if !resume {
        scaffold::run()?;
    }

    if do_scan {
        let active_domains = run_scan()?;

        if do_enrich && !active_domains.is_empty() {
            #[cfg(feature = "enrich")]
            {
                ui::step("Enriching domains with LLM suggestions...");
                if let Err(e) = enrich::run(&active_domains, Path::new(".wiki")).await {
                    ui::warn(&format!(
                        "LLM enrichment failed: {}. Continuing without enrichment.",
                        e
                    ));
                }
            }

            #[cfg(not(feature = "enrich"))]
            {
                anyhow::bail!(
                    "LLM enrichment is not enabled. Rebuild with: cargo install codefidence --features enrich"
                );
            }
        }
    }

    if do_hooks {
        ui::step("Installing Claude Code hooks...");
        if let Err(e) = hooks::install(Path::new(".")) {
            ui::warn(&format!("Failed to install hooks: {}", e));
        }
    }

    if do_claude_integration {
        scaffold::install_claude_integration()?;
    }

    // Notion import
    if let Some(notion_url) = from_notion {
        #[cfg(feature = "notion")]
        {
            let wiki_dir = Path::new(".wiki");
            let notion_domains = notion::run(&notion_url, resume, wiki_dir).await?;

            if !notion_domains.is_empty() {
                merge_notion_data(wiki_dir, &notion_domains)?;

                // Regenerate graph and index
                ui::step("Regenerating graph and index after Notion import...");
                if let Err(e) = crate::wiki::graph::run() {
                    ui::warn(&format!("Failed to regenerate graph: {}", e));
                }
                if let Err(e) = crate::wiki::index::run() {
                    ui::warn(&format!("Failed to regenerate index: {}", e));
                }
            }
        }

        #[cfg(not(feature = "notion"))]
        {
            let _ = notion_url;
            anyhow::bail!(
                "Notion support is not enabled. Rebuild with: cargo install codefidence --features notion"
            );
        }
    }

    Ok(())
}

/// Run the codebase scan and populate the wiki with detected domains.
/// Returns the list of active domains (with signal) for downstream enrichment.
fn run_scan() -> Result<Vec<scan::DomainInfo>> {
    let result = scan::run()?;

    if result.domains.is_empty() {
        ui::info(
            "No domains detected. You can add them manually with `codefidence add domain <name>`.",
        );
        return Ok(Vec::new());
    }

    // Filter out domains with no useful signals
    let total_discovered = result.domains.len();
    let active_domains: Vec<scan::DomainInfo> = result
        .domains
        .into_iter()
        .filter(|d| d.has_signal())
        .collect();

    let skipped = total_discovered - active_domains.len();
    if skipped > 0 {
        ui::info(&format!(
            "Skipped {} domain(s) with no useful signals (no models, routes, tests, or dependencies).",
            skipped
        ));
    }

    if active_domains.is_empty() {
        ui::info(
            "No domains with useful signals found. You can add context manually with `codefidence add context`.",
        );
        return Ok(Vec::new());
    }

    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();

    // Create domain directories and write _overview.md for each domain
    ui::step("Writing domain overviews...");
    for domain in &active_domains {
        let domain_dir = Path::new(".wiki/domains").join(&domain.name);
        fs::create_dir_all(&domain_dir).with_context(|| {
            format!(
                "Failed to create domain directory: {}",
                domain_dir.display()
            )
        })?;

        let overview_content = scan::generate_domain_overview(domain, &active_domains);
        let overview_path = domain_dir.join("_overview.md");
        fs::write(&overview_path, &overview_content)
            .with_context(|| format!("Failed to write {}", overview_path.display()))?;
    }

    // Generate _graph.md
    ui::step("Generating dependency graph...");
    let graph_content = scan::generate_graph(&active_domains);
    fs::write(".wiki/_graph.md", &graph_content).context("Failed to write _graph.md")?;

    // Generate _index.md
    ui::step("Generating wiki index...");
    let index_content = scan::generate_index(&active_domains, &date);
    fs::write(".wiki/_index.md", &index_content).context("Failed to write _index.md")?;

    // Generate _needs-review.md
    ui::step("Writing needs-review with collected TODOs...");
    let needs_review_content = scan::generate_needs_review(&active_domains);
    fs::write(".wiki/_needs-review.md", &needs_review_content)
        .context("Failed to write _needs-review.md")?;

    // Generate memory candidates
    ui::step("Generating memory candidates...");
    let candidate_list = candidates::generate(&active_domains);
    if candidate_list.is_empty() {
        ui::info("No memory candidates detected from scan.");
    } else {
        candidates::write_candidates_file(Path::new(".wiki"), &candidate_list)?;
        ui::info(&format!(
            "{} memory candidate(s) written to _candidates.md",
            candidate_list.len()
        ));
    }

    eprintln!();
    ui::success(&format!(
        "Populated wiki with {} domain(s) ({} skipped), {} files scanned.",
        active_domains.len(),
        skipped,
        result.total_files_scanned,
    ));

    if !result.languages_detected.is_empty() {
        ui::info(&format!(
            "Languages: {}",
            result.languages_detected.join(", ")
        ));
    }

    Ok(active_domains)
}

/// Merge Notion domain data into existing wiki notes.
#[cfg(feature = "notion")]
fn merge_notion_data(wiki_dir: &Path, notion_domains: &[notion::NotionDomainInfo]) -> Result<()> {
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut all_contradictions: Vec<(String, String, String)> = Vec::new(); // (domain, ticket1, ticket2)

    for domain_info in notion_domains {
        let domain_dir = wiki_dir.join("domains").join(&domain_info.name);
        fs::create_dir_all(&domain_dir).with_context(|| {
            format!(
                "Failed to create domain directory: {}",
                domain_dir.display()
            )
        })?;

        let overview_path = domain_dir.join("_overview.md");

        if overview_path.exists() {
            // Enrich existing overview with Notion data
            let mut content = fs::read_to_string(&overview_path)
                .with_context(|| format!("Failed to read {}", overview_path.display()))?;

            // Add business rules
            if !domain_info.business_rules.is_empty() {
                content.push_str("\n## Business rules (from Notion)\n\n");
                for rule in &domain_info.business_rules {
                    content.push_str(&format!("- {} [needs-validation]\n", rule));
                }
            }

            // Add decisions
            if !domain_info.decisions.is_empty() {
                content.push_str("\n## Decisions (from Notion)\n\n");
                for decision in &domain_info.decisions {
                    content.push_str(&format!("- {} [needs-validation]\n", decision));
                }
            }

            // Add ticket summaries
            if !domain_info.tickets.is_empty() {
                content.push_str("\n## Notion tickets\n\n");
                for ticket in &domain_info.tickets {
                    let status = ticket.status.as_deref().unwrap_or("\u{2014}");
                    content.push_str(&format!(
                        "- **{}** ({}){}\n",
                        ticket.title,
                        status,
                        ticket
                            .date
                            .as_ref()
                            .map(|d| format!(" \u{2014} {}", d))
                            .unwrap_or_default()
                    ));
                }
            }

            fs::write(&overview_path, content)
                .with_context(|| format!("Failed to write {}", overview_path.display()))?;
        } else {
            // Create a new overview from Notion data
            let mut content = format!(
                "---\ntitle: {} overview\nconfidence: needs-validation\nlast_updated: \"{}\"\nrelated_files: []\ndeprecated: false\n---\n\n# {}\n\n> Imported from Notion\n",
                capitalize(&domain_info.name),
                date,
                capitalize(&domain_info.name)
            );

            if !domain_info.business_rules.is_empty() {
                content.push_str("\n## Business rules\n\n");
                for rule in &domain_info.business_rules {
                    content.push_str(&format!("- {} [needs-validation]\n", rule));
                }
            }

            if !domain_info.decisions.is_empty() {
                content.push_str("\n## Decisions\n\n");
                for decision in &domain_info.decisions {
                    content.push_str(&format!("- {} [needs-validation]\n", decision));
                }
            }

            if !domain_info.tickets.is_empty() {
                content.push_str("\n## Notion tickets\n\n");
                for ticket in &domain_info.tickets {
                    let status = ticket.status.as_deref().unwrap_or("\u{2014}");
                    content.push_str(&format!(
                        "- **{}** ({}){}\n",
                        ticket.title,
                        status,
                        ticket
                            .date
                            .as_ref()
                            .map(|d| format!(" \u{2014} {}", d))
                            .unwrap_or_default()
                    ));
                }
            }

            fs::write(&overview_path, content)
                .with_context(|| format!("Failed to write {}", overview_path.display()))?;
        }

        // Collect contradictions
        for (t1, t2) in &domain_info.contradictions {
            all_contradictions.push((domain_info.name.clone(), t1.clone(), t2.clone()));
        }
    }

    // Write contradictions to _needs-review.md
    if !all_contradictions.is_empty() {
        let needs_review_path = wiki_dir.join("_needs-review.md");
        let mut content = if needs_review_path.exists() {
            fs::read_to_string(&needs_review_path).unwrap_or_default()
        } else {
            String::new()
        };

        content.push_str("\n## Contradictions (from Notion)\n\n");
        content.push_str("> These ticket pairs may contain contradictory information. The newer ticket likely supersedes the older one.\n\n");

        for (domain, t1, t2) in &all_contradictions {
            content.push_str(&format!("- **{}**: \"{}\" vs \"{}\"\n", domain, t1, t2));
        }

        fs::write(&needs_review_path, content).context("Failed to write _needs-review.md")?;

        ui::warn(&format!(
            "{} contradiction(s) found. See _needs-review.md.",
            all_contradictions.len()
        ));
    }

    Ok(())
}

#[cfg(feature = "notion")]
use crate::wiki::common::capitalize;
