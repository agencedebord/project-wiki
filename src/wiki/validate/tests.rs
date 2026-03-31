use super::domains::{check_domain_name_coherence, check_missing_dependencies};
use super::links::{check_broken_links, check_dead_references, check_deprecated_references};
use super::memory_items::check_memory_items;
use super::migration_status::check_migration_status;
use super::notes::{check_confidence_ratio, check_staleness};
use crate::wiki::note::{
    Confidence, MemoryItem, MemoryItemSource, MemoryItemStatus, MemoryItemType, WikiNote,
};
use chrono::NaiveDate;
use tempfile::TempDir;

fn make_note(
    path: &str,
    confidence: Confidence,
    last_updated: Option<NaiveDate>,
    related_files: Vec<String>,
    deprecated: bool,
) -> WikiNote {
    WikiNote {
        path: path.to_string(),
        domain: "test".to_string(),
        confidence,
        last_updated,
        related_files,
        deprecated,
        title: "Test".to_string(),
        content: String::new(),
        memory_items: Vec::new(),
    }
}

fn make_item(id: &str, type_: MemoryItemType, confidence: Confidence) -> MemoryItem {
    MemoryItem {
        id: id.to_string(),
        type_,
        text: "Test item".to_string(),
        confidence,
        related_files: Vec::new(),
        sources: vec![MemoryItemSource {
            kind: "file".to_string(),
            ref_: "src/test.ts".to_string(),
            line: None,
        }],
        status: MemoryItemStatus::Active,
        last_reviewed: None,
    }
}

fn make_note_with_items(path: &str, confidence: Confidence, items: Vec<MemoryItem>) -> WikiNote {
    WikiNote {
        path: path.to_string(),
        domain: "test".to_string(),
        confidence,
        last_updated: None,
        related_files: Vec::new(),
        deprecated: false,
        title: "Test".to_string(),
        content: String::new(),
        memory_items: items,
    }
}

// ─── check_broken_links ───

#[test]
fn broken_links_detects_missing_target() {
    let dir = TempDir::new().unwrap();
    let wiki = dir.path().join(".wiki");
    std::fs::create_dir_all(wiki.join("domains/billing")).unwrap();

    std::fs::write(
        wiki.join("domains/billing/_overview.md"),
        "# Billing\n\nSee [details](./nonexistent.md) for more.\n",
    )
    .unwrap();

    let md_files = vec![(
        "domains/billing/_overview.md".to_string(),
        std::fs::read_to_string(wiki.join("domains/billing/_overview.md")).unwrap(),
    )];

    let broken = check_broken_links(&md_files, &wiki).unwrap();

    assert_eq!(broken.len(), 1);
    assert_eq!(broken[0].0, "domains/billing/_overview.md");
    assert_eq!(broken[0].1, "./nonexistent.md");
}

#[test]
fn broken_links_skips_external_urls() {
    let dir = TempDir::new().unwrap();
    let md_files = vec![(
        "test.md".to_string(),
        "See [Google](https://google.com) and [local](#anchor)".to_string(),
    )];

    let broken = check_broken_links(&md_files, dir.path()).unwrap();
    assert!(broken.is_empty());
}

// ─── check_confidence_ratio ───

#[test]
fn confidence_ratio_with_mixed_notes() {
    let notes = vec![
        make_note("a.md", Confidence::Confirmed, None, vec![], false),
        make_note("b.md", Confidence::Inferred, None, vec![], false),
        make_note("c.md", Confidence::NeedsValidation, None, vec![], false),
        make_note("d.md", Confidence::Verified, None, vec![], false),
    ];

    let (low, total, pct) = check_confidence_ratio(&notes);
    assert_eq!(total, 4);
    assert_eq!(low, 2);
    assert!((pct - 50.0).abs() < 0.01);
}

#[test]
fn confidence_ratio_empty_notes() {
    let (low, total, pct) = check_confidence_ratio(&[]);
    assert_eq!(low, 0);
    assert_eq!(total, 0);
    assert!((pct - 0.0).abs() < 0.01);
}

#[test]
fn confidence_ratio_all_confirmed() {
    let notes = vec![
        make_note("a.md", Confidence::Confirmed, None, vec![], false),
        make_note("b.md", Confidence::Verified, None, vec![], false),
    ];

    let (low, total, pct) = check_confidence_ratio(&notes);
    assert_eq!(low, 0);
    assert_eq!(total, 2);
    assert!((pct - 0.0).abs() < 0.01);
}

// ─── check_staleness ───

#[test]
fn staleness_flags_old_notes() {
    let old_date = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
    let recent_date = chrono::Utc::now().date_naive();

    let notes = vec![
        make_note(
            "old.md",
            Confidence::Confirmed,
            Some(old_date),
            vec![],
            false,
        ),
        make_note(
            "new.md",
            Confidence::Confirmed,
            Some(recent_date),
            vec![],
            false,
        ),
        make_note("no-date.md", Confidence::Confirmed, None, vec![], false),
    ];

    let stale = check_staleness(&notes, 30);
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].0, "old.md");
    assert!(stale[0].1 > 30);
}

#[test]
fn staleness_empty_when_all_recent() {
    let today = chrono::Utc::now().date_naive();
    let notes = vec![make_note(
        "a.md",
        Confidence::Confirmed,
        Some(today),
        vec![],
        false,
    )];

    let stale = check_staleness(&notes, 30);
    assert!(stale.is_empty());
}

// ─── check_dead_references ───

#[test]
fn dead_references_detects_missing_file() {
    let notes = vec![make_note(
        "note.md",
        Confidence::Confirmed,
        None,
        vec!["/nonexistent/path/to/file.ts".to_string()],
        false,
    )];

    let dead = check_dead_references(&notes);
    assert_eq!(dead.len(), 1);
    assert_eq!(dead[0].0, "note.md");
    assert_eq!(dead[0].1, "/nonexistent/path/to/file.ts");
}

#[test]
fn dead_references_passes_with_existing_file() {
    let dir = TempDir::new().unwrap();
    let real_file = dir.path().join("real.ts");
    std::fs::write(&real_file, "export {}").unwrap();

    let notes = vec![make_note(
        "note.md",
        Confidence::Confirmed,
        None,
        vec![real_file.to_string_lossy().to_string()],
        false,
    )];

    let dead = check_dead_references(&notes);
    assert!(dead.is_empty());
}

// ─── check_deprecated_references ───

#[test]
fn deprecated_references_detected() {
    let dir = TempDir::new().unwrap();
    let wiki = dir.path().join(".wiki");
    std::fs::create_dir_all(wiki.join("domains/billing")).unwrap();

    let notes = vec![
        make_note(
            &wiki.join("domains/billing/old-api.md").to_string_lossy(),
            Confidence::Confirmed,
            None,
            vec![],
            true,
        ),
        make_note(
            &wiki.join("domains/billing/_overview.md").to_string_lossy(),
            Confidence::Confirmed,
            None,
            vec![],
            false,
        ),
    ];

    let md_files = vec![(
        "domains/billing/_overview.md".to_string(),
        "See [old api](./old-api.md) for legacy info.".to_string(),
    )];

    let refs = check_deprecated_references(&notes, &md_files, &wiki).unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].0, "domains/billing/_overview.md");
    assert!(refs[0].1.contains("old-api.md"));
}

#[test]
fn deprecated_references_empty_when_no_deprecated() {
    let dir = TempDir::new().unwrap();
    let wiki = dir.path().join(".wiki");

    let notes = vec![make_note(
        &wiki.join("domains/billing/_overview.md").to_string_lossy(),
        Confidence::Confirmed,
        None,
        vec![],
        false,
    )];

    let md_files = vec![(
        "domains/billing/_overview.md".to_string(),
        "Normal content with [link](./other.md).".to_string(),
    )];

    let refs = check_deprecated_references(&notes, &md_files, &wiki).unwrap();
    assert!(refs.is_empty());
}

// ─── check_memory_items ───

#[test]
fn validate_duplicate_id_same_note() {
    let notes = vec![make_note_with_items(
        "billing.md",
        Confidence::Confirmed,
        vec![
            make_item(
                "billing-001",
                MemoryItemType::Decision,
                Confidence::Confirmed,
            ),
            make_item(
                "billing-001",
                MemoryItemType::Exception,
                Confidence::Confirmed,
            ),
        ],
    )];

    let (errors, _) = check_memory_items(&notes);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("duplicate item id 'billing-001'")),
        "Expected duplicate id error, got: {:?}",
        errors
    );
}

#[test]
fn validate_duplicate_id_across_notes() {
    let notes = vec![
        make_note_with_items(
            "billing.md",
            Confidence::Confirmed,
            vec![make_item(
                "shared-001",
                MemoryItemType::Decision,
                Confidence::Confirmed,
            )],
        ),
        make_note_with_items(
            "auth.md",
            Confidence::Confirmed,
            vec![make_item(
                "shared-001",
                MemoryItemType::Exception,
                Confidence::Confirmed,
            )],
        ),
    ];

    let (errors, _) = check_memory_items(&notes);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("shared-001") && e.contains("already used")),
        "Expected cross-note duplicate error, got: {:?}",
        errors
    );
}

#[test]
fn validate_source_empty_kind() {
    let mut item = make_item(
        "billing-001",
        MemoryItemType::Decision,
        Confidence::Confirmed,
    );
    item.sources = vec![MemoryItemSource {
        kind: String::new(),
        ref_: "src/test.ts".to_string(),
        line: None,
    }];

    let notes = vec![make_note_with_items(
        "billing.md",
        Confidence::Confirmed,
        vec![item],
    )];
    let (errors, _) = check_memory_items(&notes);
    assert!(
        errors.iter().any(|e| e.contains("empty kind")),
        "Expected empty kind error, got: {:?}",
        errors
    );
}

#[test]
fn validate_source_empty_ref() {
    let mut item = make_item(
        "billing-001",
        MemoryItemType::Decision,
        Confidence::Confirmed,
    );
    item.sources = vec![MemoryItemSource {
        kind: "file".to_string(),
        ref_: String::new(),
        line: None,
    }];

    let notes = vec![make_note_with_items(
        "billing.md",
        Confidence::Confirmed,
        vec![item],
    )];
    let (errors, _) = check_memory_items(&notes);
    assert!(
        errors.iter().any(|e| e.contains("empty ref")),
        "Expected empty ref error, got: {:?}",
        errors
    );
}

#[test]
fn validate_confidence_inconsistency_warning() {
    let notes = vec![make_note_with_items(
        "billing.md",
        Confidence::Inferred,
        vec![make_item(
            "billing-001",
            MemoryItemType::Decision,
            Confidence::Confirmed,
        )],
    )];

    let (_, warnings) = check_memory_items(&notes);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("confirmed") && w.contains("inferred")),
        "Expected confidence inconsistency warning, got: {:?}",
        warnings
    );
}

#[test]
fn validate_future_date_warning() {
    let mut item = make_item(
        "billing-001",
        MemoryItemType::Decision,
        Confidence::Confirmed,
    );
    item.last_reviewed = Some("2030-01-01".to_string());

    let notes = vec![make_note_with_items(
        "billing.md",
        Confidence::Confirmed,
        vec![item],
    )];
    let (_, warnings) = check_memory_items(&notes);
    assert!(
        warnings.iter().any(|w| w.contains("future")),
        "Expected future date warning, got: {:?}",
        warnings
    );
}

#[test]
fn validate_item_without_sources_warning() {
    let mut item = make_item(
        "billing-001",
        MemoryItemType::Decision,
        Confidence::Confirmed,
    );
    item.sources = Vec::new();

    let notes = vec![make_note_with_items(
        "billing.md",
        Confidence::Confirmed,
        vec![item],
    )];
    let (_, warnings) = check_memory_items(&notes);
    assert!(
        warnings.iter().any(|w| w.contains("no sources")),
        "Expected no sources warning, got: {:?}",
        warnings
    );
}

#[test]
fn validate_all_deprecated_items_warning() {
    let mut item = make_item(
        "billing-001",
        MemoryItemType::Decision,
        Confidence::Confirmed,
    );
    item.status = MemoryItemStatus::Deprecated;

    let notes = vec![make_note_with_items(
        "billing.md",
        Confidence::Confirmed,
        vec![item],
    )];
    let (_, warnings) = check_memory_items(&notes);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("all memory items are deprecated")),
        "Expected all-deprecated warning, got: {:?}",
        warnings
    );
}

#[test]
fn validate_notes_without_items_pass() {
    let notes = vec![
        make_note("a.md", Confidence::Confirmed, None, vec![], false),
        make_note("b.md", Confidence::Inferred, None, vec![], false),
    ];

    let (errors, warnings) = check_memory_items(&notes);
    assert!(errors.is_empty());
    assert!(warnings.is_empty());
}

#[test]
fn validate_valid_memory_items_pass() {
    let notes = vec![make_note_with_items(
        "billing.md",
        Confidence::Confirmed,
        vec![
            make_item(
                "billing-001",
                MemoryItemType::Exception,
                Confidence::Confirmed,
            ),
            make_item(
                "billing-002",
                MemoryItemType::Decision,
                Confidence::Verified,
            ),
        ],
    )];

    let (errors, warnings) = check_memory_items(&notes);
    assert!(errors.is_empty(), "Unexpected errors: {:?}", errors);
    assert!(warnings.is_empty(), "Unexpected warnings: {:?}", warnings);
}

// ─── check_domain_name_coherence ───

#[test]
fn validate_domain_name_coherence_mismatch() {
    let notes = vec![WikiNote {
        path: ".wiki/domains/billing/_overview.md".to_string(),
        domain: "invoicing".to_string(), // mismatch: folder is "billing"
        confidence: Confidence::Confirmed,
        last_updated: None,
        related_files: Vec::new(),
        deprecated: false,
        title: "Test".to_string(),
        content: String::new(),
        memory_items: Vec::new(),
    }];

    let errors = check_domain_name_coherence(&notes);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("billing"));
    assert!(errors[0].contains("invoicing"));
}

#[test]
fn validate_domain_name_coherence_ok() {
    let notes = vec![WikiNote {
        path: ".wiki/domains/billing/_overview.md".to_string(),
        domain: "billing".to_string(),
        confidence: Confidence::Confirmed,
        last_updated: None,
        related_files: Vec::new(),
        deprecated: false,
        title: "Test".to_string(),
        content: String::new(),
        memory_items: Vec::new(),
    }];

    let errors = check_domain_name_coherence(&notes);
    assert!(errors.is_empty());
}

// ─── check_missing_dependencies ───

#[test]
fn validate_missing_dependency_warning() {
    let notes = vec![WikiNote {
        path: ".wiki/domains/billing/_overview.md".to_string(),
        domain: "billing".to_string(),
        confidence: Confidence::Confirmed,
        last_updated: None,
        related_files: Vec::new(),
        deprecated: false,
        title: "Billing".to_string(),
        content: "## Dependencies\n\n- [payments](../payments/_overview.md)\n".to_string(),
        memory_items: Vec::new(),
    }];

    let missing = check_missing_dependencies(&notes);
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].1, "payments");
}

#[test]
fn validate_dependency_exists_no_warning() {
    let notes = vec![
        WikiNote {
            path: ".wiki/domains/billing/_overview.md".to_string(),
            domain: "billing".to_string(),
            confidence: Confidence::Confirmed,
            last_updated: None,
            related_files: Vec::new(),
            deprecated: false,
            title: "Billing".to_string(),
            content: "## Dependencies\n\n- [auth](../auth/_overview.md)\n".to_string(),
            memory_items: Vec::new(),
        },
        WikiNote {
            path: ".wiki/domains/auth/_overview.md".to_string(),
            domain: "auth".to_string(),
            confidence: Confidence::Confirmed,
            last_updated: None,
            related_files: Vec::new(),
            deprecated: false,
            title: "Auth".to_string(),
            content: String::new(),
            memory_items: Vec::new(),
        },
    ];

    let missing = check_missing_dependencies(&notes);
    assert!(missing.is_empty());
}

#[test]
fn validate_mixed_notes_with_and_without_items() {
    let notes = vec![
        make_note("a.md", Confidence::Confirmed, None, vec![], false),
        make_note_with_items(
            "b.md",
            Confidence::Confirmed,
            vec![make_item(
                "b-001",
                MemoryItemType::BusinessRule,
                Confidence::Confirmed,
            )],
        ),
    ];

    let (errors, warnings) = check_memory_items(&notes);
    assert!(errors.is_empty());
    assert!(warnings.is_empty());
}

// ─── check_migration_status ───

#[test]
fn migration_status_all_notes_have_memory_items() {
    let notes = vec![
        make_note_with_items(
            "billing.md",
            Confidence::Confirmed,
            vec![make_item(
                "billing-001",
                MemoryItemType::Decision,
                Confidence::Confirmed,
            )],
        ),
        make_note_with_items(
            "auth.md",
            Confidence::Confirmed,
            vec![make_item(
                "auth-001",
                MemoryItemType::BusinessRule,
                Confidence::Verified,
            )],
        ),
    ];

    let status = check_migration_status(&notes);
    assert_eq!(status.total, 2);
    assert_eq!(status.without_items, 0);
    assert!(status.legacy_paths.is_empty());
}

#[test]
fn migration_status_mixed_notes() {
    let notes = vec![
        make_note("legacy.md", Confidence::Confirmed, None, vec![], false),
        make_note_with_items(
            "migrated.md",
            Confidence::Confirmed,
            vec![make_item(
                "migrated-001",
                MemoryItemType::Decision,
                Confidence::Confirmed,
            )],
        ),
        make_note("also-legacy.md", Confidence::Inferred, None, vec![], false),
    ];

    let status = check_migration_status(&notes);
    assert_eq!(status.total, 3);
    assert_eq!(status.without_items, 2);
    assert_eq!(status.legacy_paths.len(), 2);
    assert!(status.legacy_paths.contains(&"legacy.md".to_string()));
    assert!(status.legacy_paths.contains(&"also-legacy.md".to_string()));
}

#[test]
fn migration_status_no_notes_have_memory_items() {
    let notes = vec![
        make_note("a.md", Confidence::Confirmed, None, vec![], false),
        make_note("b.md", Confidence::Inferred, None, vec![], false),
        make_note("c.md", Confidence::Verified, None, vec![], false),
    ];

    let status = check_migration_status(&notes);
    assert_eq!(status.total, 3);
    assert_eq!(status.without_items, 3);
    assert_eq!(status.legacy_paths.len(), 3);
}

#[test]
fn migration_status_empty_notes() {
    let notes: Vec<WikiNote> = Vec::new();

    let status = check_migration_status(&notes);
    assert_eq!(status.total, 0);
    assert_eq!(status.without_items, 0);
    assert!(status.legacy_paths.is_empty());
}
