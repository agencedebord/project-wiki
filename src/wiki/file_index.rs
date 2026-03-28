use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::init::scan::structure::extract_domain_name;
use crate::wiki::common::collect_all_notes;

const FILE_INDEX_NAME: &str = ".file-index.json";

/// Reverse index mapping source file paths to their wiki domain.
#[derive(Debug, Serialize, Deserialize)]
pub struct FileIndex {
    /// ISO date of last rebuild.
    pub built_at: String,
    /// source_file_path (relative to project root) → domain_name.
    pub entries: HashMap<String, String>,
}

/// Build the file index from all wiki notes.
///
/// Combines two sources:
/// 1. `related_files` from each wiki note (explicit associations)
/// 2. `extract_domain_name()` for structural domain resolution
pub fn build(wiki_dir: &Path) -> Result<FileIndex> {
    let notes = collect_all_notes(wiki_dir)?;
    let mut entries = HashMap::new();

    for note in &notes {
        for file in &note.related_files {
            // Normalize path separators to forward slash
            let normalized = file.replace('\\', "/");
            entries.insert(normalized, note.domain.clone());
        }
    }

    Ok(FileIndex {
        built_at: chrono::Utc::now().format("%Y-%m-%d").to_string(),
        entries,
    })
}

/// Load the file index from disk.
pub fn load(wiki_dir: &Path) -> Result<FileIndex> {
    let path = wiki_dir.join(FILE_INDEX_NAME);
    let content =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let index: FileIndex = serde_json::from_str(&content).context("Failed to parse file index")?;
    Ok(index)
}

/// Save the file index to disk.
pub fn save(wiki_dir: &Path, index: &FileIndex) -> Result<()> {
    let path = wiki_dir.join(FILE_INDEX_NAME);
    let json = serde_json::to_string_pretty(index).context("Failed to serialize file index")?;
    fs::write(&path, json).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Load the file index from cache, or rebuild if missing.
pub fn load_or_rebuild(wiki_dir: &Path) -> Result<FileIndex> {
    match load(wiki_dir) {
        Ok(index) => Ok(index),
        Err(_) => {
            let index = build(wiki_dir)?;
            // Best-effort save — don't fail if we can't write the cache
            let _ = save(wiki_dir, &index);
            Ok(index)
        }
    }
}

/// Resolve a source file path to a domain name.
///
/// Tries in order:
/// 1. Exact match in the file index (related_files from notes)
/// 2. Structural domain resolution via path components
pub fn resolve_domain(index: &FileIndex, file_path: &str, project_root: &Path) -> Option<String> {
    let normalized = file_path.replace('\\', "/");

    // Try exact match in the index
    if let Some(domain) = index.entries.get(&normalized) {
        return Some(domain.clone());
    }

    // Try structural resolution from path components
    let abs_path = if Path::new(file_path).is_absolute() {
        file_path.into()
    } else {
        project_root.join(file_path)
    };

    extract_domain_name(&abs_path, project_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn create_wiki_with_note(dir: &TempDir, domain: &str, related_files: &[&str]) {
        let domain_dir = dir.path().join(".wiki/domains").join(domain);
        fs::create_dir_all(&domain_dir).unwrap();

        let files_yaml: String = related_files
            .iter()
            .map(|f| format!("  - {}", f))
            .collect::<Vec<_>>()
            .join("\n");

        let content = format!(
            "---\ndomain: {}\nconfidence: confirmed\nlast_updated: \"2026-03-28\"\nrelated_files:\n{}\n---\n\n# {}\n\nTest content.\n",
            domain, files_yaml, domain
        );

        fs::write(domain_dir.join("_overview.md"), content).unwrap();
    }

    #[test]
    fn build_creates_index_from_related_files() {
        let dir = TempDir::new().unwrap();
        create_wiki_with_note(
            &dir,
            "billing",
            &[
                "src/services/billing/invoice.ts",
                "src/services/billing/payment.ts",
            ],
        );

        let wiki_dir = dir.path().join(".wiki");
        let index = build(&wiki_dir).unwrap();

        assert_eq!(index.entries.len(), 2);
        assert_eq!(
            index.entries.get("src/services/billing/invoice.ts"),
            Some(&"billing".to_string())
        );
        assert_eq!(
            index.entries.get("src/services/billing/payment.ts"),
            Some(&"billing".to_string())
        );
    }

    #[test]
    fn build_handles_empty_wiki() {
        let dir = TempDir::new().unwrap();
        let wiki_dir = dir.path().join(".wiki");
        fs::create_dir_all(wiki_dir.join("domains")).unwrap();

        let index = build(&wiki_dir).unwrap();
        assert!(index.entries.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let wiki_dir = dir.path().join(".wiki");
        fs::create_dir_all(&wiki_dir).unwrap();

        let mut entries = HashMap::new();
        entries.insert("src/billing/invoice.ts".to_string(), "billing".to_string());

        let index = FileIndex {
            built_at: "2026-03-28".to_string(),
            entries,
        };

        save(&wiki_dir, &index).unwrap();
        let loaded = load(&wiki_dir).unwrap();

        assert_eq!(loaded.built_at, "2026-03-28");
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(
            loaded.entries.get("src/billing/invoice.ts"),
            Some(&"billing".to_string())
        );
    }

    #[test]
    fn load_or_rebuild_creates_cache_when_missing() {
        let dir = TempDir::new().unwrap();
        create_wiki_with_note(&dir, "auth", &["src/auth/login.ts"]);

        let wiki_dir = dir.path().join(".wiki");
        let index = load_or_rebuild(&wiki_dir).unwrap();

        assert_eq!(
            index.entries.get("src/auth/login.ts"),
            Some(&"auth".to_string())
        );
        // Cache file should have been created
        assert!(wiki_dir.join(".file-index.json").exists());
    }

    #[test]
    fn resolve_domain_exact_match() {
        let entries =
            HashMap::from([("src/billing/invoice.ts".to_string(), "billing".to_string())]);
        let index = FileIndex {
            built_at: "2026-03-28".to_string(),
            entries,
        };

        let result = resolve_domain(&index, "src/billing/invoice.ts", Path::new("/project"));
        assert_eq!(result, Some("billing".to_string()));
    }

    #[test]
    fn resolve_domain_structural_fallback() {
        let index = FileIndex {
            built_at: "2026-03-28".to_string(),
            entries: HashMap::new(),
        };

        let project_root = PathBuf::from("/project");
        // extract_domain_name needs the file to be under a domain parent dir
        let result = resolve_domain(&index, "src/services/billing/invoice.ts", &project_root);
        assert_eq!(result, Some("billing".to_string()));
    }

    #[test]
    fn resolve_domain_no_match() {
        let index = FileIndex {
            built_at: "2026-03-28".to_string(),
            entries: HashMap::new(),
        };

        let result = resolve_domain(&index, "README.md", Path::new("/project"));
        assert_eq!(result, None);
    }

    #[test]
    fn resolve_domain_normalizes_backslashes() {
        let entries =
            HashMap::from([("src/billing/invoice.ts".to_string(), "billing".to_string())]);
        let index = FileIndex {
            built_at: "2026-03-28".to_string(),
            entries,
        };

        let result = resolve_domain(&index, "src\\billing\\invoice.ts", Path::new("/project"));
        assert_eq!(result, Some("billing".to_string()));
    }
}
