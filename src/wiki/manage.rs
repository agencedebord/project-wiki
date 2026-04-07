use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use regex::Regex;
use walkdir::WalkDir;

use crate::ui;
use crate::wiki;
use crate::wiki::common::{collect_all_notes, ensure_wiki_exists, find_wiki_root};
use crate::wiki::note::{Confidence, WikiNote};

/// Resolve a target string to a note file path within .wiki/domains/.
///
/// Supports:
/// - `billing` -> `.wiki/domains/billing/_overview.md`
/// - `billing/payments.md` -> `.wiki/domains/billing/payments.md`
fn resolve_note_path(wiki_dir: &Path, target: &str) -> Result<PathBuf> {
    let domains_dir = wiki_dir.join("domains");

    if target.contains('/') {
        // Target is a relative path like "billing/payments.md"
        let path = domains_dir.join(target);
        if !path.exists() {
            bail!("Note not found: {}", path.display());
        }
        Ok(path)
    } else {
        // Target is a domain name -> resolve to _overview.md
        let path = domains_dir.join(target).join("_overview.md");
        if !path.exists() {
            bail!(
                "Domain \"{}\" not found (looked for {})",
                target,
                path.display()
            );
        }
        Ok(path)
    }
}

/// Get today's date as a string.
fn today() -> String {
    Utc::now().format("%Y-%m-%d").to_string()
}

// ─── Public commands ───

/// Set a note's confidence to `confirmed`.
pub fn confirm(target: &str) -> Result<()> {
    let wiki_dir = find_wiki_root()?;
    confirm_in(&wiki_dir, target)
}

/// Mark a domain or note as deprecated.
pub fn deprecate(target: &str) -> Result<()> {
    let wiki_dir = find_wiki_root()?;
    deprecate_in(&wiki_dir, target)
}

/// Rename a domain and update all cross-references.
pub fn rename_domain(old: &str, new: &str) -> Result<()> {
    let wiki_dir = find_wiki_root()?;
    rename_domain_in(&wiki_dir, old, new)
}

/// Import markdown files from a folder into the wiki.
pub fn import_folder(folder: &str, domain: Option<&str>) -> Result<()> {
    let wiki_dir = find_wiki_root()?;
    import_folder_in(&wiki_dir, folder, domain)
}

// ─── Internal implementations (testable with custom wiki dir) ───

/// Returns true if the target looks like a memory item id (e.g. "billing-001").
/// Heuristic: ends with `-\d+`.
fn is_item_id(target: &str) -> bool {
    if let Some(pos) = target.rfind('-') {
        let suffix = &target[pos + 1..];
        !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit())
    } else {
        false
    }
}

fn confirm_in(wiki_dir: &Path, target: &str) -> Result<()> {
    ensure_wiki_exists(wiki_dir)?;

    if is_item_id(target) {
        return confirm_item_in(wiki_dir, target);
    }

    let note_path = resolve_note_path(wiki_dir, target)?;
    let mut note = WikiNote::parse(&note_path)
        .with_context(|| format!("Failed to parse note: {}", note_path.display()))?;

    let old_confidence = note.confidence.clone();
    note.confidence = Confidence::Confirmed;
    note.last_updated = Some(Utc::now().date_naive());

    note.write(&note_path)
        .with_context(|| format!("Failed to write note: {}", note_path.display()))?;

    // Regenerate index
    if let Err(e) = wiki::index::run() {
        ui::warn(&format!("Failed to regenerate index: {}", e));
    }

    ui::success(&format!(
        "Confirmed \"{}\" ({} -> confirmed)",
        note_path.display(),
        old_confidence
    ));

    Ok(())
}

/// Confirm a single memory item by its id across all wiki notes.
fn confirm_item_in(wiki_dir: &Path, item_id: &str) -> Result<()> {
    let notes = collect_all_notes(wiki_dir)?;
    let today_str = today();

    for note_data in &notes {
        let note_path = PathBuf::from(&note_data.path);
        if !note_data.memory_items.iter().any(|i| i.id == item_id) {
            continue;
        }

        // Found the note containing this item
        let mut note = WikiNote::parse(&note_path)
            .with_context(|| format!("Failed to parse note: {}", note_path.display()))?;

        let item = note
            .memory_items
            .iter_mut()
            .find(|i| i.id == item_id)
            .expect("Item must exist — we just checked");

        if item.confidence == Confidence::Confirmed {
            ui::warn(&format!("{item_id} is already confirmed"));
            // Still update last_reviewed
            item.last_reviewed = Some(today_str);
            note.write(&note_path)
                .with_context(|| format!("Failed to write note: {}", note_path.display()))?;
            return Ok(());
        }

        let old_confidence = item.confidence.clone();
        item.confidence = Confidence::Confirmed;
        item.last_reviewed = Some(today_str);

        note.write(&note_path)
            .with_context(|| format!("Failed to write note: {}", note_path.display()))?;

        ui::success(&format!(
            "Confirmed {item_id}: {} ({old_confidence} -> confirmed)",
            note.memory_items
                .iter()
                .find(|i| i.id == item_id)
                .map(|i| i.text.as_str())
                .unwrap_or(""),
        ));

        return Ok(());
    }

    bail!("No memory item found with id '{item_id}'")
}

fn deprecate_in(wiki_dir: &Path, target: &str) -> Result<()> {
    ensure_wiki_exists(wiki_dir)?;

    let note_path = resolve_note_path(wiki_dir, target)?;
    let mut note = WikiNote::parse(&note_path)
        .with_context(|| format!("Failed to parse note: {}", note_path.display()))?;

    if note.deprecated {
        ui::warn(&format!(
            "\"{}\" is already deprecated.",
            note_path.display()
        ));
        return Ok(());
    }

    note.deprecated = true;
    note.last_updated = Some(Utc::now().date_naive());

    note.write(&note_path)
        .with_context(|| format!("Failed to write note: {}", note_path.display()))?;

    // Regenerate index
    if let Err(e) = wiki::index::run() {
        ui::warn(&format!("Failed to regenerate index: {}", e));
    }

    ui::success(&format!("Deprecated \"{}\"", note_path.display()));

    Ok(())
}

fn rename_domain_in(wiki_dir: &Path, old: &str, new: &str) -> Result<()> {
    ensure_wiki_exists(wiki_dir)?;

    let domains_dir = wiki_dir.join("domains");
    let old_dir = domains_dir.join(old);
    let new_dir = domains_dir.join(new);

    if !old_dir.exists() {
        bail!("Domain \"{}\" does not exist.", old);
    }
    if new_dir.exists() {
        bail!("Domain \"{}\" already exists.", new);
    }

    // 1. Rename the directory
    fs::rename(&old_dir, &new_dir).with_context(|| {
        format!(
            "Failed to rename {} -> {}",
            old_dir.display(),
            new_dir.display()
        )
    })?;

    ui::step(&format!("Renamed directory: {} -> {}", old, new));

    // 2. Update domain references in front matter of notes in the renamed domain
    for entry in WalkDir::new(&new_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") {
            if let Ok(content) = fs::read_to_string(path) {
                let updated = update_domain_references_in_content(&content, old, new);
                if updated != content {
                    fs::write(path, &updated)
                        .with_context(|| format!("Failed to update {}", path.display()))?;
                    ui::step(&format!("Updated references in {}", path.display()));
                }
            }
        }
    }

    // 3. Update cross-references in ALL wiki markdown files
    let mut updated_count = 0;
    for entry in WalkDir::new(wiki_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") {
            if let Ok(content) = fs::read_to_string(path) {
                let updated = update_cross_references(&content, old, new);
                if updated != content {
                    fs::write(path, &updated)
                        .with_context(|| format!("Failed to update {}", path.display()))?;
                    updated_count += 1;
                }
            }
        }
    }

    if updated_count > 0 {
        ui::step(&format!(
            "Updated cross-references in {} file(s)",
            updated_count
        ));
    }

    // 4. Regenerate graph and index
    if let Err(e) = wiki::graph::run() {
        ui::warn(&format!("Failed to regenerate graph: {}", e));
    }
    if let Err(e) = wiki::index::run() {
        ui::warn(&format!("Failed to regenerate index: {}", e));
    }

    ui::success(&format!("Domain renamed: \"{}\" -> \"{}\"", old, new));

    Ok(())
}

/// Update `domain:` references in front matter content.
fn update_domain_references_in_content(content: &str, old: &str, new: &str) -> String {
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut in_frontmatter = false;

    for line in lines.iter_mut() {
        if line.trim() == "---" {
            if !in_frontmatter {
                in_frontmatter = true;
                continue;
            } else {
                break;
            }
        }
        if in_frontmatter && line.starts_with("domain:") {
            let value = line["domain:".len()..].trim();
            if value == old {
                *line = format!("domain: {}", new);
            }
        }
    }

    let joined = lines.join("\n");
    if content.ends_with('\n') && !joined.ends_with('\n') {
        joined + "\n"
    } else {
        joined
    }
}

/// Update markdown links and text references from the old domain to the new domain.
fn update_cross_references(content: &str, old: &str, new: &str) -> String {
    // Replace path references like `domains/old/` with `domains/new/`
    // The trailing `/` already prevents partial matches (e.g. `domains/billing/` won't match `domains/billing-extra/`)
    let old_path = format!("domains/{}/", old);
    let new_path = format!("domains/{}/", new);
    let result = content.replace(&old_path, &new_path);

    // Replace `domain: old` in front matter, anchoring to end-of-line to avoid
    // partial matches (e.g. renaming `billing` must not affect `billing-extra`)
    let pattern = format!(r"(?m)^domain:\s+{}$", regex::escape(old));
    let re = Regex::new(&pattern).expect("invalid domain regex");
    let new_domain_ref = format!("domain: {}", new);
    re.replace_all(&result, new_domain_ref.as_str())
        .into_owned()
}

fn import_folder_in(wiki_dir: &Path, folder: &str, domain: Option<&str>) -> Result<()> {
    ensure_wiki_exists(wiki_dir)?;

    let source = Path::new(folder);
    if !source.exists() {
        bail!("Folder not found: {}", folder);
    }
    if !source.is_dir() {
        bail!("Not a directory: {}", folder);
    }

    // Determine the target domain name
    let domain_name = match domain {
        Some(d) => d.to_string(),
        None => source
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "imported".to_string())
            .to_lowercase()
            .replace([' ', '_'], "-"),
    };

    let domain_dir = wiki_dir.join("domains").join(&domain_name);
    fs::create_dir_all(&domain_dir).with_context(|| {
        format!(
            "Failed to create domain directory: {}",
            domain_dir.display()
        )
    })?;

    let date = today();
    let mut imported_count = 0;

    for entry in WalkDir::new(source).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "md") {
            continue;
        }

        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;

        // Determine the target filename (preserve relative structure flattened)
        let relative = path.strip_prefix(source).unwrap_or(path);
        let target_name = if relative.components().count() > 1 {
            // Flatten subdirectory structure: dir/file.md -> dir-file.md
            relative.to_string_lossy().replace(['/', '\\'], "-")
        } else {
            relative.to_string_lossy().to_string()
        };

        let target_path = domain_dir.join(&target_name);

        let final_content = if has_front_matter(&content) {
            // Preserve existing front matter, add confidence: imported if missing
            ensure_confidence_in_frontmatter(&content, "imported")
        } else {
            // Add minimal front matter
            let title = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            format!(
                "---\ntitle: {}\nconfidence: needs-validation\nlast_updated: {}\n---\n{}",
                title, date, content
            )
        };

        fs::write(&target_path, &final_content)
            .with_context(|| format!("Failed to write {}", target_path.display()))?;

        ui::step(&format!("{} -> {}", path.display(), target_path.display()));
        imported_count += 1;
    }

    if imported_count == 0 {
        ui::warn("No markdown files found in the folder.");
        return Ok(());
    }

    // Regenerate index
    if let Err(e) = wiki::index::run() {
        ui::warn(&format!("Failed to regenerate index: {}", e));
    }

    ui::success(&format!(
        "Imported {} file(s) into domain \"{}\"",
        imported_count, domain_name
    ));

    Ok(())
}

/// Check if content starts with YAML front matter delimiters.
fn has_front_matter(content: &str) -> bool {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return false;
    }
    // Check there's a closing ---
    let after_first = &trimmed[3..];
    after_first.contains("\n---")
}

/// Ensure the front matter contains a `confidence` field. If not, add one.
fn ensure_confidence_in_frontmatter(content: &str, default_confidence: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut in_frontmatter = false;
    let mut has_confidence = false;
    let mut end_idx = None;

    for (i, line) in lines.iter().enumerate() {
        if line.trim() == "---" {
            if !in_frontmatter {
                in_frontmatter = true;
                continue;
            } else {
                end_idx = Some(i);
                break;
            }
        }
        if in_frontmatter && line.starts_with("confidence:") {
            has_confidence = true;
        }
    }

    if has_confidence || end_idx.is_none() {
        // Already has confidence or no valid front matter found
        return content.to_string();
    }

    // Insert confidence before the closing ---
    let end = end_idx.unwrap();
    let mut result: Vec<String> = lines[..end].iter().map(|l| l.to_string()).collect();
    result.push(format!("confidence: {}", default_confidence));
    for line in &lines[end..] {
        result.push(line.to_string());
    }

    let joined = result.join("\n");
    if content.ends_with('\n') && !joined.ends_with('\n') {
        joined + "\n"
    } else {
        joined
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_wiki(dir: &TempDir) -> PathBuf {
        let wiki = dir.path().join(".wiki");
        fs::create_dir_all(wiki.join("domains")).unwrap();
        wiki
    }

    fn create_domain_with_note(wiki: &Path, domain: &str, filename: &str, content: &str) {
        let domain_dir = wiki.join("domains").join(domain);
        fs::create_dir_all(&domain_dir).unwrap();
        fs::write(domain_dir.join(filename), content).unwrap();
    }

    // ─── confirm tests ───

    #[test]
    fn confirm_sets_confidence_to_confirmed() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        let content = "---\ntitle: Billing overview\nconfidence: inferred\nlast_updated: \"2025-01-01\"\n---\n# Billing\n";
        create_domain_with_note(&wiki, "billing", "_overview.md", content);

        confirm_in(&wiki, "billing").unwrap();

        let updated = fs::read_to_string(wiki.join("domains/billing/_overview.md")).unwrap();
        assert!(updated.contains("confidence: confirmed"));
        assert!(!updated.contains("confidence: inferred"));
    }

    #[test]
    fn confirm_works_with_path_target() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        let content = "---\ntitle: Payments\nconfidence: seen-in-code\n---\n# Payments\n";
        create_domain_with_note(&wiki, "billing", "payments.md", content);

        confirm_in(&wiki, "billing/payments.md").unwrap();

        let updated = fs::read_to_string(wiki.join("domains/billing/payments.md")).unwrap();
        assert!(updated.contains("confidence: confirmed"));
    }

    #[test]
    fn confirm_updates_last_updated() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        let content =
            "---\ntitle: Test\nconfidence: inferred\nlast_updated: \"2020-01-01\"\n---\nContent\n";
        create_domain_with_note(&wiki, "test", "_overview.md", content);

        confirm_in(&wiki, "test").unwrap();

        let updated = fs::read_to_string(wiki.join("domains/test/_overview.md")).unwrap();
        let today = Utc::now().format("%Y-%m-%d").to_string();
        assert!(updated.contains(&today));
    }

    #[test]
    fn confirm_fails_for_missing_domain() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        let result = confirm_in(&wiki, "nonexistent");
        assert!(result.is_err());
    }

    // ─── deprecate tests ───

    #[test]
    fn deprecate_sets_deprecated_flag() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        let content =
            "---\ntitle: Old domain\nconfidence: confirmed\ndeprecated: false\n---\n# Old\n";
        create_domain_with_note(&wiki, "old", "_overview.md", content);

        deprecate_in(&wiki, "old").unwrap();

        let updated = fs::read_to_string(wiki.join("domains/old/_overview.md")).unwrap();
        assert!(updated.contains("deprecated: true"));
    }

    #[test]
    fn deprecate_warns_if_already_deprecated() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        let content =
            "---\ntitle: Old domain\nconfidence: confirmed\ndeprecated: true\n---\n# Old\n";
        create_domain_with_note(&wiki, "old", "_overview.md", content);

        // Should not error, just warn
        deprecate_in(&wiki, "old").unwrap();
    }

    #[test]
    fn deprecate_works_with_path() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        let content = "---\ntitle: Payments\nconfidence: confirmed\n---\n# Payments\n";
        create_domain_with_note(&wiki, "billing", "payments.md", content);

        deprecate_in(&wiki, "billing/payments.md").unwrap();

        let updated = fs::read_to_string(wiki.join("domains/billing/payments.md")).unwrap();
        assert!(updated.contains("deprecated: true"));
    }

    // ─── rename_domain tests ───

    #[test]
    fn rename_domain_moves_directory() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        let content =
            "---\ntitle: Billing\nconfidence: confirmed\ndomain: billing\n---\n# Billing\n";
        create_domain_with_note(&wiki, "billing", "_overview.md", content);

        rename_domain_in(&wiki, "billing", "payments").unwrap();

        assert!(!wiki.join("domains/billing").exists());
        assert!(wiki.join("domains/payments/_overview.md").exists());
    }

    #[test]
    fn rename_domain_updates_references_in_other_files() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        let billing_content = "---\ntitle: Billing\nconfidence: confirmed\n---\n# Billing\n";
        create_domain_with_note(&wiki, "billing", "_overview.md", billing_content);

        let auth_content = "---\ntitle: Auth\nconfidence: confirmed\n---\n# Auth\n\nSee [billing](domains/billing/_overview.md) for payment info.\n";
        create_domain_with_note(&wiki, "auth", "_overview.md", auth_content);

        rename_domain_in(&wiki, "billing", "payments").unwrap();

        let auth_updated = fs::read_to_string(wiki.join("domains/auth/_overview.md")).unwrap();
        assert!(auth_updated.contains("domains/payments/_overview.md"));
        assert!(!auth_updated.contains("domains/billing/"));
    }

    #[test]
    fn rename_domain_fails_if_old_missing() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        let result = rename_domain_in(&wiki, "nonexistent", "new-name");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[test]
    fn rename_domain_fails_if_new_exists() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        let content = "---\ntitle: Test\nconfidence: confirmed\n---\n# Test\n";
        create_domain_with_note(&wiki, "old", "_overview.md", content);
        create_domain_with_note(&wiki, "new", "_overview.md", content);

        let result = rename_domain_in(&wiki, "old", "new");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    // ─── import tests ───

    #[test]
    fn import_folder_imports_markdown_files() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        // Create a source folder with markdown files
        let source = dir.path().join("docs");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("guide.md"), "# Guide\n\nSome guide content.\n").unwrap();
        fs::write(source.join("api.md"), "# API\n\nAPI docs.\n").unwrap();

        import_folder_in(&wiki, source.to_str().unwrap(), None).unwrap();

        assert!(wiki.join("domains/docs/guide.md").exists());
        assert!(wiki.join("domains/docs/api.md").exists());

        // Check that front matter was added
        let guide = fs::read_to_string(wiki.join("domains/docs/guide.md")).unwrap();
        assert!(guide.contains("confidence: needs-validation"));
        assert!(guide.contains("title: guide"));
    }

    #[test]
    fn import_folder_preserves_existing_front_matter() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        let source = dir.path().join("docs");
        fs::create_dir_all(&source).unwrap();
        let content =
            "---\ntitle: My Note\nlast_updated: \"2025-06-15\"\n---\n# My Note\n\nContent.\n";
        fs::write(source.join("note.md"), content).unwrap();

        import_folder_in(&wiki, source.to_str().unwrap(), None).unwrap();

        let imported = fs::read_to_string(wiki.join("domains/docs/note.md")).unwrap();
        assert!(imported.contains("title: My Note"));
        assert!(imported.contains("confidence: imported"));
    }

    #[test]
    fn import_folder_with_domain_flag() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        let source = dir.path().join("random-folder");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("file.md"), "# File\n\nContent.\n").unwrap();

        import_folder_in(&wiki, source.to_str().unwrap(), Some("billing")).unwrap();

        assert!(wiki.join("domains/billing/file.md").exists());
    }

    #[test]
    fn import_folder_fails_for_missing_folder() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        let result = import_folder_in(&wiki, "/nonexistent/path", None);
        assert!(result.is_err());
    }

    #[test]
    fn import_folder_warns_for_empty_folder() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        let source = dir.path().join("empty");
        fs::create_dir_all(&source).unwrap();

        // Should not error but warn
        import_folder_in(&wiki, source.to_str().unwrap(), None).unwrap();
    }

    // ─── helper tests ───

    #[test]
    fn has_front_matter_detects_yaml() {
        assert!(has_front_matter("---\ntitle: Test\n---\nContent"));
        assert!(!has_front_matter("# Just markdown\n"));
        assert!(!has_front_matter("---\nno closing delimiter"));
    }

    #[test]
    fn ensure_confidence_adds_when_missing() {
        let content = "---\ntitle: Test\n---\nContent\n";
        let result = ensure_confidence_in_frontmatter(content, "imported");
        assert!(result.contains("confidence: imported"));
    }

    #[test]
    fn ensure_confidence_preserves_when_present() {
        let content = "---\ntitle: Test\nconfidence: confirmed\n---\nContent\n";
        let result = ensure_confidence_in_frontmatter(content, "imported");
        assert!(result.contains("confidence: confirmed"));
        assert!(!result.contains("confidence: imported"));
    }

    #[test]
    fn update_cross_references_replaces_paths() {
        let content = "See [billing](domains/billing/_overview.md) for details.\n";
        let result = update_cross_references(content, "billing", "payments");
        assert_eq!(
            result,
            "See [billing](domains/payments/_overview.md) for details.\n"
        );
    }

    #[test]
    fn update_cross_references_does_not_match_partial_domain() {
        // Renaming `billing` must not affect `billing-extra` in paths or frontmatter
        let content =
            "---\ndomain: billing-extra\n---\nSee [link](domains/billing-extra/_overview.md)\n";
        let result = update_cross_references(content, "billing", "payments");
        assert_eq!(
            result, content,
            "partial domain name should not be replaced"
        );
    }

    #[test]
    fn update_cross_references_replaces_frontmatter_domain() {
        let content = "---\ndomain: billing\ntitle: Test\n---\nContent\n";
        let result = update_cross_references(content, "billing", "payments");
        assert!(result.contains("domain: payments"));
        assert!(!result.contains("domain: billing"));
    }

    #[test]
    fn update_domain_references_in_content_updates_frontmatter() {
        let content = "---\ndomain: billing\ntitle: Test\n---\nContent\n";
        let result = update_domain_references_in_content(content, "billing", "payments");
        assert!(result.contains("domain: payments"));
        assert!(!result.contains("domain: billing"));
    }

    // ─── is_item_id detection ───

    #[test]
    fn test_is_item_id_valid_formats() {
        assert!(is_item_id("billing-001"));
        assert!(is_item_id("auth-42"));
        assert!(is_item_id("user-auth-003")); // domain with hyphens + number
    }

    #[test]
    fn test_is_item_id_invalid_formats() {
        assert!(!is_item_id("billing"));
        assert!(!is_item_id("billing/_overview.md"));
        assert!(!is_item_id("user-auth")); // no number at end
        assert!(!is_item_id(""));
    }

    // ─── confirm item tests ───

    fn note_content_with_items() -> String {
        r#"---
title: Billing overview
confidence: verified
last_updated: "2026-03-20"
related_files:
  - src/billing/invoice.ts
memory_items:
  - id: billing-001
    type: exception
    text: Le client X utilise encore l'ancien calcul
    confidence: inferred
    sources:
      - kind: file
        ref: src/billing/legacy.ts
    status: active
  - id: billing-002
    type: decision
    text: Pas de deduplication des lignes
    confidence: verified
    status: active
  - id: billing-003
    type: business_rule
    text: Facture emise apres synchro
    confidence: seen-in-code
    status: active
---
# Billing

Handles invoicing.
"#
        .to_string()
    }

    #[test]
    fn confirm_item_changes_confidence() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_domain_with_note(&wiki, "billing", "_overview.md", &note_content_with_items());

        confirm_item_in(&wiki, "billing-001").unwrap();

        let note = WikiNote::parse(&wiki.join("domains/billing/_overview.md")).unwrap();
        let item = note
            .memory_items
            .iter()
            .find(|i| i.id == "billing-001")
            .unwrap();
        assert_eq!(item.confidence, Confidence::Confirmed);
    }

    #[test]
    fn confirm_item_updates_last_reviewed() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_domain_with_note(&wiki, "billing", "_overview.md", &note_content_with_items());

        confirm_item_in(&wiki, "billing-001").unwrap();

        let note = WikiNote::parse(&wiki.join("domains/billing/_overview.md")).unwrap();
        let item = note
            .memory_items
            .iter()
            .find(|i| i.id == "billing-001")
            .unwrap();
        let today_str = Utc::now().format("%Y-%m-%d").to_string();
        assert_eq!(item.last_reviewed, Some(today_str));
    }

    #[test]
    fn confirm_item_preserves_other_items() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_domain_with_note(&wiki, "billing", "_overview.md", &note_content_with_items());

        confirm_item_in(&wiki, "billing-001").unwrap();

        let note = WikiNote::parse(&wiki.join("domains/billing/_overview.md")).unwrap();

        // billing-002 and billing-003 should be unchanged
        let item2 = note
            .memory_items
            .iter()
            .find(|i| i.id == "billing-002")
            .unwrap();
        assert_eq!(item2.confidence, Confidence::Verified);

        let item3 = note
            .memory_items
            .iter()
            .find(|i| i.id == "billing-003")
            .unwrap();
        assert_eq!(item3.confidence, Confidence::SeenInCode);
    }

    #[test]
    fn confirm_item_preserves_note_metadata() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_domain_with_note(&wiki, "billing", "_overview.md", &note_content_with_items());

        confirm_item_in(&wiki, "billing-001").unwrap();

        let note = WikiNote::parse(&wiki.join("domains/billing/_overview.md")).unwrap();

        // Note-level metadata should be unchanged
        assert_eq!(note.confidence, Confidence::Verified);
        assert_eq!(note.title, "Billing overview");
        assert!(note.content.contains("Handles invoicing."));
    }

    #[test]
    fn confirm_note_still_works() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_domain_with_note(&wiki, "billing", "_overview.md", &note_content_with_items());

        // Confirming the domain should change note confidence, not item confidence
        confirm_in(&wiki, "billing").unwrap();

        let note = WikiNote::parse(&wiki.join("domains/billing/_overview.md")).unwrap();
        assert_eq!(note.confidence, Confidence::Confirmed);

        // Items should not be individually modified
        let item1 = note
            .memory_items
            .iter()
            .find(|i| i.id == "billing-001")
            .unwrap();
        assert_eq!(item1.confidence, Confidence::Inferred);
    }

    #[test]
    fn confirm_item_not_found() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_domain_with_note(&wiki, "billing", "_overview.md", &note_content_with_items());

        let result = confirm_item_in(&wiki, "billing-999");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No memory item found with id 'billing-999'")
        );
    }

    #[test]
    fn confirm_item_already_confirmed() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        // Create a note where billing-001 is already confirmed
        let content = r#"---
title: Billing
confidence: verified
memory_items:
  - id: billing-001
    type: exception
    text: Already confirmed item
    confidence: confirmed
    status: active
---
# Billing
"#;
        create_domain_with_note(&wiki, "billing", "_overview.md", content);

        // Should not error, just warn
        confirm_item_in(&wiki, "billing-001").unwrap();

        // last_reviewed should still be updated
        let note = WikiNote::parse(&wiki.join("domains/billing/_overview.md")).unwrap();
        let item = note
            .memory_items
            .iter()
            .find(|i| i.id == "billing-001")
            .unwrap();
        let today_str = Utc::now().format("%Y-%m-%d").to_string();
        assert_eq!(item.last_reviewed, Some(today_str));
    }

    #[test]
    fn confirm_item_roundtrip_save_load() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_domain_with_note(&wiki, "billing", "_overview.md", &note_content_with_items());

        confirm_item_in(&wiki, "billing-002").unwrap();

        // Re-parse and verify all 3 items are present and correct
        let note = WikiNote::parse(&wiki.join("domains/billing/_overview.md")).unwrap();
        assert_eq!(note.memory_items.len(), 3);

        let item2 = note
            .memory_items
            .iter()
            .find(|i| i.id == "billing-002")
            .unwrap();
        assert_eq!(item2.confidence, Confidence::Confirmed);
        assert!(item2.last_reviewed.is_some());

        // Other items are intact
        assert!(note.memory_items.iter().any(|i| i.id == "billing-001"));
        assert!(note.memory_items.iter().any(|i| i.id == "billing-003"));
    }
}
