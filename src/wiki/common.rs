use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{bail, Context, Result};
use regex::Regex;
use walkdir::WalkDir;

use crate::wiki::note::WikiNote;

/// Regex for markdown links: `[text](target)`.
pub static LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]]*)\]\(([^)]+)\)").unwrap());

/// Directories whose immediate children are considered domain candidates.
pub const DOMAIN_PARENT_DIRS: &[&str] = &[
    "services", "modules", "features", "app", "lib", "packages", "controllers", "routes",
    "models", "api", "components", "handlers", "domains", "core", "plugins", "apps",
];

/// Resolve the wiki root directory. Searches from the current directory upward.
pub fn find_wiki_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    let mut dir = cwd.as_path();

    loop {
        let candidate = dir.join(".wiki");
        if candidate.is_dir() {
            return Ok(candidate);
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => bail!("No .wiki/ found. Run `project-wiki init` first."),
        }
    }
}

/// Capitalize the first letter of a string.
pub fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

/// Collect all wiki notes from the `domains/` subdirectory of a wiki dir.
pub fn collect_all_notes(wiki_dir: &Path) -> Result<Vec<WikiNote>> {
    let domains_dir = wiki_dir.join("domains");
    if !domains_dir.exists() {
        return Ok(Vec::new());
    }

    let mut notes = Vec::new();

    for entry in WalkDir::new(&domains_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "md") {
            if let Ok(note) = WikiNote::parse(path) {
                notes.push(note);
            }
        }
    }

    Ok(notes)
}

/// List all domain names from the `domains/` subdirectory of a wiki dir.
pub fn list_domain_names(wiki_dir: &Path) -> Result<Vec<String>> {
    let domains_dir = wiki_dir.join("domains");
    if !domains_dir.exists() {
        return Ok(Vec::new());
    }

    let mut names: Vec<String> = fs::read_dir(&domains_dir)
        .context("Failed to read domains directory")?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            e.path()
                .file_name()
                .and_then(|n| n.to_str().map(String::from))
        })
        .filter(|name| name != ".gitkeep")
        .collect();

    names.sort();
    Ok(names)
}

/// Check that a `.wiki/` directory exists, bail otherwise.
pub fn ensure_wiki_exists(wiki_dir: &Path) -> Result<()> {
    if !wiki_dir.exists() {
        bail!("No .wiki/ found. Run `project-wiki init` first.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn find_wiki_root_in_current_dir() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".wiki")).unwrap();

        // Temporarily change to the temp dir
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let result = find_wiki_root();
        std::env::set_current_dir(&original).unwrap();

        assert!(result.is_ok());
        assert!(result.unwrap().ends_with(".wiki"));
    }

    #[test]
    fn find_wiki_root_in_parent_dir() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".wiki")).unwrap();
        let subdir = dir.path().join("src").join("deep");
        std::fs::create_dir_all(&subdir).unwrap();

        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(&subdir).unwrap();

        let result = find_wiki_root();
        std::env::set_current_dir(&original).unwrap();

        assert!(result.is_ok());
        assert!(result.unwrap().ends_with(".wiki"));
    }

    #[test]
    fn capitalize_works() {
        assert_eq!(capitalize("hello"), "Hello");
        assert_eq!(capitalize(""), "");
        assert_eq!(capitalize("a"), "A");
    }

    #[test]
    fn list_domain_names_works() {
        let dir = TempDir::new().unwrap();
        let wiki = dir.path().join(".wiki");
        fs::create_dir_all(wiki.join("domains/alpha")).unwrap();
        fs::create_dir_all(wiki.join("domains/beta")).unwrap();

        let names = list_domain_names(&wiki).unwrap();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn collect_all_notes_empty_wiki() {
        let dir = TempDir::new().unwrap();
        let wiki = dir.path().join(".wiki");
        fs::create_dir_all(&wiki).unwrap();

        let notes = collect_all_notes(&wiki).unwrap();
        assert!(notes.is_empty());
    }

    #[test]
    fn ensure_wiki_exists_fails_when_missing() {
        let dir = TempDir::new().unwrap();
        let wiki = dir.path().join(".wiki");
        assert!(ensure_wiki_exists(&wiki).is_err());
    }

    #[test]
    fn ensure_wiki_exists_succeeds() {
        let dir = TempDir::new().unwrap();
        let wiki = dir.path().join(".wiki");
        fs::create_dir_all(&wiki).unwrap();
        assert!(ensure_wiki_exists(&wiki).is_ok());
    }
}
