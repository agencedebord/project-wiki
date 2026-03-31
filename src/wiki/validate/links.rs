use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::Result;
use walkdir::WalkDir;

use crate::wiki::common::LINK_RE;
use crate::wiki::note::WikiNote;

/// Collect all .md files in wiki_dir as (relative_path, content) pairs.
pub(super) fn collect_all_md_files(wiki_dir: &Path) -> Result<Vec<(String, String)>> {
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
pub(super) fn check_broken_links(
    md_files: &[(String, String)],
    wiki_dir: &Path,
) -> Result<Vec<(String, String)>> {
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

            // Resolve relative to the file's directory within wiki_dir
            let file_dir = wiki_dir.join(file_path);
            let file_parent = file_dir.parent().unwrap_or(wiki_dir);

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

/// Check 3: Parse related_files from front matter and verify they exist
pub(super) fn check_dead_references(notes: &[WikiNote]) -> Vec<(String, String)> {
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
pub(super) fn check_deprecated_references(
    notes: &[WikiNote],
    md_files: &[(String, String)],
    wiki_dir: &Path,
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

    let wiki_dir_str = wiki_dir.to_string_lossy();
    let mut results = Vec::new();

    for (file_path, content) in md_files {
        // Skip deprecated notes themselves — match both absolute and relative forms
        let full_path = wiki_dir.join(file_path).to_string_lossy().to_string();
        let legacy_path = format!(".wiki/{}", file_path);
        if deprecated_paths.contains(&full_path)
            || deprecated_paths.contains(&legacy_path)
            || deprecated_paths.contains(&format!("{}/{}", wiki_dir_str, file_path))
        {
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

/// Check 7: Find .md files in wiki_dir/domains/ not referenced in _index.md
pub(super) fn check_orphan_notes(wiki_dir: &Path) -> Result<Vec<String>> {
    let index_path = wiki_dir.join("_index.md");
    let domains_dir = wiki_dir.join("domains");

    if !domains_dir.exists() {
        return Ok(Vec::new());
    }

    let index_content = if index_path.exists() {
        fs::read_to_string(index_path).unwrap_or_default()
    } else {
        String::new()
    };

    let mut orphans = Vec::new();

    for entry in WalkDir::new(&domains_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "md") {
            continue;
        }

        // Build relative path as it would appear in _index.md (e.g., ./domains/import/dedup.md)
        let rel_from_wiki = path
            .strip_prefix(wiki_dir)
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
