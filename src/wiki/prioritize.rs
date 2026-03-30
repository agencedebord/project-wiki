//! Shared prioritization logic for memory items.
//!
//! Used by both `context` and `check_diff` to rank items consistently.

use super::note::{Confidence, MemoryItem, MemoryItemType};

/// Priority rank for memory item types.
/// Lower = higher priority: exception > decision > business_rule.
pub fn type_priority(t: &MemoryItemType) -> u8 {
    match t {
        MemoryItemType::Exception => 0,
        MemoryItemType::Decision => 1,
        MemoryItemType::BusinessRule => 2,
    }
}

/// Priority rank for confidence levels.
/// Lower = higher priority: confirmed/verified > seen-in-code > inferred > needs-validation.
pub fn confidence_priority(c: &Confidence) -> u8 {
    match c {
        Confidence::Confirmed | Confidence::Verified => 0,
        Confidence::SeenInCode => 1,
        Confidence::Inferred => 2,
        Confidence::NeedsValidation => 3,
    }
}

/// Check whether a memory item's `related_files` contain the given path.
///
/// Used by `context` to boost items matching the queried file.
pub fn has_related_file(item: &MemoryItem, file_path: &str) -> bool {
    item.related_files.iter().any(|f| f == file_path)
}

/// Check whether a memory item's `related_files` overlap with any of the given paths.
///
/// Used by `check_diff` to boost items matching modified files in a diff.
pub fn has_any_related_file(item: &MemoryItem, files: &[String]) -> bool {
    files.iter().any(|f| has_related_file(item, f))
}
