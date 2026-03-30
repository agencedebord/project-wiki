//! Promote candidates to memory items, or reject them.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::ui;
use crate::wiki::common::ensure_wiki_exists;
use crate::wiki::note::{
    Confidence, MemoryItem, MemoryItemSource, MemoryItemStatus, MemoryItemType, WikiNote,
};

// ── Types ──────────────────────────────────────────────────────────

/// A parsed candidate from _candidates.md.
#[derive(Debug, Clone)]
struct ParsedCandidate {
    id: String,
    status: String,
    type_: String,
    text: String,
    target: String,
    provenance: Vec<(String, String)>, // (kind, ref)
}

// ── Public API ─────────────────────────────────────────────────────

/// Promote a candidate to a memory item in the target note.
pub fn promote(
    wiki_dir: &Path,
    candidate_id: &str,
    confidence_override: Option<&str>,
    text_override: Option<&str>,
) -> Result<()> {
    ensure_wiki_exists(wiki_dir)?;

    let candidates_path = wiki_dir.join("_candidates.md");
    if !candidates_path.exists() {
        bail!("No _candidates.md found. Run `project-wiki generate-candidates` first.");
    }

    let mut candidates = parse_candidates_file(&candidates_path)?;

    let candidate = candidates
        .iter()
        .find(|c| c.id == candidate_id)
        .ok_or_else(|| anyhow::anyhow!("No candidate found with id '{candidate_id}'"))?
        .clone();

    if candidate.status != "pending" {
        ui::warn(&format!(
            "{candidate_id} has already been processed (status: {})",
            candidate.status
        ));
        return Ok(());
    }

    // Resolve the target note
    let target_path = if candidate.target.starts_with(".wiki/") {
        wiki_dir
            .parent()
            .unwrap_or(Path::new("."))
            .join(&candidate.target)
    } else {
        wiki_dir.join(&candidate.target)
    };

    if !target_path.exists() {
        bail!(
            "Target note '{}' not found. Create the domain first with `project-wiki add domain`.",
            candidate.target
        );
    }

    let mut note = WikiNote::parse(&target_path)
        .with_context(|| format!("Failed to parse note: {}", target_path.display()))?;

    // Determine the final item ID (handle conflicts)
    let final_id = resolve_id_conflict(candidate_id, &note);

    // Determine confidence
    let confidence = match confidence_override {
        Some(c) => parse_confidence(c)?,
        None => Confidence::Confirmed,
    };

    // Determine text
    let text = text_override.unwrap_or(&candidate.text).to_string();

    // Parse the type
    let item_type = parse_item_type(&candidate.type_)?;

    // Build sources from provenance
    let sources: Vec<MemoryItemSource> = candidate
        .provenance
        .iter()
        .map(|(kind, ref_)| MemoryItemSource {
            kind: kind.clone(),
            ref_: ref_.clone(),
            line: None,
        })
        .collect();

    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    let item = MemoryItem {
        id: final_id.clone(),
        type_: item_type,
        text: text.clone(),
        confidence: confidence.clone(),
        related_files: Vec::new(),
        sources,
        status: MemoryItemStatus::Active,
        last_reviewed: Some(today),
    };

    // Add to note
    note.memory_items.push(item);
    note.write(&target_path)
        .with_context(|| format!("Failed to write note: {}", target_path.display()))?;

    // Update candidate status in _candidates.md
    update_candidate_status(&candidates_path, candidate_id, "confirmed", &mut candidates)?;

    if final_id != candidate_id {
        ui::success(&format!(
            "Promoted {candidate_id} as {final_id} to {}",
            target_path.display()
        ));
    } else {
        ui::success(&format!(
            "Promoted {candidate_id} to {}",
            target_path.display()
        ));
    }
    ui::info(&format!(
        "  [{}] {} [{}]",
        candidate.type_, text, confidence
    ));

    Ok(())
}

/// Reject a candidate (mark as rejected, don't modify any note).
pub fn reject(wiki_dir: &Path, candidate_id: &str) -> Result<()> {
    ensure_wiki_exists(wiki_dir)?;

    let candidates_path = wiki_dir.join("_candidates.md");
    if !candidates_path.exists() {
        bail!("No _candidates.md found.");
    }

    let mut candidates = parse_candidates_file(&candidates_path)?;

    let candidate = candidates
        .iter()
        .find(|c| c.id == candidate_id)
        .ok_or_else(|| anyhow::anyhow!("No candidate found with id '{candidate_id}'"))?;

    if candidate.status != "pending" {
        ui::warn(&format!(
            "{candidate_id} has already been processed (status: {})",
            candidate.status
        ));
        return Ok(());
    }

    update_candidate_status(&candidates_path, candidate_id, "rejected", &mut candidates)?;

    ui::success(&format!("Rejected {candidate_id}"));

    Ok(())
}

// ── Parsing ────────────────────────────────────────────────────────

fn parse_candidates_file(path: &Path) -> Result<Vec<ParsedCandidate>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let mut candidates = Vec::new();
    let mut current: Option<ParsedCandidate> = None;
    let mut in_provenance = false;

    for line in content.lines() {
        if let Some(id) = line.strip_prefix("### ") {
            // Save previous
            if let Some(c) = current.take() {
                candidates.push(c);
            }
            current = Some(ParsedCandidate {
                id: id.trim().to_string(),
                status: "pending".to_string(),
                type_: String::new(),
                text: String::new(),
                target: String::new(),
                provenance: Vec::new(),
            });
            in_provenance = false;
        } else if let Some(c) = current.as_mut() {
            if line.contains("**status**:") {
                c.status = line
                    .split("**status**:")
                    .nth(1)
                    .unwrap_or("pending")
                    .trim()
                    .to_string();
                in_provenance = false;
            } else if line.contains("**type**:") {
                c.type_ = line
                    .split("**type**:")
                    .nth(1)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                in_provenance = false;
            } else if line.contains("**target**:") {
                c.target = line
                    .split("**target**:")
                    .nth(1)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                in_provenance = false;
            } else if line.contains("**provenance**:") {
                in_provenance = true;
            } else if in_provenance && line.trim().starts_with("- ") {
                let entry = line.trim().strip_prefix("- ").unwrap_or("");
                if let Some((kind, ref_)) = entry.split_once(": ") {
                    c.provenance
                        .push((kind.trim().to_string(), ref_.trim().to_string()));
                }
            } else if let Some(quoted) = line.strip_prefix("> ") {
                if c.text.is_empty() {
                    c.text = quoted.trim().to_string();
                }
                in_provenance = false;
            } else if !line.trim().starts_with("- ") || !in_provenance {
                in_provenance = false;
            }
        }
    }

    if let Some(c) = current {
        candidates.push(c);
    }

    Ok(candidates)
}

fn update_candidate_status(
    path: &Path,
    candidate_id: &str,
    new_status: &str,
    _candidates: &mut [ParsedCandidate],
) -> Result<()> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let mut result = String::new();
    let mut in_target_candidate = false;

    for line in content.lines() {
        if let Some(id) = line.strip_prefix("### ") {
            in_target_candidate = id.trim() == candidate_id;
        }

        if in_target_candidate && line.contains("**status**:") {
            result.push_str(&format!("- **status**: {new_status}"));
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }

    std::fs::write(path, &result).with_context(|| format!("Failed to write {}", path.display()))?;

    Ok(())
}

fn resolve_id_conflict(proposed_id: &str, note: &WikiNote) -> String {
    let existing_ids: HashSet<&str> = note.memory_items.iter().map(|i| i.id.as_str()).collect();

    if !existing_ids.contains(proposed_id) {
        return proposed_id.to_string();
    }

    // Find the highest numbered ID in this domain and increment
    let domain_prefix = proposed_id
        .rfind('-')
        .map(|pos| &proposed_id[..pos])
        .unwrap_or(proposed_id);

    let max_num = note
        .memory_items
        .iter()
        .filter_map(|i| {
            i.id.strip_prefix(domain_prefix)
                .and_then(|s| s.strip_prefix('-'))
                .and_then(|s| s.parse::<u32>().ok())
        })
        .max()
        .unwrap_or(0);

    format!("{}-{:03}", domain_prefix, max_num + 1)
}

fn parse_confidence(s: &str) -> Result<Confidence> {
    match s {
        "confirmed" => Ok(Confidence::Confirmed),
        "verified" => Ok(Confidence::Verified),
        "seen-in-code" => Ok(Confidence::SeenInCode),
        "inferred" => Ok(Confidence::Inferred),
        "needs-validation" => Ok(Confidence::NeedsValidation),
        _ => bail!("Unknown confidence level: '{s}'"),
    }
}

fn parse_item_type(s: &str) -> Result<MemoryItemType> {
    match s {
        "exception" => Ok(MemoryItemType::Exception),
        "decision" => Ok(MemoryItemType::Decision),
        "business_rule" => Ok(MemoryItemType::BusinessRule),
        _ => bail!("Unknown item type: '{s}'"),
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_wiki(dir: &TempDir) -> std::path::PathBuf {
        let wiki = dir.path().join(".wiki");
        fs::create_dir_all(wiki.join("domains/billing")).unwrap();
        wiki
    }

    fn create_candidates_file(wiki: &Path, content: &str) {
        fs::write(wiki.join("_candidates.md"), content).unwrap();
    }

    fn create_note(wiki: &Path, domain: &str, content: &str) {
        let path = wiki.join("domains").join(domain).join("_overview.md");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
    }

    fn candidates_content() -> String {
        r#"# Memory Candidates

> Auto-generated proposals.

## billing

### billing-001

- **status**: pending
- **type**: exception
- **confidence**: inferred
- **provenance**:
  - file: src/billing/legacy.ts
  - test: tests/billing/legacy.test.ts
- **rationale**: Legacy naming pattern
- **target**: .wiki/domains/billing/_overview.md

> Le client X utilise encore l'ancien calcul

**Action** : `confirm` | `reformulate` | `reject`

### billing-002

- **status**: pending
- **type**: decision
- **confidence**: inferred
- **provenance**:
  - comment: [NOTE] We decided not to deduplicate
- **rationale**: Decision comment pattern
- **target**: .wiki/domains/billing/_overview.md

> Pas de deduplication des lignes importees

**Action** : `confirm` | `reformulate` | `reject`
"#
        .to_string()
    }

    fn note_content() -> String {
        r#"---
title: Billing overview
confidence: verified
last_updated: "2026-03-20"
related_files:
  - src/billing/invoice.ts
---
# Billing

Handles invoicing.
"#
        .to_string()
    }

    #[test]
    fn test_promote_basic() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_candidates_file(&wiki, &candidates_content());
        create_note(&wiki, "billing", &note_content());

        promote(&wiki, "billing-001", None, None).unwrap();

        let note = WikiNote::parse(&wiki.join("domains/billing/_overview.md")).unwrap();
        assert_eq!(note.memory_items.len(), 1);
        assert_eq!(note.memory_items[0].id, "billing-001");
        assert_eq!(note.memory_items[0].type_, MemoryItemType::Exception);
        assert_eq!(note.memory_items[0].confidence, Confidence::Confirmed);
    }

    #[test]
    fn test_promote_with_text_override() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_candidates_file(&wiki, &candidates_content());
        create_note(&wiki, "billing", &note_content());

        promote(&wiki, "billing-001", None, Some("Custom reformulated text")).unwrap();

        let note = WikiNote::parse(&wiki.join("domains/billing/_overview.md")).unwrap();
        assert_eq!(note.memory_items[0].text, "Custom reformulated text");
    }

    #[test]
    fn test_promote_with_confidence_override() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_candidates_file(&wiki, &candidates_content());
        create_note(&wiki, "billing", &note_content());

        promote(&wiki, "billing-001", Some("seen-in-code"), None).unwrap();

        let note = WikiNote::parse(&wiki.join("domains/billing/_overview.md")).unwrap();
        assert_eq!(note.memory_items[0].confidence, Confidence::SeenInCode);
    }

    #[test]
    fn test_promote_updates_candidate_status() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_candidates_file(&wiki, &candidates_content());
        create_note(&wiki, "billing", &note_content());

        promote(&wiki, "billing-001", None, None).unwrap();

        let content = fs::read_to_string(wiki.join("_candidates.md")).unwrap();
        // billing-001 should be confirmed now
        assert!(content.contains("**status**: confirmed"));
    }

    #[test]
    fn test_promote_sets_last_reviewed() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_candidates_file(&wiki, &candidates_content());
        create_note(&wiki, "billing", &note_content());

        promote(&wiki, "billing-001", None, None).unwrap();

        let note = WikiNote::parse(&wiki.join("domains/billing/_overview.md")).unwrap();
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert_eq!(note.memory_items[0].last_reviewed, Some(today));
    }

    #[test]
    fn test_promote_preserves_existing_items() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_candidates_file(&wiki, &candidates_content());

        // Note already has an item
        let content = r#"---
title: Billing
confidence: verified
memory_items:
  - id: billing-existing
    type: business_rule
    text: Existing rule
    confidence: confirmed
    status: active
---
# Billing
"#;
        create_note(&wiki, "billing", content);

        promote(&wiki, "billing-001", None, None).unwrap();

        let note = WikiNote::parse(&wiki.join("domains/billing/_overview.md")).unwrap();
        assert_eq!(note.memory_items.len(), 2);
        assert!(note.memory_items.iter().any(|i| i.id == "billing-existing"));
        assert!(note.memory_items.iter().any(|i| i.id == "billing-001"));
    }

    #[test]
    fn test_promote_preserves_markdown_content() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_candidates_file(&wiki, &candidates_content());
        create_note(&wiki, "billing", &note_content());

        promote(&wiki, "billing-001", None, None).unwrap();

        let note = WikiNote::parse(&wiki.join("domains/billing/_overview.md")).unwrap();
        assert!(note.content.contains("Handles invoicing."));
    }

    #[test]
    fn test_reject_basic() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_candidates_file(&wiki, &candidates_content());
        create_note(&wiki, "billing", &note_content());

        reject(&wiki, "billing-002").unwrap();

        let content = fs::read_to_string(wiki.join("_candidates.md")).unwrap();
        // billing-002 should be rejected
        // Check that the status line for billing-002 section was updated
        let sections: Vec<&str> = content.split("### ").collect();
        let billing_002 = sections.iter().find(|s| s.starts_with("billing-002"));
        assert!(billing_002.is_some());
        assert!(billing_002.unwrap().contains("**status**: rejected"));
    }

    #[test]
    fn test_reject_does_not_modify_note() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_candidates_file(&wiki, &candidates_content());
        create_note(&wiki, "billing", &note_content());

        let before = fs::read_to_string(wiki.join("domains/billing/_overview.md")).unwrap();

        reject(&wiki, "billing-001").unwrap();

        let after = fs::read_to_string(wiki.join("domains/billing/_overview.md")).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn test_promote_candidate_not_found() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_candidates_file(&wiki, &candidates_content());

        let result = promote(&wiki, "billing-999", None, None);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No candidate found with id 'billing-999'")
        );
    }

    #[test]
    fn test_promote_candidate_already_processed() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        let content = candidates_content().replace("**status**: pending", "**status**: confirmed");
        create_candidates_file(&wiki, &content);
        create_note(&wiki, "billing", &note_content());

        // Should not error, just warn
        promote(&wiki, "billing-001", None, None).unwrap();

        // Note should not have been modified
        let note = WikiNote::parse(&wiki.join("domains/billing/_overview.md")).unwrap();
        assert!(note.memory_items.is_empty());
    }

    #[test]
    fn test_promote_id_conflict_auto_resolve() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_candidates_file(&wiki, &candidates_content());

        // Note already has billing-001
        let content = r#"---
title: Billing
confidence: verified
memory_items:
  - id: billing-001
    type: business_rule
    text: Existing item
    confidence: confirmed
    status: active
---
# Billing
"#;
        create_note(&wiki, "billing", content);

        promote(&wiki, "billing-001", None, None).unwrap();

        let note = WikiNote::parse(&wiki.join("domains/billing/_overview.md")).unwrap();
        assert_eq!(note.memory_items.len(), 2);
        // The new item should have a different ID
        let new_item = note
            .memory_items
            .iter()
            .find(|i| i.id != "billing-001")
            .expect("Should have a new item with different ID");
        assert_eq!(new_item.id, "billing-002");
    }

    #[test]
    fn test_resolve_id_conflict() {
        let note = WikiNote {
            path: "test.md".to_string(),
            domain: "billing".to_string(),
            confidence: Confidence::Verified,
            last_updated: None,
            related_files: Vec::new(),
            deprecated: false,
            title: "Test".to_string(),
            content: String::new(),
            memory_items: vec![MemoryItem {
                id: "billing-001".to_string(),
                type_: MemoryItemType::Exception,
                text: "Existing".to_string(),
                confidence: Confidence::Confirmed,
                related_files: Vec::new(),
                sources: Vec::new(),
                status: MemoryItemStatus::Active,
                last_reviewed: None,
            }],
        };

        assert_eq!(resolve_id_conflict("billing-002", &note), "billing-002"); // no conflict
        assert_eq!(resolve_id_conflict("billing-001", &note), "billing-002"); // conflict resolved
    }

    #[test]
    fn test_parse_candidates_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("_candidates.md");
        fs::write(&path, candidates_content()).unwrap();

        let candidates = parse_candidates_file(&path).unwrap();
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].id, "billing-001");
        assert_eq!(candidates[0].type_, "exception");
        assert_eq!(candidates[0].status, "pending");
        assert!(!candidates[0].provenance.is_empty());
        assert_eq!(candidates[1].id, "billing-002");
        assert_eq!(candidates[1].type_, "decision");
    }
}
