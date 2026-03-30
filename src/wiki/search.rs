use anyhow::Result;
use console::style;
use walkdir::WalkDir;

use crate::ui;
use crate::wiki::common;

pub fn run(term: &str) -> Result<()> {
    let wiki_dir = common::find_wiki_root()?;

    ui::action(&format!("Searching for \"{}\"", term));

    let term_lower = term.to_lowercase();
    let mut total_matches: usize = 0;

    // Search all .md files in the entire .wiki/ directory
    for entry in WalkDir::new(wiki_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "md") {
            continue;
        }

        // Skip template files
        if path.to_string_lossy().contains("_templates") {
            continue;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let lines: Vec<&str> = content.lines().collect();
        let mut file_matches: Vec<(usize, &str)> = Vec::new();

        for (i, line) in lines.iter().enumerate() {
            if line.to_lowercase().contains(&term_lower) {
                file_matches.push((i, line));
            }
        }

        if file_matches.is_empty() {
            continue;
        }

        total_matches += file_matches.len();

        // Print file path
        let display_path = path.strip_prefix(".").unwrap_or(path);
        println!();
        println!("  {}", style(display_path.display()).bold().underlined());

        for (line_idx, _matched_line) in &file_matches {
            let idx = *line_idx;

            // Context: 1 line before
            if idx > 0 {
                println!(
                    "  {} {}",
                    style(format!("{:>4}:", idx)).dim(),
                    style(lines[idx - 1]).dim()
                );
            }

            // Matching line with highlighted term
            let highlighted = highlight_term(lines[idx], &term_lower, term);
            println!(
                "  {} {}",
                style(format!("{:>4}:", idx + 1)).dim(),
                highlighted
            );

            // Context: 1 line after
            if idx + 1 < lines.len() {
                println!(
                    "  {} {}",
                    style(format!("{:>4}:", idx + 2)).dim(),
                    style(lines[idx + 1]).dim()
                );
            }
        }
    }

    println!();
    if total_matches == 0 {
        ui::info(&format!("No matches found for \"{}\".", term));
    } else {
        ui::success(&format!(
            "Found {} match{}.",
            total_matches,
            if total_matches == 1 { "" } else { "es" }
        ));
    }

    Ok(())
}

/// Highlight all occurrences of `term` in `line` (case-insensitive).
///
/// Uses a fast byte-offset path when lowercasing preserves byte length,
/// and falls back to a safe char-by-char approach when case-mapping changes
/// byte length (e.g. German ß → SS, Turkish İ → i̇).
fn highlight_term(line: &str, term_lower: &str, _original_term: &str) -> String {
    let line_lower = line.to_lowercase();

    // Fast path: byte lengths match, so byte offsets from the lowercased
    // string are valid indices into the original string.
    if line.len() == line_lower.len() {
        let mut result = String::new();
        let mut last_end = 0;

        for (start, _) in line_lower.match_indices(term_lower) {
            let end = start + term_lower.len();
            result.push_str(&line[last_end..start]);
            result.push_str(&format!("{}", style(&line[start..end]).bold().cyan()));
            last_end = end;
        }
        result.push_str(&line[last_end..]);
        return result;
    }

    // Slow path: build a mapping from each byte offset in the lowercased
    // string back to the corresponding byte offset in the original string.
    // We walk both strings char-by-char in lockstep.
    let mut lower_to_orig: Vec<usize> = Vec::with_capacity(line_lower.len() + 1);
    let mut orig_byte = 0;
    for ch in line.chars() {
        let lower_chars_len: usize = ch.to_lowercase().map(|c| c.len_utf8()).sum();
        for _ in 0..lower_chars_len {
            lower_to_orig.push(orig_byte);
        }
        orig_byte += ch.len_utf8();
    }
    // Sentinel: map the end-of-string position.
    lower_to_orig.push(orig_byte);

    let mut result = String::new();
    let mut last_orig_end = 0;

    for (lower_start, _) in line_lower.match_indices(term_lower) {
        let lower_end = lower_start + term_lower.len();
        let orig_start = lower_to_orig[lower_start];
        let orig_end = lower_to_orig[lower_end];

        result.push_str(&line[last_orig_end..orig_start]);
        result.push_str(&format!(
            "{}",
            style(&line[orig_start..orig_end]).bold().cyan()
        ));
        last_orig_end = orig_end;
    }
    result.push_str(&line[last_orig_end..]);
    result
}

/// Testable search logic that works with a custom wiki root directory.
#[cfg(test)]
fn search_in_dir(wiki_dir: &std::path::Path, term: &str) -> Result<Vec<SearchResult>> {
    let term_lower = term.to_lowercase();
    let mut results: Vec<SearchResult> = Vec::new();

    for entry in WalkDir::new(wiki_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.extension().is_some_and(|ext| ext == "md") {
            continue;
        }

        if path.to_string_lossy().contains("_templates") {
            continue;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        for line in content.lines() {
            if line.to_lowercase().contains(&term_lower) {
                results.push(SearchResult {
                    path: path.to_string_lossy().to_string(),
                    line: line.to_string(),
                });
            }
        }
    }

    Ok(results)
}

#[cfg(test)]
#[derive(Debug)]
struct SearchResult {
    path: String,
    line: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_wiki(dir: &TempDir) {
        let wiki = dir.path().join(".wiki");
        fs::create_dir_all(wiki.join("domains")).unwrap();
        fs::create_dir_all(wiki.join("decisions")).unwrap();
    }

    fn write_domain_note(dir: &TempDir, domain: &str, filename: &str, content: &str) {
        let domain_dir = dir.path().join(".wiki/domains").join(domain);
        fs::create_dir_all(&domain_dir).unwrap();
        fs::write(domain_dir.join(filename), content).unwrap();
    }

    fn write_decision(dir: &TempDir, filename: &str, content: &str) {
        let decisions_dir = dir.path().join(".wiki/decisions");
        fs::create_dir_all(&decisions_dir).unwrap();
        fs::write(decisions_dir.join(filename), content).unwrap();
    }

    #[test]
    fn finds_term_in_wiki_note() {
        let dir = TempDir::new().unwrap();
        setup_wiki(&dir);
        write_domain_note(
            &dir,
            "billing",
            "_overview.md",
            "# Billing\n\nHandles invoice processing and payments.\n",
        );

        let wiki_dir = dir.path().join(".wiki");
        let results = search_in_dir(&wiki_dir, "invoice").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].line.contains("invoice"));
    }

    #[test]
    fn case_insensitive_search() {
        let dir = TempDir::new().unwrap();
        setup_wiki(&dir);
        write_domain_note(
            &dir,
            "auth",
            "_overview.md",
            "# Auth\n\nHandles Authentication and authorization.\n",
        );

        let wiki_dir = dir.path().join(".wiki");
        let results = search_in_dir(&wiki_dir, "authentication").unwrap();
        assert_eq!(results.len(), 1);

        // Also search with different case
        let results_upper = search_in_dir(&wiki_dir, "AUTHENTICATION").unwrap();
        assert_eq!(results_upper.len(), 1);
    }

    #[test]
    fn no_results_returns_empty() {
        let dir = TempDir::new().unwrap();
        setup_wiki(&dir);
        write_domain_note(
            &dir,
            "billing",
            "_overview.md",
            "# Billing\n\nHandles payments.\n",
        );

        let wiki_dir = dir.path().join(".wiki");
        let results = search_in_dir(&wiki_dir, "nonexistent-term-xyz").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_in_decisions_too() {
        let dir = TempDir::new().unwrap();
        setup_wiki(&dir);
        write_decision(
            &dir,
            "2026-03-26-use-stripe.md",
            "# Use Stripe for payments\n\nWe decided to use Stripe.\n",
        );

        let wiki_dir = dir.path().join(".wiki");
        let results = search_in_dir(&wiki_dir, "Stripe").unwrap();
        assert_eq!(results.len(), 2); // appears in title and body
        assert!(results[0].path.contains("decisions"));
    }

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn highlight_term_never_panics(
            line in "\\PC{0,200}",
            term in "\\PC{1,50}"
        ) {
            let term_lower = term.to_lowercase();
            if !term_lower.is_empty() {
                let _ = highlight_term(&line, &term_lower, &term);
            }
        }
    }
}
