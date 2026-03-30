use std::io::Read as _;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::wiki::common::{ensure_wiki_exists, find_wiki_root};
use crate::wiki::file_index;
use crate::wiki::note::{Confidence, MemoryItem, MemoryItemStatus, MemoryItemType, WikiNote};

/// Maximum length for the compact context injected into Claude's context window.
const MAX_CONTEXT_LEN: usize = 2000;

// ─── Hook JSON types ───

#[derive(Deserialize)]
struct HookInput {
    tool_input: serde_json::Value,
    #[allow(dead_code)]
    cwd: Option<String>,
}

#[derive(Serialize)]
struct HookOutput {
    #[serde(rename = "additionalContext")]
    additional_context: String,
}

// ─── Public API ───

/// JSON output for `context --json`.
#[derive(Debug, Serialize)]
pub struct ContextJsonOutput {
    pub schema_version: String,
    pub domain: Option<String>,
    pub confidence: Option<String>,
    pub last_updated: Option<String>,
    pub memory_items: Vec<ContextJsonItem>,
    pub warnings: Vec<String>,
    pub fallback_mode: bool,
}

/// JSON representation of a memory item in context output.
#[derive(Debug, Serialize)]
pub struct ContextJsonItem {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub text: String,
    pub confidence: String,
}

/// CLI entry point: print context for a file to stdout.
pub fn run(file: &str, json: bool) -> Result<()> {
    let wiki_dir = find_wiki_root()?;

    let project_root = wiki_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Wiki directory has no parent"))?;

    if json {
        let output = resolve_context_json(file, &wiki_dir, project_root)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&output)
                .map_err(|e| anyhow::anyhow!("JSON serialization: {e}"))?
        );
        return Ok(());
    }

    match resolve_context(file, &wiki_dir, project_root)? {
        Some(ctx) => {
            println!("{}", ctx);
            Ok(())
        }
        None => {
            eprintln!("[project-wiki] No wiki context found for: {}", file);
            Ok(())
        }
    }
}

/// Resolve context as structured JSON for a file.
fn resolve_context_json(
    file_path: &str,
    wiki_dir: &Path,
    project_root: &Path,
) -> Result<ContextJsonOutput> {
    ensure_wiki_exists(wiki_dir)?;

    let index = file_index::load_or_rebuild(wiki_dir)?;

    let domain = file_index::resolve_domain(&index, file_path, project_root);

    if domain.is_none() {
        return Ok(ContextJsonOutput {
            schema_version: "1".to_string(),
            domain: None,
            confidence: None,
            last_updated: None,
            memory_items: Vec::new(),
            warnings: vec!["No domain found for this file".to_string()],
            fallback_mode: false,
        });
    }

    let domain = domain.unwrap();
    let overview_path = wiki_dir.join("domains").join(&domain).join("_overview.md");

    if !overview_path.exists() {
        return Ok(ContextJsonOutput {
            schema_version: "1".to_string(),
            domain: Some(domain),
            confidence: None,
            last_updated: None,
            memory_items: Vec::new(),
            warnings: vec!["Domain overview not found".to_string()],
            fallback_mode: false,
        });
    }

    let note = WikiNote::parse(&overview_path)?;
    let fallback_mode = note.memory_items.is_empty();

    let prioritized = prioritize_memory_items(&note.memory_items, file_path, MAX_MEMORY_ITEMS);
    let items: Vec<ContextJsonItem> = prioritized
        .into_iter()
        .map(|item| ContextJsonItem {
            id: item.id.clone(),
            type_: item.type_.to_string(),
            text: item.text.clone(),
            confidence: item.confidence.to_string(),
        })
        .collect();

    let mut warnings = Vec::new();
    let low_confidence_items: Vec<&MemoryItem> = note
        .memory_items
        .iter()
        .filter(|i| {
            matches!(
                i.confidence,
                Confidence::Inferred | Confidence::NeedsValidation
            )
        })
        .collect();
    if !low_confidence_items.is_empty() {
        warnings.push(format!(
            "{} item(s) have low confidence — verify before relying on them",
            low_confidence_items.len()
        ));
    }

    Ok(ContextJsonOutput {
        schema_version: "1".to_string(),
        domain: Some(domain),
        confidence: Some(note.confidence.to_string()),
        last_updated: note.last_updated.map(|d| d.to_string()),
        memory_items: items,
        warnings,
        fallback_mode,
    })
}

/// Hook entry point: read JSON from stdin, write JSON to stdout.
pub fn run_from_stdin() -> Result<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;

    let hook_input: HookInput = serde_json::from_str(&input).unwrap_or(HookInput {
        tool_input: serde_json::Value::Null,
        cwd: None,
    });

    // Extract file_path from tool_input
    let file_path = hook_input
        .tool_input
        .get("file_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if file_path.is_empty() {
        // No file path in the hook input — nothing to do
        return Ok(());
    }

    // Try to find the wiki, but don't fail if it doesn't exist
    let wiki_dir = match find_wiki_root() {
        Ok(dir) => dir,
        Err(_) => return Ok(()), // No wiki — silent exit
    };

    let project_root = match wiki_dir.parent() {
        Some(root) => root,
        None => return Ok(()),
    };

    if let Some(ctx) = resolve_context(file_path, &wiki_dir, project_root)? {
        let output = HookOutput {
            additional_context: ctx,
        };
        println!("{}", serde_json::to_string(&output)?);
    }

    Ok(())
}

/// Core logic: resolve wiki context for a given file path.
pub fn resolve_context(
    file_path: &str,
    wiki_dir: &Path,
    project_root: &Path,
) -> Result<Option<String>> {
    ensure_wiki_exists(wiki_dir)?;

    let index = file_index::load_or_rebuild(wiki_dir)?;

    let domain = match file_index::resolve_domain(&index, file_path, project_root) {
        Some(d) => d,
        None => return Ok(None),
    };

    // Read the domain's _overview.md
    let overview_path = wiki_dir.join("domains").join(&domain).join("_overview.md");
    if !overview_path.exists() {
        return Ok(None);
    }

    let note = WikiNote::parse(&overview_path)?;
    Ok(Some(compact_summary(&note, &domain, file_path)))
}

/// Maximum number of memory items to include in context output.
const MAX_MEMORY_ITEMS: usize = 3;

/// Format a WikiNote into a compact summary for LLM context injection.
///
/// If the note has structured `memory_items`, uses prioritized v1 format.
/// Otherwise, falls back to extracting sections from markdown content.
pub fn compact_summary(note: &WikiNote, domain: &str, file_path: &str) -> String {
    if note.memory_items.is_empty() {
        compact_summary_fallback(note, domain)
    } else {
        compact_summary_v1(note, domain, file_path)
    }
}

/// V1 format: structured memory items with type-based prioritization.
fn compact_summary_v1(note: &WikiNote, domain: &str, file_path: &str) -> String {
    let updated = note
        .last_updated
        .map(|d| d.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let mut parts = Vec::new();

    parts.push(format!(
        "[project-wiki] Domain: {} (confidence: {}, updated: {})",
        domain, note.confidence, updated
    ));

    // Prioritize and select memory items
    let prioritized = prioritize_memory_items(&note.memory_items, file_path, MAX_MEMORY_ITEMS);
    let total_active = note
        .memory_items
        .iter()
        .filter(|i| matches!(i.status, MemoryItemStatus::Active))
        .count();

    if !prioritized.is_empty() {
        parts.push("Memory:".to_string());
        for item in &prioritized {
            parts.push(format!(
                "  [{}] {} [{}]",
                item.type_, item.text, item.confidence
            ));
        }
        if total_active > prioritized.len() {
            parts.push(format!(
                "  (+{} more items)",
                total_active - prioritized.len()
            ));
        }
    }

    // Dependencies from markdown (still useful alongside memory items)
    let sections = extract_sections(&note.content);
    if let Some(deps) = sections.get("dependencies") {
        let items = extract_bullet_points(deps, 10);
        if !items.is_empty() {
            parts.push(format!("Dependencies: {}", items.join(", ")));
        }
    }

    if !note.related_files.is_empty() {
        let files: Vec<&str> = note
            .related_files
            .iter()
            .take(10)
            .map(|s| s.as_str())
            .collect();
        parts.push(format!("Related files: {}", files.join(", ")));
    }

    // Warnings
    let low_confidence_count = note
        .memory_items
        .iter()
        .filter(|i| {
            matches!(
                i.confidence,
                Confidence::Inferred | Confidence::NeedsValidation
            ) && matches!(i.status, MemoryItemStatus::Active)
        })
        .count();

    if low_confidence_count > 0 {
        parts.push(format!(
            "WARNING: {} item(s) have low confidence — verify before relying on them.",
            low_confidence_count
        ));
    } else if note.confidence == Confidence::NeedsValidation
        || note.confidence == Confidence::Inferred
    {
        parts.push(format!(
            "WARNING: This wiki note has low confidence ({}). Verify before relying on it.",
            note.confidence
        ));
    }

    truncate_output(parts.join("\n"))
}

/// Fallback format: extract sections from markdown (for notes without memory_items).
fn compact_summary_fallback(note: &WikiNote, domain: &str) -> String {
    let updated = note
        .last_updated
        .map(|d| d.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let mut parts = Vec::new();

    parts.push(format!(
        "[project-wiki] Domain: {} (confidence: {}, updated: {})",
        domain, note.confidence, updated
    ));

    let sections = extract_sections(&note.content);

    if let Some(behaviors) = sections.get("key behaviors") {
        let items = extract_bullet_points(behaviors, 5);
        if !items.is_empty() {
            parts.push(format!("Key behaviors: {}", items.join(" — ")));
        }
    }

    if let Some(rules) = sections.get("business rules") {
        let items = extract_bullet_points(rules, 5);
        if !items.is_empty() {
            parts.push(format!("Business rules: {}", items.join(" — ")));
        }
    }

    if let Some(deps) = sections.get("dependencies") {
        let items = extract_bullet_points(deps, 10);
        if !items.is_empty() {
            parts.push(format!("Dependencies: {}", items.join(", ")));
        }
    }

    if !note.related_files.is_empty() {
        let files: Vec<&str> = note
            .related_files
            .iter()
            .take(10)
            .map(|s| s.as_str())
            .collect();
        parts.push(format!("Related files: {}", files.join(", ")));
    }

    if note.confidence == Confidence::NeedsValidation || note.confidence == Confidence::Inferred {
        parts.push(format!(
            "WARNING: This wiki note has low confidence ({}). Verify before relying on it.",
            note.confidence
        ));
    }

    truncate_output(parts.join("\n"))
}

// ─── Prioritization ───

/// Sort and select the top memory items for context injection.
///
/// Priority order:
/// 1. Type: exception > decision > business_rule
/// 2. Confidence: confirmed/verified > seen-in-code > inferred > needs-validation
/// 3. Related file match: items whose related_files match the queried file come first
fn prioritize_memory_items<'a>(
    items: &'a [MemoryItem],
    file_path: &str,
    max: usize,
) -> Vec<&'a MemoryItem> {
    let mut active_items: Vec<&MemoryItem> = items
        .iter()
        .filter(|i| matches!(i.status, MemoryItemStatus::Active))
        .collect();

    active_items.sort_by(|a, b| {
        let key_a = (
            type_priority(&a.type_),
            confidence_priority(&a.confidence),
            if has_related_file(a, file_path) {
                0u8
            } else {
                1u8
            },
        );
        let key_b = (
            type_priority(&b.type_),
            confidence_priority(&b.confidence),
            if has_related_file(b, file_path) {
                0u8
            } else {
                1u8
            },
        );
        key_a.cmp(&key_b)
    });

    active_items.into_iter().take(max).collect()
}

fn type_priority(t: &MemoryItemType) -> u8 {
    match t {
        MemoryItemType::Exception => 0,
        MemoryItemType::Decision => 1,
        MemoryItemType::BusinessRule => 2,
    }
}

fn confidence_priority(c: &Confidence) -> u8 {
    match c {
        Confidence::Confirmed | Confidence::Verified => 0,
        Confidence::SeenInCode => 1,
        Confidence::Inferred => 2,
        Confidence::NeedsValidation => 3,
    }
}

fn has_related_file(item: &MemoryItem, file_path: &str) -> bool {
    item.related_files.iter().any(|f| f == file_path)
}

// ─── Helpers ───

fn truncate_output(result: String) -> String {
    if result.len() > MAX_CONTEXT_LEN {
        let mut truncated = result[..MAX_CONTEXT_LEN - 20].to_string();
        truncated.push_str("\n[... truncated]");
        truncated
    } else {
        result
    }
}

/// Extract markdown sections (## heading -> body) from content.
fn extract_sections(content: &str) -> std::collections::HashMap<String, String> {
    let mut sections = std::collections::HashMap::new();
    let mut current_heading = String::new();
    let mut current_body = String::new();

    for line in content.lines() {
        if line.starts_with("## ") {
            if !current_heading.is_empty() {
                sections.insert(
                    current_heading.to_lowercase(),
                    current_body.trim().to_string(),
                );
            }
            current_heading = line.trim_start_matches("## ").to_string();
            current_body = String::new();
        } else if !current_heading.is_empty() {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }

    if !current_heading.is_empty() {
        sections.insert(
            current_heading.to_lowercase(),
            current_body.trim().to_string(),
        );
    }

    sections
}

/// Extract bullet points from a section body, up to `max` items.
fn extract_bullet_points(body: &str, max: usize) -> Vec<String> {
    body.lines()
        .filter(|line| line.trim_start().starts_with("- "))
        .take(max)
        .map(|line| {
            line.trim_start()
                .trim_start_matches("- ")
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty() && !s.starts_with('_'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiki::note::{
        Confidence, MemoryItem, MemoryItemSource, MemoryItemStatus, MemoryItemType,
    };
    use chrono::NaiveDate;

    fn make_note(domain: &str, confidence: Confidence, content: &str) -> WikiNote {
        WikiNote {
            path: format!(".wiki/domains/{}/_overview.md", domain),
            domain: domain.to_string(),
            confidence,
            last_updated: Some(NaiveDate::from_ymd_opt(2026, 3, 28).unwrap()),
            related_files: vec![format!("src/{}/main.ts", domain)],
            deprecated: false,
            title: format!("{} overview", domain),
            content: content.to_string(),
            memory_items: Vec::new(),
        }
    }

    fn make_item(
        id: &str,
        type_: MemoryItemType,
        text: &str,
        confidence: Confidence,
    ) -> MemoryItem {
        MemoryItem {
            id: id.to_string(),
            type_,
            text: text.to_string(),
            confidence,
            related_files: Vec::new(),
            sources: vec![MemoryItemSource {
                kind: "file".to_string(),
                ref_: "src/test.ts".to_string(),
                line: None,
            }],
            status: MemoryItemStatus::Active,
            last_reviewed: None,
        }
    }

    fn make_note_with_items(
        domain: &str,
        confidence: Confidence,
        items: Vec<MemoryItem>,
    ) -> WikiNote {
        WikiNote {
            path: format!(".wiki/domains/{}/_overview.md", domain),
            domain: domain.to_string(),
            confidence,
            last_updated: Some(NaiveDate::from_ymd_opt(2026, 3, 28).unwrap()),
            related_files: vec![format!("src/{}/main.ts", domain)],
            deprecated: false,
            title: format!("{} overview", domain),
            content: "## Dependencies\n- payments\n- taxes\n".to_string(),
            memory_items: items,
        }
    }

    // ─── Fallback tests (notes without memory_items) ───

    #[test]
    fn compact_summary_includes_domain_info() {
        let note = make_note(
            "billing",
            Confidence::Confirmed,
            "## Key behaviors\n- Generates invoices\n- Handles refunds\n",
        );
        let summary = compact_summary(&note, "billing", "src/billing/main.ts");

        assert!(summary.contains("[project-wiki] Domain: billing"));
        assert!(summary.contains("confirmed"));
        assert!(summary.contains("2026-03-28"));
    }

    #[test]
    fn compact_summary_includes_behaviors() {
        let note = make_note(
            "billing",
            Confidence::Confirmed,
            "## Key behaviors\n- Generates invoices\n- Handles refunds\n",
        );
        let summary = compact_summary(&note, "billing", "src/billing/main.ts");

        assert!(summary.contains("Generates invoices"));
        assert!(summary.contains("Handles refunds"));
    }

    #[test]
    fn compact_summary_includes_business_rules() {
        let note = make_note(
            "billing",
            Confidence::Confirmed,
            "## Business rules\n- No dedup on import\n",
        );
        let summary = compact_summary(&note, "billing", "src/billing/main.ts");

        assert!(summary.contains("No dedup on import"));
    }

    #[test]
    fn compact_summary_includes_related_files() {
        let note = make_note("billing", Confidence::Confirmed, "# Billing\n");
        let summary = compact_summary(&note, "billing", "src/billing/main.ts");

        assert!(summary.contains("src/billing/main.ts"));
    }

    #[test]
    fn compact_summary_warns_on_low_confidence() {
        let note = make_note("billing", Confidence::NeedsValidation, "# Billing\n");
        let summary = compact_summary(&note, "billing", "src/billing/main.ts");

        assert!(summary.contains("WARNING"));
        assert!(summary.contains("needs-validation"));
    }

    #[test]
    fn compact_summary_no_warning_on_confirmed() {
        let note = make_note("billing", Confidence::Confirmed, "# Billing\n");
        let summary = compact_summary(&note, "billing", "src/billing/main.ts");

        assert!(!summary.contains("WARNING"));
    }

    #[test]
    fn compact_summary_truncates_long_content() {
        let mut long_content = String::new();
        for section in &[
            "Key behaviors",
            "Business rules",
            "Dependencies",
            "Architecture notes",
        ] {
            long_content.push_str(&format!("## {}\n", section));
            for i in 0..50 {
                long_content.push_str(&format!(
                    "- Item {} in {} with a very long description that adds significant length to the output to ensure we exceed the truncation threshold eventually\n",
                    i, section
                ));
            }
        }
        let mut note = make_note("billing", Confidence::Confirmed, &long_content);
        note.related_files = (0..50)
            .map(|i| format!("src/billing/very/deep/nested/module_{}/handler.ts", i))
            .collect();
        let summary = compact_summary(&note, "billing", "src/billing/main.ts");

        assert!(summary.len() <= MAX_CONTEXT_LEN);
        assert!(summary.contains("[... truncated]"));
    }

    #[test]
    fn extract_sections_parses_headings() {
        let content = "## Description\nSome text.\n\n## Key behaviors\n- One\n- Two\n";
        let sections = extract_sections(content);

        assert!(sections.contains_key("description"));
        assert!(sections.contains_key("key behaviors"));
        assert!(sections["key behaviors"].contains("- One"));
    }

    #[test]
    fn extract_bullet_points_limits_count() {
        let body = "- One\n- Two\n- Three\n- Four\n- Five\n- Six\n";
        let items = extract_bullet_points(body, 3);
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn extract_bullet_points_skips_placeholders() {
        let body = "- _None detected._\n- Real item\n";
        let items = extract_bullet_points(body, 10);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0], "Real item");
    }

    // ─── V1 tests (notes with memory_items) ───

    #[test]
    fn context_v1_prioritize_exception_first() {
        let items = vec![
            make_item(
                "b-001",
                MemoryItemType::BusinessRule,
                "Rule A",
                Confidence::Confirmed,
            ),
            make_item(
                "b-002",
                MemoryItemType::Decision,
                "Decision B",
                Confidence::Confirmed,
            ),
            make_item(
                "b-003",
                MemoryItemType::Exception,
                "Exception C",
                Confidence::Confirmed,
            ),
        ];
        let note = make_note_with_items("billing", Confidence::Confirmed, items);
        let summary = compact_summary(&note, "billing", "src/billing/main.ts");

        let exc_pos = summary.find("[exception]").unwrap();
        let dec_pos = summary.find("[decision]").unwrap();
        let rule_pos = summary.find("[business_rule]").unwrap();
        assert!(exc_pos < dec_pos, "exception should come before decision");
        assert!(
            dec_pos < rule_pos,
            "decision should come before business_rule"
        );
    }

    #[test]
    fn context_v1_secondary_sort_by_confidence() {
        let items = vec![
            make_item(
                "b-001",
                MemoryItemType::Decision,
                "Inferred decision",
                Confidence::Inferred,
            ),
            make_item(
                "b-002",
                MemoryItemType::Decision,
                "Confirmed decision",
                Confidence::Confirmed,
            ),
        ];
        let note = make_note_with_items("billing", Confidence::Confirmed, items);
        let summary = compact_summary(&note, "billing", "src/billing/main.ts");

        let confirmed_pos = summary.find("Confirmed decision").unwrap();
        let inferred_pos = summary.find("Inferred decision").unwrap();
        assert!(
            confirmed_pos < inferred_pos,
            "confirmed should come before inferred"
        );
    }

    #[test]
    fn context_v1_secondary_sort_by_related_file() {
        let mut item_related = make_item(
            "b-001",
            MemoryItemType::Decision,
            "Related decision",
            Confidence::Confirmed,
        );
        item_related.related_files = vec!["src/billing/invoice.ts".to_string()];

        let item_unrelated = make_item(
            "b-002",
            MemoryItemType::Decision,
            "Unrelated decision",
            Confidence::Confirmed,
        );

        let note = make_note_with_items(
            "billing",
            Confidence::Confirmed,
            vec![item_unrelated, item_related],
        );
        let summary = compact_summary(&note, "billing", "src/billing/invoice.ts");

        let related_pos = summary.find("Related decision").unwrap();
        let unrelated_pos = summary.find("Unrelated decision").unwrap();
        assert!(
            related_pos < unrelated_pos,
            "related file match should come first"
        );
    }

    #[test]
    fn context_v1_limit_3_items() {
        let items = vec![
            make_item(
                "b-001",
                MemoryItemType::Exception,
                "Exc 1",
                Confidence::Confirmed,
            ),
            make_item(
                "b-002",
                MemoryItemType::Decision,
                "Dec 2",
                Confidence::Confirmed,
            ),
            make_item(
                "b-003",
                MemoryItemType::BusinessRule,
                "Rule 3",
                Confidence::Confirmed,
            ),
            make_item(
                "b-004",
                MemoryItemType::BusinessRule,
                "Rule 4",
                Confidence::Confirmed,
            ),
            make_item(
                "b-005",
                MemoryItemType::BusinessRule,
                "Rule 5",
                Confidence::Confirmed,
            ),
        ];
        let note = make_note_with_items("billing", Confidence::Confirmed, items);
        let summary = compact_summary(&note, "billing", "src/billing/main.ts");

        assert!(summary.contains("Exc 1"));
        assert!(summary.contains("Dec 2"));
        assert!(summary.contains("Rule 3"));
        assert!(!summary.contains("Rule 4"));
        assert!(!summary.contains("Rule 5"));
        assert!(summary.contains("(+2 more items)"));
    }

    #[test]
    fn context_v1_format_type_and_confidence_brackets() {
        let items = vec![make_item(
            "b-001",
            MemoryItemType::Exception,
            "Client X uses old calc",
            Confidence::Confirmed,
        )];
        let note = make_note_with_items("billing", Confidence::Confirmed, items);
        let summary = compact_summary(&note, "billing", "src/billing/main.ts");

        assert!(summary.contains("[exception] Client X uses old calc [confirmed]"));
    }

    #[test]
    fn context_v1_warning_low_confidence_items() {
        let items = vec![
            make_item(
                "b-001",
                MemoryItemType::Decision,
                "Dec A",
                Confidence::Confirmed,
            ),
            make_item(
                "b-002",
                MemoryItemType::BusinessRule,
                "Rule B",
                Confidence::Inferred,
            ),
        ];
        let note = make_note_with_items("billing", Confidence::Confirmed, items);
        let summary = compact_summary(&note, "billing", "src/billing/main.ts");

        assert!(summary.contains("WARNING"));
        assert!(summary.contains("1 item(s) have low confidence"));
    }

    #[test]
    fn context_v1_no_warning_all_confirmed() {
        let items = vec![make_item(
            "b-001",
            MemoryItemType::Decision,
            "Dec A",
            Confidence::Confirmed,
        )];
        let note = make_note_with_items("billing", Confidence::Confirmed, items);
        let summary = compact_summary(&note, "billing", "src/billing/main.ts");

        assert!(!summary.contains("WARNING"));
    }

    #[test]
    fn context_v1_includes_dependencies() {
        let items = vec![make_item(
            "b-001",
            MemoryItemType::Decision,
            "Dec A",
            Confidence::Confirmed,
        )];
        let note = make_note_with_items("billing", Confidence::Confirmed, items);
        let summary = compact_summary(&note, "billing", "src/billing/main.ts");

        assert!(summary.contains("Dependencies: payments, taxes"));
    }

    #[test]
    fn context_v1_filters_deprecated_items() {
        let mut deprecated_item = make_item(
            "b-001",
            MemoryItemType::Exception,
            "Old exception",
            Confidence::Confirmed,
        );
        deprecated_item.status = MemoryItemStatus::Deprecated;

        let active_item = make_item(
            "b-002",
            MemoryItemType::Decision,
            "Active decision",
            Confidence::Confirmed,
        );

        let note = make_note_with_items(
            "billing",
            Confidence::Confirmed,
            vec![deprecated_item, active_item],
        );
        let summary = compact_summary(&note, "billing", "src/billing/main.ts");

        assert!(!summary.contains("Old exception"));
        assert!(summary.contains("Active decision"));
    }

    #[test]
    fn context_fallback_when_no_memory_items() {
        let note = make_note(
            "billing",
            Confidence::Confirmed,
            "## Key behaviors\n- Generates invoices\n## Business rules\n- No dedup\n",
        );
        let summary = compact_summary(&note, "billing", "src/billing/main.ts");

        // Fallback should show markdown sections, not "Memory:" header
        assert!(!summary.contains("Memory:"));
        assert!(summary.contains("Key behaviors:"));
        assert!(summary.contains("Business rules:"));
    }

    // ─── Prioritization unit tests ───

    #[test]
    fn prioritize_respects_type_order() {
        let items = vec![
            make_item(
                "1",
                MemoryItemType::BusinessRule,
                "Rule",
                Confidence::Confirmed,
            ),
            make_item("2", MemoryItemType::Exception, "Exc", Confidence::Confirmed),
            make_item("3", MemoryItemType::Decision, "Dec", Confidence::Confirmed),
        ];

        let result = prioritize_memory_items(&items, "", 3);
        assert_eq!(result[0].type_, MemoryItemType::Exception);
        assert_eq!(result[1].type_, MemoryItemType::Decision);
        assert_eq!(result[2].type_, MemoryItemType::BusinessRule);
    }

    #[test]
    fn prioritize_filters_deprecated() {
        let mut dep = make_item("1", MemoryItemType::Exception, "Old", Confidence::Confirmed);
        dep.status = MemoryItemStatus::Deprecated;
        let active = make_item("2", MemoryItemType::Decision, "New", Confidence::Confirmed);

        let items = vec![dep, active];
        let result = prioritize_memory_items(&items, "", 3);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "2");
    }

    #[test]
    fn prioritize_respects_max() {
        let items: Vec<MemoryItem> = (0..10)
            .map(|i| {
                make_item(
                    &format!("b-{:03}", i),
                    MemoryItemType::BusinessRule,
                    &format!("Rule {}", i),
                    Confidence::Confirmed,
                )
            })
            .collect();

        let result = prioritize_memory_items(&items, "", 3);
        assert_eq!(result.len(), 3);
    }
}
