use std::collections::HashMap;

use crate::wiki::note::{Confidence, MemoryItemStatus, WikiNote};

use super::MAX_CONTEXT_LEN;
use super::MAX_MEMORY_ITEMS;
use super::prioritize::prioritize_memory_items;

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
        "[codefidence] Domain: {} (confidence: {}, updated: {})",
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
        "[codefidence] Domain: {} (confidence: {}, updated: {})",
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

pub(super) fn truncate_output(result: String) -> String {
    if result.len() > MAX_CONTEXT_LEN {
        let boundary = result.floor_char_boundary(MAX_CONTEXT_LEN - 20);
        let mut truncated = result[..boundary].to_string();
        truncated.push_str("\n[... truncated]");
        truncated
    } else {
        result
    }
}

/// Extract markdown sections (## heading -> body) from content.
pub(super) fn extract_sections(content: &str) -> HashMap<String, String> {
    let mut sections = HashMap::new();
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
pub(super) fn extract_bullet_points(body: &str, max: usize) -> Vec<String> {
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
