use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use walkdir::WalkDir;

use crate::ui;
use crate::wiki::common;
use crate::wiki::common::{DOMAIN_PARENT_DIRS, LINK_RE};
use crate::wiki::config;
use crate::wiki::note::{Confidence, WikiNote};

pub fn run() -> Result<()> {
    let wiki_dir = common::find_wiki_root()?;
    let wiki_config = config::load(&wiki_dir);

    ui::app_header(env!("CARGO_PKG_VERSION"));
    ui::action("Validating wiki");

    let mut errors: usize = 0;
    let mut warnings: usize = 0;
    let mut passes: usize = 0;

    let notes = common::collect_all_notes(&wiki_dir)?;
    let md_files = collect_all_md_files()?;

    // ─── 1. Broken links ───
    ui::header("Broken links");
    let broken = check_broken_links(&md_files)?;
    if broken.is_empty() {
        ui::resolved("No broken links found.");
        passes += 1;
    } else {
        for (file, target) in &broken {
            ui::unresolved(&format!("{} -> {} (not found)", file, target));
        }
        errors += broken.len();
    }

    // ─── 2. Undocumented domains ───
    ui::header("Undocumented domains");
    let undocumented = check_undocumented_domains()?;
    if undocumented.is_empty() {
        ui::resolved("All code domains are documented.");
        passes += 1;
    } else {
        for domain in &undocumented {
            ui::warn(&format!(
                "Domain '{}' found in code but not in wiki",
                domain
            ));
        }
        warnings += undocumented.len();
    }

    // ─── 3. Dead references ───
    ui::header("Dead references");
    let dead_refs = check_dead_references(&notes);
    if dead_refs.is_empty() {
        ui::resolved("All related_files references are valid.");
        passes += 1;
    } else {
        for (note_path, ref_path) in &dead_refs {
            ui::unresolved(&format!(
                "{} references {} (not found)",
                note_path, ref_path
            ));
        }
        errors += dead_refs.len();
    }

    // ─── 4. Deprecated references ───
    ui::header("Deprecated references");
    let deprecated_refs = check_deprecated_references(&notes, &md_files)?;
    if deprecated_refs.is_empty() {
        ui::resolved("No active notes link to deprecated notes.");
        passes += 1;
    } else {
        for (source, target) in &deprecated_refs {
            ui::warn(&format!("{} links to deprecated note {}", source, target));
        }
        warnings += deprecated_refs.len();
    }

    // ─── 5. Confidence ratio ───
    ui::header("Confidence ratio");
    let (low_confidence_count, total_count, low_pct) = check_confidence_ratio(&notes);
    if total_count == 0 {
        ui::resolved("No notes to check.");
        passes += 1;
    } else if low_pct > 40.0 {
        ui::warn(&format!(
            "{}/{} notes ({:.0}%) are inferred or needs-validation (threshold: 40%)",
            low_confidence_count, total_count, low_pct
        ));
        warnings += 1;
    } else {
        ui::resolved(&format!(
            "{}/{} notes ({:.0}%) are inferred or needs-validation — within threshold",
            low_confidence_count, total_count, low_pct
        ));
        passes += 1;
    }

    // ─── 6. Staleness ───
    ui::header("Staleness");
    let staleness_days = wiki_config.staleness_days;
    let stale = check_staleness(&notes, staleness_days);
    if stale.is_empty() {
        ui::resolved(&format!(
            "No stale notes (all updated within {} days).",
            staleness_days
        ));
        passes += 1;
    } else {
        for (path, days) in &stale {
            ui::warn(&format!("{} — last updated {} days ago", path, days));
        }
        warnings += stale.len();
    }

    // ─── 7. Orphan notes ───
    ui::header("Orphan notes");
    let orphans = check_orphan_notes()?;
    if orphans.is_empty() {
        ui::resolved("All domain notes are referenced in _index.md.");
        passes += 1;
    } else {
        for path in &orphans {
            ui::warn(&format!("{} is not referenced in _index.md", path));
        }
        warnings += orphans.len();
    }

    // ─── Summary ───
    ui::header("Summary");
    let summary_lines = vec![format!(
        "{} passed  {} warnings  {} errors",
        passes, warnings, errors
    )];
    let summary_strings: Vec<String> = summary_lines;
    ui::summary_box(&summary_strings);

    if errors > 0 {
        eprintln!();
        ui::error("Validation failed.");
        eprintln!();
        bail!(
            "Validation failed with {} error(s) and {} warning(s).",
            errors,
            warnings
        );
    } else if warnings > 0 {
        eprintln!();
        ui::done("Validation passed with warnings.");
    } else {
        eprintln!();
        ui::done("Validation passed.");
    }
    eprintln!();

    Ok(())
}

// ─── Helpers ───

fn collect_all_md_files() -> Result<Vec<(String, String)>> {
    // Returns (relative_path, content) for all .md files in .wiki/
    let wiki_dir = Path::new(".wiki");
    let mut files = Vec::new();

    for entry in WalkDir::new(wiki_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") {
            let rel = path
                .strip_prefix(wiki_dir)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();
            if let Ok(content) = fs::read_to_string(path) {
                files.push((rel, content));
            }
        }
    }

    Ok(files)
}

/// Check 1: Scan all .md files for markdown links [text](path) and verify targets exist
fn check_broken_links(md_files: &[(String, String)]) -> Result<Vec<(String, String)>> {
    let mut broken = Vec::new();

    for (file_path, content) in md_files {
        for cap in LINK_RE.captures_iter(content) {
            let target = &cap[2];

            // Skip external URLs, anchors, and mermaid/code blocks
            if target.starts_with("http://")
                || target.starts_with("https://")
                || target.starts_with('#')
            {
                continue;
            }

            // Resolve relative to the file's directory within .wiki/
            let file_dir = Path::new(".wiki").join(file_path);
            let file_parent = file_dir.parent().unwrap_or(Path::new(".wiki"));

            // Strip any anchor fragment
            let target_path_str = target.split('#').next().unwrap_or(target);
            if target_path_str.is_empty() {
                continue;
            }

            let resolved = file_parent.join(target_path_str);
            if !resolved.exists() {
                broken.push((file_path.clone(), target.to_string()));
            }
        }
    }

    Ok(broken)
}

/// Check 2: Find domains in codebase not documented in .wiki/domains/
fn check_undocumented_domains() -> Result<Vec<String>> {
    let project_root = std::env::current_dir().context("Failed to get current directory")?;
    let wiki_domains_dir = Path::new(".wiki/domains");

    // Collect existing wiki domains
    let mut documented: HashSet<String> = HashSet::new();
    if wiki_domains_dir.exists() {
        for entry in fs::read_dir(wiki_domains_dir)? {
            let entry = entry?;
            if entry.path().is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    documented.insert(name.to_string());
                }
            }
        }
    }

    // Scan codebase for domain-like directories
    let mut code_domains: HashSet<String> = HashSet::new();
    let src_dir = project_root.join("src");

    let search_roots: Vec<std::path::PathBuf> = if src_dir.exists() {
        vec![src_dir]
    } else {
        vec![project_root.clone()]
    };

    for root in &search_roots {
        for entry in WalkDir::new(root)
            .max_depth(4)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_dir() {
                continue;
            }

            let path = entry.path();
            if let Some(parent_name) = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
            {
                if DOMAIN_PARENT_DIRS.contains(&parent_name.to_lowercase().as_str()) {
                    if let Some(domain_name) = path.file_name().and_then(|n| n.to_str()) {
                        let normalized = domain_name.to_lowercase().replace('_', "-");
                        code_domains.insert(normalized);
                    }
                }
            }
        }
    }

    let mut undocumented: Vec<String> = code_domains.difference(&documented).cloned().collect();
    undocumented.sort();

    Ok(undocumented)
}

/// Check 3: Parse related_files from front matter and verify they exist
fn check_dead_references(notes: &[WikiNote]) -> Vec<(String, String)> {
    let mut dead = Vec::new();

    for note in notes {
        for ref_file in &note.related_files {
            let ref_path = Path::new(ref_file);
            if !ref_path.exists() {
                dead.push((note.path.clone(), ref_file.clone()));
            }
        }
    }

    dead
}

/// Check 4: Find active notes that link to deprecated notes
fn check_deprecated_references(
    notes: &[WikiNote],
    md_files: &[(String, String)],
) -> Result<Vec<(String, String)>> {
    // Find deprecated note paths
    let deprecated_paths: HashSet<String> = notes
        .iter()
        .filter(|n| n.deprecated)
        .map(|n| n.path.clone())
        .collect();

    if deprecated_paths.is_empty() {
        return Ok(Vec::new());
    }

    // Build a set of deprecated file names for matching in links
    let deprecated_filenames: HashMap<String, String> = notes
        .iter()
        .filter(|n| n.deprecated)
        .filter_map(|n| {
            Path::new(&n.path)
                .file_name()
                .map(|f| (f.to_string_lossy().to_string(), n.path.clone()))
        })
        .collect();

    let mut results = Vec::new();

    for (file_path, content) in md_files {
        // Skip deprecated notes themselves
        let full_path = format!(".wiki/{}", file_path);
        if deprecated_paths.contains(&full_path) {
            continue;
        }

        for cap in LINK_RE.captures_iter(content) {
            let target = &cap[2];
            let target_filename = Path::new(target)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();

            if let Some(deprecated_path) = deprecated_filenames.get(&target_filename) {
                results.push((file_path.clone(), deprecated_path.clone()));
            }
        }
    }

    Ok(results)
}

/// Check 5: Count notes by confidence, warn if >40% are low-confidence
fn check_confidence_ratio(notes: &[WikiNote]) -> (usize, usize, f64) {
    let total = notes.len();
    if total == 0 {
        return (0, 0, 0.0);
    }

    let low = notes
        .iter()
        .filter(|n| {
            matches!(
                n.confidence,
                Confidence::Inferred | Confidence::NeedsValidation
            )
        })
        .count();

    let pct = low as f64 / total as f64 * 100.0;
    (low, total, pct)
}

/// Check 6: Find notes with last_updated older than the configured staleness threshold
fn check_staleness(notes: &[WikiNote], staleness_days: u32) -> Vec<(String, i64)> {
    let today = Utc::now().date_naive();
    let threshold = i64::from(staleness_days);

    notes
        .iter()
        .filter_map(|n| {
            n.last_updated.and_then(|date| {
                let days = (today - date).num_days();
                if days > threshold {
                    Some((n.path.clone(), days))
                } else {
                    None
                }
            })
        })
        .collect()
}

/// Check 7: Find .md files in .wiki/domains/ not referenced in _index.md
fn check_orphan_notes() -> Result<Vec<String>> {
    let index_path = Path::new(".wiki/_index.md");
    let domains_dir = Path::new(".wiki/domains");

    if !domains_dir.exists() {
        return Ok(Vec::new());
    }

    let index_content = if index_path.exists() {
        fs::read_to_string(index_path).unwrap_or_default()
    } else {
        String::new()
    };

    let mut orphans = Vec::new();

    for entry in WalkDir::new(domains_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "md") {
            continue;
        }

        // Build relative path as it would appear in _index.md (e.g., ./domains/import/dedup.md)
        let rel_from_wiki = path
            .strip_prefix(".wiki")
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        // Check both with and without ./ prefix
        let with_dot = format!("./{}", rel_from_wiki);
        let without_dot = rel_from_wiki.clone();

        // Also check just the filename
        let filename = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();

        if !index_content.contains(&with_dot)
            && !index_content.contains(&without_dot)
            && !index_content.contains(&filename)
        {
            orphans.push(rel_from_wiki);
        }
    }

    orphans.sort();
    Ok(orphans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiki::note::{Confidence, WikiNote};
    use chrono::NaiveDate;
    use tempfile::TempDir;

    fn make_note(
        path: &str,
        confidence: Confidence,
        last_updated: Option<NaiveDate>,
        related_files: Vec<String>,
        deprecated: bool,
    ) -> WikiNote {
        WikiNote {
            path: path.to_string(),
            domain: "test".to_string(),
            confidence,
            last_updated,
            related_files,
            deprecated,
            title: "Test".to_string(),
            content: String::new(),
            memory_items: Vec::new(),
        }
    }

    // ─── check_broken_links ───

    #[test]
    fn broken_links_detects_missing_target() {
        let dir = TempDir::new().unwrap();
        let wiki = dir.path().join(".wiki");
        std::fs::create_dir_all(wiki.join("domains/billing")).unwrap();

        std::fs::write(
            wiki.join("domains/billing/_overview.md"),
            "# Billing\n\nSee [details](./nonexistent.md) for more.\n",
        )
        .unwrap();

        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let md_files = vec![(
            "domains/billing/_overview.md".to_string(),
            std::fs::read_to_string(wiki.join("domains/billing/_overview.md")).unwrap(),
        )];

        let broken = check_broken_links(&md_files).unwrap();
        std::env::set_current_dir(&original).unwrap();

        assert_eq!(broken.len(), 1);
        assert_eq!(broken[0].0, "domains/billing/_overview.md");
        assert_eq!(broken[0].1, "./nonexistent.md");
    }

    #[test]
    fn broken_links_skips_external_urls() {
        let md_files = vec![(
            "test.md".to_string(),
            "See [Google](https://google.com) and [local](#anchor)".to_string(),
        )];

        let broken = check_broken_links(&md_files).unwrap();
        assert!(broken.is_empty());
    }

    // ─── check_confidence_ratio ───

    #[test]
    fn confidence_ratio_with_mixed_notes() {
        let notes = vec![
            make_note("a.md", Confidence::Confirmed, None, vec![], false),
            make_note("b.md", Confidence::Inferred, None, vec![], false),
            make_note("c.md", Confidence::NeedsValidation, None, vec![], false),
            make_note("d.md", Confidence::Verified, None, vec![], false),
        ];

        let (low, total, pct) = check_confidence_ratio(&notes);
        assert_eq!(total, 4);
        assert_eq!(low, 2);
        assert!((pct - 50.0).abs() < 0.01);
    }

    #[test]
    fn confidence_ratio_empty_notes() {
        let (low, total, pct) = check_confidence_ratio(&[]);
        assert_eq!(low, 0);
        assert_eq!(total, 0);
        assert!((pct - 0.0).abs() < 0.01);
    }

    #[test]
    fn confidence_ratio_all_confirmed() {
        let notes = vec![
            make_note("a.md", Confidence::Confirmed, None, vec![], false),
            make_note("b.md", Confidence::Verified, None, vec![], false),
        ];

        let (low, total, pct) = check_confidence_ratio(&notes);
        assert_eq!(low, 0);
        assert_eq!(total, 2);
        assert!((pct - 0.0).abs() < 0.01);
    }

    // ─── check_staleness ───

    #[test]
    fn staleness_flags_old_notes() {
        let old_date = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
        let recent_date = chrono::Utc::now().date_naive();

        let notes = vec![
            make_note(
                "old.md",
                Confidence::Confirmed,
                Some(old_date),
                vec![],
                false,
            ),
            make_note(
                "new.md",
                Confidence::Confirmed,
                Some(recent_date),
                vec![],
                false,
            ),
            make_note("no-date.md", Confidence::Confirmed, None, vec![], false),
        ];

        let stale = check_staleness(&notes, 30);
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].0, "old.md");
        assert!(stale[0].1 > 30);
    }

    #[test]
    fn staleness_empty_when_all_recent() {
        let today = chrono::Utc::now().date_naive();
        let notes = vec![make_note(
            "a.md",
            Confidence::Confirmed,
            Some(today),
            vec![],
            false,
        )];

        let stale = check_staleness(&notes, 30);
        assert!(stale.is_empty());
    }

    // ─── check_dead_references ───

    #[test]
    fn dead_references_detects_missing_file() {
        let notes = vec![make_note(
            "note.md",
            Confidence::Confirmed,
            None,
            vec!["/nonexistent/path/to/file.ts".to_string()],
            false,
        )];

        let dead = check_dead_references(&notes);
        assert_eq!(dead.len(), 1);
        assert_eq!(dead[0].0, "note.md");
        assert_eq!(dead[0].1, "/nonexistent/path/to/file.ts");
    }

    #[test]
    fn dead_references_passes_with_existing_file() {
        let dir = TempDir::new().unwrap();
        let real_file = dir.path().join("real.ts");
        std::fs::write(&real_file, "export {}").unwrap();

        let notes = vec![make_note(
            "note.md",
            Confidence::Confirmed,
            None,
            vec![real_file.to_string_lossy().to_string()],
            false,
        )];

        let dead = check_dead_references(&notes);
        assert!(dead.is_empty());
    }

    // ─── check_deprecated_references ───

    #[test]
    fn deprecated_references_detected() {
        let notes = vec![
            make_note(
                ".wiki/domains/billing/old-api.md",
                Confidence::Confirmed,
                None,
                vec![],
                true,
            ),
            make_note(
                ".wiki/domains/billing/_overview.md",
                Confidence::Confirmed,
                None,
                vec![],
                false,
            ),
        ];

        let md_files = vec![(
            "domains/billing/_overview.md".to_string(),
            "See [old api](./old-api.md) for legacy info.".to_string(),
        )];

        let refs = check_deprecated_references(&notes, &md_files).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].0, "domains/billing/_overview.md");
        assert!(refs[0].1.contains("old-api.md"));
    }

    #[test]
    fn deprecated_references_empty_when_no_deprecated() {
        let notes = vec![make_note(
            ".wiki/domains/billing/_overview.md",
            Confidence::Confirmed,
            None,
            vec![],
            false,
        )];

        let md_files = vec![(
            "domains/billing/_overview.md".to_string(),
            "Normal content with [link](./other.md).".to_string(),
        )];

        let refs = check_deprecated_references(&notes, &md_files).unwrap();
        assert!(refs.is_empty());
    }
}
