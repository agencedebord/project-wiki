use anyhow::Result;

use super::{CheckDiffResult, DomainWarning, Sensitivity};

pub(super) fn format_text(result: &CheckDiffResult) -> String {
    let mut lines = Vec::new();

    lines.push("[project-wiki] Diff check".to_string());
    lines.push(String::new());

    let domain_count = result.domains.len();
    lines.push(format!("{} file(s) analyzed", result.files_analyzed));
    lines.push(format!("{} domain(s) affected", domain_count));

    let sensitivity_label = match result.sensitivity {
        Sensitivity::Low => "low",
        Sensitivity::Medium => "medium",
        Sensitivity::High => "high",
    };
    lines.push(format!("Sensitivity: {sensitivity_label}"));

    if !result.domains.is_empty() {
        lines.push(String::new());
        lines.push("Affected domains".to_string());
        for d in &result.domains {
            let item_count = d.memory_items.len();
            lines.push(format!(
                "  {} ({}) — {} file(s), {} item(s)",
                d.name,
                d.role,
                d.files.len(),
                item_count
            ));
        }
    }

    // Memory items
    let has_items = result.domains.iter().any(|d| !d.memory_items.is_empty());
    if has_items {
        lines.push(String::new());
        lines.push("Priority memory".to_string());
        for d in &result.domains {
            if d.memory_items.is_empty() {
                continue;
            }
            lines.push(format!("  {}:", d.name));
            for item in &d.memory_items {
                let related_marker = if item.directly_related { " *" } else { "" };
                lines.push(format!(
                    "    [{}] {} [{}]{}",
                    item.type_, item.text, item.confidence, related_marker
                ));
            }
        }
    }

    // Warnings
    let all_warnings: Vec<&DomainWarning> =
        result.domains.iter().flat_map(|d| &d.warnings).collect();
    if !all_warnings.is_empty() {
        lines.push(String::new());
        lines.push("Warnings".to_string());
        for w in &all_warnings {
            let detail = format_warning_detail(w);
            lines.push(format!("  \u{26a0} {detail}"));
        }
    }

    // Suggested actions
    if !result.suggested_actions.is_empty() {
        lines.push(String::new());
        lines.push("Suggested actions".to_string());
        for action in &result.suggested_actions {
            lines.push(format!("  \u{2192} {action}"));
        }
    }

    // Unresolved files
    if !result.unresolved_files.is_empty() {
        lines.push(String::new());
        lines.push("Unresolved files".to_string());
        for f in &result.unresolved_files {
            lines.push(format!("  {f}"));
        }
    }

    lines.join("\n")
}

/// Format a single warning into a human-readable detail string.
pub(super) fn format_warning_detail(w: &DomainWarning) -> String {
    match w.kind.as_str() {
        "stale" => {
            let days = w.days.unwrap_or(0);
            format!("{} is stale ({days} days)", w.note)
        }
        "low_confidence" => {
            format!("{} has low confidence", w.note)
        }
        "no_memory" => {
            format!("No structured memory for {}", w.note)
        }
        "no_note" => "No wiki note found for domain".to_string(),
        other => other.to_string(),
    }
}

pub(super) fn format_json(result: &CheckDiffResult) -> Result<String> {
    serde_json::to_string_pretty(result).map_err(|e| anyhow::anyhow!("JSON serialization: {e}"))
}

/// Format the check-diff result as a GitHub PR comment.
/// Returns `None` if sensitivity is `Low` (no comment needed).
pub fn format_pr_comment(result: &CheckDiffResult) -> Option<String> {
    if result.sensitivity == Sensitivity::Low {
        return None;
    }

    let mut lines = Vec::new();

    // Header with unique marker for idempotent updates
    lines.push("## \u{1f9e0} project-wiki \u{2014} Memory Check".to_string());
    lines.push("<!-- project-wiki-memory-check -->".to_string());
    lines.push(String::new());
    lines.push(format!("**Sensitivity: {}**", result.sensitivity));

    // Domains touched
    if !result.domains.is_empty() {
        lines.push(String::new());
        lines.push("### Domains touched".to_string());
        for d in &result.domains {
            lines.push(format!(
                "- **{}** ({} file(s), {} memory item(s))",
                d.name,
                d.files.len(),
                d.memory_items.len()
            ));
        }
    }

    // Priority memory table
    let has_items = result.domains.iter().any(|d| !d.memory_items.is_empty());
    if has_items {
        lines.push(String::new());
        lines.push("### Priority memory".to_string());
        lines.push("| Type | Item | Confidence |".to_string());
        lines.push("|------|------|------------|".to_string());
        for d in &result.domains {
            for item in &d.memory_items {
                let escaped_text = item.text.replace('|', "\\|");
                lines.push(format!(
                    "| {} | {} | {} |",
                    item.type_, escaped_text, item.confidence
                ));
            }
        }
    }

    // Warnings
    let all_warnings: Vec<&DomainWarning> =
        result.domains.iter().flat_map(|d| &d.warnings).collect();
    if !all_warnings.is_empty() {
        lines.push(String::new());
        lines.push("### Warnings".to_string());
        for w in &all_warnings {
            let detail = format_warning_detail(w);
            lines.push(format!("- \u{26a0}\u{fe0f} {detail}"));
        }
    }

    // Suggested actions
    if !result.suggested_actions.is_empty() {
        lines.push(String::new());
        lines.push("### Suggested actions".to_string());
        for action in &result.suggested_actions {
            lines.push(format!("- {action}"));
        }
    }

    Some(lines.join("\n"))
}
