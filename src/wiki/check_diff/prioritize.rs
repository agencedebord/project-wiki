use crate::wiki::note::{MemoryItem, MemoryItemStatus};
use crate::wiki::prioritize::{confidence_priority, has_any_related_file, type_priority};

use super::DomainItemOutput;

/// Filter, prioritize, and convert memory items to output format.
pub(super) fn prioritize_and_format_items(
    items: &[MemoryItem],
    modified_files: &[String],
    max_items: usize,
    note_path: &str,
) -> Vec<DomainItemOutput> {
    let active: Vec<&MemoryItem> = items
        .iter()
        .filter(|i| i.status != MemoryItemStatus::Deprecated)
        .collect();

    let mut scored: Vec<(&MemoryItem, (u8, u8, bool))> = active
        .into_iter()
        .map(|item| {
            let related = has_any_related_file(item, modified_files);
            (
                item,
                (
                    type_priority(&item.type_),
                    confidence_priority(&item.confidence),
                    related,
                ),
            )
        })
        .collect();

    // Sort: type priority asc, confidence priority asc, related first (false > true reversed)
    scored.sort_by(|a, b| {
        let type_cmp = a.1.0.cmp(&b.1.0);
        if type_cmp != std::cmp::Ordering::Equal {
            return type_cmp;
        }
        let conf_cmp = a.1.1.cmp(&b.1.1);
        if conf_cmp != std::cmp::Ordering::Equal {
            return conf_cmp;
        }
        // Related files first (true = 0, false = 1 for sorting)
        b.1.2.cmp(&a.1.2)
    });

    scored
        .into_iter()
        .take(max_items)
        .map(|(item, (_, _, related))| DomainItemOutput {
            id: item.id.clone(),
            type_: item.type_.to_string(),
            text: item.text.clone(),
            confidence: item.confidence.to_string(),
            directly_related: related,
            source_note: note_path.to_string(),
        })
        .collect()
}
