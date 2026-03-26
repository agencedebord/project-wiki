use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::ui;

#[derive(Debug, Serialize, Deserialize)]
struct VectorIndex {
    version: u32,
    entries: HashMap<String, String>, // path -> content hash
}

/// Generate or update vector embeddings for all wiki notes.
pub fn index(wiki_dir: &Path) -> Result<()> {
    if !wiki_dir.exists() {
        bail!("No .wiki/ found. Run `project-wiki init` first.");
    }

    ui::coming_soon("vectorization");
    ui::info("Vector indexing will be available in v2.0");

    // Prepare the .vectors directory for future use
    let vectors_dir = wiki_dir.join(".vectors");
    fs::create_dir_all(&vectors_dir)
        .context("Failed to create .vectors directory")?;

    // Walk all notes and build a hash index (placeholder)
    let domains_dir = wiki_dir.join("domains");
    let mut entries = HashMap::new();

    if domains_dir.exists() {
        for entry in WalkDir::new(&domains_dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "md") {
                if let Ok(content) = fs::read_to_string(path) {
                    let hash = simple_hash(&content);
                    let rel_path = path
                        .strip_prefix(wiki_dir)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .to_string();
                    entries.insert(rel_path, hash);
                }
            }
        }
    }

    // Save placeholder index
    let index = VectorIndex {
        version: 1,
        entries,
    };

    let index_path = vectors_dir.join("index.json");
    let json = serde_json::to_string_pretty(&index)
        .context("Failed to serialize vector index")?;
    fs::write(&index_path, json)
        .with_context(|| format!("Failed to write {}", index_path.display()))?;

    // Create empty embeddings placeholder
    let embeddings_path = vectors_dir.join("embeddings.json");
    if !embeddings_path.exists() {
        fs::write(&embeddings_path, "[]")
            .with_context(|| format!("Failed to write {}", embeddings_path.display()))?;
    }

    ui::info(&format!(
        "Indexed {} note(s). Embeddings will be generated in v2.0.",
        index.entries.len()
    ));

    Ok(())
}

/// Semantic search across wiki notes (placeholder: falls back to keyword search).
pub fn search(_wiki_dir: &Path, _query: &str, _top_k: usize) -> Result<Vec<String>> {
    // Placeholder: will use vector similarity in v2.0
    Ok(Vec::new())
}

/// Simple hash function for content change detection.
fn simple_hash(content: &str) -> String {
    // Use a basic checksum approach (not cryptographic, just for change detection)
    let mut hash: u64 = 0;
    for byte in content.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
    }
    format!("{:016x}", hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn simple_hash_deterministic() {
        let h1 = simple_hash("hello world");
        let h2 = simple_hash("hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn simple_hash_different_for_different_content() {
        let h1 = simple_hash("hello");
        let h2 = simple_hash("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn index_creates_vectors_directory() {
        let dir = TempDir::new().unwrap();
        let wiki_dir = dir.path().join(".wiki");
        fs::create_dir_all(wiki_dir.join("domains")).unwrap();

        index(&wiki_dir).unwrap();

        assert!(wiki_dir.join(".vectors").is_dir());
        assert!(wiki_dir.join(".vectors/index.json").exists());
        assert!(wiki_dir.join(".vectors/embeddings.json").exists());
    }

    #[test]
    fn index_catalogs_notes() {
        let dir = TempDir::new().unwrap();
        let wiki_dir = dir.path().join(".wiki");
        let domain_dir = wiki_dir.join("domains/billing");
        fs::create_dir_all(&domain_dir).unwrap();
        fs::write(
            domain_dir.join("_overview.md"),
            "---\ntitle: Billing\n---\nBilling content.\n",
        )
        .unwrap();

        index(&wiki_dir).unwrap();

        let index_content = fs::read_to_string(wiki_dir.join(".vectors/index.json")).unwrap();
        let parsed: VectorIndex = serde_json::from_str(&index_content).unwrap();
        assert_eq!(parsed.entries.len(), 1);
        assert!(parsed.entries.contains_key("domains/billing/_overview.md"));
    }

    #[test]
    fn index_fails_without_wiki() {
        let dir = TempDir::new().unwrap();
        let wiki_dir = dir.path().join(".wiki");

        let result = index(&wiki_dir);
        assert!(result.is_err());
    }

    #[test]
    fn search_returns_empty_placeholder() {
        let dir = TempDir::new().unwrap();
        let wiki_dir = dir.path().join(".wiki");
        fs::create_dir_all(&wiki_dir).unwrap();

        let results = search(&wiki_dir, "test query", 5).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn vector_index_serialization_roundtrip() {
        let mut entries = HashMap::new();
        entries.insert("domains/auth/_overview.md".to_string(), "abc123".to_string());

        let index = VectorIndex {
            version: 1,
            entries,
        };

        let json = serde_json::to_string(&index).unwrap();
        let parsed: VectorIndex = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(
            parsed.entries.get("domains/auth/_overview.md"),
            Some(&"abc123".to_string())
        );
    }
}
