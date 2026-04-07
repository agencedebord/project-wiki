use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use chrono::Utc;
use regex::Regex;

use crate::graph_utils::transitive_reduce;
use crate::i18n::t;
use crate::ui;
use crate::wiki::common;
use crate::wiki::common::LINK_RE;
use crate::wiki::config;

static SECTION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^##\s+").unwrap());

/// Returns a sanitized Mermaid node ID safe for `graph LR`.
/// If the name contains only alphanumeric chars and underscores, return as-is.
/// Otherwise, replace hyphens (and other problematic chars) with underscores for the ID,
/// and use bracket notation so the original name is displayed as the label.
fn mermaid_node(name: &str) -> String {
    let is_safe = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if is_safe {
        name.to_string()
    } else {
        let sanitized: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!("{}[\"{}\"]", sanitized, name)
    }
}

/// Returns only the sanitized ID portion (no bracket label).
/// Use this for edges and style references after the node has already been declared.
fn mermaid_id(name: &str) -> String {
    let is_safe = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if is_safe {
        name.to_string()
    } else {
        name.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }
}

pub fn run() -> Result<()> {
    let wiki_dir = common::find_wiki_root()?;
    let wiki_config = config::load(&wiki_dir);
    let lang = &wiki_config.language;

    ui::action("Regenerating dependency graph");

    let domains_dir = wiki_dir.join("domains");
    if !domains_dir.exists() {
        ui::info("No domains directory found. Nothing to graph.");
        return Ok(());
    }

    // 1. Read all _overview.md files
    let mut dependency_map: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut all_domains: Vec<String> = Vec::new();

    let entries = fs::read_dir(&domains_dir).context("Failed to read .wiki/domains")?;
    for entry in entries.filter_map(|e| e.ok()) {
        if !entry.path().is_dir() {
            continue;
        }

        let domain_name = entry.file_name().to_string_lossy().to_string();

        all_domains.push(domain_name.clone());

        let overview_path = entry.path().join("_overview.md");
        if !overview_path.exists() {
            continue;
        }

        let content = fs::read_to_string(&overview_path)
            .with_context(|| format!("Failed to read {}", overview_path.display()))?;

        // 2. Parse the "## Dependencies" section
        let deps = parse_dependencies_section(&content);
        if !deps.is_empty() {
            dependency_map.insert(domain_name, deps);
        }
    }

    all_domains.sort();

    // 2b. Apply transitive reduction to simplify the graph
    let mut adj: HashMap<String, HashSet<String>> = HashMap::new();
    for (source, targets) in &dependency_map {
        let entry = adj.entry(source.clone()).or_default();
        for (target, _) in targets {
            entry.insert(target.clone());
        }
    }
    transitive_reduce(&mut adj);

    // Remove edges that were eliminated by transitive reduction
    for (source, targets) in dependency_map.iter_mut() {
        if let Some(reduced) = adj.get(source) {
            targets.retain(|(target, _)| reduced.contains(target));
        }
    }

    // 3. Count connections per domain for styling
    let mut connection_count: HashMap<String, usize> = HashMap::new();
    for domain in &all_domains {
        connection_count.insert(domain.clone(), 0);
    }
    for (source, targets) in &dependency_map {
        *connection_count.entry(source.clone()).or_default() += targets.len();
        for (target, _) in targets {
            *connection_count.entry(target.clone()).or_default() += 1;
        }
    }

    // 4. Generate the Mermaid graph
    let date = Utc::now().format("%Y-%m-%d").to_string();
    let mut mermaid_lines: Vec<String> = Vec::new();

    // Track which domains have been declared with their label (bracket notation)
    let mut declared: HashSet<String> = HashSet::new();

    let mut sorted_sources: Vec<&String> = dependency_map.keys().collect();
    sorted_sources.sort();

    for source in &sorted_sources {
        let targets = &dependency_map[*source];
        // Declare source node with label on first occurrence
        if declared.insert((*source).clone()) {
            mermaid_lines.push(format!("    {}", mermaid_node(source)));
        }
        for (target, label) in targets {
            // Declare target node with label on first occurrence
            if declared.insert(target.clone()) {
                mermaid_lines.push(format!("    {}", mermaid_node(target)));
            }
            if label.is_empty() {
                mermaid_lines.push(format!(
                    "    {} --> {}",
                    mermaid_id(source),
                    mermaid_id(target)
                ));
            } else {
                mermaid_lines.push(format!(
                    "    {} -->|{}| {}",
                    mermaid_id(source),
                    label,
                    mermaid_id(target)
                ));
            }
        }
    }

    // Add isolated domains (no deps, not depended upon)
    let connected: HashSet<&str> = dependency_map
        .iter()
        .flat_map(|(source, targets)| {
            let mut names = vec![source.as_str()];
            names.extend(targets.iter().map(|(t, _)| t.as_str()));
            names
        })
        .collect();

    for domain in &all_domains {
        if !connected.contains(domain.as_str()) {
            mermaid_lines.push(format!("    {}", mermaid_node(domain)));
        }
    }

    // Style highly-connected nodes
    let max_connections = connection_count.values().max().copied().unwrap_or(0);
    let threshold = if max_connections > 2 {
        max_connections / 2
    } else {
        2
    };

    let mut style_lines: Vec<String> = Vec::new();
    let mut sorted_domains: Vec<&String> = connection_count.keys().collect();
    sorted_domains.sort();
    for domain in &sorted_domains {
        let count = connection_count[*domain];
        if count >= threshold && count > 1 {
            style_lines.push(format!(
                "    style {} fill:#e74c3c,color:#fff",
                mermaid_id(domain)
            ));
        }
    }

    let graph_body = if mermaid_lines.is_empty() {
        "    %% No dependencies detected".to_string()
    } else {
        let mut all_lines = mermaid_lines;
        if !style_lines.is_empty() {
            all_lines.push(String::new()); // blank line before styles
            all_lines.extend(style_lines);
        }
        all_lines.join("\n")
    };

    let output = format!(
        "# {}\n\n\
         > {}\n\
         > Last regenerated: {}\n\n\
         ```mermaid\n\
         graph LR\n\
         {}\n\
         ```\n",
        t("dependency_graph", lang),
        t("auto_generated_graph", lang),
        date,
        graph_body
    );

    // 5. Write to .wiki/_graph.md
    let graph_path = wiki_dir.join("_graph.md");
    fs::write(&graph_path, &output)
        .with_context(|| format!("Failed to write {}", graph_path.display()))?;

    ui::success(&format!(
        "Graph regenerated with {} domain(s), {} edge(s).",
        all_domains.len(),
        dependency_map.values().map(|v| v.len()).sum::<usize>()
    ));

    Ok(())
}

/// Parse the "## Dependencies" / "## Dépendances" section from an overview file.
/// Looks for markdown links: `- [Label](path)` and extracts the target domain name.
/// Matches both English and French section headers.
fn parse_dependencies_section(content: &str) -> Vec<(String, String)> {
    let mut in_deps_section = false;
    let mut deps: Vec<(String, String)> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Detect section boundaries
        if SECTION_RE.is_match(trimmed) {
            let lower = trimmed.to_lowercase();
            if lower.contains("dependencies") || lower.contains("dépendances") {
                in_deps_section = true;
                continue;
            } else if in_deps_section {
                // Hit next section, stop
                break;
            }
        }

        if !in_deps_section {
            continue;
        }

        // Skip non-list items
        if !trimmed.starts_with('-') && !trimmed.starts_with('*') {
            continue;
        }

        // Extract link
        if let Some(cap) = LINK_RE.captures(trimmed) {
            let _label = cap[1].to_string();
            let path = &cap[2];

            // Extract domain name from the link path
            // Expected format: ../domain-name/_overview.md or similar
            if let Some(domain) = extract_domain_from_link(path) {
                // Try to extract a description after the link (e.g., "— imports from X")
                let after_link = &trimmed[cap.get(0).unwrap().end()..];
                let desc = after_link
                    .trim_start_matches([' ', '\u{2014}', '-'])
                    .trim()
                    .to_string();

                deps.push((domain, desc));
            }
        }
    }

    deps
}

/// Extract domain name from a link path like `../users/_overview.md`
fn extract_domain_from_link(path: &str) -> Option<String> {
    let components: Vec<&str> = path.split('/').collect();

    // Look for patterns like: ../domain/_overview.md or domains/domain/_overview.md
    for (i, component) in components.iter().enumerate() {
        if *component == ".." && i + 1 < components.len() {
            let candidate = components[i + 1];
            if !candidate.starts_with('_') && !candidate.is_empty() && candidate != ".." {
                return Some(candidate.to_string());
            }
        }
    }

    // Fallback: second-to-last component if last is a .md file
    if components.len() >= 2 {
        let last = components[components.len() - 1];
        if last.ends_with(".md") {
            let candidate = components[components.len() - 2];
            if !candidate.is_empty() && candidate != "." && candidate != ".." {
                return Some(candidate.to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── parse_dependencies_section ───

    #[test]
    fn parse_deps_with_links() {
        let content = "\
# Overview

Some intro text.

## Dependencies

- [Users](../users/_overview.md) — shared auth
- [Billing](../billing/_overview.md)

## Notes

Other stuff.
";
        let deps = parse_dependencies_section(content);
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].0, "users");
        assert_eq!(deps[0].1, "shared auth");
        assert_eq!(deps[1].0, "billing");
        assert_eq!(deps[1].1, "");
    }

    #[test]
    fn parse_deps_no_dependencies_section() {
        let content = "\
# Overview

Some intro text.

## Notes

Nothing here about deps.
";
        let deps = parse_dependencies_section(content);
        assert!(deps.is_empty());
    }

    #[test]
    fn parse_deps_empty_section() {
        let content = "\
## Dependencies

## Next section
";
        let deps = parse_dependencies_section(content);
        assert!(deps.is_empty());
    }

    // ─── extract_domain_from_link ───

    #[test]
    fn extract_domain_from_relative_link() {
        assert_eq!(
            extract_domain_from_link("../users/_overview.md"),
            Some("users".to_string())
        );
    }

    #[test]
    fn extract_domain_from_domains_path() {
        assert_eq!(
            extract_domain_from_link("domains/billing/_overview.md"),
            Some("billing".to_string())
        );
    }

    #[test]
    fn extract_domain_from_deep_path() {
        assert_eq!(
            extract_domain_from_link("../../billing/_overview.md"),
            Some("billing".to_string())
        );
    }

    #[test]
    fn extract_domain_returns_none_for_bare_file() {
        // Single component, no directory
        assert_eq!(extract_domain_from_link("readme.md"), None);
    }

    // ─── mermaid_node ───

    #[test]
    fn mermaid_node_safe_name() {
        assert_eq!(mermaid_node("billing"), "billing");
        assert_eq!(mermaid_node("user_auth"), "user_auth");
    }

    #[test]
    fn mermaid_node_hyphenated_name() {
        let result = mermaid_node("user-auth");
        assert_eq!(result, "user_auth[\"user-auth\"]");
    }

    // ─── mermaid_id ───

    #[test]
    fn mermaid_id_safe_name() {
        assert_eq!(mermaid_id("billing"), "billing");
    }

    #[test]
    fn mermaid_id_hyphenated_name() {
        assert_eq!(mermaid_id("user-auth"), "user_auth");
    }

    #[test]
    fn mermaid_id_preserves_underscores() {
        assert_eq!(mermaid_id("my_module"), "my_module");
    }

    #[test]
    fn parse_deps_french_section_header() {
        let content = "\
# Overview

## Dépendances

- [Users](../users/_overview.md) — authentification partagée
- [Billing](../billing/_overview.md)

## Notes du code

Autres choses.
";
        let deps = parse_dependencies_section(content);
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].0, "users");
        assert_eq!(deps[0].1, "authentification partagée");
        assert_eq!(deps[1].0, "billing");
    }
}
