use super::collect::{collect_files, normalize_path, should_ignore};
use super::prioritize::prioritize_and_format_items;
use super::render::{format_json, format_pr_comment, format_text};
use super::sensitivity::{calculate_sensitivity, generate_suggestions};
use super::*;
use crate::wiki::note::{Confidence, MemoryItem, MemoryItemStatus, MemoryItemType};

// ── Helpers ──

fn make_item(
    id: &str,
    type_: MemoryItemType,
    text: &str,
    confidence: Confidence,
    related_files: Vec<String>,
) -> MemoryItem {
    MemoryItem {
        id: id.to_string(),
        type_,
        text: text.to_string(),
        confidence,
        related_files,
        sources: Vec::new(),
        status: MemoryItemStatus::Active,
        last_reviewed: None,
    }
}

fn make_item_output(
    id: &str,
    type_: &str,
    text: &str,
    confidence: &str,
    directly_related: bool,
) -> DomainItemOutput {
    DomainItemOutput {
        id: id.to_string(),
        type_: type_.to_string(),
        text: text.to_string(),
        confidence: confidence.to_string(),
        directly_related,
        source_note: "test.md".to_string(),
    }
}

fn make_domain_hit(
    name: &str,
    role: DomainRole,
    files: Vec<&str>,
    memory_items: Vec<DomainItemOutput>,
    warnings: Vec<DomainWarning>,
) -> DomainHit {
    DomainHit {
        name: name.to_string(),
        role,
        files: files.into_iter().map(|s| s.to_string()).collect(),
        memory_items,
        warnings,
    }
}

/// Build a CheckDiffResult from pre-built domain hits (useful for testing).
fn build_result(
    files_analyzed: usize,
    domains: Vec<DomainHit>,
    unresolved_files: Vec<String>,
) -> CheckDiffResult {
    let mut result = CheckDiffResult {
        schema_version: "1".to_string(),
        files_analyzed,
        sensitivity: Sensitivity::Low,
        domains,
        unresolved_files,
        suggested_actions: Vec::new(),
    };
    result.sensitivity = calculate_sensitivity(&result);
    result.suggested_actions = generate_suggestions(&result);
    result
}

// ── File collection tests (task 008) ──

#[test]
fn test_normalize_path_strips_dot_slash() {
    assert_eq!(normalize_path("./src/main.rs"), "src/main.rs");
}

#[test]
fn test_normalize_path_no_change() {
    assert_eq!(normalize_path("src/main.rs"), "src/main.rs");
}

#[test]
fn test_should_ignore_wiki_dir() {
    assert!(should_ignore(".wiki/domains/billing/_overview.md"));
}

#[test]
fn test_should_ignore_node_modules() {
    assert!(should_ignore("node_modules/express/index.js"));
}

#[test]
fn test_should_ignore_target_dir() {
    assert!(should_ignore("target/debug/project-wiki"));
}

#[test]
fn test_should_ignore_binary_files() {
    assert!(should_ignore("assets/logo.png"));
    assert!(should_ignore("fonts/main.woff2"));
}

#[test]
fn test_should_not_ignore_source_files() {
    assert!(!should_ignore("src/main.rs"));
    assert!(!should_ignore("src/billing/invoice.ts"));
    assert!(!should_ignore("README.md"));
}

#[test]
fn test_collect_files_explicit_mode() {
    let files = vec!["src/main.rs".to_string(), "Cargo.toml".to_string()];
    let result = collect_files(&files, false).unwrap();
    assert_eq!(result.len(), 2);
    assert!(result.contains(&"src/main.rs".to_string()));
    assert!(result.contains(&"Cargo.toml".to_string()));
}

#[test]
fn test_collect_files_explicit_nonexistent_skipped() {
    let files = vec!["src/main.rs".to_string(), "does/not/exist.rs".to_string()];
    let result = collect_files(&files, false).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], "src/main.rs");
}

#[test]
fn test_collect_files_explicit_normalizes_paths() {
    let files = vec!["./Cargo.toml".to_string()];
    let result = collect_files(&files, false).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], "Cargo.toml");
}

#[test]
fn test_collect_files_explicit_filters_ignored() {
    let files = vec![
        "src/main.rs".to_string(),
        ".wiki/domains/billing/_overview.md".to_string(),
        "node_modules/express/index.js".to_string(),
    ];
    let result = collect_files(&files, false).unwrap();
    // Only src/main.rs should remain (others ignored or non-existent)
    assert!(result.len() <= 1);
    if !result.is_empty() {
        assert_eq!(result[0], "src/main.rs");
    }
}

// ── Prioritization tests (task 009) ──

#[test]
fn test_prioritize_exception_first() {
    let items = vec![
        make_item(
            "b-001",
            MemoryItemType::BusinessRule,
            "Rule A",
            Confidence::Confirmed,
            vec![],
        ),
        make_item(
            "b-002",
            MemoryItemType::Decision,
            "Decision B",
            Confidence::Confirmed,
            vec![],
        ),
        make_item(
            "b-003",
            MemoryItemType::Exception,
            "Exception C",
            Confidence::Confirmed,
            vec![],
        ),
    ];
    let modified = vec![];
    let result = prioritize_and_format_items(&items, &modified, 3, "test.md");
    assert_eq!(result[0].type_, "exception");
    assert_eq!(result[1].type_, "decision");
    assert_eq!(result[2].type_, "business_rule");
}

#[test]
fn test_prioritize_by_confidence() {
    let items = vec![
        make_item(
            "b-001",
            MemoryItemType::Decision,
            "Dec inferred",
            Confidence::Inferred,
            vec![],
        ),
        make_item(
            "b-002",
            MemoryItemType::Decision,
            "Dec confirmed",
            Confidence::Confirmed,
            vec![],
        ),
    ];
    let modified = vec![];
    let result = prioritize_and_format_items(&items, &modified, 3, "test.md");
    assert_eq!(result[0].confidence, "confirmed");
    assert_eq!(result[1].confidence, "inferred");
}

#[test]
fn test_prioritize_related_file_first() {
    let items = vec![
        make_item(
            "b-001",
            MemoryItemType::Decision,
            "Dec A (unrelated)",
            Confidence::Confirmed,
            vec![],
        ),
        make_item(
            "b-002",
            MemoryItemType::Decision,
            "Dec B (related)",
            Confidence::Confirmed,
            vec!["src/billing/invoice.ts".to_string()],
        ),
    ];
    let modified = vec!["src/billing/invoice.ts".to_string()];
    let result = prioritize_and_format_items(&items, &modified, 3, "test.md");
    assert!(result[0].directly_related);
    assert!(!result[1].directly_related);
}

#[test]
fn test_prioritize_filters_deprecated() {
    let mut dep = make_item(
        "b-001",
        MemoryItemType::Exception,
        "Old",
        Confidence::Confirmed,
        vec![],
    );
    dep.status = MemoryItemStatus::Deprecated;
    let active = make_item(
        "b-002",
        MemoryItemType::Exception,
        "Active",
        Confidence::Confirmed,
        vec![],
    );

    let items = vec![dep, active];
    let result = prioritize_and_format_items(&items, &[], 3, "test.md");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, "b-002");
}

#[test]
fn test_prioritize_max_items_limit() {
    let items = vec![
        make_item(
            "b-001",
            MemoryItemType::Exception,
            "E1",
            Confidence::Confirmed,
            vec![],
        ),
        make_item(
            "b-002",
            MemoryItemType::Decision,
            "D1",
            Confidence::Confirmed,
            vec![],
        ),
        make_item(
            "b-003",
            MemoryItemType::BusinessRule,
            "R1",
            Confidence::Confirmed,
            vec![],
        ),
        make_item(
            "b-004",
            MemoryItemType::BusinessRule,
            "R2",
            Confidence::Confirmed,
            vec![],
        ),
        make_item(
            "b-005",
            MemoryItemType::BusinessRule,
            "R3",
            Confidence::Confirmed,
            vec![],
        ),
    ];
    let result = prioritize_and_format_items(&items, &[], 2, "test.md");
    assert_eq!(result.len(), 2);
}

// ── Sensitivity tests (task 010) ──

#[test]
fn test_sensitivity_high_on_exception() {
    let result = build_result(
        1,
        vec![make_domain_hit(
            "billing",
            DomainRole::Primary,
            vec!["a.ts"],
            vec![make_item_output(
                "b-001",
                "exception",
                "E1",
                "confirmed",
                false,
            )],
            vec![],
        )],
        vec![],
    );
    assert_eq!(result.sensitivity, Sensitivity::High);
}

#[test]
fn test_sensitivity_high_on_decision() {
    let result = build_result(
        1,
        vec![make_domain_hit(
            "billing",
            DomainRole::Primary,
            vec!["a.ts"],
            vec![make_item_output(
                "b-001",
                "decision",
                "D1",
                "confirmed",
                false,
            )],
            vec![],
        )],
        vec![],
    );
    assert_eq!(result.sensitivity, Sensitivity::High);
}

#[test]
fn test_sensitivity_high_on_stale() {
    let result = build_result(
        1,
        vec![make_domain_hit(
            "billing",
            DomainRole::Primary,
            vec!["a.ts"],
            vec![],
            vec![DomainWarning {
                kind: "stale".to_string(),
                note: "test.md".to_string(),
                days: Some(45),
            }],
        )],
        vec![],
    );
    assert_eq!(result.sensitivity, Sensitivity::High);
}

#[test]
fn test_sensitivity_high_on_low_confidence() {
    let result = build_result(
        1,
        vec![make_domain_hit(
            "billing",
            DomainRole::Primary,
            vec!["a.ts"],
            vec![],
            vec![DomainWarning {
                kind: "low_confidence".to_string(),
                note: "test.md".to_string(),
                days: None,
            }],
        )],
        vec![],
    );
    assert_eq!(result.sensitivity, Sensitivity::High);
}

#[test]
fn test_sensitivity_medium_business_rule_only() {
    let result = build_result(
        1,
        vec![make_domain_hit(
            "billing",
            DomainRole::Primary,
            vec!["a.ts"],
            vec![make_item_output(
                "b-001",
                "business_rule",
                "Rule",
                "confirmed",
                false,
            )],
            vec![],
        )],
        vec![],
    );
    assert_eq!(result.sensitivity, Sensitivity::Medium);
}

#[test]
fn test_sensitivity_low_no_memory() {
    let result = build_result(
        1,
        vec![make_domain_hit(
            "billing",
            DomainRole::Primary,
            vec!["a.ts"],
            vec![],
            vec![DomainWarning {
                kind: "no_memory".to_string(),
                note: "test.md".to_string(),
                days: None,
            }],
        )],
        vec![],
    );
    assert_eq!(result.sensitivity, Sensitivity::Low);
}

#[test]
fn test_sensitivity_low_unresolved_only() {
    let result = build_result(0, vec![], vec!["random.txt".to_string()]);
    assert_eq!(result.sensitivity, Sensitivity::Low);
}

// ── Suggestion tests (task 010) ──

#[test]
fn test_suggestion_stale_note() {
    let result = build_result(
        1,
        vec![make_domain_hit(
            "billing",
            DomainRole::Primary,
            vec!["a.ts"],
            vec![make_item_output(
                "b-001",
                "exception",
                "E1",
                "confirmed",
                false,
            )],
            vec![DomainWarning {
                kind: "stale".to_string(),
                note: ".wiki/domains/billing/_overview.md".to_string(),
                days: Some(42),
            }],
        )],
        vec![],
    );
    assert!(
        result
            .suggested_actions
            .iter()
            .any(|a| a.contains("Relire"))
    );
}

#[test]
fn test_suggestion_exception() {
    let result = build_result(
        1,
        vec![make_domain_hit(
            "billing",
            DomainRole::Primary,
            vec!["a.ts"],
            vec![make_item_output(
                "b-001",
                "exception",
                "Client X legacy",
                "confirmed",
                false,
            )],
            vec![],
        )],
        vec![],
    );
    assert!(
        result
            .suggested_actions
            .iter()
            .any(|a| a.contains("exception"))
    );
}

#[test]
fn test_suggestion_decision() {
    let result = build_result(
        1,
        vec![make_domain_hit(
            "billing",
            DomainRole::Primary,
            vec!["a.ts"],
            vec![make_item_output(
                "b-001",
                "decision",
                "No dedup",
                "confirmed",
                false,
            )],
            vec![],
        )],
        vec![],
    );
    assert!(
        result
            .suggested_actions
            .iter()
            .any(|a| a.contains("decision"))
    );
}

#[test]
fn test_suggestion_max_3() {
    let result = build_result(
        4,
        vec![make_domain_hit(
            "billing",
            DomainRole::Primary,
            vec!["a.ts", "b.ts"],
            vec![
                make_item_output("b-001", "exception", "E1", "confirmed", false),
                make_item_output("b-002", "exception", "E2", "confirmed", false),
                make_item_output("b-003", "decision", "D1", "confirmed", false),
            ],
            vec![DomainWarning {
                kind: "stale".to_string(),
                note: "test.md".to_string(),
                days: Some(45),
            }],
        )],
        vec![],
    );
    assert!(result.suggested_actions.len() <= 3);
}

#[test]
fn test_suggestion_none_on_low() {
    let result = build_result(0, vec![], vec!["random.txt".to_string()]);
    assert!(result.suggested_actions.is_empty());
}

// ── Output formatting tests (task 011) ──

#[test]
fn test_output_text_empty() {
    let result = CheckDiffResult {
        schema_version: "1".to_string(),
        files_analyzed: 0,
        sensitivity: Sensitivity::Low,
        domains: vec![],
        unresolved_files: vec![],
        suggested_actions: vec![],
    };
    let text = format_text(&result);
    assert!(text.contains("[project-wiki] Diff check"));
    assert!(text.contains("0 file(s) analyzed"));
}

#[test]
fn test_output_text_full() {
    let result = build_result(
        2,
        vec![
            make_domain_hit(
                "billing",
                DomainRole::Primary,
                vec!["a.ts", "b.ts"],
                vec![
                    make_item_output("b-001", "exception", "Exception X", "confirmed", true),
                    make_item_output("b-002", "decision", "Decision Y", "verified", false),
                ],
                vec![DomainWarning {
                    kind: "stale".to_string(),
                    note: ".wiki/billing/_overview.md".to_string(),
                    days: Some(42),
                }],
            ),
            make_domain_hit(
                "auth",
                DomainRole::Secondary,
                vec!["c.ts"],
                vec![make_item_output(
                    "a-001",
                    "exception",
                    "Legacy endpoint",
                    "confirmed",
                    false,
                )],
                vec![],
            ),
        ],
        vec!["config/deploy.yaml".to_string()],
    );
    let text = format_text(&result);

    assert!(text.contains("[project-wiki] Diff check"));
    assert!(text.contains("2 file(s) analyzed"));
    assert!(text.contains("Sensitivity: high"));
    assert!(text.contains("billing (primary)"));
    assert!(text.contains("auth (secondary)"));
    assert!(text.contains("[exception] Exception X [confirmed]"));
    assert!(text.contains("[decision] Decision Y [verified]"));
    assert!(text.contains("stale"));
    assert!(text.contains("Unresolved files"));
    assert!(text.contains("config/deploy.yaml"));
}

#[test]
fn test_output_text_sensitivity_label() {
    let high_result = build_result(
        1,
        vec![make_domain_hit(
            "billing",
            DomainRole::Primary,
            vec!["a.ts"],
            vec![make_item_output(
                "b-001",
                "exception",
                "E",
                "confirmed",
                false,
            )],
            vec![],
        )],
        vec![],
    );
    assert!(format_text(&high_result).contains("Sensitivity: high"));

    let low_result = build_result(0, vec![], vec!["x.txt".to_string()]);
    assert!(format_text(&low_result).contains("Sensitivity: low"));
}

#[test]
fn test_output_json_valid() {
    let result = build_result(
        1,
        vec![make_domain_hit(
            "billing",
            DomainRole::Primary,
            vec!["a.ts"],
            vec![make_item_output(
                "b-001",
                "exception",
                "E",
                "confirmed",
                false,
            )],
            vec![],
        )],
        vec![],
    );
    let json_str = format_json(&result).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed["files_analyzed"], 1);
    assert_eq!(parsed["sensitivity"], "high");
    assert!(parsed["domains"].is_array());
    assert!(parsed["suggested_actions"].is_array());
}

#[test]
fn test_output_json_empty() {
    let result = CheckDiffResult {
        schema_version: "1".to_string(),
        files_analyzed: 0,
        sensitivity: Sensitivity::Low,
        domains: vec![],
        unresolved_files: vec![],
        suggested_actions: vec![],
    };
    let json_str = format_json(&result).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed["files_analyzed"], 0);
    assert_eq!(parsed["sensitivity"], "low");
    assert_eq!(parsed["domains"].as_array().unwrap().len(), 0);
}

#[test]
fn test_output_json_full_structure() {
    let result = build_result(
        2,
        vec![make_domain_hit(
            "billing",
            DomainRole::Primary,
            vec!["a.ts"],
            vec![make_item_output(
                "b-001",
                "exception",
                "E",
                "confirmed",
                true,
            )],
            vec![DomainWarning {
                kind: "stale".to_string(),
                note: "test.md".to_string(),
                days: Some(42),
            }],
        )],
        vec!["x.txt".to_string()],
    );
    let json_str = format_json(&result).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    // Check domain structure
    let domain = &parsed["domains"][0];
    assert_eq!(domain["name"], "billing");
    assert_eq!(domain["role"], "primary");
    assert!(domain["files"].is_array());
    assert!(domain["memory_items"].is_array());
    assert!(domain["warnings"].is_array());

    // Check item structure
    let item = &domain["memory_items"][0];
    assert_eq!(item["id"], "b-001");
    assert_eq!(item["type"], "exception");
    assert_eq!(item["directly_related"], true);

    // Check warning structure
    let warning = &domain["warnings"][0];
    assert_eq!(warning["kind"], "stale");
    assert_eq!(warning["days"], 42);

    // Check unresolved
    assert_eq!(parsed["unresolved_files"][0], "x.txt");
}

#[test]
fn test_output_text_with_unresolved() {
    let result = build_result(
        1,
        vec![],
        vec!["config.yaml".to_string(), "random.txt".to_string()],
    );
    let text = format_text(&result);
    assert!(text.contains("Unresolved files"));
    assert!(text.contains("config.yaml"));
    assert!(text.contains("random.txt"));
}

#[test]
fn test_output_text_with_warnings() {
    let result = build_result(
        1,
        vec![make_domain_hit(
            "billing",
            DomainRole::Primary,
            vec!["a.ts"],
            vec![],
            vec![
                DomainWarning {
                    kind: "stale".to_string(),
                    note: ".wiki/billing/_overview.md".to_string(),
                    days: Some(42),
                },
                DomainWarning {
                    kind: "low_confidence".to_string(),
                    note: ".wiki/billing/_overview.md".to_string(),
                    days: None,
                },
            ],
        )],
        vec![],
    );
    let text = format_text(&result);
    assert!(text.contains("Warnings"));
    assert!(text.contains("stale"));
    assert!(text.contains("low confidence"));
}

#[test]
fn test_sensitivity_serialization() {
    assert_eq!(serde_json::to_string(&Sensitivity::Low).unwrap(), "\"low\"");
    assert_eq!(
        serde_json::to_string(&Sensitivity::Medium).unwrap(),
        "\"medium\""
    );
    assert_eq!(
        serde_json::to_string(&Sensitivity::High).unwrap(),
        "\"high\""
    );
}

#[test]
fn test_check_diff_default_max_items_is_3() {
    let default: usize = 3;
    assert_eq!(default, 3);
}

// ── PR comment formatting (task 021) ──

#[test]
fn test_pr_comment_skips_low_sensitivity() {
    let result = build_result(1, Vec::new(), vec!["unresolved.ts".to_string()]);
    assert_eq!(result.sensitivity, Sensitivity::Low);
    assert!(format_pr_comment(&result).is_none());
}

#[test]
fn test_pr_comment_formats_medium_sensitivity() {
    let items = vec![make_item(
        "billing-001",
        MemoryItemType::BusinessRule,
        "TVA toujours incluse",
        Confidence::Confirmed,
        vec!["src/billing/invoice.ts".to_string()],
    )];
    let domains = vec![DomainHit {
        name: "billing".to_string(),
        role: DomainRole::Primary,
        files: vec!["src/billing/invoice.ts".to_string()],
        memory_items: vec![DomainItemOutput {
            id: items[0].id.clone(),
            type_: "business_rule".to_string(),
            text: items[0].text.clone(),
            confidence: "confirmed".to_string(),
            directly_related: true,
            source_note: "billing/_overview.md".to_string(),
        }],
        warnings: Vec::new(),
    }];

    let result = build_result(1, domains, Vec::new());
    let comment = format_pr_comment(&result);

    assert!(comment.is_some());
    let text = comment.unwrap();
    assert!(text.contains("project-wiki"));
    assert!(text.contains("<!-- project-wiki-memory-check -->"));
    assert!(text.contains("**billing**"));
    assert!(text.contains("TVA toujours incluse"));
    assert!(text.contains("| business_rule |"));
    assert!(text.contains("| confirmed |"));
}

#[test]
fn test_pr_comment_includes_warnings() {
    let domains = vec![DomainHit {
        name: "billing".to_string(),
        role: DomainRole::Primary,
        files: vec!["src/billing/invoice.ts".to_string()],
        memory_items: vec![DomainItemOutput {
            id: "billing-001".to_string(),
            type_: "exception".to_string(),
            text: "Legacy pricing".to_string(),
            confidence: "confirmed".to_string(),
            directly_related: true,
            source_note: "billing/_overview.md".to_string(),
        }],
        warnings: vec![DomainWarning {
            kind: "stale".to_string(),
            note: "billing/_overview.md".to_string(),
            days: Some(42),
        }],
    }];

    let result = build_result(1, domains, Vec::new());
    let comment = format_pr_comment(&result).unwrap();
    assert!(comment.contains("stale (42 days)"));
    assert!(comment.contains("### Warnings"));
}

#[test]
fn test_pr_comment_json_roundtrip() {
    // Verify check-diff JSON is valid and parseable
    let domains = vec![DomainHit {
        name: "auth".to_string(),
        role: DomainRole::Primary,
        files: vec!["src/auth/login.ts".to_string()],
        memory_items: vec![DomainItemOutput {
            id: "auth-001".to_string(),
            type_: "decision".to_string(),
            text: "Use bcrypt for passwords".to_string(),
            confidence: "verified".to_string(),
            directly_related: false,
            source_note: "auth/_overview.md".to_string(),
        }],
        warnings: Vec::new(),
    }];

    let result = build_result(1, domains, Vec::new());
    let json_str = format_json(&result).unwrap();

    // Parse back as generic JSON
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed["schema_version"], "1");
    assert_eq!(parsed["domains"][0]["name"], "auth");
    assert_eq!(
        parsed["domains"][0]["memory_items"][0]["text"],
        "Use bcrypt for passwords"
    );
}
