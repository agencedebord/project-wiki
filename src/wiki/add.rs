use std::fs;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use chrono::Utc;

use crate::ui;
use crate::wiki;
use crate::wiki::common::{capitalize, ensure_wiki_exists, list_domain_names};

const DOMAIN_OVERVIEW_TEMPLATE: &str = include_str!("../templates/domain_overview.md");
const DECISION_TEMPLATE: &str = include_str!("../templates/decision.md");

/// Normalize a domain name: lowercase, replace spaces and underscores with hyphens.
/// Also strips path separators and double dots for security.
fn normalize_domain_name(name: &str) -> String {
    let normalized = name.trim()
        .to_lowercase()
        .replace(' ', "-")
        .replace('_', "-");

    // Strip any path separators and dots for security
    normalized
        .replace('/', "")
        .replace('\\', "")
        .replace("..", "")
}

/// Generate a slug from text: lowercase, hyphens, max 50 chars.
fn slugify(text: &str, max_len: usize) -> String {
    let slug: String = text
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c
            } else {
                '-'
            }
        })
        .collect();

    // Collapse multiple hyphens
    let mut result = String::new();
    let mut prev_hyphen = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_hyphen && !result.is_empty() {
                result.push('-');
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }

    // Trim trailing hyphens and truncate
    let trimmed = result.trim_end_matches('-');
    if trimmed.len() > max_len {
        // Cut at max_len, but don't break mid-word if possible
        let cut = &trimmed[..max_len];
        cut.trim_end_matches('-').to_string()
    } else {
        trimmed.to_string()
    }
}

/// Alias for backward compatibility in this module.
fn list_domains(wiki_dir: &Path) -> Result<Vec<String>> {
    list_domain_names(wiki_dir)
}

// ─── Public commands ───

pub fn domain(name: &str) -> Result<()> {
    domain_in(Path::new(".wiki"), name)
}

pub fn context(text: &str, domain: Option<&str>) -> Result<()> {
    context_in(Path::new(".wiki"), text, domain)
}

pub fn decision(text: &str) -> Result<()> {
    decision_in(Path::new(".wiki"), text)
}

// ─── Internal implementations (testable with custom wiki dir) ───

fn domain_in(wiki_dir: &Path, name: &str) -> Result<()> {
    ensure_wiki_exists(wiki_dir)?;

    let normalized = normalize_domain_name(name);

    if normalized.is_empty() || normalized == "." || normalized.contains('/') || normalized.contains('\\') {
        bail!("Invalid domain name: \"{}\"", name);
    }

    let domain_dir = wiki_dir.join("domains").join(&normalized);

    if domain_dir.exists() {
        bail!("Domain \"{}\" already exists.", normalized);
    }

    fs::create_dir_all(&domain_dir)
        .with_context(|| format!("Failed to create domain directory: {}", domain_dir.display()))?;

    let date = Utc::now().format("%Y-%m-%d").to_string();
    let content = DOMAIN_OVERVIEW_TEMPLATE
        .replace("{domain}", &normalized)
        .replace("{Domain}", &capitalize(&normalized.replace('-', " ")))
        .replace("{date}", &date);

    let overview_path = domain_dir.join("_overview.md");
    fs::write(&overview_path, &content)
        .with_context(|| format!("Failed to write {}", overview_path.display()))?;

    // Regenerate index — domain was still created even if this fails
    if let Err(e) = wiki::index::run() {
        ui::warn(&format!("Failed to regenerate index: {}", e));
    }

    ui::success(&format!(
        "Domain \"{}\" created at {}",
        normalized,
        overview_path.display()
    ));

    Ok(())
}

fn context_in(wiki_dir: &Path, text: &str, domain_arg: Option<&str>) -> Result<()> {
    ensure_wiki_exists(wiki_dir)?;

    let target_domain = match domain_arg {
        Some(d) => normalize_domain_name(d),
        None => guess_domain(wiki_dir, text)?,
    };

    let domain_dir = wiki_dir.join("domains").join(&target_domain);
    if !domain_dir.exists() {
        let available = list_domains(wiki_dir)?;
        if available.is_empty() {
            bail!("No domains found. Create one first with `project-wiki add domain <name>`.");
        }
        bail!(
            "Domain \"{}\" not found. Available domains: {}. Use --domain to specify.",
            target_domain,
            available.join(", ")
        );
    }

    let overview_path = domain_dir.join("_overview.md");
    if !overview_path.exists() {
        bail!(
            "Overview file not found at {}",
            overview_path.display()
        );
    }

    let content = fs::read_to_string(&overview_path)
        .with_context(|| format!("Failed to read {}", overview_path.display()))?;

    let bullet = format!("- {} [confirmed]", text);
    let updated_content = append_to_section(&content, &bullet);

    // Update last_updated in front matter
    let date = Utc::now().format("%Y-%m-%d").to_string();
    let final_content = update_last_updated(&updated_content, &date);

    fs::write(&overview_path, &final_content)
        .with_context(|| format!("Failed to write {}", overview_path.display()))?;

    // Regenerate index
    if let Err(e) = wiki::index::run() {
        ui::warn(&format!("Failed to regenerate index: {}", e));
    }

    ui::success(&format!(
        "Added context to domain \"{}\":",
        target_domain
    ));
    ui::info(&format!("  {}", bullet));

    Ok(())
}

fn decision_in(wiki_dir: &Path, text: &str) -> Result<()> {
    ensure_wiki_exists(wiki_dir)?;

    let date = Utc::now().format("%Y-%m-%d").to_string();
    let slug = slugify(text, 50);
    let filename = format!("{}-{}.md", date, slug);

    let decisions_dir = wiki_dir.join("decisions");
    fs::create_dir_all(&decisions_dir)
        .context("Failed to create decisions directory")?;

    let file_path = decisions_dir.join(&filename);

    let content = DECISION_TEMPLATE
        .replace("{title}", text)
        .replace("{date}", &date)
        .replace("{domain}", "—");

    // Set confidence to confirmed (already in template)
    fs::write(&file_path, &content)
        .with_context(|| format!("Failed to write {}", file_path.display()))?;

    // Regenerate index
    if let Err(e) = wiki::index::run() {
        ui::warn(&format!("Failed to regenerate index: {}", e));
    }

    ui::success(&format!(
        "Decision created at {}",
        file_path.display()
    ));

    Ok(())
}

/// Try to guess the domain from the text by checking if any domain name appears in it.
fn guess_domain(wiki_dir: &Path, text: &str) -> Result<String> {
    let domains = list_domains(wiki_dir)?;

    if domains.is_empty() {
        bail!("No domains found. Create one first with `project-wiki add domain <name>`.");
    }

    let text_lower = text.to_lowercase();
    let matches: Vec<&String> = domains
        .iter()
        .filter(|d| text_lower.contains(&d.to_lowercase()))
        .collect();

    match matches.len() {
        1 => Ok(matches[0].clone()),
        0 => {
            bail!(
                "Could not guess domain from text. Available domains: {}. Use --domain to specify.",
                domains.join(", ")
            );
        }
        _ => {
            bail!(
                "Multiple domains matched: {}. Use --domain to specify.",
                matches.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            );
        }
    }
}

/// Append a bullet point to the "Key behaviors" or "Business rules" section.
fn append_to_section(content: &str, bullet: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::new();
    let mut inserted = false;

    // Try to find "Key behaviors" section first, then "Business rules"
    let target_sections = ["## Key behaviors", "## Business rules"];

    let mut target_line_idx: Option<usize> = None;
    for section in &target_sections {
        for (i, line) in lines.iter().enumerate() {
            if line.trim() == *section {
                target_line_idx = Some(i);
                break;
            }
        }
        if target_line_idx.is_some() {
            break;
        }
    }

    if let Some(section_idx) = target_line_idx {
        // Find the end of this section (next ## heading or end of file)
        let mut insert_at = lines.len();
        for i in (section_idx + 1)..lines.len() {
            if lines[i].starts_with("## ") {
                insert_at = i;
                break;
            }
        }

        // Insert the bullet before the next section (or at end)
        for (i, line) in lines.iter().enumerate() {
            if i == insert_at && !inserted {
                result.push(bullet.to_string());
                result.push(String::new());
                inserted = true;
            }
            result.push(line.to_string());
        }

        if !inserted {
            result.push(bullet.to_string());
        }
    } else {
        // No target section found — append at end
        for line in &lines {
            result.push(line.to_string());
        }
        result.push(String::new());
        result.push(bullet.to_string());
    }

    result.join("\n") + "\n"
}

/// Update the last_updated field in front matter.
fn update_last_updated(content: &str, date: &str) -> String {
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut in_frontmatter = false;
    let mut updated = false;

    for line in lines.iter_mut() {
        if line.trim() == "---" {
            if !in_frontmatter {
                in_frontmatter = true;
                continue;
            } else {
                break;
            }
        }
        if in_frontmatter && line.starts_with("last_updated:") {
            *line = format!("last_updated: {}", date);
            updated = true;
        }
    }

    if !updated {
        // If there's no last_updated field, we don't add one — keep content as-is
    }

    // Preserve trailing newline
    let joined = lines.join("\n");
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
        fs::create_dir_all(wiki.join("decisions")).unwrap();
        wiki
    }

    fn create_domain(dir: &TempDir, name: &str) {
        let domain_dir = dir.path().join(".wiki/domains").join(name);
        fs::create_dir_all(&domain_dir).unwrap();

        let date = Utc::now().format("%Y-%m-%d").to_string();
        let content = DOMAIN_OVERVIEW_TEMPLATE
            .replace("{domain}", name)
            .replace("{Domain}", &capitalize(&name.replace('-', " ")))
            .replace("{date}", &date);
        fs::write(domain_dir.join("_overview.md"), content).unwrap();
    }

    #[test]
    fn add_domain_creates_directory_and_overview() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        domain_in(&wiki, "billing").unwrap();

        let overview = wiki.join("domains/billing/_overview.md");
        assert!(overview.exists());

        let content = fs::read_to_string(&overview).unwrap();
        assert!(content.contains("domain: billing"));
        assert!(content.contains("confidence: inferred"));
    }

    #[test]
    fn add_domain_normalizes_name() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        domain_in(&wiki, "User Auth").unwrap();

        assert!(wiki.join("domains/user-auth/_overview.md").exists());

        // Also test underscores
        domain_in(&wiki, "payment_gateway").unwrap();
        assert!(wiki.join("domains/payment-gateway/_overview.md").exists());
    }

    #[test]
    fn add_domain_fails_if_already_exists() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        domain_in(&wiki, "billing").unwrap();
        let result = domain_in(&wiki, "billing");

        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("already exists"),
            "Error should mention domain already exists"
        );
    }

    #[test]
    fn add_context_appends_to_existing_domain() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_domain(&dir, "billing");

        context_in(&wiki, "Invoices must be paid within 30 days", Some("billing")).unwrap();

        let content = fs::read_to_string(wiki.join("domains/billing/_overview.md")).unwrap();
        assert!(content.contains("Invoices must be paid within 30 days [confirmed]"));
    }

    #[test]
    fn add_context_with_domain_flag() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_domain(&dir, "auth");

        context_in(&wiki, "Passwords must be at least 8 characters", Some("auth")).unwrap();

        let content = fs::read_to_string(wiki.join("domains/auth/_overview.md")).unwrap();
        assert!(content.contains("Passwords must be at least 8 characters [confirmed]"));
    }

    #[test]
    fn add_context_fails_if_domain_not_found() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);
        create_domain(&dir, "billing");

        let result = context_in(&wiki, "Some context", Some("nonexistent"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn add_decision_creates_file_with_date_prefix() {
        let dir = TempDir::new().unwrap();
        let wiki = setup_wiki(&dir);

        decision_in(&wiki, "Use Stripe for payment processing").unwrap();

        let decisions_dir = wiki.join("decisions");
        let entries: Vec<_> = fs::read_dir(&decisions_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "md"))
            .collect();

        assert_eq!(entries.len(), 1);

        let filename = entries[0].file_name().to_string_lossy().to_string();
        let date = Utc::now().format("%Y-%m-%d").to_string();
        assert!(
            filename.starts_with(&date),
            "Filename '{}' should start with date '{}'",
            filename,
            date
        );
        assert!(filename.contains("use-stripe"));
    }

    #[test]
    fn add_decision_generates_proper_slug() {
        assert_eq!(slugify("Use Stripe for payments", 50), "use-stripe-for-payments");
        assert_eq!(slugify("Don't use Redis!", 50), "don-t-use-redis");
        assert_eq!(
            slugify("A very long decision title that exceeds the maximum slug length allowed", 30),
            "a-very-long-decision-title-tha"
        );
    }

    #[test]
    fn normalize_domain_name_works() {
        assert_eq!(normalize_domain_name("Billing"), "billing");
        assert_eq!(normalize_domain_name("User Auth"), "user-auth");
        assert_eq!(normalize_domain_name("payment_gateway"), "payment-gateway");
        assert_eq!(normalize_domain_name("  spaces  "), "spaces");
    }

    #[test]
    fn append_to_section_inserts_in_key_behaviors() {
        let content = "---\ntitle: Test\n---\n\n# Test\n\n## Key behaviors\n_Placeholder._\n\n## Business rules\n_Placeholder._\n";
        let result = append_to_section(content, "- New behavior [confirmed]");
        assert!(result.contains("- New behavior [confirmed]"));
        // The bullet should appear before "## Business rules"
        let behavior_pos = result.find("- New behavior").unwrap();
        let rules_pos = result.find("## Business rules").unwrap();
        assert!(behavior_pos < rules_pos);
    }

    #[test]
    fn update_last_updated_replaces_date() {
        let content = "---\ndomain: test\nlast_updated: 2020-01-01\n---\nContent.\n";
        let result = update_last_updated(content, "2026-03-26");
        assert!(result.contains("last_updated: 2026-03-26"));
        assert!(!result.contains("2020-01-01"));
    }

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn slugify_never_panics(s in "\\PC*", max_len in 1usize..200) {
            let _ = slugify(&s, max_len);
        }

        #[test]
        fn slugify_output_is_ascii(s in "\\PC*") {
            let result = slugify(&s, 50);
            assert!(result.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
        }

        #[test]
        fn normalize_domain_is_idempotent(s in "[a-zA-Z0-9 _-]{1,50}") {
            let once = normalize_domain_name(&s);
            let twice = normalize_domain_name(&once);
            prop_assert_eq!(once, twice);
        }
    }
}
