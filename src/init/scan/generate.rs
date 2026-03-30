use std::collections::HashSet;

use crate::wiki::common::capitalize;

use super::DomainInfo;

/// Generate a markdown overview for a domain.
pub fn generate_domain_overview(domain: &DomainInfo, all_domains: &[DomainInfo]) -> String {
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let title = domain
        .name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string() + &domain.name[1..])
        .unwrap_or_default();

    let related_files_section = format_related_files(&domain.files);

    let mut sections = Vec::new();

    // Front matter
    sections.push(format!(
        "---\ndomain: {}\nconfidence: inferred\nlast_updated: {}\n{}\n---",
        domain.name, date, related_files_section
    ));

    // Title and description
    sections.push(format!(
        "# {}\n\n## Description\n_Auto-generated from codebase scan. Needs human review._ `[inferred]`",
        title
    ));

    // Key behaviors
    sections.push("## Key behaviors\n_To be documented._".to_string());

    // Data models
    sections.push(format_list_section("Data models", &domain.models, |m| {
        format!("- {} `[seen-in-code]`", m)
    }));

    // API routes
    sections.push(format_list_section("API routes", &domain.routes, |r| {
        format!("- {} `[seen-in-code]`", r)
    }));

    // Dependencies
    if domain.dependencies.is_empty() {
        sections.push("## Dependencies\n_None detected._".to_string());
    } else {
        let deps_list: String = domain
            .dependencies
            .iter()
            .map(|d| {
                format!(
                    "- [{}](../{d}/_overview.md) — imports from {} module",
                    capitalize(d),
                    d,
                    d = d
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("## Dependencies\n{}", deps_list));
    }

    // Referenced by
    let referenced_by: Vec<&DomainInfo> = all_domains
        .iter()
        .filter(|d| d.name != domain.name && d.dependencies.contains(&domain.name))
        .collect();

    if referenced_by.is_empty() {
        sections.push("## Referenced by\n_None detected._".to_string());
    } else {
        let refs_list: String = referenced_by
            .iter()
            .map(|d| {
                format!(
                    "- [{}](../{d}/_overview.md)",
                    capitalize(&d.name),
                    d = d.name
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("## Referenced by\n{}", refs_list));
    }

    // Test coverage
    sections.push(format_list_section(
        "Test coverage",
        &domain.test_files,
        |t| format!("- {}", t),
    ));

    sections.join("\n\n")
}

/// Generate a Mermaid dependency graph.
pub fn generate_graph(domains: &[DomainInfo]) -> String {
    let mut mermaid_lines = Vec::new();

    for domain in domains {
        for dep in &domain.dependencies {
            mermaid_lines.push(format!("    {} --> {}", domain.name, dep));
        }
    }

    // Also add isolated domains (no deps, no reverse deps)
    let connected: HashSet<&str> = domains
        .iter()
        .flat_map(|d| {
            let mut names = vec![d.name.as_str()];
            names.extend(d.dependencies.iter().map(|s| s.as_str()));
            names
        })
        .filter(|name| {
            domains.iter().any(|d| {
                d.name == *name
                    && (!d.dependencies.is_empty()
                        || domains
                            .iter()
                            .any(|other| other.dependencies.contains(&d.name)))
            })
        })
        .collect();

    for domain in domains {
        if !connected.contains(domain.name.as_str()) && domain.dependencies.is_empty() {
            mermaid_lines.push(format!("    {}", domain.name));
        }
    }

    let graph_body = if mermaid_lines.is_empty() {
        "    %% No dependencies detected".to_string()
    } else {
        mermaid_lines.join("\n")
    };

    format!(
        "# Domain dependency graph\n\n\
         > Auto-generated from codebase scan. Do not edit manually.\n\n\
         ```mermaid\n\
         graph LR\n\
         {}\n\
         ```\n",
        graph_body
    )
}

/// Generate the _index.md content.
pub fn generate_index(domains: &[DomainInfo], date: &str) -> String {
    let domain_list = if domains.is_empty() {
        "_No domains documented yet._".to_string()
    } else {
        domains
            .iter()
            .map(|d| {
                format!(
                    "- [{}](domains/{d}/_overview.md) — {} files, {} models `[inferred]`",
                    capitalize(&d.name),
                    d.files.len(),
                    d.models.len(),
                    d = d.name,
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "# Project Wiki\n\n\
         > Auto-generated knowledge base. Managed by [project-wiki](https://github.com/agencedebord/project-wiki).\n\n\
         ## Domains\n\n\
         {}\n\n\
         ## Recent decisions\n\n\
         | Date | Decision | Domain |\n\
         |------|----------|--------|\n\n\
         ## Last updated\n\n\
         - Initialized on {}\n",
        domain_list, date
    )
}

/// Generate the _needs-review.md content.
pub fn generate_needs_review(domains: &[DomainInfo]) -> String {
    let mut questions: Vec<String> = Vec::new();

    for domain in domains {
        for comment in &domain.comments {
            questions.push(format!("- **{}**: {}", capitalize(&domain.name), comment));
        }
    }

    let questions_section = if questions.is_empty() {
        "_No open questions found._".to_string()
    } else {
        questions.join("\n")
    };

    format!(
        "# Needs review\n\n\
         > Items below were generated automatically and need human validation.\n\
         > Answer or validate each item, then remove it from this list.\n\n\
         ## Open questions\n\n\
         {}\n\n\
         ## Unresolved contradictions\n\n\
         _None detected._\n",
        questions_section
    )
}

// ─── Helpers ───

fn format_related_files(files: &[String]) -> String {
    if files.is_empty() {
        "related_files: []".to_string()
    } else {
        let yaml: String = files
            .iter()
            .map(|f| format!("  - {}", f))
            .collect::<Vec<_>>()
            .join("\n");
        format!("related_files:\n{}", yaml)
    }
}

fn format_list_section<F>(title: &str, items: &[String], formatter: F) -> String
where
    F: Fn(&str) -> String,
{
    if items.is_empty() {
        format!("## {}\n_None detected._", title)
    } else {
        let list: String = items
            .iter()
            .map(|i| formatter(i))
            .collect::<Vec<_>>()
            .join("\n");
        format!("## {}\n{}", title, list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_overview_contains_domain_name() {
        let domain = DomainInfo {
            name: "billing".to_string(),
            files: vec!["src/billing/invoice.ts".to_string()],
            dependencies: vec![],
            models: vec!["Invoice".to_string()],
            routes: vec![],
            comments: vec![],
            test_files: vec![],
        };

        let overview = generate_domain_overview(&domain, std::slice::from_ref(&domain));
        assert!(overview.contains("Billing"));
        assert!(overview.contains("Invoice"));
        assert!(overview.contains("inferred"));
    }

    #[test]
    fn generate_graph_with_no_deps() {
        let domains = vec![DomainInfo {
            name: "billing".to_string(),
            files: vec![],
            dependencies: vec![],
            models: vec![],
            routes: vec![],
            comments: vec![],
            test_files: vec![],
        }];

        let graph = generate_graph(&domains);
        assert!(graph.contains("billing"));
        assert!(graph.contains("mermaid"));
    }

    #[test]
    fn generate_graph_with_deps() {
        let domains = vec![
            DomainInfo {
                name: "billing".to_string(),
                files: vec![],
                dependencies: vec!["users".to_string()],
                models: vec![],
                routes: vec![],
                comments: vec![],
                test_files: vec![],
            },
            DomainInfo {
                name: "users".to_string(),
                files: vec![],
                dependencies: vec![],
                models: vec![],
                routes: vec![],
                comments: vec![],
                test_files: vec![],
            },
        ];

        let graph = generate_graph(&domains);
        assert!(graph.contains("billing --> users"));
    }

    #[test]
    fn generate_index_contains_domain_entries() {
        let domains = vec![DomainInfo {
            name: "billing".to_string(),
            files: vec!["a.ts".to_string(), "b.ts".to_string()],
            dependencies: vec![],
            models: vec!["Invoice".to_string()],
            routes: vec![],
            comments: vec![],
            test_files: vec![],
        }];

        let index = generate_index(&domains, "2025-01-01");
        assert!(index.contains("Billing"));
        assert!(index.contains("2 files"));
        assert!(index.contains("1 models"));
    }

    #[test]
    fn generate_needs_review_with_comments() {
        let domains = vec![DomainInfo {
            name: "billing".to_string(),
            files: vec![],
            dependencies: vec![],
            models: vec![],
            routes: vec![],
            comments: vec!["[TODO] Fix invoice calculation".to_string()],
            test_files: vec![],
        }];

        let review = generate_needs_review(&domains);
        assert!(review.contains("Fix invoice calculation"));
        assert!(review.contains("Billing"));
    }

    #[test]
    fn generate_needs_review_empty() {
        let domains = vec![DomainInfo {
            name: "billing".to_string(),
            files: vec![],
            dependencies: vec![],
            models: vec![],
            routes: vec![],
            comments: vec![],
            test_files: vec![],
        }];

        let review = generate_needs_review(&domains);
        assert!(review.contains("No open questions found"));
    }
}
