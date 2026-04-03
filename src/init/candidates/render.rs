use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};

use super::Candidate;
use crate::i18n::t;

/// Format candidates into a markdown file.
pub fn format_candidates_markdown(candidates: &[Candidate], lang: &str) -> String {
    let intro = t("candidates_intro", lang);
    let intro_lines: Vec<String> = intro
        .lines()
        .map(|l| {
            if l.starts_with('>') {
                l.to_string()
            } else {
                format!("> {}", l)
            }
        })
        .collect();

    let mut lines = vec![format!("# {}", t("memory_candidates", lang)), String::new()];
    lines.extend(intro_lines);

    if candidates.is_empty() {
        lines.push(String::new());
        lines.push(t("no_candidates", lang).to_string());
        return lines.join("\n");
    }

    // Group by domain
    let mut current_domain = String::new();

    for c in candidates {
        if c.domain != current_domain {
            lines.push(String::new());
            lines.push(format!("## {}", c.domain));
            current_domain = c.domain.clone();
        }

        lines.push(String::new());
        lines.push(format!("### {}", c.id));
        lines.push(String::new());
        lines.push("- **status**: pending".to_string());
        lines.push(format!("- **type**: {}", c.type_));
        lines.push("- **confidence**: inferred".to_string());
        lines.push("- **provenance**:".to_string());
        for p in &c.provenance {
            lines.push(format!("  - {}: {}", p.kind, p.ref_));
        }
        lines.push(format!("- **rationale**: {}", c.rationale));
        lines.push(format!("- **target**: {}", c.target_note));
        lines.push(String::new());
        lines.push(format!("> {}", c.text));
        lines.push(String::new());
        lines.push("**Action** : `confirm` | `reformulate` | `reject`".to_string());
    }

    lines.push(String::new());
    lines.join("\n")
}

/// Write the _candidates.md file to disk.
/// If the file already exists, preserves candidates that have been processed (non-pending).
pub fn write_candidates_file(wiki_dir: &Path, candidates: &[Candidate], lang: &str) -> Result<()> {
    let path = wiki_dir.join("_candidates.md");

    if candidates.is_empty() {
        // Don't create file if no candidates
        return Ok(());
    }

    // Check for existing processed candidates
    let existing_processed = if path.exists() {
        parse_processed_ids(&path)?
    } else {
        HashSet::new()
    };

    // Filter out candidates whose IDs clash with already-processed ones
    let new_candidates: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| !existing_processed.contains(&c.id))
        .collect();

    if new_candidates.is_empty() && !existing_processed.is_empty() {
        // All candidates already processed, don't overwrite
        return Ok(());
    }

    // Generate the full file with new candidates
    let owned: Vec<Candidate> = new_candidates.into_iter().cloned().collect();
    let content = format_candidates_markdown(&owned, lang);
    std::fs::write(&path, &content)
        .with_context(|| format!("Failed to write {}", path.display()))?;

    Ok(())
}

/// Parse existing _candidates.md to find IDs that have been confirmed or rejected.
pub(super) fn parse_processed_ids(path: &Path) -> Result<HashSet<String>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let mut ids = HashSet::new();
    let mut current_id = String::new();

    for line in content.lines() {
        if let Some(id) = line.strip_prefix("### ") {
            current_id = id.trim().to_string();
        }
        if line.contains("**status**:") && !current_id.is_empty() {
            let status = line.split("**status**:").nth(1).unwrap_or("").trim();
            if status != "pending" {
                ids.insert(current_id.clone());
            }
        }
    }

    Ok(ids)
}
