use std::io::Read as _;
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::wiki::common::{ensure_wiki_exists, find_wiki_root};
use crate::wiki::config;
use crate::wiki::file_index;
use crate::wiki::note::{Confidence, WikiNote};

// ─── Types ───

#[derive(Debug)]
#[allow(dead_code)] // Used for classification; will be leveraged in future hook modes
pub enum DriftKind {
    Stale,
    LowConfidence,
    RelatedFileModified,
    /// A memory item's source line is within a modified hunk.
    MemoryItemImpacted,
}

/// A parsed hunk range from a unified diff.
#[derive(Debug, Clone, PartialEq)]
pub struct HunkRange {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
}

/// How close a source line must be to a hunk boundary to be considered impacted.
const HUNK_PROXIMITY_TOLERANCE: u32 = 5;

#[derive(Debug)]
pub struct DriftWarning {
    pub domain: String,
    #[allow(dead_code)] // Used for classification; will be leveraged in future hook modes
    pub kind: DriftKind,
    pub message: String,
}

// ─── Hook JSON types ───

#[derive(Deserialize)]
struct HookInput {
    tool_input: serde_json::Value,
}

#[derive(Serialize)]
struct HookOutput {
    #[serde(rename = "additionalContext")]
    additional_context: String,
}

// ─── Public API ───

/// CLI entry point: check drift for a file and print warnings.
pub fn run(file: &str) -> Result<()> {
    let wiki_dir = find_wiki_root()?;

    let project_root = wiki_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Wiki directory has no parent"))?;

    let warnings = detect(file, &wiki_dir, project_root)?;

    if warnings.is_empty() {
        eprintln!("[project-wiki] No drift detected for: {}", file);
    } else {
        eprintln!("{}", format_warnings(&warnings));
    }

    Ok(())
}

/// Hook entry point: read JSON from stdin, write JSON to stdout.
pub fn run_from_stdin() -> Result<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;

    let hook_input: HookInput = serde_json::from_str(&input).unwrap_or(HookInput {
        tool_input: serde_json::Value::Null,
    });

    let file_path = hook_input
        .tool_input
        .get("file_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if file_path.is_empty() {
        return Ok(());
    }

    let wiki_dir = match find_wiki_root() {
        Ok(dir) => dir,
        Err(_) => return Ok(()),
    };

    let project_root = match wiki_dir.parent() {
        Some(root) => root,
        None => return Ok(()),
    };

    let warnings = detect(file_path, &wiki_dir, project_root)?;

    if !warnings.is_empty() {
        let output = HookOutput {
            additional_context: format_warnings(&warnings),
        };
        println!("{}", serde_json::to_string(&output)?);
    }

    Ok(())
}

/// Core logic: detect drift for a file path.
pub fn detect(file_path: &str, wiki_dir: &Path, project_root: &Path) -> Result<Vec<DriftWarning>> {
    ensure_wiki_exists(wiki_dir)?;

    let index = file_index::load_or_rebuild(wiki_dir)?;

    let domain = match file_index::resolve_domain(&index, file_path, project_root) {
        Some(d) => d,
        None => return Ok(Vec::new()),
    };

    let overview_path = wiki_dir.join("domains").join(&domain).join("_overview.md");
    if !overview_path.exists() {
        return Ok(Vec::new());
    }

    let note = WikiNote::parse(&overview_path)?;
    let wiki_config = config::load(wiki_dir);

    let mut warnings = Vec::new();

    // Check: is the note stale?
    if let Some(updated) = note.last_updated {
        let today = chrono::Utc::now().date_naive();
        let days_old = (today - updated).num_days();
        if days_old > wiki_config.staleness_days as i64 {
            warnings.push(DriftWarning {
                domain: domain.clone(),
                kind: DriftKind::Stale,
                message: format!(
                    "Wiki note for \"{}\" was last updated {} days ago (threshold: {}).",
                    domain, days_old, wiki_config.staleness_days
                ),
            });
        }
    }

    // Check: low confidence?
    if note.confidence == Confidence::NeedsValidation || note.confidence == Confidence::Inferred {
        warnings.push(DriftWarning {
            domain: domain.clone(),
            kind: DriftKind::LowConfidence,
            message: format!(
                "Wiki note for \"{}\" has low confidence ({}). Consider verifying and confirming it.",
                domain, note.confidence
            ),
        });
    }

    // Check: is this file tracked by the domain?
    let normalized = file_path.replace('\\', "/");
    if note
        .related_files
        .iter()
        .any(|f| f.replace('\\', "/") == normalized)
    {
        warnings.push(DriftWarning {
            domain: domain.clone(),
            kind: DriftKind::RelatedFileModified,
            message: format!(
                "You modified \"{}\", which is tracked by wiki domain \"{}\". Review the wiki note to check if it needs updating.",
                file_path, domain
            ),
        });
    }

    // Diff-aware: check memory items against actual hunks (single git call)
    let has_active_items = note
        .memory_items
        .iter()
        .any(|i| i.status != crate::wiki::note::MemoryItemStatus::Deprecated);

    if has_active_items {
        if let Some(raw_diff) = get_raw_diff(file_path) {
            let hunks = parse_hunk_headers(&raw_diff);
            if !hunks.is_empty() {
                let diff_idents = extract_diff_identifiers(&raw_diff);
                let item_warnings =
                    check_items_against_diff(&domain, &note, file_path, &hunks, &diff_idents);
                warnings.extend(item_warnings);
            }
        }
    }

    Ok(warnings)
}

// ─── Diff-aware analysis (task 022) ───

static HUNK_HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@").unwrap());

/// Identifier-like tokens for heuristic matching.
static IDENT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-zA-Z_]\w{2,}").unwrap());

/// Get the raw unified diff for a file (single git invocation).
fn get_raw_diff(file_path: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["diff", "--unified=0", "--", file_path])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Parse hunk headers from unified diff output.
fn parse_hunk_headers(diff_output: &str) -> Vec<HunkRange> {
    let mut hunks = Vec::new();

    for line in diff_output.lines() {
        if let Some(cap) = HUNK_HEADER_RE.captures(line) {
            let old_start: u32 = cap[1].parse().unwrap_or(0);
            let old_count: u32 = cap
                .get(2)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(1);
            let new_start: u32 = cap[3].parse().unwrap_or(0);
            let new_count: u32 = cap
                .get(4)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(1);

            hunks.push(HunkRange {
                old_start,
                old_count,
                new_start,
                new_count,
            });
        }
    }

    hunks
}

/// Check if a source line falls within (or near) a hunk range.
/// Hunk covers old lines [old_start, old_start + old_count - 1].
/// With tolerance, we check [old_start - tolerance, old_start + old_count - 1 + tolerance].
fn is_line_near_hunk(line: u32, hunk: &HunkRange, tolerance: u32) -> bool {
    let start = hunk.old_start.saturating_sub(tolerance);
    // old_count=0 means pure insertion at old_start; treat as a single-point range
    let last_line = if hunk.old_count == 0 {
        hunk.old_start
    } else {
        hunk.old_start.saturating_add(hunk.old_count - 1)
    };
    let end = last_line.saturating_add(tolerance);
    line >= start && line <= end
}

/// Common keywords to exclude from identifier heuristic matching.
const STOPWORDS: &[&str] = &[
    "let", "var", "const", "for", "while", "if", "else", "return", "function", "import", "export",
    "from", "this", "self", "true", "false", "null", "none", "some", "string", "number", "boolean",
    "type", "enum", "struct", "class", "pub", "mod", "use", "new", "match", "async", "await",
    "try", "catch", "throw", "error", "result", "value", "index", "data", "item", "list", "map",
    "set", "get", "put", "delete", "post", "with", "that", "the",
];

/// Extract identifiers from diff output (added/removed lines only).
/// Filters common language keywords and requires 4+ chars to reduce noise.
fn extract_diff_identifiers(diff_output: &str) -> Vec<String> {
    let mut identifiers = std::collections::HashSet::new();

    for line in diff_output.lines() {
        // Only look at added/removed lines (not headers)
        if (line.starts_with('+') || line.starts_with('-'))
            && !line.starts_with("+++")
            && !line.starts_with("---")
        {
            for cap in IDENT_RE.find_iter(line) {
                let ident = cap.as_str().to_lowercase();
                if ident.len() >= 4 && !STOPWORDS.contains(&ident.as_str()) {
                    identifiers.insert(ident);
                }
            }
        }
    }

    identifiers.into_iter().collect()
}

/// Check memory items against diff hunks and return drift warnings.
fn check_items_against_diff(
    domain: &str,
    note: &WikiNote,
    file_path: &str,
    hunks: &[HunkRange],
    diff_identifiers: &[String],
) -> Vec<DriftWarning> {
    let mut warnings = Vec::new();
    let normalized = file_path.replace('\\', "/");

    for item in &note.memory_items {
        // Skip deprecated items
        if item.status == crate::wiki::note::MemoryItemStatus::Deprecated {
            continue;
        }

        let mut impacted = false;
        let mut reason = String::new();

        // Check 1: source line proximity
        for source in &item.sources {
            let source_ref = source.ref_.replace('\\', "/");
            if source_ref != normalized {
                continue;
            }

            if let Some(line) = source.line {
                for hunk in hunks {
                    if is_line_near_hunk(line, hunk, HUNK_PROXIMITY_TOLERANCE) {
                        impacted = true;
                        reason = format!(
                            "source file {} modified near line {} (hunk {}-{})",
                            file_path,
                            line,
                            hunk.old_start,
                            hunk.old_start.saturating_add(hunk.old_count)
                        );
                        break;
                    }
                }
            }

            if impacted {
                break;
            }
        }

        // Check 2: heuristic — identifier from diff appears as whole word in item text
        if !impacted && !diff_identifiers.is_empty() {
            let item_words: Vec<String> = IDENT_RE
                .find_iter(&item.text.to_lowercase())
                .map(|m| m.as_str().to_string())
                .collect();
            for ident in diff_identifiers {
                if item_words.iter().any(|w| w == ident) {
                    impacted = true;
                    reason = format!("identifier \"{}\" from diff matches item text", ident);
                    break;
                }
            }
        }

        if impacted {
            warnings.push(DriftWarning {
                domain: domain.to_string(),
                kind: DriftKind::MemoryItemImpacted,
                message: format!(
                    "{} potentially impacted\n    [{}] {}\n    Reason: {}\n    Action: verify if this {} still holds",
                    item.id, item.type_, item.text, reason, item.type_
                ),
            });
        }
    }

    warnings
}

/// Format warnings into a human-readable string.
pub fn format_warnings(warnings: &[DriftWarning]) -> String {
    let mut lines = vec!["[project-wiki] Wiki drift detected:".to_string()];

    for w in warnings {
        lines.push(format!("  - {}", w.message));
    }

    if let Some(first) = warnings.first() {
        lines.push(format!("  Run: project-wiki consult {}", first.domain));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_wiki_note(
        dir: &TempDir,
        domain: &str,
        confidence: &str,
        last_updated: &str,
        related_files: &[&str],
    ) {
        let domain_dir = dir.path().join(".wiki/domains").join(domain);
        fs::create_dir_all(&domain_dir).unwrap();

        let files_yaml = if related_files.is_empty() {
            "related_files: []".to_string()
        } else {
            let items: String = related_files
                .iter()
                .map(|f| format!("  - {}", f))
                .collect::<Vec<_>>()
                .join("\n");
            format!("related_files:\n{}", items)
        };

        let content = format!(
            "---\ndomain: {}\nconfidence: {}\nlast_updated: \"{}\"\n{}\n---\n\n# {}\n\n## Key behaviors\n- Does stuff\n",
            domain, confidence, last_updated, files_yaml, domain
        );

        fs::write(domain_dir.join("_overview.md"), content).unwrap();
    }

    #[test]
    fn detect_stale_note() {
        let dir = TempDir::new().unwrap();
        create_wiki_note(
            &dir,
            "billing",
            "confirmed",
            "2025-01-01",
            &["src/billing/invoice.ts"],
        );

        let wiki_dir = dir.path().join(".wiki");
        let warnings = detect("src/billing/invoice.ts", &wiki_dir, dir.path()).unwrap();

        assert!(warnings.iter().any(|w| matches!(w.kind, DriftKind::Stale)));
    }

    #[test]
    fn detect_low_confidence() {
        let dir = TempDir::new().unwrap();
        create_wiki_note(
            &dir,
            "billing",
            "inferred",
            "2026-03-28",
            &["src/billing/invoice.ts"],
        );

        let wiki_dir = dir.path().join(".wiki");
        let warnings = detect("src/billing/invoice.ts", &wiki_dir, dir.path()).unwrap();

        assert!(
            warnings
                .iter()
                .any(|w| matches!(w.kind, DriftKind::LowConfidence))
        );
    }

    #[test]
    fn detect_related_file_modified() {
        let dir = TempDir::new().unwrap();
        create_wiki_note(
            &dir,
            "billing",
            "confirmed",
            "2026-03-28",
            &["src/billing/invoice.ts"],
        );

        let wiki_dir = dir.path().join(".wiki");
        let warnings = detect("src/billing/invoice.ts", &wiki_dir, dir.path()).unwrap();

        assert!(
            warnings
                .iter()
                .any(|w| matches!(w.kind, DriftKind::RelatedFileModified))
        );
    }

    #[test]
    fn detect_no_warnings_for_fresh_confirmed_note() {
        let dir = TempDir::new().unwrap();
        create_wiki_note(&dir, "billing", "confirmed", "2026-03-28", &[]);

        let wiki_dir = dir.path().join(".wiki");
        // File not in related_files, note is fresh and confirmed
        let warnings = detect("src/billing/invoice.ts", &wiki_dir, dir.path()).unwrap();

        assert!(
            warnings.is_empty(),
            "Expected no warnings, got: {:?}",
            warnings.iter().map(|w| &w.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn detect_no_domain_returns_empty() {
        let dir = TempDir::new().unwrap();
        let wiki_dir = dir.path().join(".wiki");
        fs::create_dir_all(wiki_dir.join("domains")).unwrap();

        let warnings = detect("README.md", &wiki_dir, dir.path()).unwrap();
        assert!(warnings.is_empty());
    }

    #[test]
    fn format_warnings_includes_all_messages() {
        let warnings = vec![
            DriftWarning {
                domain: "billing".to_string(),
                kind: DriftKind::Stale,
                message: "Note is stale".to_string(),
            },
            DriftWarning {
                domain: "billing".to_string(),
                kind: DriftKind::LowConfidence,
                message: "Low confidence".to_string(),
            },
        ];

        let output = format_warnings(&warnings);
        assert!(output.contains("Note is stale"));
        assert!(output.contains("Low confidence"));
        assert!(output.contains("project-wiki consult billing"));
    }

    // ── Diff-aware tests (task 022) ──

    #[test]
    fn parse_hunk_headers_standard() {
        let diff = "\
diff --git a/src/billing/pricing.ts b/src/billing/pricing.ts
index abc1234..def5678 100644
--- a/src/billing/pricing.ts
+++ b/src/billing/pricing.ts
@@ -38,13 +38,15 @@ function calculatePrice() {
+  const tax = 0.2;
@@ -100,3 +102,5 @@ function applyDiscount() {
+  // new discount logic
";
        let hunks = parse_hunk_headers(diff);
        assert_eq!(hunks.len(), 2);

        assert_eq!(hunks[0].old_start, 38);
        assert_eq!(hunks[0].old_count, 13);
        assert_eq!(hunks[0].new_start, 38);
        assert_eq!(hunks[0].new_count, 15);

        assert_eq!(hunks[1].old_start, 100);
        assert_eq!(hunks[1].old_count, 3);
        assert_eq!(hunks[1].new_start, 102);
        assert_eq!(hunks[1].new_count, 5);
    }

    #[test]
    fn parse_hunk_headers_single_line_change() {
        // When count is omitted, it defaults to 1
        let diff = "@@ -42 +42 @@ function legacy()\n";
        let hunks = parse_hunk_headers(diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_start, 42);
        assert_eq!(hunks[0].old_count, 1);
    }

    #[test]
    fn parse_hunk_headers_empty_diff() {
        let hunks = parse_hunk_headers("");
        assert!(hunks.is_empty());
    }

    #[test]
    fn line_proximity_within_hunk() {
        let hunk = HunkRange {
            old_start: 38,
            old_count: 13,
            new_start: 38,
            new_count: 15,
        };

        // Line 42 is within 38-51 → impacted
        assert!(is_line_near_hunk(42, &hunk, HUNK_PROXIMITY_TOLERANCE));
        // Line 40 is within range → impacted
        assert!(is_line_near_hunk(40, &hunk, HUNK_PROXIMITY_TOLERANCE));
        // Line 35 is within tolerance (38 - 5 = 33) → impacted
        assert!(is_line_near_hunk(35, &hunk, HUNK_PROXIMITY_TOLERANCE));
    }

    #[test]
    fn line_proximity_outside_hunk() {
        let hunk = HunkRange {
            old_start: 38,
            old_count: 13,
            new_start: 38,
            new_count: 15,
        };

        // Line 200 is far away → not impacted
        assert!(!is_line_near_hunk(200, &hunk, HUNK_PROXIMITY_TOLERANCE));
        // Line 10 is far below → not impacted
        assert!(!is_line_near_hunk(10, &hunk, HUNK_PROXIMITY_TOLERANCE));
    }

    #[test]
    fn check_items_line_proximity_triggers_warning() {
        use crate::wiki::note::{
            Confidence, MemoryItem, MemoryItemSource, MemoryItemStatus, MemoryItemType,
        };

        let note = WikiNote {
            path: String::new(),
            title: String::new(),
            domain: "billing".to_string(),
            confidence: Confidence::Confirmed,
            last_updated: None,
            related_files: Vec::new(),
            content: String::new(),
            deprecated: false,
            memory_items: vec![MemoryItem {
                id: "billing-001".to_string(),
                type_: MemoryItemType::Exception,
                text: "Legacy pricing uses old tax rate".to_string(),
                confidence: Confidence::Confirmed,
                related_files: vec!["src/billing/pricing.ts".to_string()],
                sources: vec![MemoryItemSource {
                    kind: "file".to_string(),
                    ref_: "src/billing/pricing.ts".to_string(),
                    line: Some(42),
                }],
                status: MemoryItemStatus::Active,
                last_reviewed: None,
            }],
        };

        let hunks = vec![HunkRange {
            old_start: 38,
            old_count: 13,
            new_start: 38,
            new_count: 15,
        }];

        let warnings =
            check_items_against_diff("billing", &note, "src/billing/pricing.ts", &hunks, &[]);

        assert_eq!(warnings.len(), 1);
        assert!(matches!(warnings[0].kind, DriftKind::MemoryItemImpacted));
        assert!(warnings[0].message.contains("billing-001"));
        assert!(warnings[0].message.contains("line 42"));
    }

    #[test]
    fn check_items_far_hunk_no_warning() {
        use crate::wiki::note::{
            Confidence, MemoryItem, MemoryItemSource, MemoryItemStatus, MemoryItemType,
        };

        let note = WikiNote {
            path: String::new(),
            title: String::new(),
            domain: "billing".to_string(),
            confidence: Confidence::Confirmed,
            last_updated: None,
            related_files: Vec::new(),
            content: String::new(),
            deprecated: false,
            memory_items: vec![MemoryItem {
                id: "billing-001".to_string(),
                type_: MemoryItemType::Exception,
                text: "Legacy pricing uses old tax rate".to_string(),
                confidence: Confidence::Confirmed,
                related_files: vec!["src/billing/pricing.ts".to_string()],
                sources: vec![MemoryItemSource {
                    kind: "file".to_string(),
                    ref_: "src/billing/pricing.ts".to_string(),
                    line: Some(42),
                }],
                status: MemoryItemStatus::Active,
                last_reviewed: None,
            }],
        };

        // Hunk at lines 200-210, far from line 42
        let hunks = vec![HunkRange {
            old_start: 200,
            old_count: 10,
            new_start: 200,
            new_count: 12,
        }];

        let warnings =
            check_items_against_diff("billing", &note, "src/billing/pricing.ts", &hunks, &[]);

        assert!(warnings.is_empty(), "No warning expected for distant hunk");
    }

    #[test]
    fn check_items_no_line_info_fallback() {
        use crate::wiki::note::{
            Confidence, MemoryItem, MemoryItemSource, MemoryItemStatus, MemoryItemType,
        };

        let note = WikiNote {
            path: String::new(),
            title: String::new(),
            domain: "billing".to_string(),
            confidence: Confidence::Confirmed,
            last_updated: None,
            related_files: Vec::new(),
            content: String::new(),
            deprecated: false,
            memory_items: vec![MemoryItem {
                id: "billing-001".to_string(),
                type_: MemoryItemType::Exception,
                text: "Legacy pricing uses old tax rate".to_string(),
                confidence: Confidence::Confirmed,
                related_files: vec!["src/billing/pricing.ts".to_string()],
                sources: vec![MemoryItemSource {
                    kind: "file".to_string(),
                    ref_: "src/billing/pricing.ts".to_string(),
                    line: None, // No line info
                }],
                status: MemoryItemStatus::Active,
                last_reviewed: None,
            }],
        };

        let hunks = vec![HunkRange {
            old_start: 38,
            old_count: 13,
            new_start: 38,
            new_count: 15,
        }];

        // No line info → source proximity check skipped, no identifiers → no warning
        let warnings =
            check_items_against_diff("billing", &note, "src/billing/pricing.ts", &hunks, &[]);

        assert!(
            warnings.is_empty(),
            "No warning expected when source has no line info and no identifier match"
        );
    }

    #[test]
    fn check_items_identifier_heuristic() {
        use crate::wiki::note::{Confidence, MemoryItem, MemoryItemStatus, MemoryItemType};

        let note = WikiNote {
            path: String::new(),
            title: String::new(),
            domain: "billing".to_string(),
            confidence: Confidence::Confirmed,
            last_updated: None,
            related_files: Vec::new(),
            content: String::new(),
            deprecated: false,
            memory_items: vec![MemoryItem {
                id: "billing-002".to_string(),
                type_: MemoryItemType::Decision,
                text: "calculateLegacyPrice must never apply discount".to_string(),
                confidence: Confidence::Verified,
                related_files: Vec::new(),
                sources: Vec::new(), // No sources at all
                status: MemoryItemStatus::Active,
                last_reviewed: None,
            }],
        };

        let hunks = vec![HunkRange {
            old_start: 10,
            old_count: 5,
            new_start: 10,
            new_count: 7,
        }];

        // Identifier "calculatelegacyprice" (lowercased) matches item text
        let diff_idents = vec!["calculatelegacyprice".to_string()];

        let warnings = check_items_against_diff(
            "billing",
            &note,
            "src/billing/pricing.ts",
            &hunks,
            &diff_idents,
        );

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("billing-002"));
        assert!(warnings[0].message.contains("identifier"));
    }

    #[test]
    fn line_proximity_exact_boundary() {
        let hunk = HunkRange {
            old_start: 38,
            old_count: 13, // covers lines 38-50
            new_start: 38,
            new_count: 15,
        };

        // Exact upper boundary: 50 + tolerance(5) = 55
        assert!(is_line_near_hunk(55, &hunk, HUNK_PROXIMITY_TOLERANCE));
        // One past upper boundary
        assert!(!is_line_near_hunk(56, &hunk, HUNK_PROXIMITY_TOLERANCE));
        // Exact lower boundary: 38 - tolerance(5) = 33
        assert!(is_line_near_hunk(33, &hunk, HUNK_PROXIMITY_TOLERANCE));
        // One before lower boundary
        assert!(!is_line_near_hunk(32, &hunk, HUNK_PROXIMITY_TOLERANCE));
    }

    #[test]
    fn line_proximity_zero_count_insertion() {
        // Pure insertion: old_count=0 means no old lines deleted
        let hunk = HunkRange {
            old_start: 42,
            old_count: 0,
            new_start: 42,
            new_count: 3,
        };

        // Should still detect proximity around the insertion point
        assert!(is_line_near_hunk(42, &hunk, HUNK_PROXIMITY_TOLERANCE));
        assert!(is_line_near_hunk(37, &hunk, HUNK_PROXIMITY_TOLERANCE)); // 42 - 5
        assert!(is_line_near_hunk(47, &hunk, HUNK_PROXIMITY_TOLERANCE)); // 42 + 5
        assert!(!is_line_near_hunk(48, &hunk, HUNK_PROXIMITY_TOLERANCE)); // 42 + 6
    }

    #[test]
    fn extract_identifiers_filters_stopwords() {
        let diff = "+  let result = calculateLegacyPrice(value);\n\
                    -  const oldPrice = getBaseAmount();\n";
        let idents = extract_diff_identifiers(diff);

        // Should NOT contain stopwords
        assert!(!idents.contains(&"result".to_string()));
        assert!(!idents.contains(&"const".to_string()));
        assert!(!idents.contains(&"value".to_string()));

        // Should contain meaningful identifiers (4+ chars, not stopwords)
        assert!(idents.contains(&"calculatelegacyprice".to_string()));
        assert!(idents.contains(&"oldprice".to_string()));
        assert!(idents.contains(&"getbaseamount".to_string()));
    }

    #[test]
    fn identifier_heuristic_no_substring_match() {
        // "tax" should NOT match "syntax" via word-boundary matching
        use crate::wiki::note::{Confidence, MemoryItem, MemoryItemStatus, MemoryItemType};

        let note = WikiNote {
            path: String::new(),
            title: String::new(),
            domain: "billing".to_string(),
            confidence: Confidence::Confirmed,
            last_updated: None,
            related_files: Vec::new(),
            content: String::new(),
            deprecated: false,
            memory_items: vec![MemoryItem {
                id: "billing-003".to_string(),
                type_: MemoryItemType::BusinessRule,
                text: "Check syntax before processing".to_string(),
                confidence: Confidence::Confirmed,
                related_files: Vec::new(),
                sources: Vec::new(),
                status: MemoryItemStatus::Active,
                last_reviewed: None,
            }],
        };

        let hunks = vec![HunkRange {
            old_start: 10,
            old_count: 5,
            new_start: 10,
            new_count: 7,
        }];

        // "taxrate" should not trigger a match on "syntax"
        let diff_idents = vec!["taxrate".to_string()];

        let warnings = check_items_against_diff(
            "billing",
            &note,
            "src/billing/pricing.ts",
            &hunks,
            &diff_idents,
        );

        assert!(
            warnings.is_empty(),
            "Should not match 'taxrate' against 'syntax': {:?}",
            warnings.iter().map(|w| &w.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn existing_drift_tests_still_pass() {
        // Regression: ensure all original DriftKind variants still work
        let dir = TempDir::new().unwrap();
        create_wiki_note(
            &dir,
            "billing",
            "inferred",
            "2025-01-01",
            &["src/billing/invoice.ts"],
        );

        let wiki_dir = dir.path().join(".wiki");
        let warnings = detect("src/billing/invoice.ts", &wiki_dir, dir.path()).unwrap();

        // Should have at least Stale + LowConfidence + RelatedFileModified
        assert!(warnings.iter().any(|w| matches!(w.kind, DriftKind::Stale)));
        assert!(
            warnings
                .iter()
                .any(|w| matches!(w.kind, DriftKind::LowConfidence))
        );
        assert!(
            warnings
                .iter()
                .any(|w| matches!(w.kind, DriftKind::RelatedFileModified))
        );
    }
}
