use super::prioritize::prioritize_memory_items;
use super::render::{compact_summary, extract_bullet_points, extract_sections};
use super::{ContextJsonItem, ContextJsonOutput, MAX_CONTEXT_LEN};
use crate::wiki::note::{
    Confidence, MemoryItem, MemoryItemSource, MemoryItemStatus, MemoryItemType, WikiNote,
};
use chrono::NaiveDate;

fn make_note(domain: &str, confidence: Confidence, content: &str) -> WikiNote {
    WikiNote {
        path: format!(".wiki/domains/{}/_overview.md", domain),
        domain: domain.to_string(),
        confidence,
        last_updated: Some(NaiveDate::from_ymd_opt(2026, 3, 28).unwrap()),
        related_files: vec![format!("src/{}/main.ts", domain)],
        deprecated: false,
        title: format!("{} overview", domain),
        content: content.to_string(),
        memory_items: Vec::new(),
    }
}

fn make_item(
    id: &str,
    type_: MemoryItemType,
    text: &str,
    confidence: Confidence,
) -> MemoryItem {
    MemoryItem {
        id: id.to_string(),
        type_,
        text: text.to_string(),
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

fn make_note_with_items(
    domain: &str,
    confidence: Confidence,
    items: Vec<MemoryItem>,
) -> WikiNote {
    WikiNote {
        path: format!(".wiki/domains/{}/_overview.md", domain),
        domain: domain.to_string(),
        confidence,
        last_updated: Some(NaiveDate::from_ymd_opt(2026, 3, 28).unwrap()),
        related_files: vec![format!("src/{}/main.ts", domain)],
        deprecated: false,
        title: format!("{} overview", domain),
        content: "## Dependencies\n- payments\n- taxes\n".to_string(),
        memory_items: items,
    }
}

// ─── Fallback tests (notes without memory_items) ───

#[test]
fn compact_summary_includes_domain_info() {
    let note = make_note(
        "billing",
        Confidence::Confirmed,
        "## Key behaviors\n- Generates invoices\n- Handles refunds\n",
    );
    let summary = compact_summary(&note, "billing", "src/billing/main.ts");

    assert!(summary.contains("[project-wiki] Domain: billing"));
    assert!(summary.contains("confirmed"));
    assert!(summary.contains("2026-03-28"));
}

#[test]
fn compact_summary_includes_behaviors() {
    let note = make_note(
        "billing",
        Confidence::Confirmed,
        "## Key behaviors\n- Generates invoices\n- Handles refunds\n",
    );
    let summary = compact_summary(&note, "billing", "src/billing/main.ts");

    assert!(summary.contains("Generates invoices"));
    assert!(summary.contains("Handles refunds"));
}

#[test]
fn compact_summary_includes_business_rules() {
    let note = make_note(
        "billing",
        Confidence::Confirmed,
        "## Business rules\n- No dedup on import\n",
    );
    let summary = compact_summary(&note, "billing", "src/billing/main.ts");

    assert!(summary.contains("No dedup on import"));
}

#[test]
fn compact_summary_includes_related_files() {
    let note = make_note("billing", Confidence::Confirmed, "# Billing\n");
    let summary = compact_summary(&note, "billing", "src/billing/main.ts");

    assert!(summary.contains("src/billing/main.ts"));
}

#[test]
fn compact_summary_warns_on_low_confidence() {
    let note = make_note("billing", Confidence::NeedsValidation, "# Billing\n");
    let summary = compact_summary(&note, "billing", "src/billing/main.ts");

    assert!(summary.contains("WARNING"));
    assert!(summary.contains("needs-validation"));
}

#[test]
fn compact_summary_no_warning_on_confirmed() {
    let note = make_note("billing", Confidence::Confirmed, "# Billing\n");
    let summary = compact_summary(&note, "billing", "src/billing/main.ts");

    assert!(!summary.contains("WARNING"));
}

#[test]
fn compact_summary_truncates_long_content() {
    let mut long_content = String::new();
    for section in &[
        "Key behaviors",
        "Business rules",
        "Dependencies",
        "Architecture notes",
    ] {
        long_content.push_str(&format!("## {}\n", section));
        for i in 0..50 {
            long_content.push_str(&format!(
                "- Item {} in {} with a very long description that adds significant length to the output to ensure we exceed the truncation threshold eventually\n",
                i, section
            ));
        }
    }
    let mut note = make_note("billing", Confidence::Confirmed, &long_content);
    note.related_files = (0..50)
        .map(|i| format!("src/billing/very/deep/nested/module_{}/handler.ts", i))
        .collect();
    let summary = compact_summary(&note, "billing", "src/billing/main.ts");

    assert!(summary.len() <= MAX_CONTEXT_LEN);
    assert!(summary.contains("[... truncated]"));
}

#[test]
fn extract_sections_parses_headings() {
    let content = "## Description\nSome text.\n\n## Key behaviors\n- One\n- Two\n";
    let sections = extract_sections(content);

    assert!(sections.contains_key("description"));
    assert!(sections.contains_key("key behaviors"));
    assert!(sections["key behaviors"].contains("- One"));
}

#[test]
fn extract_bullet_points_limits_count() {
    let body = "- One\n- Two\n- Three\n- Four\n- Five\n- Six\n";
    let items = extract_bullet_points(body, 3);
    assert_eq!(items.len(), 3);
}

#[test]
fn extract_bullet_points_skips_placeholders() {
    let body = "- _None detected._\n- Real item\n";
    let items = extract_bullet_points(body, 10);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0], "Real item");
}

// ─── V1 tests (notes with memory_items) ───

#[test]
fn context_v1_prioritize_exception_first() {
    let items = vec![
        make_item(
            "b-001",
            MemoryItemType::BusinessRule,
            "Rule A",
            Confidence::Confirmed,
        ),
        make_item(
            "b-002",
            MemoryItemType::Decision,
            "Decision B",
            Confidence::Confirmed,
        ),
        make_item(
            "b-003",
            MemoryItemType::Exception,
            "Exception C",
            Confidence::Confirmed,
        ),
    ];
    let note = make_note_with_items("billing", Confidence::Confirmed, items);
    let summary = compact_summary(&note, "billing", "src/billing/main.ts");

    let exc_pos = summary.find("[exception]").unwrap();
    let dec_pos = summary.find("[decision]").unwrap();
    let rule_pos = summary.find("[business_rule]").unwrap();
    assert!(exc_pos < dec_pos, "exception should come before decision");
    assert!(
        dec_pos < rule_pos,
        "decision should come before business_rule"
    );
}

#[test]
fn context_v1_secondary_sort_by_confidence() {
    let items = vec![
        make_item(
            "b-001",
            MemoryItemType::Decision,
            "Inferred decision",
            Confidence::Inferred,
        ),
        make_item(
            "b-002",
            MemoryItemType::Decision,
            "Confirmed decision",
            Confidence::Confirmed,
        ),
    ];
    let note = make_note_with_items("billing", Confidence::Confirmed, items);
    let summary = compact_summary(&note, "billing", "src/billing/main.ts");

    let confirmed_pos = summary.find("Confirmed decision").unwrap();
    let inferred_pos = summary.find("Inferred decision").unwrap();
    assert!(
        confirmed_pos < inferred_pos,
        "confirmed should come before inferred"
    );
}

#[test]
fn context_v1_secondary_sort_by_related_file() {
    let mut item_related = make_item(
        "b-001",
        MemoryItemType::Decision,
        "Related decision",
        Confidence::Confirmed,
    );
    item_related.related_files = vec!["src/billing/invoice.ts".to_string()];

    let item_unrelated = make_item(
        "b-002",
        MemoryItemType::Decision,
        "Unrelated decision",
        Confidence::Confirmed,
    );

    let note = make_note_with_items(
        "billing",
        Confidence::Confirmed,
        vec![item_unrelated, item_related],
    );
    let summary = compact_summary(&note, "billing", "src/billing/invoice.ts");

    let related_pos = summary.find("Related decision").unwrap();
    let unrelated_pos = summary.find("Unrelated decision").unwrap();
    assert!(
        related_pos < unrelated_pos,
        "related file match should come first"
    );
}

#[test]
fn context_v1_limit_3_items() {
    let items = vec![
        make_item(
            "b-001",
            MemoryItemType::Exception,
            "Exc 1",
            Confidence::Confirmed,
        ),
        make_item(
            "b-002",
            MemoryItemType::Decision,
            "Dec 2",
            Confidence::Confirmed,
        ),
        make_item(
            "b-003",
            MemoryItemType::BusinessRule,
            "Rule 3",
            Confidence::Confirmed,
        ),
        make_item(
            "b-004",
            MemoryItemType::BusinessRule,
            "Rule 4",
            Confidence::Confirmed,
        ),
        make_item(
            "b-005",
            MemoryItemType::BusinessRule,
            "Rule 5",
            Confidence::Confirmed,
        ),
    ];
    let note = make_note_with_items("billing", Confidence::Confirmed, items);
    let summary = compact_summary(&note, "billing", "src/billing/main.ts");

    assert!(summary.contains("Exc 1"));
    assert!(summary.contains("Dec 2"));
    assert!(summary.contains("Rule 3"));
    assert!(!summary.contains("Rule 4"));
    assert!(!summary.contains("Rule 5"));
    assert!(summary.contains("(+2 more items)"));
}

#[test]
fn context_v1_format_type_and_confidence_brackets() {
    let items = vec![make_item(
        "b-001",
        MemoryItemType::Exception,
        "Client X uses old calc",
        Confidence::Confirmed,
    )];
    let note = make_note_with_items("billing", Confidence::Confirmed, items);
    let summary = compact_summary(&note, "billing", "src/billing/main.ts");

    assert!(summary.contains("[exception] Client X uses old calc [confirmed]"));
}

#[test]
fn context_v1_warning_low_confidence_items() {
    let items = vec![
        make_item(
            "b-001",
            MemoryItemType::Decision,
            "Dec A",
            Confidence::Confirmed,
        ),
        make_item(
            "b-002",
            MemoryItemType::BusinessRule,
            "Rule B",
            Confidence::Inferred,
        ),
    ];
    let note = make_note_with_items("billing", Confidence::Confirmed, items);
    let summary = compact_summary(&note, "billing", "src/billing/main.ts");

    assert!(summary.contains("WARNING"));
    assert!(summary.contains("1 item(s) have low confidence"));
}

#[test]
fn context_v1_no_warning_all_confirmed() {
    let items = vec![make_item(
        "b-001",
        MemoryItemType::Decision,
        "Dec A",
        Confidence::Confirmed,
    )];
    let note = make_note_with_items("billing", Confidence::Confirmed, items);
    let summary = compact_summary(&note, "billing", "src/billing/main.ts");

    assert!(!summary.contains("WARNING"));
}

#[test]
fn context_v1_includes_dependencies() {
    let items = vec![make_item(
        "b-001",
        MemoryItemType::Decision,
        "Dec A",
        Confidence::Confirmed,
    )];
    let note = make_note_with_items("billing", Confidence::Confirmed, items);
    let summary = compact_summary(&note, "billing", "src/billing/main.ts");

    assert!(summary.contains("Dependencies: payments, taxes"));
}

#[test]
fn context_v1_filters_deprecated_items() {
    let mut deprecated_item = make_item(
        "b-001",
        MemoryItemType::Exception,
        "Old exception",
        Confidence::Confirmed,
    );
    deprecated_item.status = MemoryItemStatus::Deprecated;

    let active_item = make_item(
        "b-002",
        MemoryItemType::Decision,
        "Active decision",
        Confidence::Confirmed,
    );

    let note = make_note_with_items(
        "billing",
        Confidence::Confirmed,
        vec![deprecated_item, active_item],
    );
    let summary = compact_summary(&note, "billing", "src/billing/main.ts");

    assert!(!summary.contains("Old exception"));
    assert!(summary.contains("Active decision"));
}

#[test]
fn context_fallback_when_no_memory_items() {
    let note = make_note(
        "billing",
        Confidence::Confirmed,
        "## Key behaviors\n- Generates invoices\n## Business rules\n- No dedup\n",
    );
    let summary = compact_summary(&note, "billing", "src/billing/main.ts");

    // Fallback should show markdown sections, not "Memory:" header
    assert!(!summary.contains("Memory:"));
    assert!(summary.contains("Key behaviors:"));
    assert!(summary.contains("Business rules:"));
}

// ─── Prioritization unit tests ───

#[test]
fn prioritize_respects_type_order() {
    let items = vec![
        make_item(
            "1",
            MemoryItemType::BusinessRule,
            "Rule",
            Confidence::Confirmed,
        ),
        make_item("2", MemoryItemType::Exception, "Exc", Confidence::Confirmed),
        make_item("3", MemoryItemType::Decision, "Dec", Confidence::Confirmed),
    ];

    let result = prioritize_memory_items(&items, "", 3);
    assert_eq!(result[0].type_, MemoryItemType::Exception);
    assert_eq!(result[1].type_, MemoryItemType::Decision);
    assert_eq!(result[2].type_, MemoryItemType::BusinessRule);
}

#[test]
fn prioritize_filters_deprecated() {
    let mut dep = make_item("1", MemoryItemType::Exception, "Old", Confidence::Confirmed);
    dep.status = MemoryItemStatus::Deprecated;
    let active = make_item("2", MemoryItemType::Decision, "New", Confidence::Confirmed);

    let items = vec![dep, active];
    let result = prioritize_memory_items(&items, "", 3);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, "2");
}

#[test]
fn prioritize_respects_max() {
    let items: Vec<MemoryItem> = (0..10)
        .map(|i| {
            make_item(
                &format!("b-{:03}", i),
                MemoryItemType::BusinessRule,
                &format!("Rule {}", i),
                Confidence::Confirmed,
            )
        })
        .collect();

    let result = prioritize_memory_items(&items, "", 3);
    assert_eq!(result.len(), 3);
}

// ─── Snapshot tests: exact text format non-regression ───

#[test]
fn snapshot_v1_full_context_output() {
    let items = vec![
        make_item(
            "b-001",
            MemoryItemType::Exception,
            "Client X uses legacy pricing engine",
            Confidence::Confirmed,
        ),
        make_item(
            "b-002",
            MemoryItemType::Decision,
            "No dedup on CSV import",
            Confidence::Confirmed,
        ),
        make_item(
            "b-003",
            MemoryItemType::BusinessRule,
            "TVA always included in displayed prices",
            Confidence::Inferred,
        ),
    ];
    let note = make_note_with_items("billing", Confidence::Confirmed, items);
    let summary = compact_summary(&note, "billing", "src/billing/main.ts");

    let expected = "\
[project-wiki] Domain: billing (confidence: confirmed, updated: 2026-03-28)
Memory:
  [exception] Client X uses legacy pricing engine [confirmed]
  [decision] No dedup on CSV import [confirmed]
  [business_rule] TVA always included in displayed prices [inferred]
Dependencies: payments, taxes
Related files: src/billing/main.ts
WARNING: 1 item(s) have low confidence — verify before relying on them.";

    assert_eq!(summary, expected);
}

#[test]
fn snapshot_fallback_format_without_memory_items() {
    let note = make_note(
        "auth",
        Confidence::Confirmed,
        "## Key behaviors\n- JWT tokens expire after 1h\n- Refresh tokens are rotated\n\
## Business rules\n- Max 5 failed login attempts\n\
## Dependencies\n- user-service\n- redis\n",
    );
    let summary = compact_summary(&note, "auth", "src/auth/login.ts");

    let expected = "\
[project-wiki] Domain: auth (confidence: confirmed, updated: 2026-03-28)
Key behaviors: JWT tokens expire after 1h — Refresh tokens are rotated
Business rules: Max 5 failed login attempts
Dependencies: user-service, redis
Related files: src/auth/main.ts";

    assert_eq!(summary, expected);
}

#[test]
fn snapshot_truncation_preserves_marker() {
    // Build a note that will exceed MAX_CONTEXT_LEN (2000 chars)
    let mut items = Vec::new();
    for i in 0..3 {
        items.push(make_item(
            &format!("b-{:03}", i),
            MemoryItemType::BusinessRule,
            &format!(
                "Very long business rule number {} that contains a tremendous amount of text to push us over the truncation limit and ensure the output gets cut off properly with the truncation marker appended at the very end of the string so we can verify the behavior is correct",
                i
            ),
            Confidence::Confirmed,
        ));
    }
    let mut note = make_note_with_items("billing", Confidence::Confirmed, items);
    // Add many related files to inflate the output well past 2000 chars
    note.related_files = (0..100)
        .map(|i| format!("src/billing/very/deep/nested/module_{}/handler_with_long_name.ts", i))
        .collect();
    // Add long markdown content for dependencies
    let mut deps = "## Dependencies\n".to_string();
    for i in 0..50 {
        deps.push_str(&format!("- extremely-long-dependency-package-name-number-{}\n", i));
    }
    note.content = deps;

    let summary = compact_summary(&note, "billing", "src/billing/main.ts");

    assert!(
        summary.len() <= MAX_CONTEXT_LEN,
        "output length {} exceeds MAX_CONTEXT_LEN {}",
        summary.len(),
        MAX_CONTEXT_LEN
    );
    assert!(
        summary.ends_with("[... truncated]"),
        "truncated output must end with '[... truncated]', got: ...{}",
        &summary[summary.len().saturating_sub(30)..]
    );
    // The header line must still be intact
    assert!(summary.starts_with("[project-wiki] Domain: billing (confidence: confirmed, updated: 2026-03-28)"));
}

// ── Schema version tests (task 028) ──

#[test]
fn test_schema_version_serializes_as_top_level_field() {
    let output = ContextJsonOutput {
        schema_version: "1".to_string(),
        domain: None,
        confidence: None,
        last_updated: None,
        memory_items: Vec::new(),
        warnings: Vec::new(),
        fallback_mode: false,
    };
    let json: serde_json::Value = serde_json::to_value(&output).unwrap();
    assert_eq!(json["schema_version"], "1");
    assert!(
        json.get("schema_version").is_some(),
        "schema_version must be a top-level JSON field"
    );
}

// ── JSON shape tests ──

fn make_json_output_full() -> ContextJsonOutput {
    ContextJsonOutput {
        schema_version: "1".to_string(),
        domain: Some("billing".to_string()),
        confidence: Some("confirmed".to_string()),
        last_updated: Some("2026-03-28".to_string()),
        memory_items: vec![
            ContextJsonItem {
                id: "b-001".to_string(),
                type_: "exception".to_string(),
                text: "Client X uses legacy calc".to_string(),
                confidence: "confirmed".to_string(),
            },
            ContextJsonItem {
                id: "b-002".to_string(),
                type_: "decision".to_string(),
                text: "No dedup on import".to_string(),
                confidence: "inferred".to_string(),
            },
        ],
        warnings: vec![
            "1 item(s) have low confidence — verify before relying on them".to_string(),
        ],
        fallback_mode: false,
    }
}

fn make_json_output_no_domain() -> ContextJsonOutput {
    ContextJsonOutput {
        schema_version: "1".to_string(),
        domain: None,
        confidence: None,
        last_updated: None,
        memory_items: Vec::new(),
        warnings: vec!["No domain found for this file".to_string()],
        fallback_mode: false,
    }
}

#[test]
fn json_shape_full_structure_has_all_fields() {
    let output = make_json_output_full();
    let parsed: serde_json::Value = serde_json::to_value(&output).unwrap();

    // All top-level fields must be present
    assert!(parsed.get("schema_version").is_some(), "missing schema_version");
    assert!(parsed.get("domain").is_some(), "missing domain");
    assert!(parsed.get("confidence").is_some(), "missing confidence");
    assert!(parsed.get("last_updated").is_some(), "missing last_updated");
    assert!(parsed.get("memory_items").is_some(), "missing memory_items");
    assert!(parsed.get("warnings").is_some(), "missing warnings");
    assert!(parsed.get("fallback_mode").is_some(), "missing fallback_mode");

    // Exactly 7 top-level keys (no extra fields)
    assert_eq!(parsed.as_object().unwrap().len(), 7);
}

#[test]
fn json_shape_schema_version_is_one() {
    let output = make_json_output_full();
    let parsed: serde_json::Value = serde_json::to_value(&output).unwrap();
    assert_eq!(parsed["schema_version"], "1");
}

#[test]
fn json_shape_memory_items_array_with_correct_fields() {
    let output = make_json_output_full();
    let parsed: serde_json::Value = serde_json::to_value(&output).unwrap();

    let items = parsed["memory_items"].as_array().unwrap();
    assert_eq!(items.len(), 2);

    let item = &items[0];
    // Each memory item must have exactly these 4 fields
    assert!(item.get("id").is_some(), "missing id");
    assert!(item.get("type").is_some(), "missing type (serde rename)");
    assert!(item.get("text").is_some(), "missing text");
    assert!(item.get("confidence").is_some(), "missing confidence");
    assert_eq!(
        item.as_object().unwrap().len(),
        4,
        "memory item should have exactly 4 fields"
    );

    // Verify values
    assert_eq!(item["id"], "b-001");
    assert_eq!(item["type"], "exception");
    assert_eq!(item["text"], "Client X uses legacy calc");
    assert_eq!(item["confidence"], "confirmed");
}

#[test]
fn json_shape_fallback_mode_is_bool() {
    let output = make_json_output_full();
    let parsed: serde_json::Value = serde_json::to_value(&output).unwrap();

    assert!(parsed["fallback_mode"].is_boolean());
    assert_eq!(parsed["fallback_mode"], false);
}

#[test]
fn json_shape_warnings_is_string_array() {
    let output = make_json_output_full();
    let parsed: serde_json::Value = serde_json::to_value(&output).unwrap();

    let warnings = parsed["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].is_string());
    assert!(warnings[0].as_str().unwrap().contains("low confidence"));
}

#[test]
fn json_shape_no_domain_found_uses_nulls() {
    let output = make_json_output_no_domain();
    let parsed: serde_json::Value = serde_json::to_value(&output).unwrap();

    assert_eq!(parsed["schema_version"], "1");
    assert!(parsed["domain"].is_null(), "domain should be null");
    assert!(parsed["confidence"].is_null(), "confidence should be null");
    assert!(parsed["last_updated"].is_null(), "last_updated should be null");
    assert_eq!(parsed["memory_items"].as_array().unwrap().len(), 0);
    assert_eq!(parsed["fallback_mode"], false);
    assert!(!parsed["warnings"].as_array().unwrap().is_empty(), "should have a warning");
    assert!(parsed["warnings"][0].as_str().unwrap().contains("No domain"));
}

#[test]
fn json_shape_no_memory_items_with_fallback_mode() {
    let output = ContextJsonOutput {
        schema_version: "1".to_string(),
        domain: Some("billing".to_string()),
        confidence: Some("confirmed".to_string()),
        last_updated: Some("2026-03-28".to_string()),
        memory_items: Vec::new(),
        warnings: Vec::new(),
        fallback_mode: true,
    };
    let parsed: serde_json::Value = serde_json::to_value(&output).unwrap();

    assert_eq!(parsed["domain"], "billing");
    assert_eq!(parsed["memory_items"].as_array().unwrap().len(), 0);
    assert_eq!(parsed["fallback_mode"], true);
    assert_eq!(parsed["warnings"].as_array().unwrap().len(), 0);
}

#[test]
fn json_shape_type_field_uses_serde_rename() {
    // Verify that the struct field `type_` serializes as `type` (not `type_`)
    let item = ContextJsonItem {
        id: "x-001".to_string(),
        type_: "business_rule".to_string(),
        text: "Some rule".to_string(),
        confidence: "verified".to_string(),
    };
    let parsed: serde_json::Value = serde_json::to_value(&item).unwrap();

    assert!(parsed.get("type").is_some(), "field should be named 'type'");
    assert!(parsed.get("type_").is_none(), "field should NOT be named 'type_'");
}

#[test]
fn json_shape_roundtrip_from_string() {
    // Verify JSON can be serialized to string and parsed back identically
    let output = make_json_output_full();
    let json_str = serde_json::to_string_pretty(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    assert_eq!(parsed["schema_version"], "1");
    assert_eq!(parsed["domain"], "billing");
    assert_eq!(parsed["confidence"], "confirmed");
    assert_eq!(parsed["last_updated"], "2026-03-28");
    assert_eq!(parsed["memory_items"].as_array().unwrap().len(), 2);
    assert_eq!(parsed["memory_items"][1]["type"], "decision");
    assert_eq!(parsed["memory_items"][1]["text"], "No dedup on import");
    assert_eq!(parsed["fallback_mode"], false);
    assert_eq!(parsed["warnings"].as_array().unwrap().len(), 1);
}
