use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use console::style;
use dialoguer::{Confirm, Input, Select};

use crate::ui;
use crate::wiki::common::{ensure_wiki_exists, list_domain_names};
use crate::wiki::note::{Confidence, MemoryItem, MemoryItemStatus, MemoryItemType, WikiNote};

// ── Types ──────────────────────────────────────────────────────────

enum DomainAction {
    Confirm,
    LeaveAsIs,
    Skip,
}

enum ItemAction {
    Confirm,
    Reject,
    EditText(String),
    Skip,
}

struct ReviewSummary {
    domain: String,
    items_confirmed: usize,
    items_rejected: usize,
    items_edited: usize,
    items_added: usize,
    items_skipped: usize,
    domain_confirmed: bool,
}

impl ReviewSummary {
    fn has_changes(&self) -> bool {
        self.domain_confirmed
            || self.items_confirmed > 0
            || self.items_rejected > 0
            || self.items_edited > 0
            || self.items_added > 0
    }
}

// ── Public API ─────────────────────────────────────────────────────

pub fn run(wiki_dir: &Path, domain: Option<&str>, all: bool) -> Result<()> {
    ensure_wiki_exists(wiki_dir)?;

    let domains = if let Some(d) = domain {
        // Verify it exists
        let overview = wiki_dir.join("domains").join(d).join("_overview.md");
        if !overview.exists() {
            bail!("Domain \"{}\" not found.", d);
        }
        vec![d.to_string()]
    } else {
        let mut names = list_domain_names(wiki_dir)?;
        if names.is_empty() {
            ui::info("No domains found in the wiki.");
            return Ok(());
        }

        if !all {
            // Filter to domains that have unconfirmed notes or items
            names.retain(|name| {
                let overview = wiki_dir.join("domains").join(name).join("_overview.md");
                if let Ok(note) = WikiNote::parse(&overview) {
                    !note.deprecated && has_unconfirmed_content(&note)
                } else {
                    false
                }
            });

            if names.is_empty() {
                ui::success("All domains are confirmed. Use --all to review anyway.");
                return Ok(());
            }
        }

        names.sort();
        names
    };

    let total = domains.len();
    let mut summaries: Vec<ReviewSummary> = Vec::new();
    let mut any_changes = false;

    for (idx, domain_name) in domains.iter().enumerate() {
        let summary = review_domain(wiki_dir, domain_name, idx + 1, total)?;
        if summary.has_changes() {
            any_changes = true;
        }
        summaries.push(summary);
    }

    // Regenerate index once after all reviews
    if any_changes {
        if let Err(e) = crate::wiki::index::run() {
            ui::warn(&format!("Failed to regenerate index: {}", e));
        }
    }

    // Print final summary
    if !summaries.is_empty() {
        print_final_summary(&summaries);
    }

    Ok(())
}

// ── Domain review ──────────────────────────────────────────────────

fn review_domain(
    wiki_dir: &Path,
    domain_name: &str,
    current: usize,
    total: usize,
) -> Result<ReviewSummary> {
    let overview_path = wiki_dir
        .join("domains")
        .join(domain_name)
        .join("_overview.md");
    let mut note = WikiNote::parse(&overview_path)
        .with_context(|| format!("Failed to parse {}", overview_path.display()))?;

    // Display the domain
    display_domain(&note, current, total);

    // Prompt for domain-level action
    let domain_action = prompt_domain_action()?;
    let mut domain_confirmed = false;

    let empty_summary = ReviewSummary {
        domain: domain_name.to_string(),
        items_confirmed: 0,
        items_rejected: 0,
        items_edited: 0,
        items_added: 0,
        items_skipped: 0,
        domain_confirmed: false,
    };

    match domain_action {
        DomainAction::Skip => return Ok(empty_summary),
        DomainAction::Confirm => {
            note.confidence = Confidence::Confirmed;
            note.last_updated = Some(Utc::now().date_naive());
            domain_confirmed = true;
        }
        DomainAction::LeaveAsIs => {}
    }

    // Review individual memory items
    let mut items_confirmed = 0;
    let mut items_rejected = 0;
    let mut items_edited = 0;
    let mut items_skipped = 0;

    let today = Utc::now().format("%Y-%m-%d").to_string();

    for item in note.memory_items.iter_mut() {
        if item.is_high_confidence() && item.status == MemoryItemStatus::Active {
            continue;
        }
        if item.status == MemoryItemStatus::Deprecated {
            continue;
        }

        eprintln!();
        display_item(item);

        match prompt_item_action(item)? {
            ItemAction::Confirm => {
                item.confidence = Confidence::Confirmed;
                item.last_reviewed = Some(today.clone());
                items_confirmed += 1;
            }
            ItemAction::Reject => {
                item.status = MemoryItemStatus::Deprecated;
                item.last_reviewed = Some(today.clone());
                items_rejected += 1;
            }
            ItemAction::EditText(new_text) => {
                item.text = new_text;
                item.confidence = Confidence::Confirmed;
                item.last_reviewed = Some(today.clone());
                items_edited += 1;
            }
            ItemAction::Skip => {
                items_skipped += 1;
            }
        }
    }

    // Ask if user wants to add a new memory item
    eprintln!();
    let mut items_added = 0;
    loop {
        let add = Confirm::new()
            .with_prompt("  Add a memory item?")
            .default(false)
            .interact()?;

        if !add {
            break;
        }

        if let Some(new_item) = prompt_new_item(domain_name, &note.memory_items, &today)? {
            note.memory_items.push(new_item);
            items_added += 1;
        }
    }

    // Write changes
    note.write(&overview_path)
        .with_context(|| format!("Failed to write {}", overview_path.display()))?;

    let summary = ReviewSummary {
        domain: domain_name.to_string(),
        items_confirmed,
        items_rejected,
        items_edited,
        items_added,
        items_skipped,
        domain_confirmed,
    };

    print_domain_summary(&summary);

    Ok(summary)
}

// ── Display helpers ────────────────────────────────────────────────

fn display_domain(note: &WikiNote, current: usize, total: usize) {
    eprintln!();
    ui::action(&format!("Review: {} ({}/{})", note.domain, current, total));
    eprintln!();

    // Title and metadata
    if !note.title.is_empty() {
        eprintln!("  {}", style(&note.title).bold());
    }

    let updated = note
        .last_updated
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "unknown".to_string());

    eprintln!(
        "  Confidence: {}  |  Updated: {}  |  Files: {}",
        style_confidence(&note.confidence),
        style(&updated).dim(),
        style(note.related_files.len()).cyan()
    );

    // Related files
    if !note.related_files.is_empty() {
        eprintln!();
        eprintln!("  Related files:");
        for f in &note.related_files {
            eprintln!("    {}", style(f).dim());
        }
    }

    // Memory items overview
    if !note.memory_items.is_empty() {
        eprintln!();
        eprintln!("  Memory Items:");
        eprintln!();
        for (i, item) in note.memory_items.iter().enumerate() {
            let confidence_marker = match &item.confidence {
                Confidence::Confirmed | Confidence::Verified => style("✓").green().to_string(),
                Confidence::Inferred => style("⚠").yellow().to_string(),
                Confidence::NeedsValidation => style("⚠").red().to_string(),
                Confidence::SeenInCode => style("~").cyan().to_string(),
            };

            let status_suffix = if item.status == MemoryItemStatus::Deprecated {
                format!(" {}", style("(deprecated)").dim())
            } else {
                String::new()
            };

            eprintln!(
                "  {}. [{}] {} [{}] {}{}",
                i + 1,
                style(&item.type_).dim(),
                style(&item.id).white(),
                style_confidence(&item.confidence),
                confidence_marker,
                status_suffix,
            );
            eprintln!("     {}", &item.text);

            if !item.related_files.is_empty() {
                for f in &item.related_files {
                    eprintln!("     {} {}", style("→").dim(), style(f).dim());
                }
            }

            eprintln!();
        }
    }
}

fn display_item(item: &MemoryItem) {
    eprintln!(
        "  [{}] {} [{}]",
        style(&item.type_).dim(),
        style(&item.id).white().bold(),
        style_confidence(&item.confidence),
    );
    eprintln!("  \"{}\"", &item.text);
}

fn style_confidence(confidence: &Confidence) -> console::StyledObject<String> {
    let text = confidence.to_string();
    match confidence {
        Confidence::Confirmed | Confidence::Verified => style(text).green(),
        Confidence::SeenInCode => style(text).cyan(),
        Confidence::Inferred => style(text).yellow(),
        Confidence::NeedsValidation => style(text).red(),
    }
}

// ── Prompts ────────────────────────────────────────────────────────

fn prompt_domain_action() -> Result<DomainAction> {
    let choices = vec!["Confirm domain note", "Leave as-is", "Skip domain"];

    let selection = Select::new()
        .with_prompt("  Domain action")
        .items(&choices)
        .default(1)
        .interact()?;

    Ok(match selection {
        0 => DomainAction::Confirm,
        1 => DomainAction::LeaveAsIs,
        2 => DomainAction::Skip,
        _ => DomainAction::Skip,
    })
}

fn prompt_item_action(item: &MemoryItem) -> Result<ItemAction> {
    let choices = vec!["Confirm", "Reject (deprecate)", "Edit text", "Skip"];

    let selection = Select::new()
        .with_prompt("  Action")
        .items(&choices)
        .default(0)
        .interact()?;

    Ok(match selection {
        0 => ItemAction::Confirm,
        1 => ItemAction::Reject,
        2 => {
            let new_text: String = Input::new()
                .with_prompt("  New text")
                .with_initial_text(&item.text)
                .interact_text()?;
            ItemAction::EditText(new_text)
        }
        3 => ItemAction::Skip,
        _ => ItemAction::Skip,
    })
}

fn prompt_new_item(
    domain: &str,
    existing: &[MemoryItem],
    today: &str,
) -> Result<Option<MemoryItem>> {
    // Type
    let type_choices = vec!["exception", "decision", "business_rule"];
    let type_idx = Select::new()
        .with_prompt("  Type")
        .items(&type_choices)
        .default(0)
        .interact()?;

    let type_ = match type_idx {
        0 => MemoryItemType::Exception,
        1 => MemoryItemType::Decision,
        2 => MemoryItemType::BusinessRule,
        _ => MemoryItemType::Exception,
    };

    // Text
    let text: String = Input::new().with_prompt("  Text").interact_text()?;

    if text.trim().is_empty() {
        ui::warn("Empty text, skipping.");
        return Ok(None);
    }

    // Confidence
    let conf_choices = vec!["confirmed", "verified", "seen-in-code", "inferred"];
    let conf_idx = Select::new()
        .with_prompt("  Confidence")
        .items(&conf_choices)
        .default(0)
        .interact()?;

    let confidence = match conf_idx {
        0 => Confidence::Confirmed,
        1 => Confidence::Verified,
        2 => Confidence::SeenInCode,
        3 => Confidence::Inferred,
        _ => Confidence::Confirmed,
    };

    // Generate next ID
    let next_num = next_item_number(domain, existing);
    let id = format!("{}-{:03}", domain, next_num);

    ui::success(&format!("Added {}", id));

    Ok(Some(MemoryItem {
        id,
        type_,
        text,
        confidence,
        related_files: Vec::new(),
        sources: Vec::new(),
        status: MemoryItemStatus::Active,
        last_reviewed: Some(today.to_string()),
    }))
}

// ── Helpers ────────────────────────────────────────────────────────

fn has_unconfirmed_content(note: &WikiNote) -> bool {
    if !matches!(
        note.confidence,
        Confidence::Confirmed | Confidence::Verified
    ) {
        return true;
    }

    note.memory_items
        .iter()
        .any(|item| item.status == MemoryItemStatus::Active && !item.is_high_confidence())
}

fn next_item_number(domain: &str, existing: &[MemoryItem]) -> u32 {
    let prefix = format!("{}-", domain);
    let max = existing
        .iter()
        .filter_map(|item| {
            item.id
                .strip_prefix(&prefix)
                .and_then(|suffix| suffix.parse::<u32>().ok())
        })
        .max()
        .unwrap_or(0);

    max + 1
}

fn print_domain_summary(summary: &ReviewSummary) {
    eprintln!();

    let mut parts = Vec::new();
    if summary.domain_confirmed {
        parts.push(format!("domain {}", style("confirmed").green()));
    }
    if summary.items_confirmed > 0 {
        parts.push(format!(
            "{} confirmed",
            style(summary.items_confirmed).green()
        ));
    }
    if summary.items_edited > 0 {
        parts.push(format!("{} edited", style(summary.items_edited).cyan()));
    }
    if summary.items_added > 0 {
        parts.push(format!("{} added", style(summary.items_added).blue()));
    }
    if summary.items_rejected > 0 {
        parts.push(format!("{} rejected", style(summary.items_rejected).red()));
    }
    if summary.items_skipped > 0 {
        parts.push(format!("{} skipped", style(summary.items_skipped).dim()));
    }

    if parts.is_empty() {
        ui::info(&format!("{}: no changes", summary.domain));
    } else {
        ui::success(&format!("{}: {}", summary.domain, parts.join(", ")));
    }
}

fn print_final_summary(summaries: &[ReviewSummary]) {
    eprintln!();

    let total_confirmed: usize = summaries.iter().map(|s| s.items_confirmed).sum();
    let total_rejected: usize = summaries.iter().map(|s| s.items_rejected).sum();
    let total_edited: usize = summaries.iter().map(|s| s.items_edited).sum();
    let total_added: usize = summaries.iter().map(|s| s.items_added).sum();
    let total_skipped: usize = summaries.iter().map(|s| s.items_skipped).sum();
    let domains_confirmed: usize = summaries.iter().filter(|s| s.domain_confirmed).count();
    let domains_reviewed = summaries.len();

    let mut lines = Vec::new();
    lines.push(format!(
        "Reviewed {} domain(s)",
        style(domains_reviewed).white().bold()
    ));
    if domains_confirmed > 0 {
        lines.push(format!(
            "  {} domain(s) confirmed",
            style(domains_confirmed).green()
        ));
    }
    if total_confirmed > 0 {
        lines.push(format!(
            "  {} item(s) confirmed",
            style(total_confirmed).green()
        ));
    }
    if total_edited > 0 {
        lines.push(format!("  {} item(s) edited", style(total_edited).cyan()));
    }
    if total_added > 0 {
        lines.push(format!("  {} item(s) added", style(total_added).blue()));
    }
    if total_rejected > 0 {
        lines.push(format!(
            "  {} item(s) rejected",
            style(total_rejected).red()
        ));
    }
    if total_skipped > 0 {
        lines.push(format!("  {} item(s) skipped", style(total_skipped).dim()));
    }

    ui::summary_box(&lines);
}
