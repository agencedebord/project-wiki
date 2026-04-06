use super::dedupe::{assign_ids, deduplicate, prioritize_and_limit};
use super::heuristics::{
    detect_business_rule_candidates, detect_decision_candidates, detect_exception_candidates,
    is_excluded_path, is_generic_text, truncate_text,
};
use super::render::{format_candidates_markdown, parse_processed_ids, write_candidates_file};
use super::*;
use crate::init::scan::{CodeComment, DomainInfo};

fn make_domain(
    name: &str,
    files: Vec<&str>,
    comments: Vec<&str>,
    test_files: Vec<&str>,
) -> DomainInfo {
    DomainInfo {
        name: name.to_string(),
        files: files.into_iter().map(|s| s.to_string()).collect(),
        dependencies: Vec::new(),
        models: Vec::new(),
        routes: Vec::new(),
        comments: comments
            .into_iter()
            .map(|s| {
                // Parse "[TAG] text" format used in test fixtures
                if let Some(rest) = s.strip_prefix('[') {
                    if let Some(idx) = rest.find(']') {
                        let tag = &rest[..idx];
                        let text = rest[idx + 1..].trim().to_string();
                        return CodeComment {
                            tag: tag.to_string(),
                            text,
                            file_path: "test.py".to_string(),
                        };
                    }
                }
                CodeComment {
                    tag: "NOTE".to_string(),
                    text: s.to_string(),
                    file_path: "test.py".to_string(),
                }
            })
            .collect(),
        test_files: test_files.into_iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn test_heuristic_exception_legacy_naming() {
    // Create a temp file with enough lines
    let dir = tempfile::TempDir::new().unwrap();
    let file_path = dir.path().join("legacy_pricing.ts");
    let content = "line\n".repeat(60);
    std::fs::write(&file_path, &content).unwrap();

    let domain = make_domain("billing", vec![file_path.to_str().unwrap()], vec![], vec![]);

    let mut candidates = Vec::new();
    detect_exception_candidates(&domain, &mut candidates);

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].type_, CandidateType::Exception);
    assert!(candidates[0].text.contains("legacy_pricing"));
}

#[test]
fn test_heuristic_exception_does_not_match_standard_migration_commands() {
    let dir = tempfile::TempDir::new().unwrap();
    let content = "line\n".repeat(60);

    // Standard Django management commands should NOT be flagged as exceptions
    for name in &[
        "squashmigrations.py",
        "makemigrations.py",
        "showmigrations.py",
        "optimizemigration.py",
    ] {
        let file_path = dir.path().join(name);
        std::fs::write(&file_path, &content).unwrap();

        let domain = make_domain("core", vec![file_path.to_str().unwrap()], vec![], vec![]);
        let mut candidates = Vec::new();
        detect_exception_candidates(&domain, &mut candidates);

        assert!(
            candidates.is_empty(),
            "Standard command '{}' should NOT be flagged as exception, got: {:?}",
            name,
            candidates.iter().map(|c| &c.text).collect::<Vec<_>>()
        );
    }
}

#[test]
fn test_heuristic_exception_still_matches_standalone_migration() {
    // A file literally named "migration.py" should still match
    let dir = tempfile::TempDir::new().unwrap();
    let content = "line\n".repeat(60);
    let file_path = dir.path().join("migration.py");
    std::fs::write(&file_path, &content).unwrap();

    let domain = make_domain("core", vec![file_path.to_str().unwrap()], vec![], vec![]);
    let mut candidates = Vec::new();
    detect_exception_candidates(&domain, &mut candidates);

    assert_eq!(
        candidates.len(),
        1,
        "Standalone 'migration.py' should be flagged as exception"
    );
}

#[test]
fn test_heuristic_exception_compat_naming() {
    let dir = tempfile::TempDir::new().unwrap();
    let file_path = dir.path().join("compat_handler.ts");
    let content = "line\n".repeat(60);
    std::fs::write(&file_path, &content).unwrap();

    let domain = make_domain("auth", vec![file_path.to_str().unwrap()], vec![], vec![]);

    let mut candidates = Vec::new();
    detect_exception_candidates(&domain, &mut candidates);

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].type_, CandidateType::Exception);
}

#[test]
fn test_heuristic_decision_comment_pattern() {
    let domain = make_domain(
        "billing",
        vec![],
        vec!["[NOTE] We decided to not deduplicate imported rows"],
        vec![],
    );

    let mut candidates = Vec::new();
    detect_decision_candidates(&domain, &mut candidates);

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].type_, CandidateType::Decision);
    assert!(candidates[0].text.contains("not deduplicate imported rows"));
}

#[test]
fn test_heuristic_business_rule_test_plus_todo() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path().join("invoice.ts");
    let content = "line\n".repeat(60);
    std::fs::write(&source, &content).unwrap();

    let test_file = dir.path().join("invoice.test.ts");
    std::fs::write(&test_file, "test content").unwrap();

    let domain = make_domain(
        "billing",
        vec![source.to_str().unwrap()],
        vec!["[TODO] Invoice is only emitted after full sync completes"],
        vec![test_file.to_str().unwrap()],
    );

    let mut candidates = Vec::new();
    detect_business_rule_candidates(&domain, &mut candidates);

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].type_, CandidateType::BusinessRule);
}

#[test]
fn test_heuristic_max_5_candidates() {
    let dir = tempfile::TempDir::new().unwrap();
    let content = "line\n".repeat(60);

    let mut files = Vec::new();
    for i in 0..10 {
        let path = dir.path().join(format!("legacy_{i}.ts"));
        std::fs::write(&path, &content).unwrap();
        files.push(path.to_str().unwrap().to_string());
    }

    let domain = DomainInfo {
        name: "billing".to_string(),
        files,
        dependencies: Vec::new(),
        models: Vec::new(),
        routes: Vec::new(),
        comments: Vec::new(),
        test_files: Vec::new(),
    };

    let result = generate(&[domain]);
    assert!(result.len() <= MAX_CANDIDATES);
}

#[test]
fn test_heuristic_exclusion_utils() {
    let dir = tempfile::TempDir::new().unwrap();
    let utils_path = dir.path().join("utils.ts");
    let content = "line\n".repeat(60);
    std::fs::write(&utils_path, &content).unwrap();

    assert!(is_excluded_path(utils_path.to_str().unwrap()));
}

#[test]
fn test_heuristic_exclusion_test_only() {
    assert!(is_excluded_path("src/billing/__tests__/invoice.test.ts"));
    assert!(is_excluded_path("src/billing/invoice.test.ts"));
    assert!(is_excluded_path("tests/billing/invoice_test.rs"));
}

#[test]
fn test_heuristic_dedup_same_file_same_type() {
    let mut candidates = vec![
        Candidate {
            id: String::new(),
            domain: "billing".to_string(),
            type_: CandidateType::Exception,
            text: "First".to_string(),
            rationale: String::new(),
            provenance: vec![ProvenanceEntry {
                kind: "file".to_string(),
                ref_: "src/legacy.ts".to_string(),
            }],
            target_note: String::new(),
        },
        Candidate {
            id: String::new(),
            domain: "billing".to_string(),
            type_: CandidateType::Exception,
            text: "Second".to_string(),
            rationale: String::new(),
            provenance: vec![ProvenanceEntry {
                kind: "file".to_string(),
                ref_: "src/legacy.ts".to_string(),
            }],
            target_note: String::new(),
        },
    ];

    deduplicate(&mut candidates);
    assert_eq!(candidates.len(), 1);
}

#[test]
fn test_heuristic_keep_same_file_different_type() {
    let mut candidates = vec![
        Candidate {
            id: String::new(),
            domain: "billing".to_string(),
            type_: CandidateType::Exception,
            text: "Exception".to_string(),
            rationale: String::new(),
            provenance: vec![ProvenanceEntry {
                kind: "file".to_string(),
                ref_: "src/legacy.ts".to_string(),
            }],
            target_note: String::new(),
        },
        Candidate {
            id: String::new(),
            domain: "billing".to_string(),
            type_: CandidateType::Decision,
            text: "Decision".to_string(),
            rationale: String::new(),
            provenance: vec![ProvenanceEntry {
                kind: "file".to_string(),
                ref_: "src/legacy.ts".to_string(),
            }],
            target_note: String::new(),
        },
    ];

    deduplicate(&mut candidates);
    assert_eq!(candidates.len(), 2);
}

#[test]
fn test_heuristic_priority_exception_first() {
    let mut candidates = vec![
        Candidate {
            id: String::new(),
            domain: "billing".to_string(),
            type_: CandidateType::BusinessRule,
            text: "Rule".to_string(),
            rationale: String::new(),
            provenance: vec![ProvenanceEntry {
                kind: "file".to_string(),
                ref_: "a.ts".to_string(),
            }],
            target_note: String::new(),
        },
        Candidate {
            id: String::new(),
            domain: "billing".to_string(),
            type_: CandidateType::Exception,
            text: "Exception".to_string(),
            rationale: String::new(),
            provenance: vec![ProvenanceEntry {
                kind: "file".to_string(),
                ref_: "b.ts".to_string(),
            }],
            target_note: String::new(),
        },
    ];

    prioritize_and_limit(&mut candidates);
    assert_eq!(candidates[0].type_, CandidateType::Exception);
    assert_eq!(candidates[1].type_, CandidateType::BusinessRule);
}

#[test]
fn test_heuristic_provenance_always_present() {
    let dir = tempfile::TempDir::new().unwrap();
    let file_path = dir.path().join("legacy_handler.ts");
    let content = "line\n".repeat(60);
    std::fs::write(&file_path, &content).unwrap();

    let domain = make_domain(
        "auth",
        vec![file_path.to_str().unwrap()],
        vec!["[NOTE] We decided to keep legacy endpoint for compat"],
        vec![],
    );

    let result = generate(&[domain]);
    for c in &result {
        assert!(
            !c.provenance.is_empty(),
            "Candidate {} should have provenance",
            c.id
        );
    }
}

#[test]
fn test_heuristic_empty_scan_no_candidates() {
    let result = generate(&[]);
    assert!(result.is_empty());
}

#[test]
fn test_assign_ids() {
    let mut candidates = vec![
        Candidate {
            id: String::new(),
            domain: "billing".to_string(),
            type_: CandidateType::Exception,
            text: "A".to_string(),
            rationale: String::new(),
            provenance: vec![],
            target_note: String::new(),
        },
        Candidate {
            id: String::new(),
            domain: "billing".to_string(),
            type_: CandidateType::Decision,
            text: "B".to_string(),
            rationale: String::new(),
            provenance: vec![],
            target_note: String::new(),
        },
        Candidate {
            id: String::new(),
            domain: "auth".to_string(),
            type_: CandidateType::BusinessRule,
            text: "C".to_string(),
            rationale: String::new(),
            provenance: vec![],
            target_note: String::new(),
        },
    ];

    assign_ids(&mut candidates);
    assert_eq!(candidates[0].id, "billing-001");
    assert_eq!(candidates[1].id, "billing-002");
    assert_eq!(candidates[2].id, "auth-001");
}

#[test]
fn test_is_generic_text() {
    assert!(is_generic_text("short"));
    assert!(is_generic_text("This module handles billing"));
    assert!(!is_generic_text(
        "We decided to not deduplicate imported rows because clients have homonyms"
    ));
}

#[test]
fn test_truncate_text() {
    assert_eq!(truncate_text("short", 120), "short");
    let long = "a".repeat(200);
    let result = truncate_text(&long, 50);
    assert_eq!(result.len(), 50);
    assert!(result.ends_with("..."));
}

// ── File generation tests (task 015) ──

fn make_candidate(id: &str, domain: &str, type_: CandidateType, text: &str) -> Candidate {
    Candidate {
        id: id.to_string(),
        domain: domain.to_string(),
        type_,
        text: text.to_string(),
        rationale: "Test rationale".to_string(),
        provenance: vec![ProvenanceEntry {
            kind: "file".to_string(),
            ref_: "src/test.ts".to_string(),
        }],
        target_note: format!(".wiki/domains/{}/_overview.md", domain),
    }
}

#[test]
fn test_generate_candidates_file_format() {
    let candidates = vec![
        make_candidate(
            "billing-001",
            "billing",
            CandidateType::Exception,
            "Legacy pricing",
        ),
        make_candidate(
            "billing-002",
            "billing",
            CandidateType::Decision,
            "No dedup",
        ),
    ];

    let md = format_candidates_markdown(&candidates, "en");
    assert!(md.starts_with("# Memory Candidates"));
    assert!(md.contains("## billing"));
    assert!(md.contains("### billing-001"));
    assert!(md.contains("### billing-002"));
}

#[test]
fn test_generate_candidates_all_fields_present() {
    let candidates = vec![make_candidate(
        "billing-001",
        "billing",
        CandidateType::Exception,
        "E1",
    )];

    let md = format_candidates_markdown(&candidates, "en");
    assert!(md.contains("**status**: pending"));
    assert!(md.contains("**type**: exception"));
    assert!(md.contains("**confidence**: inferred"));
    assert!(md.contains("**provenance**:"));
    assert!(md.contains("file: src/test.ts"));
    assert!(md.contains("**rationale**: Test rationale"));
    assert!(md.contains("**target**:"));
    assert!(md.contains("> E1"));
    assert!(md.contains("**Action**"));
}

#[test]
fn test_generate_candidates_zero_candidates() {
    let md = format_candidates_markdown(&[], "en");
    assert!(md.contains("No candidates detected."));
}

#[test]
fn test_generate_candidates_status_pending() {
    let candidates = vec![make_candidate(
        "billing-001",
        "billing",
        CandidateType::Exception,
        "E1",
    )];
    let md = format_candidates_markdown(&candidates, "en");
    assert!(md.contains("**status**: pending"));
}

#[test]
fn test_write_candidates_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let wiki_dir = dir.path();

    let candidates = vec![make_candidate(
        "billing-001",
        "billing",
        CandidateType::Exception,
        "E1",
    )];

    write_candidates_file(wiki_dir, &candidates, "en").unwrap();

    let path = wiki_dir.join("_candidates.md");
    assert!(path.exists());
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("billing-001"));
}

#[test]
fn test_write_candidates_empty_no_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let wiki_dir = dir.path();

    write_candidates_file(wiki_dir, &[], "en").unwrap();

    let path = wiki_dir.join("_candidates.md");
    assert!(!path.exists());
}

#[test]
fn test_write_candidates_idempotent() {
    let dir = tempfile::TempDir::new().unwrap();
    let wiki_dir = dir.path();

    let candidates = vec![make_candidate(
        "billing-001",
        "billing",
        CandidateType::Exception,
        "E1",
    )];

    // Write once
    write_candidates_file(wiki_dir, &candidates, "en").unwrap();

    // Modify the file to mark one as confirmed
    let path = wiki_dir.join("_candidates.md");
    let content = std::fs::read_to_string(&path).unwrap();
    let modified = content.replace("**status**: pending", "**status**: confirmed");
    std::fs::write(&path, &modified).unwrap();

    // Write again — should not overwrite confirmed candidate
    write_candidates_file(wiki_dir, &candidates, "en").unwrap();

    let final_content = std::fs::read_to_string(&path).unwrap();
    // If the candidate was already confirmed, a new file with 0 pending candidates
    // should not overwrite
    assert!(!final_content.contains("billing-001") || final_content.contains("confirmed"));
}

#[test]
fn test_parse_processed_ids() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("_candidates.md");
    let content = r#"# Memory Candidates

## billing

### billing-001

- **status**: confirmed
- **type**: exception

### billing-002

- **status**: pending
- **type**: decision
"#;
    std::fs::write(&path, content).unwrap();

    let ids = parse_processed_ids(&path).unwrap();
    assert!(ids.contains("billing-001"));
    assert!(!ids.contains("billing-002"));
}

#[test]
fn test_format_candidates_french() {
    let candidates = vec![make_candidate(
        "billing-001",
        "billing",
        CandidateType::Exception,
        "E1",
    )];

    let md = format_candidates_markdown(&candidates, "fr");
    assert!(md.starts_with("# Candidats mémoire"));
    assert!(md.contains("confirmer, rejeter ou reformuler"));
    assert!(!md.contains("Memory Candidates"));
}

#[test]
fn test_format_candidates_french_empty() {
    let md = format_candidates_markdown(&[], "fr");
    assert!(md.contains("Aucun candidat détecté"));
    assert!(!md.contains("No candidates detected"));
}
