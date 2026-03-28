use std::io::Read as _;
use std::path::Path;

use anyhow::Result;
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
}

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

    Ok(warnings)
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
}
