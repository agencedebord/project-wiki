use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::wiki::common::find_wiki_root;
use crate::wiki::file_index;

const DRIFT_PENDING_FILE: &str = ".drift-pending";

/// Drift pending marker — written by git hooks, consumed by Claude Code hooks and `status`.
#[derive(Debug, Serialize, Deserialize)]
pub struct DriftPending {
    pub event: String,
    pub timestamp: String,
    pub domains: Vec<String>,
    pub untracked_files: Vec<String>,
    pub files_changed: usize,
}

/// Entry point for `codefidence git-hook --event <event>`.
pub fn run(
    event: &str,
    old_ref: Option<&str>,
    new_ref: Option<&str>,
    branch_flag: Option<u8>,
) -> Result<()> {
    // post-checkout fires on every file checkout too; only trigger on branch switches
    if event == "post-checkout" && branch_flag != Some(1) {
        return Ok(());
    }

    // Find the wiki — if it doesn't exist, nothing to do
    let wiki_dir = match find_wiki_root() {
        Ok(dir) => dir,
        Err(_) => return Ok(()),
    };

    let project_root = match wiki_dir.parent() {
        Some(root) => root.to_path_buf(),
        None => return Ok(()),
    };

    // Get changed files from git
    let changed_files = match get_changed_files(event, old_ref, new_ref) {
        Ok(files) => files,
        Err(_) => return Ok(()), // ORIG_HEAD missing or git error — silent skip
    };

    if changed_files.is_empty() {
        return Ok(());
    }

    // Load file index (build from notes if cache is missing)
    let index = match file_index::load_or_rebuild(&wiki_dir) {
        Ok(idx) => idx,
        Err(_) => return Ok(()), // Can't load/build index — silent skip
    };

    // Resolve each file to a domain or flag as untracked
    let mut domains = BTreeSet::new();
    let mut untracked_files = Vec::new();
    let mut relevant_count: usize = 0;

    for file in &changed_files {
        // Skip wiki files and common non-code files
        if should_skip(file) {
            continue;
        }

        relevant_count += 1;

        match file_index::resolve_domain(&index, file, &project_root) {
            Some(domain) => {
                domains.insert(domain);
            }
            None => {
                untracked_files.push(file.clone());
            }
        }
    }

    if domains.is_empty() && untracked_files.is_empty() {
        return Ok(());
    }

    let domains_vec: Vec<String> = domains.into_iter().collect();

    // Print warning to stderr
    print_warning(event, &domains_vec, &untracked_files);

    // Write (or merge into) .drift-pending
    write_drift_pending(
        &wiki_dir,
        event,
        &domains_vec,
        &untracked_files,
        relevant_count,
    )?;

    Ok(())
}

/// Read the drift-pending marker file, if it exists.
pub fn read_drift_pending(wiki_dir: &Path) -> Option<DriftPending> {
    let path = wiki_dir.join(DRIFT_PENDING_FILE);
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Delete the drift-pending marker file (consume it).
pub fn consume_drift_pending(wiki_dir: &Path) {
    let path = wiki_dir.join(DRIFT_PENDING_FILE);
    let _ = fs::remove_file(path);
}

// ─── Internals ───

/// Get the list of changed files from git based on the hook event type.
fn get_changed_files(
    event: &str,
    old_ref: Option<&str>,
    new_ref: Option<&str>,
) -> Result<Vec<String>> {
    let output = match event {
        "post-merge" => Command::new("git")
            .args([
                "diff-tree",
                "-r",
                "--name-only",
                "--no-commit-id",
                "ORIG_HEAD",
                "HEAD",
            ])
            .output()
            .context("Failed to run git diff-tree for post-merge")?,

        // post-rewrite is called by git after rebase or amend with $1 = "rebase" or "amend"
        "post-rewrite" => Command::new("git")
            .args(["diff", "--name-only", "ORIG_HEAD", "HEAD"])
            .output()
            .context("Failed to run git diff for post-rewrite")?,

        "post-checkout" => {
            let old = old_ref.unwrap_or("HEAD@{1}");
            let new = new_ref.unwrap_or("HEAD");
            Command::new("git")
                .args(["diff-tree", "-r", "--name-only", "--no-commit-id", old, new])
                .output()
                .context("Failed to run git diff-tree for post-checkout")?
        }

        // NOTE: HEAD~1 fails on the initial commit — this is caught below and silently skipped
        "post-commit" => Command::new("git")
            .args([
                "diff-tree",
                "-r",
                "--name-only",
                "--no-commit-id",
                "HEAD~1",
                "HEAD",
            ])
            .output()
            .context("Failed to run git diff-tree for post-commit")?,

        _ => return Ok(Vec::new()),
    };

    if !output.status.success() {
        anyhow::bail!("git command failed with status {}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let files: Vec<String> = stdout
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
        .collect();

    Ok(files)
}

/// Check if a file should be skipped (wiki files, lock files, binaries, etc.).
///
/// Git always outputs forward slashes in diff output, even on Windows.
fn should_skip(file: &str) -> bool {
    // Skip wiki files
    if file.starts_with(".wiki/") {
        return true;
    }

    // Skip binary and non-code files by extension
    let skip_extensions = [
        ".png", ".jpg", ".jpeg", ".gif", ".svg", ".ico", ".woff", ".woff2", ".ttf", ".eot", ".mp3",
        ".mp4", ".zip", ".tar", ".gz", ".pdf",
    ];

    let lower = file.to_lowercase();
    for ext in &skip_extensions {
        if lower.ends_with(ext) {
            return true;
        }
    }

    // Skip lock files by name (these change frequently but carry no wiki-relevant info)
    let skip_names = [
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "Cargo.lock",
        "Gemfile.lock",
        "poetry.lock",
        "composer.lock",
    ];

    let filename = Path::new(file)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    skip_names.contains(&filename)
}

/// Print a one-liner warning to stderr.
fn print_warning(event: &str, domains: &[String], untracked_files: &[String]) {
    if !domains.is_empty() {
        eprintln!(
            "[codefidence] \u{26a0} Wiki drift: {} domain(s) may need updating after {}: {}",
            domains.len(),
            event.replace("post-", ""),
            domains.join(", ")
        );
    }

    if !untracked_files.is_empty() {
        let display_count = untracked_files.len().min(5);
        let files_display: Vec<&str> = untracked_files
            .iter()
            .take(display_count)
            .map(|s| s.as_str())
            .collect();
        let suffix = if untracked_files.len() > display_count {
            format!(" (+{} more)", untracked_files.len() - display_count)
        } else {
            String::new()
        };
        eprintln!(
            "[codefidence] \u{26a0} {} file(s) not covered by wiki: {}{}",
            untracked_files.len(),
            files_display.join(", "),
            suffix
        );
    }

    eprintln!("[codefidence] Run: codefidence check-diff");
}

/// Write or merge drift-pending marker file.
fn write_drift_pending(
    wiki_dir: &Path,
    event: &str,
    domains: &[String],
    untracked_files: &[String],
    files_changed: usize,
) -> Result<()> {
    let path = wiki_dir.join(DRIFT_PENDING_FILE);

    // Merge with existing pending data if present
    let mut all_domains: BTreeSet<String> = domains.iter().cloned().collect();
    let mut all_untracked: BTreeSet<String> = untracked_files.iter().cloned().collect();
    let mut total_files = files_changed;

    if let Some(existing) = read_drift_pending(wiki_dir) {
        for d in existing.domains {
            all_domains.insert(d);
        }
        for f in existing.untracked_files {
            all_untracked.insert(f);
        }
        total_files += existing.files_changed;
    }

    let pending = DriftPending {
        event: event.to_string(),
        timestamp: Utc::now().to_rfc3339(),
        domains: all_domains.into_iter().collect(),
        untracked_files: all_untracked.into_iter().collect(),
        files_changed: total_files,
    };

    let json =
        serde_json::to_string_pretty(&pending).context("Failed to serialize drift pending")?;
    fs::write(&path, json).with_context(|| format!("Failed to write {}", path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn should_skip_wiki_files() {
        assert!(should_skip(".wiki/_index.md"));
        assert!(should_skip(".wiki/domains/billing/_overview.md"));
    }

    #[test]
    fn should_skip_binary_files() {
        assert!(should_skip("assets/logo.png"));
        assert!(should_skip("fonts/Inter.woff2"));
    }

    #[test]
    fn should_skip_lock_files() {
        assert!(should_skip("package-lock.json"));
        assert!(should_skip("Cargo.lock"));
        assert!(should_skip("deep/path/yarn.lock"));
    }

    #[test]
    fn should_not_skip_source_files() {
        assert!(!should_skip("src/billing/invoice.ts"));
        assert!(!should_skip("lib/auth/login.rs"));
        assert!(!should_skip("README.md"));
    }

    #[test]
    fn write_and_read_drift_pending() {
        let dir = TempDir::new().unwrap();
        let wiki_dir = dir.path().join(".wiki");
        fs::create_dir_all(&wiki_dir).unwrap();

        write_drift_pending(
            &wiki_dir,
            "post-merge",
            &["billing".to_string(), "auth".to_string()],
            &["src/new-module/handler.ts".to_string()],
            5,
        )
        .unwrap();

        let pending = read_drift_pending(&wiki_dir).unwrap();
        assert_eq!(pending.event, "post-merge");
        assert_eq!(pending.domains, vec!["auth", "billing"]); // sorted by BTreeSet
        assert_eq!(pending.untracked_files, vec!["src/new-module/handler.ts"]);
        assert_eq!(pending.files_changed, 5);
    }

    #[test]
    fn merge_drift_pending() {
        let dir = TempDir::new().unwrap();
        let wiki_dir = dir.path().join(".wiki");
        fs::create_dir_all(&wiki_dir).unwrap();

        // First write
        write_drift_pending(
            &wiki_dir,
            "post-merge",
            &["billing".to_string()],
            &["src/new.ts".to_string()],
            3,
        )
        .unwrap();

        // Second write — should merge
        write_drift_pending(
            &wiki_dir,
            "post-commit",
            &["auth".to_string(), "billing".to_string()],
            &["src/other.ts".to_string()],
            2,
        )
        .unwrap();

        let pending = read_drift_pending(&wiki_dir).unwrap();
        assert_eq!(pending.event, "post-commit"); // latest event
        assert_eq!(pending.domains, vec!["auth", "billing"]); // merged + deduped
        assert_eq!(pending.untracked_files, vec!["src/new.ts", "src/other.ts"]); // merged
        assert_eq!(pending.files_changed, 5); // summed
    }

    #[test]
    fn consume_drift_pending_removes_file() {
        let dir = TempDir::new().unwrap();
        let wiki_dir = dir.path().join(".wiki");
        fs::create_dir_all(&wiki_dir).unwrap();

        write_drift_pending(&wiki_dir, "post-merge", &["billing".to_string()], &[], 1).unwrap();
        assert!(read_drift_pending(&wiki_dir).is_some());

        consume_drift_pending(&wiki_dir);
        assert!(read_drift_pending(&wiki_dir).is_none());
    }

    #[test]
    fn read_drift_pending_returns_none_when_missing() {
        let dir = TempDir::new().unwrap();
        let wiki_dir = dir.path().join(".wiki");
        fs::create_dir_all(&wiki_dir).unwrap();

        assert!(read_drift_pending(&wiki_dir).is_none());
    }
}
