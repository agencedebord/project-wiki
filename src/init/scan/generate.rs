use std::collections::HashSet;

use crate::i18n::t;
use crate::init::analyze::LlmAnalysis;
use crate::wiki::common::capitalize;

use super::DomainInfo;

/// Generate a markdown overview for a domain.
/// When `analysis` is provided (LLM mode), the output is real documentation.
/// When `analysis` is None (should not happen in normal flow), falls back to structural stub.
pub fn generate_domain_overview(
    domain: &DomainInfo,
    all_domains: &[DomainInfo],
    analysis: Option<&LlmAnalysis>,
    lang: &str,
) -> String {
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let title = capitalize(&domain.name);

    let related_files_section = format_related_files(&domain.files);

    let confidence = if analysis.is_some() {
        "llm-analyzed"
    } else {
        "inferred"
    };

    let mut sections = Vec::new();

    // Front matter
    sections.push(format!(
        "---\ndomain: {}\nconfidence: {}\nlast_updated: {}\n{}\n---",
        domain.name, confidence, date, related_files_section
    ));

    if let Some(analysis) = analysis {
        // ─── LLM-first output (the default) ───

        // Title + description
        sections.push(format!(
            "# {}\n\n## {}\n{} `[llm-analyzed]`",
            title, t("what_this_domain_does", lang), analysis.description
        ));

        // Key behaviors
        if !analysis.behaviors.is_empty() {
            let list: String = analysis
                .behaviors
                .iter()
                .map(|b| format!("- **{}**: {} `[llm-analyzed]`", b.summary, b.detail))
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("## {}\n{}", t("key_behaviors", lang), list));
        }

        // Domain interactions
        if !analysis.interactions.is_empty() {
            let list: String = analysis
                .interactions
                .iter()
                .map(|i| {
                    format!(
                        "- **{}**: {} `[llm-analyzed]`",
                        capitalize(&i.target_domain),
                        i.description
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("## {}\n{}", t("domain_interactions", lang), list));
        }

        // Gotchas and edge cases
        if !analysis.gotchas.is_empty() {
            let list: String = analysis
                .gotchas
                .iter()
                .map(|g| format!("- {} `[llm-analyzed]`", g))
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("## {}\n{}", t("gotchas", lang), list));
        }
    } else {
        // ─── Structural fallback (no LLM) ───
        let mut fallback = format!(
            "# {}\n\n## {}\n{} `[inferred]`",
            title, t("description", lang), t("llm_not_available", lang)
        );

        // Include structural signals so the overview is not completely empty
        if !domain.models.is_empty() {
            fallback.push_str(&format!(
                "\n\n## {}\n{}",
                t("detected_models", lang),
                domain
                    .models
                    .iter()
                    .map(|m| format!("- {}", m))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if !domain.routes.is_empty() {
            fallback.push_str(&format!(
                "\n\n## {}\n{}",
                t("detected_routes", lang),
                domain
                    .routes
                    .iter()
                    .map(|r| format!("- `{}`", r))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        sections.push(fallback);
    }

    // Notes from code (TODO/FIXME/HACK/NOTE) — always included if present
    if let Some(s) =
        format_list_section_opt(t("notes_from_code", lang), &domain.comments, |c| format!("- {}", c))
    {
        sections.push(s);
    }

    // Dependencies (always included if present)
    if !domain.dependencies.is_empty() {
        let deps_list: String = domain
            .dependencies
            .iter()
            .map(|d| format!("- [{}](../{d}/_overview.md)", capitalize(d), d = d))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("## {}\n{}", t("dependencies", lang), deps_list));
    }

    // Referenced by (always included if present)
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
        sections.push(format!("## {}\n{}", t("referenced_by", lang), refs_list));
    }

    sections.join("\n\n")
}

/// Generate a Mermaid dependency graph.
pub fn generate_graph(domains: &[DomainInfo], lang: &str) -> String {
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
        "# {}\n\n\
         > {}\n\n\
         ```mermaid\n\
         graph LR\n\
         {}\n\
         ```\n",
        t("dependency_graph", lang),
        t("auto_generated_scan", lang),
        graph_body
    )
}

/// Generate the _index.md content.
pub fn generate_index(domains: &[DomainInfo], date: &str, lang: &str) -> String {
    let domain_list = if domains.is_empty() {
        t("no_domains_yet", lang).to_string()
    } else {
        domains
            .iter()
            .map(|d| {
                format!(
                    "- [{}](domains/{d}/_overview.md)",
                    capitalize(&d.name),
                    d = d.name,
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "# Codefidence\n\n\
         > {} [codefidence](https://github.com/agencedebord/codefidence).\n\n\
         ## {}\n\n\
         {}\n\n\
         ## {}\n\n\
         | Date | Decision | Domain |\n\
         |------|----------|--------|\n\n\
         ## {}\n\n\
         - {} {}\n",
        t("auto_generated_kb", lang),
        t("domains", lang),
        domain_list,
        t("recent_decisions", lang),
        t("last_updated", lang),
        t("initialized_on", lang),
        date
    )
}

/// Generate the _needs-review.md content.
pub fn generate_needs_review(domains: &[DomainInfo], lang: &str) -> String {
    let mut questions: Vec<String> = Vec::new();

    for domain in domains {
        for comment in &domain.comments {
            questions.push(format!("- **{}**: {}", capitalize(&domain.name), comment));
        }
    }

    let questions_section = if questions.is_empty() {
        t("no_open_questions", lang).to_string()
    } else {
        questions.join("\n")
    };

    format!(
        "# {}\n\n\
         > {}\n\n\
         ## {}\n\n\
         {}\n\n\
         ## {}\n\n\
         {}\n",
        t("needs_review", lang),
        t("needs_review_intro", lang),
        t("open_questions", lang),
        questions_section,
        t("unresolved_contradictions", lang),
        t("none_detected", lang),
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
    use crate::init::analyze::{Behavior, Interaction, LlmAnalysis, LlmCandidate};

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

    fn sample_analysis() -> LlmAnalysis {
        LlmAnalysis {
            description:
                "Handles billing operations including invoice creation and payment processing."
                    .to_string(),
            behaviors: vec![Behavior {
                summary: "Invoice validation".to_string(),
                detail: "Validates invoice amounts are positive before persisting.".to_string(),
            }],
            interactions: vec![Interaction {
                target_domain: "users".to_string(),
                description: "Fetches customer billing profiles.".to_string(),
            }],
            gotchas: vec!["Refunds older than 30 days silently fail.".to_string()],
            memory_candidates: vec![LlmCandidate {
                type_: "business_rule".to_string(),
                text: "Invoices expire after 30 days".to_string(),
            }],
        }
    }

    #[test]
    fn generate_overview_with_analysis_contains_real_docs() {
        let domain = domain_with_signal();
        let analysis = sample_analysis();
        let overview =
            generate_domain_overview(&domain, std::slice::from_ref(&domain), Some(&analysis), "en");

        assert!(overview.contains("## What this domain does"));
        assert!(overview.contains("Handles billing operations"));
        assert!(overview.contains("[llm-analyzed]"));
        assert!(overview.contains("## Key behaviors"));
        assert!(overview.contains("Invoice validation"));
        assert!(overview.contains("## Domain interactions"));
        assert!(overview.contains("Users"));
        assert!(overview.contains("## Gotchas and edge cases"));
        assert!(overview.contains("Refunds older than 30 days"));
        assert!(overview.contains("confidence: llm-analyzed"));
    }

    #[test]
    fn generate_overview_with_analysis_no_inventory() {
        let domain = domain_with_signal();
        let analysis = sample_analysis();
        let overview =
            generate_domain_overview(&domain, std::slice::from_ref(&domain), Some(&analysis), "en");

        assert!(
            !overview.contains("## Data models"),
            "Should not contain model inventory"
        );
        assert!(
            !overview.contains("## API routes"),
            "Should not contain route inventory"
        );
        assert!(
            !overview.contains("## Behavior candidates"),
            "Should not contain mechanical behavior candidates"
        );
        assert!(
            !overview.contains("## Test coverage"),
            "Should not contain test file inventory"
        );
    }

    #[test]
    fn generate_overview_without_analysis_fallback() {
        let domain = domain_with_signal();
        let overview = generate_domain_overview(&domain, std::slice::from_ref(&domain), None, "en");

        assert!(overview.contains("confidence: inferred"));
        assert!(overview.contains("LLM analysis was not available"));
        assert!(!overview.contains("[llm-analyzed]"));
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
            test_files: vec![],
        };
        let analysis = LlmAnalysis {
            description: "Utility functions.".to_string(),
            behaviors: vec![],
            interactions: vec![],
            gotchas: vec![],
            memory_candidates: vec![],
        };
        let overview =
            generate_domain_overview(&domain, std::slice::from_ref(&domain), Some(&analysis), "en");

        assert!(overview.contains("## What this domain does"));
        assert!(
            !overview.contains("## Key behaviors"),
            "Empty behaviors section should be omitted"
        );
        assert!(
            !overview.contains("## Domain interactions"),
            "Empty interactions section should be omitted"
        );
        assert!(
            !overview.contains("## Gotchas"),
            "Empty gotchas section should be omitted"
        );
        assert!(
            !overview.contains("## Dependencies"),
            "Empty dependencies section should be omitted"
        );
    }

    #[test]
    fn generate_overview_includes_dependencies_and_refs() {
        let billing = DomainInfo {
            name: "billing".to_string(),
            files: vec!["src/billing/invoice.ts".to_string()],
            dependencies: vec!["users".to_string()],
            models: vec![],
            routes: vec![],
            comments: vec!["[TODO] Fix calculation".to_string()],
            test_files: vec![],
        };
        let users = DomainInfo {
            name: "users".to_string(),
            files: vec!["src/users/mod.ts".to_string()],
            dependencies: vec![],
            models: vec![],
            routes: vec![],
            comments: vec![],
            test_files: vec![],
        };
        let all = vec![billing.clone(), users];
        let analysis = LlmAnalysis {
            description: "Billing domain.".to_string(),
            behaviors: vec![],
            interactions: vec![],
            gotchas: vec![],
            memory_candidates: vec![],
        };
        let overview = generate_domain_overview(&billing, &all, Some(&analysis), "en");

        assert!(overview.contains("## Dependencies"));
        assert!(overview.contains("[Users](../users/_overview.md)"));
        assert!(overview.contains("## Notes from code"));
        assert!(overview.contains("[TODO] Fix calculation"));
    }

    #[test]
    fn generate_index_clean_format() {
        let domains = vec![DomainInfo {
            name: "billing".to_string(),
            files: vec!["a.ts".to_string(), "b.ts".to_string()],
            dependencies: vec![],
            models: vec!["Invoice".to_string()],
            routes: vec![],
            comments: vec![],
            test_files: vec![],
        }];

        let index = generate_index(&domains, "2025-01-01", "en");
        assert!(index.contains("Billing"));
        assert!(index.contains("domains/billing/_overview.md"));
        // No file/model counts in index anymore
        assert!(
            !index.contains("2 files"),
            "Index should not contain file counts"
        );
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

        let graph = generate_graph(&domains, "en");
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

        let graph = generate_graph(&domains, "en");
        assert!(graph.contains("billing --> users"));
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

        let review = generate_needs_review(&domains, "en");
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

        let review = generate_needs_review(&domains, "en");
        assert!(review.contains("No open questions found"));
    }

    // ─── French output tests ───

    #[test]
    fn generate_overview_french_headers() {
        let domain = domain_with_signal();
        let analysis = sample_analysis();
        let overview =
            generate_domain_overview(&domain, std::slice::from_ref(&domain), Some(&analysis), "fr");

        assert!(overview.contains("## Ce que fait ce domaine"));
        assert!(overview.contains("## Comportements clés"));
        assert!(overview.contains("## Interactions avec d'autres domaines"));
        assert!(overview.contains("## Pièges et cas limites"));
        assert!(!overview.contains("## What this domain does"));
        assert!(!overview.contains("## Key behaviors"));
    }

    #[test]
    fn generate_overview_french_fallback() {
        let domain = domain_with_signal();
        let overview =
            generate_domain_overview(&domain, std::slice::from_ref(&domain), None, "fr");

        assert!(overview.contains("## Description"));
        assert!(overview.contains("L'analyse LLM"));
    }

    #[test]
    fn generate_index_french() {
        let domains = vec![DomainInfo {
            name: "billing".to_string(),
            files: vec!["a.ts".to_string()],
            dependencies: vec![],
            models: vec![],
            routes: vec![],
            comments: vec![],
            test_files: vec![],
        }];

        let index = generate_index(&domains, "2025-01-01", "fr");
        assert!(index.contains("## Domaines"));
        assert!(index.contains("## Décisions récentes"));
        assert!(index.contains("## Dernière mise à jour"));
        assert!(index.contains("Initialisé le"));
        assert!(!index.contains("## Domains"));
    }

    #[test]
    fn generate_graph_french() {
        let domains = vec![DomainInfo {
            name: "billing".to_string(),
            files: vec![],
            dependencies: vec![],
            models: vec![],
            routes: vec![],
            comments: vec![],
            test_files: vec![],
        }];

        let graph = generate_graph(&domains, "fr");
        assert!(graph.contains("Graphe de dépendances des domaines"));
        assert!(!graph.contains("Domain dependency graph"));
    }

    #[test]
    fn generate_needs_review_french() {
        let domains = vec![DomainInfo {
            name: "billing".to_string(),
            files: vec![],
            dependencies: vec![],
            models: vec![],
            routes: vec![],
            comments: vec!["[TODO] Fix calculation".to_string()],
            test_files: vec![],
        }];

        let review = generate_needs_review(&domains, "fr");
        assert!(review.contains("À vérifier"));
        assert!(review.contains("Questions ouvertes"));
        assert!(review.contains("Contradictions non résolues"));
        assert!(!review.contains("Needs review"));
    }
}
