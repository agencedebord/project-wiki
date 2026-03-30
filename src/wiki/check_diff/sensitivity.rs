use super::{CheckDiffResult, Sensitivity};

pub(super) fn calculate_sensitivity(result: &CheckDiffResult) -> Sensitivity {
    if result.domains.is_empty() && !result.unresolved_files.is_empty() {
        return Sensitivity::Low;
    }

    for domain in &result.domains {
        // High: any exception or decision
        for item in &domain.memory_items {
            if item.type_ == "exception" || item.type_ == "decision" {
                return Sensitivity::High;
            }
        }
        // High: stale note or low confidence note
        for w in &domain.warnings {
            if w.kind == "stale" || w.kind == "low_confidence" {
                return Sensitivity::High;
            }
        }
    }

    // Medium: domains with memory items but no high signal
    let has_items = result.domains.iter().any(|d| !d.memory_items.is_empty());

    if has_items {
        return Sensitivity::Medium;
    }

    Sensitivity::Low
}

pub(super) fn generate_suggestions(result: &CheckDiffResult) -> Vec<String> {
    if result.sensitivity == Sensitivity::Low {
        return Vec::new();
    }

    let mut suggestions = Vec::new();
    let max_suggestions = 3;

    for domain in &result.domains {
        if suggestions.len() >= max_suggestions {
            break;
        }

        // Stale note suggestion
        for w in &domain.warnings {
            if suggestions.len() >= max_suggestions {
                break;
            }
            if w.kind == "stale" {
                suggestions.push(format!("Relire {}", w.note));
            }
        }

        // Exception/decision suggestions
        for item in &domain.memory_items {
            if suggestions.len() >= max_suggestions {
                break;
            }
            let short_text = if item.text.chars().count() > 50 {
                let truncated: String = item.text.chars().take(50).collect();
                format!("{}...", truncated)
            } else {
                item.text.clone()
            };

            if item.type_ == "exception" {
                suggestions.push(format!(
                    "Verifier si l'exception '{}' reste valide",
                    short_text
                ));
            } else if item.type_ == "decision" {
                suggestions.push(format!(
                    "Verifier si la decision '{}' reste valide",
                    short_text
                ));
            }
        }
    }

    // Medium fallback suggestion
    if suggestions.is_empty() && result.sensitivity == Sensitivity::Medium {
        if let Some(d) = result.domains.first() {
            suggestions.push(format!(
                "Consulter la memoire du domaine {} si le changement est significatif",
                d.name
            ));
        }
    }

    suggestions
}
