use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use super::{Candidate, CandidateType, ProvenanceEntry};
use crate::init::scan::DomainInfo;

// ── Regex patterns for candidate detection ─────────────────────────

/// Patterns in file paths or function names suggesting an exception.
/// Uses word boundaries around `migration` to avoid matching standard framework
/// commands like `squashmigrations`, `makemigrations`, `showmigrations`.
static RE_EXCEPTION_NAMING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(legacy|compat|override|workaround|\bmigration\b|deprecated|old_|_old|_v1\b|v1_)",
    )
    .unwrap()
});

/// Patterns in comments suggesting a deliberate decision.
pub(super) static RE_DECISION_COMMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(decision|chosen|we\s+decided|deliberately|intentionally|on\s+purpose|trade-?off|ADR)",
    )
    .unwrap()
});

/// Patterns for util/helper files to exclude.
static RE_UTILS_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(utils?|helpers?|constants?|config|setup|index)\.(ts|js|py|rs|go)$").unwrap()
});

// ── Constants ──────────────────────────────────────────────────────

const MIN_FILE_LINES: usize = 50;

// ── Heuristic: exception detection ─────────────────────────────────

pub(super) fn detect_exception_candidates(domain: &DomainInfo, candidates: &mut Vec<Candidate>) {
    for file in &domain.files {
        if is_excluded_path(file) {
            continue;
        }

        if RE_EXCEPTION_NAMING.is_match(file) {
            // Check the file has some substance
            let line_count = count_file_lines(file);
            if line_count < MIN_FILE_LINES {
                continue;
            }

            let filename = std::path::Path::new(file)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            let mut provenance = vec![ProvenanceEntry {
                kind: "file".to_string(),
                ref_: file.clone(),
            }];

            // Check for associated test
            if let Some(test_file) = find_test_for_file(file, &domain.test_files) {
                provenance.push(ProvenanceEntry {
                    kind: "test".to_string(),
                    ref_: test_file,
                });
            }

            candidates.push(Candidate {
                id: String::new(), // assigned later
                domain: domain.name.clone(),
                type_: CandidateType::Exception,
                text: format!(
                    "{} seems to contain a special case or legacy behavior",
                    filename
                ),
                rationale: format!(
                    "File path '{}' contains exception naming pattern (legacy/compat/override)",
                    file
                ),
                provenance,
                target_note: format!(".wiki/domains/{}/_overview.md", domain.name),
            });
        }
    }
}

// ── Heuristic: decision detection ──────────────────────────────────

pub(super) fn detect_decision_candidates(domain: &DomainInfo, candidates: &mut Vec<Candidate>) {
    for comment in &domain.comments {
        let text = &comment.text;

        if RE_DECISION_COMMENT.is_match(text) && !is_generic_text(text) {
            candidates.push(Candidate {
                id: String::new(),
                domain: domain.name.clone(),
                type_: CandidateType::Decision,
                text: truncate_text(text, 120),
                rationale: "Comment contains decision pattern (decided/deliberately/intentionally)"
                    .to_string(),
                provenance: vec![ProvenanceEntry {
                    kind: "comment".to_string(),
                    ref_: comment.to_string(),
                }],
                target_note: format!(".wiki/domains/{}/_overview.md", domain.name),
            });
        }
    }
}

// ── Heuristic: business_rule detection ─────────────────────────────

pub(super) fn detect_business_rule_candidates(
    domain: &DomainInfo,
    candidates: &mut Vec<Candidate>,
) {
    // Look for files with tests AND meaningful TODO/HACK/NOTE comments
    let files_with_tests: HashSet<&str> = domain
        .test_files
        .iter()
        .filter_map(|tf| infer_source_for_test(tf))
        .collect();

    for file in &domain.files {
        if is_excluded_path(file) {
            continue;
        }

        let line_count = count_file_lines(file);
        if line_count < MIN_FILE_LINES {
            continue;
        }

        // Check if this file has an associated test
        let file_base = std::path::Path::new(file)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        let has_test = files_with_tests.iter().any(|src| src.contains(&file_base));

        if !has_test {
            continue;
        }

        // Look for relevant comments in this domain for this file
        let relevant_comments: Vec<_> = domain
            .comments
            .iter()
            .filter(|c| !RE_DECISION_COMMENT.is_match(&c.text) && !is_generic_text(&c.text))
            .collect();

        if relevant_comments.is_empty() {
            continue;
        }

        let first_comment = &relevant_comments[0];
        let comment_text = &first_comment.text;

        let mut provenance = vec![ProvenanceEntry {
            kind: "file".to_string(),
            ref_: file.clone(),
        }];

        if let Some(test_file) = find_test_for_file(file, &domain.test_files) {
            provenance.push(ProvenanceEntry {
                kind: "test".to_string(),
                ref_: test_file,
            });
        }

        provenance.push(ProvenanceEntry {
            kind: "comment".to_string(),
            ref_: first_comment.to_string(),
        });

        candidates.push(Candidate {
            id: String::new(),
            domain: domain.name.clone(),
            type_: CandidateType::BusinessRule,
            text: truncate_text(comment_text, 120),
            rationale: format!(
                "File '{}' has tests and contains a TODO/HACK/NOTE comment",
                file
            ),
            provenance,
            target_note: format!(".wiki/domains/{}/_overview.md", domain.name),
        });
    }
}

// ── Helpers ────────────────────────────────────────────────────────

pub(super) fn is_excluded_path(path: &str) -> bool {
    // Exclude test-only files
    if path.contains("/test/")
        || path.contains("/tests/")
        || path.contains("/__tests__/")
        || path.contains(".test.")
        || path.contains(".spec.")
        || path.contains("_test.")
    {
        return true;
    }

    // Exclude utility files
    RE_UTILS_PATH.is_match(path)
}

pub(super) fn is_generic_text(text: &str) -> bool {
    let generic_patterns = [
        "this module",
        "this file",
        "this class",
        "this function",
        "this service",
        "handles",
        "manages",
        "processes",
        "implements",
    ];
    let lower = text.to_lowercase();
    // Too short to be meaningful
    if text.len() < 15 {
        return true;
    }
    generic_patterns
        .iter()
        .any(|p| lower.starts_with(p) && lower.len() < 60)
}

pub(super) fn count_file_lines(path: &str) -> usize {
    std::fs::read_to_string(path)
        .map(|c| c.lines().count())
        .unwrap_or(0)
}

pub(super) fn find_test_for_file(source_file: &str, test_files: &[String]) -> Option<String> {
    let stem = std::path::Path::new(source_file)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())?;

    test_files.iter().find(|tf| tf.contains(&stem)).cloned()
}

pub(super) fn infer_source_for_test(test_file: &str) -> Option<&str> {
    let stem = std::path::Path::new(test_file).file_stem()?;
    let name = stem.to_str()?;
    // Strip common test suffixes: file.test.ts -> file, file_test.rs -> file
    let base = name
        .strip_suffix(".test")
        .or_else(|| name.strip_suffix(".spec"))
        .or_else(|| name.strip_suffix("_test"))
        .unwrap_or(name);
    Some(base)
}

pub(super) fn truncate_text(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        // Find a valid char boundary for truncation (MSRV-compatible fallback)
        let mut boundary = max_len - 3;
        while boundary > 0 && !text.is_char_boundary(boundary) {
            boundary -= 1;
        }
        format!("{}...", &text[..boundary])
    }
}
