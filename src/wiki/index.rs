use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;

use crate::ui;
use crate::wiki::common;
use crate::wiki::common::capitalize;
use crate::wiki::config;
use crate::wiki::note::{Confidence, WikiNote};

#[derive(Debug, Serialize)]
struct IndexJson {
    generated: String,
    domains: Vec<DomainJson>,
    decisions: Vec<DecisionJson>,
    health: HealthJson,
}

#[derive(Debug, Serialize)]
struct DomainJson {
    name: String,
    path: String,
    notes_count: usize,
    confidence: String,
    last_updated: String,
    summary: String,
    related_domains: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DecisionJson {
    title: String,
    date: String,
    path: String,
}

#[derive(Debug, Serialize)]
struct HealthJson {
    total_notes: usize,
    confirmed: usize,
    verified: usize,
    seen_in_code: usize,
    inferred: usize,
    needs_validation: usize,
}

pub fn run() -> Result<()> {
    let wiki_dir = common::find_wiki_root()?;
    let wiki_config = config::load(&wiki_dir);

    ui::action("Regenerating wiki index");

    let date = Utc::now().format("%Y-%m-%d").to_string();

    // 1. Collect domains and their notes
    let domains_dir = wiki_dir.join("domains");
    let domain_sections = if domains_dir.exists() {
        build_domain_sections(&domains_dir)?
    } else {
        Vec::new()
    };

    // 2. Collect decisions
    let decisions = collect_decisions(&wiki_dir)?;

    // 3. Collect all notes for health stats
    let notes = common::collect_all_notes(&wiki_dir)?;
    let total = notes.len();
    let confirmed = notes
        .iter()
        .filter(|n| matches!(n.confidence, Confidence::Confirmed | Confidence::Verified))
        .count();
    let seen_in_code = notes
        .iter()
        .filter(|n| matches!(n.confidence, Confidence::SeenInCode))
        .count();
    let inferred = notes
        .iter()
        .filter(|n| matches!(n.confidence, Confidence::Inferred))
        .count();
    let needs_validation = notes
        .iter()
        .filter(|n| matches!(n.confidence, Confidence::NeedsValidation))
        .count();

    let today = Utc::now().date_naive();
    let staleness_threshold = i64::from(wiki_config.staleness_days);
    let stale_count = notes
        .iter()
        .filter(|n| {
            n.last_updated
                .map_or(false, |d| (today - d).num_days() > staleness_threshold)
        })
        .count();

    // ─── Build output ───
    let mut output = String::new();

    output.push_str("# Project Wiki\n\n");
    output.push_str("> Auto-generated. Do not edit manually.\n");
    output.push_str(&format!("> Last updated: {}\n", date));

    // Domains section
    output.push_str("\n## Domains\n");

    if domain_sections.is_empty() {
        output.push_str("\n_No domains documented yet._\n");
    } else {
        for (domain_name, note_lines) in &domain_sections {
            let title = capitalize(domain_name);
            output.push_str(&format!("\n### {}\n", title));
            for line in note_lines {
                output.push_str(&format!("{}\n", line));
            }
        }
    }

    // Decisions section
    output.push_str("\n## Decisions\n\n");
    if decisions.is_empty() {
        output.push_str("_No decisions recorded yet._\n");
    } else {
        output.push_str("| Date | Decision | Domain |\n");
        output.push_str("|------|----------|--------|\n");
        for decision in &decisions {
            output.push_str(&format!(
                "| {} | [{}]({}) | {} |\n",
                decision.date, decision.title, decision.path, decision.domain
            ));
        }
    }

    // Health section
    output.push_str("\n## Health\n\n");
    output.push_str(&format!("- Total notes: {}\n", total));
    if total > 0 {
        output.push_str(&format!(
            "- Confirmed: {} ({}%)\n",
            confirmed,
            percentage(confirmed, total)
        ));
        output.push_str(&format!(
            "- Seen in code: {} ({}%)\n",
            seen_in_code,
            percentage(seen_in_code, total)
        ));
        output.push_str(&format!(
            "- Inferred: {} ({}%)\n",
            inferred,
            percentage(inferred, total)
        ));
        if needs_validation > 0 {
            output.push_str(&format!(
                "- Needs validation: {} ({}%)\n",
                needs_validation,
                percentage(needs_validation, total)
            ));
        }
    }
    output.push_str(&format!(
        "- Stale (>{} days): {}\n",
        wiki_config.staleness_days, stale_count
    ));

    // Write to file
    let index_path = wiki_dir.join("_index.md");
    fs::write(&index_path, &output)
        .with_context(|| format!("Failed to write {}", index_path.display()))?;

    // ─── Build JSON index ───
    let verified = notes
        .iter()
        .filter(|n| matches!(n.confidence, Confidence::Verified))
        .count();

    let json_domains = build_domain_json(&domains_dir, &domain_sections)?;
    let json_decisions: Vec<DecisionJson> = decisions
        .iter()
        .map(|d| DecisionJson {
            title: d.title.clone(),
            date: d.date.clone(),
            path: d.path.clone(),
        })
        .collect();

    let index_json = IndexJson {
        generated: date,
        domains: json_domains,
        decisions: json_decisions,
        health: HealthJson {
            total_notes: total,
            confirmed,
            verified,
            seen_in_code,
            inferred,
            needs_validation,
        },
    };

    let json_path = wiki_dir.join("_index.json");
    let json_output =
        serde_json::to_string_pretty(&index_json).context("Failed to serialize index JSON")?;
    fs::write(&json_path, &json_output)
        .with_context(|| format!("Failed to write {}", json_path.display()))?;

    ui::success(&format!(
        "Index regenerated: {} domain(s), {} note(s), {} decision(s).",
        domain_sections.len(),
        total,
        decisions.len()
    ));

    Ok(())
}

/// Build domain entries for the JSON index.
fn build_domain_json(
    domains_dir: &Path,
    domain_sections: &[(String, Vec<String>)],
) -> Result<Vec<DomainJson>> {
    let mut result = Vec::new();

    for (domain_name, note_lines) in domain_sections {
        let domain_dir = domains_dir.join(domain_name);
        let overview_path = domain_dir.join("_overview.md");

        // Count notes
        let notes_count = note_lines.len();

        // Determine dominant confidence and last_updated from the overview
        let (confidence, last_updated, summary, related_domains) =
            if let Ok(note) = WikiNote::parse(&overview_path) {
                let summary = note
                    .content
                    .lines()
                    .find(|line| {
                        let trimmed = line.trim();
                        !trimmed.is_empty() && !trimmed.starts_with('#')
                    })
                    .unwrap_or("")
                    .trim()
                    .to_string();

                let last_updated = note
                    .last_updated
                    .map(|d| d.format("%Y-%m-%d").to_string())
                    .unwrap_or_default();

                // Extract related domains from overview content (look for domain links)
                let mut related = Vec::new();
                for line in note.content.lines() {
                    // Match patterns like [domain-name](../other-domain/...)
                    if let Some(start) = line.find("](../") {
                        let rest = &line[start + 5..];
                        if let Some(end) = rest.find('/') {
                            let other_domain = &rest[..end];
                            if other_domain != *domain_name
                                && !related.contains(&other_domain.to_string())
                            {
                                related.push(other_domain.to_string());
                            }
                        }
                    }
                }

                (note.confidence.to_string(), last_updated, summary, related)
            } else {
                (String::new(), String::new(), String::new(), Vec::new())
            };

        let path = if overview_path.exists() {
            format!("domains/{}/_overview.md", domain_name)
        } else {
            format!("domains/{}", domain_name)
        };

        result.push(DomainJson {
            name: domain_name.clone(),
            path,
            notes_count,
            confidence,
            last_updated,
            summary,
            related_domains,
        });
    }

    Ok(result)
}

/// Build the domain sections: for each domain, list notes with description and confidence.
fn build_domain_sections(
    domains_dir: &Path,
) -> Result<Vec<(String, Vec<String>)>> {
    let mut sections: Vec<(String, Vec<String>)> = Vec::new();

    let mut domain_names: Vec<String> = Vec::new();
    for entry in fs::read_dir(domains_dir).context("Failed to read domains dir")? {
        let entry = entry?;
        if entry.path().is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                domain_names.push(name.to_string());
            }
        }
    }
    domain_names.sort();

    for domain_name in &domain_names {
        let domain_dir = domains_dir.join(domain_name);
        let mut note_lines: Vec<String> = Vec::new();

        // Collect all .md files in this domain
        let mut md_files: Vec<std::path::PathBuf> = Vec::new();
        for entry in fs::read_dir(&domain_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "md") {
                md_files.push(path);
            }
        }

        // Sort: _overview.md first, then alphabetical
        md_files.sort_by(|a, b| {
            let a_is_overview = a
                .file_name()
                .map_or(false, |n| n == "_overview.md");
            let b_is_overview = b
                .file_name()
                .map_or(false, |n| n == "_overview.md");

            if a_is_overview && !b_is_overview {
                std::cmp::Ordering::Less
            } else if !a_is_overview && b_is_overview {
                std::cmp::Ordering::Greater
            } else {
                a.cmp(b)
            }
        });

        for md_path in &md_files {
            let note = match WikiNote::parse(md_path) {
                Ok(n) => n,
                Err(_) => continue,
            };

            let filename = md_path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();

            // Build display title
            let display_title = if !note.title.is_empty() {
                note.title.clone()
            } else if filename == "_overview.md" {
                "Overview".to_string()
            } else {
                // Derive from filename: remove .md, replace - with space, capitalize
                let stem = filename.trim_end_matches(".md");
                capitalize(&stem.replace('-', " "))
            };

            let rel_path = format!(
                "./domains/{}/{}",
                domain_name, filename
            );

            note_lines.push(format!(
                "- [{}]({}) — `[{}]`",
                display_title, rel_path, note.confidence
            ));
        }

        if !note_lines.is_empty() {
            sections.push((domain_name.clone(), note_lines));
        }
    }

    Ok(sections)
}

struct DecisionEntry {
    date: String,
    title: String,
    path: String,
    domain: String,
}

fn collect_decisions(wiki_dir: &Path) -> Result<Vec<DecisionEntry>> {
    let decisions_dir = wiki_dir.join("decisions");
    if !decisions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries: Vec<DecisionEntry> = Vec::new();

    for entry in fs::read_dir(&decisions_dir)? {
        let entry = entry?;
        let path = entry.path();

        if !path.extension().map_or(false, |ext| ext == "md") {
            continue;
        }

        let filename = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();

        // Try to extract date from filename (e.g., 2026-03-26-no-dedup.md)
        let (date, title_slug) = if filename.len() >= 11 && filename[..10].chars().filter(|c| *c == '-').count() == 2 {
            let date_part = &filename[..10];
            let rest = filename[11..].trim_end_matches(".md");
            (date_part.to_string(), rest.to_string())
        } else {
            let title_slug = filename.trim_end_matches(".md").to_string();
            ("—".to_string(), title_slug)
        };

        // Try to read the note for a title and domain
        let (title, domain) = if let Ok(content) = fs::read_to_string(&path) {
            let title = extract_title_from_content(&content)
                .unwrap_or_else(|| capitalize(&title_slug.replace('-', " ")));

            let domain = extract_domain_from_content(&content)
                .unwrap_or_else(|| "—".to_string());

            (title, domain)
        } else {
            (
                capitalize(&title_slug.replace('-', " ")),
                "—".to_string(),
            )
        };

        let rel_path = format!("./decisions/{}", filename);
        entries.push(DecisionEntry {
            date,
            title,
            path: rel_path,
            domain,
        });
    }

    // Sort by date descending
    entries.sort_by(|a, b| b.date.cmp(&a.date));

    Ok(entries)
}

fn extract_title_from_content(content: &str) -> Option<String> {
    // Look for # Title line
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") && !trimmed.starts_with("## ") {
            return Some(trimmed[2..].trim().to_string());
        }
    }
    None
}

fn extract_domain_from_content(content: &str) -> Option<String> {
    // Look for domain: in front matter or content
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("domain:") {
            let value = trimmed["domain:".len()..].trim();
            if !value.is_empty() {
                return Some(capitalize(value));
            }
        }
    }
    None
}

fn percentage(count: usize, total: usize) -> u32 {
    if total == 0 {
        0
    } else {
        ((count as f64 / total as f64) * 100.0).round() as u32
    }
}
