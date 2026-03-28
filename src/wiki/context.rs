use std::io::Read as _;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::wiki::common::{ensure_wiki_exists, find_wiki_root};
use crate::wiki::file_index;
use crate::wiki::note::{Confidence, WikiNote};

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

/// CLI entry point: print context for a file to stdout.
pub fn run(file: &str) -> Result<()> {
    let wiki_dir = find_wiki_root()?;

    let project_root = wiki_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Wiki directory has no parent"))?;

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
    Ok(Some(compact_summary(&note, &domain)))
}

/// Format a WikiNote into a compact summary for LLM context injection.
pub fn compact_summary(note: &WikiNote, domain: &str) -> String {
    let updated = note
        .last_updated
        .map(|d| d.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let mut parts = Vec::new();

    parts.push(format!(
        "[project-wiki] Domain: {} (confidence: {}, updated: {})",
        domain, note.confidence, updated
    ));

    // Extract key sections from the markdown content
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

    let result = parts.join("\n");

    // Truncate if too long
    if result.len() > MAX_CONTEXT_LEN {
        let mut truncated = result[..MAX_CONTEXT_LEN - 20].to_string();
        truncated.push_str("\n[... truncated]");
        truncated
    } else {
        result
    }
}

// ─── Helpers ───

/// Extract markdown sections (## heading → body) from content.
fn extract_sections(content: &str) -> std::collections::HashMap<String, String> {
    let mut sections = std::collections::HashMap::new();
    let mut current_heading = String::new();
    let mut current_body = String::new();

    for line in content.lines() {
        if line.starts_with("## ") {
            // Save previous section
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

    // Save last section
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
        .filter(|s| !s.is_empty() && !s.starts_with('_')) // Skip "_None detected._" placeholders
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiki::note::Confidence;
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
        }
    }

    #[test]
    fn compact_summary_includes_domain_info() {
        let note = make_note(
            "billing",
            Confidence::Confirmed,
            "## Key behaviors\n- Generates invoices\n- Handles refunds\n",
        );
        let summary = compact_summary(&note, "billing");

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
        let summary = compact_summary(&note, "billing");

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
        let summary = compact_summary(&note, "billing");

        assert!(summary.contains("No dedup on import"));
    }

    #[test]
    fn compact_summary_includes_related_files() {
        let note = make_note("billing", Confidence::Confirmed, "# Billing\n");
        let summary = compact_summary(&note, "billing");

        assert!(summary.contains("src/billing/main.ts"));
    }

    #[test]
    fn compact_summary_warns_on_low_confidence() {
        let note = make_note("billing", Confidence::NeedsValidation, "# Billing\n");
        let summary = compact_summary(&note, "billing");

        assert!(summary.contains("WARNING"));
        assert!(summary.contains("needs-validation"));
    }

    #[test]
    fn compact_summary_no_warning_on_confirmed() {
        let note = make_note("billing", Confidence::Confirmed, "# Billing\n");
        let summary = compact_summary(&note, "billing");

        assert!(!summary.contains("WARNING"));
    }

    #[test]
    fn compact_summary_truncates_long_content() {
        // Build content with many sections to exceed MAX_CONTEXT_LEN
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
        // Add many related files to further increase length
        note.related_files = (0..50)
            .map(|i| format!("src/billing/very/deep/nested/module_{}/handler.ts", i))
            .collect();
        let summary = compact_summary(&note, "billing");

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
}
