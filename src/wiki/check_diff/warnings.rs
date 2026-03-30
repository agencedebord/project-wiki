use crate::wiki::config;
use crate::wiki::note::{Confidence, MemoryItemStatus, WikiNote};

use super::DomainWarning;

pub(super) fn build_warnings(
    note: &WikiNote,
    domain: &str,
    note_path: &str,
    wiki_config: &config::WikiConfig,
) -> Vec<DomainWarning> {
    let mut warnings = Vec::new();

    // Stale check
    if let Some(updated) = note.last_updated {
        let today = chrono::Utc::now().date_naive();
        let days_old = (today - updated).num_days();
        if days_old > wiki_config.staleness_days as i64 {
            warnings.push(DomainWarning {
                kind: "stale".to_string(),
                note: note_path.to_string(),
                days: Some(days_old),
            });
        }
    }

    // Low confidence note
    if matches!(
        note.confidence,
        Confidence::Inferred | Confidence::NeedsValidation
    ) {
        warnings.push(DomainWarning {
            kind: "low_confidence".to_string(),
            note: note_path.to_string(),
            days: None,
        });
    }

    // No memory items
    if note.memory_items.is_empty() {
        warnings.push(DomainWarning {
            kind: "no_memory".to_string(),
            note: note_path.to_string(),
            days: None,
        });
    } else {
        // Low confidence items
        let low_count = note
            .memory_items
            .iter()
            .filter(|i| i.status != MemoryItemStatus::Deprecated)
            .filter(|i| {
                matches!(
                    i.confidence,
                    Confidence::Inferred | Confidence::NeedsValidation
                )
            })
            .count();
        if low_count > 0 {
            warnings.push(DomainWarning {
                kind: format!("{low_count} item(s) with low confidence in {domain}"),
                note: note_path.to_string(),
                days: None,
            });
        }
    }

    warnings
}
