use std::collections::HashSet;

use crate::wiki::common::capitalize;

use super::DomainInfo;

/// Generate a markdown overview for a domain.
/// Only includes sections that have actual content — no empty placeholders.
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

    // Title + honest structural description
    sections.push(format!(
        "# {}\n\n## Description\n{} `[inferred]`",
        title,
        domain.structural_description()
    ));

    // Data models (only if non-empty)
    if let Some(s) = format_list_section_opt("Data models", &domain.models, |m| {
        format!("- {} `[seen-in-code]`", m)
    }) {
        sections.push(s);
    }

    // API routes (only if non-empty)
    if let Some(s) = format_list_section_opt("API routes", &domain.routes, |r| {
        format!("- {} `[seen-in-code]`", r)
    }) {
        sections.push(s);
    }

    // Behavior candidates (deterministic inference from signals)
    if let Some(s) = generate_behavior_candidates(domain) {
        sections.push(s);
    }

    // Notes from code (TODO/FIXME/HACK/NOTE)
    if let Some(s) =
        format_list_section_opt("Notes from code", &domain.comments, |c| format!("- {}", c))
    {
        sections.push(s);
    }

    // Dependencies (only if non-empty)
    if !domain.dependencies.is_empty() {
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

    // Referenced by (only if non-empty)
    let referenced_by: Vec<&DomainInfo> = all_domains
        .iter()
        .filter(|d| d.name != domain.name && d.dependencies.contains(&domain.name))
        .collect();

    if !referenced_by.is_empty() {
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

    // Test coverage (only if non-empty)
    if let Some(s) =
        format_list_section_opt("Test coverage", &domain.test_files, |t| format!("- {}", t))
    {
        sections.push(s);
    }

    sections.join("\n\n")
}

/// Generate behavior candidates from deterministic rules.
/// Each candidate is tagged [needs-validation] — these are proposals, not facts.
fn generate_behavior_candidates(domain: &DomainInfo) -> Option<String> {
    let mut candidates: Vec<String> = Vec::new();

    // routes + models = API + data layer
    if !domain.routes.is_empty() && !domain.models.is_empty() {
        candidates.push(format!(
            "Exposes API endpoints and defines data models for {} `[needs-validation]`",
            domain.name
        ));
    }

    // models + tests = validated data layer
    if !domain.models.is_empty() && !domain.test_files.is_empty() {
        candidates.push(format!(
            "Data models for {} have test coverage `[needs-validation]`",
            domain.name
        ));
    }

    // routes + dependencies = orchestration layer
    if !domain.routes.is_empty() && !domain.dependencies.is_empty() {
        let dep_names = domain.dependencies.join(", ");
        candidates.push(format!(
            "Orchestrates requests across {} `[needs-validation]`",
            dep_names
        ));
    }

    // heavy dependency count = integration hub
    if domain.dependencies.len() >= 3 {
        candidates.push(format!(
            "Acts as an integration hub connecting {} other domains `[needs-validation]`",
            domain.dependencies.len()
        ));
    }

    if candidates.is_empty() {
        None
    } else {
        let list = candidates
            .iter()
            .map(|c| format!("- {}", c))
            .collect::<Vec<_>>()
            .join("\n");
        Some(format!("## Behavior candidates\n{}", list))
    }
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
        "# Codefidence\n\n\
         > Auto-generated knowledge base. Managed by [codefidence](https://github.com/agencedebord/codefidence).\n\n\
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

fn format_list_section_opt<F>(title: &str, items: &[String], formatter: F) -> Option<String>
where
    F: Fn(&str) -> String,
{
    if items.is_empty() {
        None
    } else {
        let list: String = items
            .iter()
            .map(|i| formatter(i))
            .collect::<Vec<_>>()
            .join("\n");
        Some(format!("## {}\n{}", title, list))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domain_with_signal() -> DomainInfo {
        DomainInfo {
            name: "billing".to_string(),
            files: vec![
                "src/billing/invoice.ts".to_string(),
                "src/billing/payment.ts".to_string(),
            ],
            dependencies: vec![],
            models: vec!["Invoice".to_string()],
            routes: vec![],
            comments: vec![],
            test_files: vec![],
        }
    }

    #[test]
    fn generate_overview_contains_domain_name_and_structural_desc() {
        let domain = domain_with_signal();
        let overview = generate_domain_overview(&domain, std::slice::from_ref(&domain));
        assert!(overview.contains("Billing"));
        assert!(overview.contains("Invoice"));
        assert!(overview.contains("This domain contains"));
        assert!(overview.contains("[inferred]"));
    }

    #[test]
    fn generate_overview_no_key_behaviors_placeholder() {
        let domain = domain_with_signal();
        let overview = generate_domain_overview(&domain, std::slice::from_ref(&domain));
        assert!(
            !overview.contains("Key behaviors"),
            "Should not contain Key behaviors section"
        );
        assert!(
            !overview.contains("_To be documented._"),
            "Should not contain placeholder text"
        );
    }

    #[test]
    fn generate_overview_no_generic_description() {
        let domain = domain_with_signal();
        let overview = generate_domain_overview(&domain, std::slice::from_ref(&domain));
        assert!(
            !overview.contains("Auto-generated from codebase scan"),
            "Should not contain generic description"
        );
    }

    #[test]
    fn generate_overview_omits_empty_sections() {
        let domain = DomainInfo {
            name: "utils".to_string(),
            files: vec!["src/utils/helpers.ts".to_string()],
            dependencies: vec![],
            models: vec![],
            routes: vec![],
            comments: vec![],
            test_files: vec!["tests/utils.test.ts".to_string()],
        };
        let overview = generate_domain_overview(&domain, std::slice::from_ref(&domain));
        assert!(
            !overview.contains("## Data models"),
            "Empty models section should be omitted"
        );
        assert!(
            !overview.contains("## API routes"),
            "Empty routes section should be omitted"
        );
        assert!(
            !overview.contains("## Dependencies"),
            "Empty dependencies section should be omitted"
        );
        assert!(
            !overview.contains("## Referenced by"),
            "Empty referenced by section should be omitted"
        );
        assert!(
            !overview.contains("_None detected._"),
            "Should not contain 'None detected' placeholders"
        );
        // Test coverage should still be present
        assert!(overview.contains("## Test coverage"));
    }

    #[test]
    fn generate_overview_includes_populated_sections() {
        let domain = DomainInfo {
            name: "billing".to_string(),
            files: vec!["src/billing/invoice.ts".to_string()],
            dependencies: vec!["users".to_string()],
            models: vec!["Invoice".to_string()],
            routes: vec!["GET /invoices".to_string()],
            comments: vec!["[TODO] Fix calculation".to_string()],
            test_files: vec!["tests/billing.test.ts".to_string()],
        };
        let overview = generate_domain_overview(&domain, std::slice::from_ref(&domain));
        assert!(overview.contains("## Data models"));
        assert!(overview.contains("## API routes"));
        assert!(overview.contains("## Dependencies"));
        assert!(overview.contains("## Notes from code"));
        assert!(overview.contains("## Test coverage"));
        assert!(overview.contains("[TODO] Fix calculation"));
    }

    #[test]
    fn generate_overview_behavior_candidates_routes_and_models() {
        let domain = DomainInfo {
            name: "billing".to_string(),
            files: vec!["src/billing/invoice.ts".to_string()],
            dependencies: vec![],
            models: vec!["Invoice".to_string()],
            routes: vec!["GET /invoices".to_string()],
            comments: vec![],
            test_files: vec![],
        };
        let overview = generate_domain_overview(&domain, std::slice::from_ref(&domain));
        assert!(
            overview.contains("## Behavior candidates"),
            "Should have behavior candidates when routes + models"
        );
        assert!(overview.contains("[needs-validation]"));
        assert!(overview.contains("Exposes API endpoints"));
    }

    #[test]
    fn generate_overview_no_behavior_candidates_without_signal() {
        let domain = DomainInfo {
            name: "utils".to_string(),
            files: vec!["src/utils/helpers.ts".to_string()],
            dependencies: vec![],
            models: vec![],
            routes: vec![],
            comments: vec![],
            test_files: vec!["tests/utils.test.ts".to_string()],
        };
        let overview = generate_domain_overview(&domain, std::slice::from_ref(&domain));
        assert!(
            !overview.contains("## Behavior candidates"),
            "Should not have behavior candidates without enough signal"
        );
    }

    #[test]
    fn generate_behavior_candidates_integration_hub() {
        let domain = DomainInfo {
            name: "api".to_string(),
            files: vec!["src/api/index.ts".to_string()],
            dependencies: vec![
                "billing".to_string(),
                "users".to_string(),
                "auth".to_string(),
            ],
            models: vec![],
            routes: vec![],
            comments: vec![],
            test_files: vec![],
        };
        let result = generate_behavior_candidates(&domain);
        assert!(result.is_some());
        assert!(result.unwrap().contains("integration hub"));
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
