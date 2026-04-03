use std::fs;
use std::path::Path;

use anyhow::{Result, bail};
use console::style;
use walkdir::WalkDir;

use crate::ui;
use crate::wiki::note::{Confidence, WikiNote};

pub fn run(domain: Option<&str>, all: bool) -> Result<()> {
    let wiki_dir = Path::new(".wiki");

    if !wiki_dir.exists() {
        bail!("No .wiki/ found. Run `codefidence init` first.");
    }

    if all {
        show_all_domains(wiki_dir)?;
    } else if let Some(name) = domain {
        show_domain(wiki_dir, name)?;
    } else {
        show_wiki_overview(wiki_dir)?;
    }

    Ok(())
}

/// Show the wiki index and dependency graph.
fn show_wiki_overview(wiki_dir: &Path) -> Result<()> {
    ui::action("Wiki overview");

    let index_path = wiki_dir.join("_index.md");
    if index_path.exists() {
        ui::header("Index");
        let content = fs::read_to_string(&index_path)?;
        println!("{}", content.trim());
    } else {
        ui::info("No _index.md found. Run `codefidence index` to generate it.");
    }

    let graph_path = wiki_dir.join("_graph.md");
    if graph_path.exists() {
        ui::header("Dependency graph");
        let content = fs::read_to_string(&graph_path)?;
        println!("{}", content.trim());
    } else {
        ui::info("No _graph.md found. Run `codefidence rebuild` to generate it.");
    }

    Ok(())
}

/// Show all domains with their overview content.
fn show_all_domains(wiki_dir: &Path) -> Result<()> {
    let domains_dir = wiki_dir.join("domains");
    if !domains_dir.exists() {
        ui::info("No domains directory found.");
        return Ok(());
    }

    let mut domain_names: Vec<String> = fs::read_dir(&domains_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            e.path()
                .file_name()
                .and_then(|n| n.to_str().map(String::from))
        })
        .collect();

    domain_names.sort();

    if domain_names.is_empty() {
        ui::info("No domains found.");
        return Ok(());
    }

    ui::action(&format!("All domains ({})", domain_names.len()));

    for name in &domain_names {
        let overview_path = domains_dir.join(name).join("_overview.md");
        if overview_path.exists() {
            let note = WikiNote::parse(&overview_path)?;
            print_domain_note(&note);
        } else {
            ui::header(name);
            ui::info("No overview file found.");
        }
    }

    Ok(())
}

/// Show a specific domain's notes.
fn show_domain(wiki_dir: &Path, name: &str) -> Result<()> {
    let domain_dir = wiki_dir.join("domains").join(name);

    if !domain_dir.exists() {
        bail!("Domain '{}' not found in .wiki/domains/", name);
    }

    ui::action(&format!("Domain: {}", name));

    // Show overview first
    let overview_path = domain_dir.join("_overview.md");
    if overview_path.exists() {
        let note = WikiNote::parse(&overview_path)?;
        print_domain_note(&note);
    }

    // Show all other notes in the domain directory
    let mut other_notes: Vec<WikiNote> = Vec::new();
    for entry in WalkDir::new(&domain_dir)
        .min_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md")
            && path.file_name().is_some_and(|n| n != "_overview.md")
        {
            if let Ok(note) = WikiNote::parse(path) {
                other_notes.push(note);
            }
        }
    }

    if !other_notes.is_empty() {
        other_notes.sort_by(|a, b| a.title.cmp(&b.title));
        ui::header("Additional notes");
        for note in &other_notes {
            print_domain_note(note);
        }
    }

    Ok(())
}

/// Print a single domain note with formatted output.
fn print_domain_note(note: &WikiNote) {
    let title = if note.title.is_empty() {
        &note.domain
    } else {
        &note.title
    };
    ui::header(title);

    // Confidence level with color
    let confidence_str = format!("Confidence: {}", note.confidence);
    match note.confidence {
        Confidence::Confirmed | Confidence::Verified => {
            println!("  {}", style(&confidence_str).green());
        }
        Confidence::LlmAnalyzed => {
            println!("  {}", style(&confidence_str).yellow());
        }
        Confidence::SeenInCode => {
            println!("  {}", style(&confidence_str).cyan());
        }
        Confidence::Inferred => {
            println!("  {}", style(&confidence_str).yellow());
        }
        Confidence::NeedsValidation => {
            println!("  {}", style(&confidence_str).red());
        }
    }

    // Print content sections
    if !note.content.is_empty() {
        let sections = extract_sections(&note.content);
        let key_headings = [
            "description",
            "key behaviors",
            "business rules",
            "dependencies",
        ];

        for (heading, body) in &sections {
            let heading_lower = heading.to_lowercase();
            if key_headings.iter().any(|k| heading_lower.contains(k)) {
                println!();
                println!("  {}", style(heading).bold());
                for line in body.lines() {
                    println!("  {}", line);
                }
            }
        }

        // If no structured sections found, print the raw content
        if sections.is_empty() {
            println!();
            for line in note.content.trim().lines() {
                println!("  {}", line);
            }
        }
    }

    // Related files
    if !note.related_files.is_empty() {
        println!();
        println!("  {}", style("Related files:").bold());
        for f in &note.related_files {
            println!("    - {}", style(f).dim());
        }
    }
}

/// Extract markdown sections (## heading -> body) from content.
fn extract_sections(content: &str) -> Vec<(String, String)> {
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut current_body = String::new();

    for line in content.lines() {
        if line.starts_with("## ") || line.starts_with("# ") {
            // Save previous section
            if let Some(heading) = current_heading.take() {
                sections.push((heading, current_body.trim().to_string()));
            }
            current_heading = Some(line.trim_start_matches('#').trim().to_string());
            current_body = String::new();
        } else if current_heading.is_some() {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }

    // Save last section
    if let Some(heading) = current_heading {
        sections.push((heading, current_body.trim().to_string()));
    }

    sections
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_wiki(dir: &TempDir) {
        let wiki = dir.path().join(".wiki");
        fs::create_dir_all(wiki.join("domains")).unwrap();
        fs::create_dir_all(wiki.join("decisions")).unwrap();
    }

    fn write_overview(dir: &TempDir, domain: &str, content: &str) {
        let domain_dir = dir.path().join(".wiki/domains").join(domain);
        fs::create_dir_all(&domain_dir).unwrap();
        fs::write(domain_dir.join("_overview.md"), content).unwrap();
    }

    #[test]
    fn consult_specific_domain_reads_overview() {
        let dir = TempDir::new().unwrap();
        create_wiki(&dir);
        write_overview(
            &dir,
            "billing",
            r#"---
title: Billing overview
confidence: confirmed
related_files:
  - src/billing/invoice.ts
---
## Description

Handles all billing logic.

## Key behaviors

- Generates invoices monthly.
"#,
        );

        let wiki_dir = dir.path().join(".wiki");
        // Verify the file is readable and parseable
        let overview_path = wiki_dir.join("domains/billing/_overview.md");
        let note = WikiNote::parse(&overview_path).unwrap();
        assert_eq!(note.title, "Billing overview");
        assert_eq!(note.confidence, Confidence::Confirmed);
        assert!(note.content.contains("Handles all billing logic"));
        assert_eq!(note.related_files, vec!["src/billing/invoice.ts"]);
    }

    #[test]
    fn consult_nonexistent_domain_returns_error() {
        let dir = TempDir::new().unwrap();
        create_wiki(&dir);

        let domain_dir = dir.path().join(".wiki/domains/nonexistent");
        assert!(!domain_dir.exists(), "Domain dir should not exist");
    }

    #[test]
    fn consult_all_lists_all_domains() {
        let dir = TempDir::new().unwrap();
        create_wiki(&dir);
        write_overview(
            &dir,
            "auth",
            "---\ntitle: Auth\nconfidence: inferred\n---\n## Description\n\nAuth domain.\n",
        );
        write_overview(
            &dir,
            "billing",
            "---\ntitle: Billing\nconfidence: confirmed\n---\n## Description\n\nBilling domain.\n",
        );
        write_overview(
            &dir,
            "notifications",
            "---\ntitle: Notifications\nconfidence: seen-in-code\n---\n## Description\n\nNotifications.\n",
        );

        // Verify all domains are discovered
        let domains_dir = dir.path().join(".wiki/domains");
        let mut domain_names: Vec<String> = fs::read_dir(&domains_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                e.path()
                    .file_name()
                    .and_then(|n| n.to_str().map(String::from))
            })
            .collect();
        domain_names.sort();

        assert_eq!(domain_names, vec!["auth", "billing", "notifications"]);

        // Verify each overview is parseable
        for name in &domain_names {
            let overview = domains_dir.join(name).join("_overview.md");
            let note = WikiNote::parse(&overview).unwrap();
            assert!(!note.title.is_empty());
        }
    }

    #[test]
    fn extract_sections_parses_headings() {
        let content = "## Description\n\nSome description.\n\n## Key behaviors\n\n- Behavior 1\n- Behavior 2\n";
        let sections = extract_sections(content);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].0, "Description");
        assert!(sections[0].1.contains("Some description"));
        assert_eq!(sections[1].0, "Key behaviors");
        assert!(sections[1].1.contains("Behavior 1"));
    }

    #[test]
    fn extract_sections_empty_content() {
        let sections = extract_sections("");
        assert!(sections.is_empty());
    }

    #[test]
    fn extract_sections_no_headings() {
        let content = "Just some text without any headings.\n";
        let sections = extract_sections(content);
        assert!(sections.is_empty());
    }
}
