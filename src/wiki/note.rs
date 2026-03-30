use std::fmt;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

// ── Confidence ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum Confidence {
    Confirmed,
    Verified,
    SeenInCode,
    #[default]
    Inferred,
    NeedsValidation,
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Confidence::Confirmed => write!(f, "confirmed"),
            Confidence::Verified => write!(f, "verified"),
            Confidence::SeenInCode => write!(f, "seen-in-code"),
            Confidence::Inferred => write!(f, "inferred"),
            Confidence::NeedsValidation => write!(f, "needs-validation"),
        }
    }
}

// ── Memory Item types ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryItemType {
    Decision,
    BusinessRule,
    Exception,
}

impl fmt::Display for MemoryItemType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryItemType::Decision => write!(f, "decision"),
            MemoryItemType::BusinessRule => write!(f, "business_rule"),
            MemoryItemType::Exception => write!(f, "exception"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryItemStatus {
    #[default]
    Active,
    Deprecated,
}

impl fmt::Display for MemoryItemStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryItemStatus::Active => write!(f, "active"),
            MemoryItemStatus::Deprecated => write!(f, "deprecated"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryItemSource {
    pub kind: String,

    #[serde(rename = "ref")]
    pub ref_: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryItem {
    pub id: String,

    #[serde(rename = "type")]
    pub type_: MemoryItemType,

    pub text: String,

    pub confidence: Confidence,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_files: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<MemoryItemSource>,

    #[serde(default)]
    pub status: MemoryItemStatus,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reviewed: Option<String>,
}

impl MemoryItem {
    /// Parse `last_reviewed` string into a `NaiveDate`.
    #[allow(dead_code)] // Will be used by context, check-diff, and confirm commands
    pub fn last_reviewed_date(&self) -> Option<NaiveDate> {
        self.last_reviewed
            .as_ref()
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
    }

    /// Returns `true` if this item has high confidence (`confirmed` or `verified`).
    #[allow(dead_code)] // Will be used by context and check-diff commands
    pub fn is_high_confidence(&self) -> bool {
        matches!(
            self.confidence,
            Confidence::Confirmed | Confidence::Verified
        )
    }
}

// ── Front matter & WikiNote ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontMatter {
    #[serde(default)]
    pub title: String,

    #[serde(default)]
    pub confidence: Confidence,

    #[serde(default)]
    pub last_updated: Option<String>,

    #[serde(default)]
    pub related_files: Vec<String>,

    #[serde(default)]
    pub deprecated: bool,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memory_items: Vec<MemoryItem>,
}

#[derive(Debug, Clone)]
pub struct WikiNote {
    pub path: String,
    pub domain: String,
    pub confidence: Confidence,
    pub last_updated: Option<NaiveDate>,
    pub related_files: Vec<String>,
    pub deprecated: bool,
    pub title: String,
    pub content: String,
    pub memory_items: Vec<MemoryItem>,
}

impl WikiNote {
    pub fn parse(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;

        let parsed = gray_matter::Matter::<gray_matter::engine::YAML>::new().parse(&raw);

        let front_matter: FrontMatter = if let Some(pod) = parsed.data {
            pod.deserialize().unwrap_or_else(|_| FrontMatter {
                title: String::new(),
                confidence: Confidence::default(),
                last_updated: None,
                related_files: Vec::new(),
                deprecated: false,
                memory_items: Vec::new(),
            })
        } else {
            FrontMatter {
                title: String::new(),
                confidence: Confidence::default(),
                last_updated: None,
                related_files: Vec::new(),
                deprecated: false,
                memory_items: Vec::new(),
            }
        };

        let last_updated = front_matter
            .last_updated
            .as_ref()
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());

        let domain = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        Ok(WikiNote {
            path: path.to_string_lossy().to_string(),
            domain,
            confidence: front_matter.confidence,
            last_updated,
            related_files: front_matter.related_files,
            deprecated: front_matter.deprecated,
            title: front_matter.title,
            content: parsed.content,
            memory_items: front_matter.memory_items,
        })
    }

    #[allow(dead_code)] // Used in tests, will be needed for wiki-update command
    pub fn write(&self, path: &Path) -> Result<()> {
        let front = FrontMatter {
            title: self.title.clone(),
            confidence: self.confidence.clone(),
            last_updated: self.last_updated.map(|d| d.format("%Y-%m-%d").to_string()),
            related_files: self.related_files.clone(),
            deprecated: self.deprecated,
            memory_items: self.memory_items.clone(),
        };

        let yaml = serde_yml::to_string(&front).context("Failed to serialize front matter")?;

        let output = format!("---\n{}---\n{}", yaml, self.content);
        fs::write(path, output).with_context(|| format!("Failed to write {}", path.display()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::TempDir;

    fn write_note(dir: &TempDir, name: &str, content: &str) -> std::path::PathBuf {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn parse_note_with_full_front_matter() {
        let dir = TempDir::new().unwrap();
        let content = r#"---
title: Billing overview
confidence: confirmed
last_updated: "2025-06-15"
related_files:
  - src/billing/invoice.ts
  - src/billing/payment.ts
deprecated: false
---
# Billing

This domain handles invoicing.
"#;
        let path = write_note(&dir, "billing/_overview.md", content);
        let note = WikiNote::parse(&path).unwrap();

        assert_eq!(note.title, "Billing overview");
        assert_eq!(note.confidence, Confidence::Confirmed);
        assert_eq!(
            note.last_updated,
            Some(NaiveDate::from_ymd_opt(2025, 6, 15).unwrap())
        );
        assert_eq!(note.related_files.len(), 2);
        assert!(!note.deprecated);
        assert_eq!(note.domain, "billing");
        assert!(note.content.contains("This domain handles invoicing."));
    }

    #[test]
    fn parse_note_with_missing_optional_fields() {
        let dir = TempDir::new().unwrap();
        let content = r#"---
title: Minimal note
---
Some content.
"#;
        let path = write_note(&dir, "auth/_overview.md", content);
        let note = WikiNote::parse(&path).unwrap();

        assert_eq!(note.title, "Minimal note");
        assert_eq!(note.confidence, Confidence::Inferred); // default
        assert!(note.last_updated.is_none());
        assert!(note.related_files.is_empty());
        assert!(!note.deprecated);
    }

    #[test]
    fn parse_note_with_no_front_matter() {
        let dir = TempDir::new().unwrap();
        let content = "# Just a heading\n\nSome plain markdown.\n";
        let path = write_note(&dir, "misc/plain.md", content);
        let note = WikiNote::parse(&path).unwrap();

        assert_eq!(note.title, "");
        assert_eq!(note.confidence, Confidence::Inferred);
        assert!(note.last_updated.is_none());
    }

    #[test]
    fn write_and_read_roundtrip() {
        let dir = TempDir::new().unwrap();
        let note = WikiNote {
            path: "test.md".to_string(),
            domain: "billing".to_string(),
            confidence: Confidence::Verified,
            last_updated: Some(NaiveDate::from_ymd_opt(2025, 12, 1).unwrap()),
            related_files: vec!["src/foo.ts".to_string()],
            deprecated: true,
            title: "Test note".to_string(),
            content: "Hello world.\n".to_string(),
            memory_items: Vec::new(),
        };

        let out_path = dir.path().join("roundtrip.md");
        note.write(&out_path).unwrap();

        let parsed = WikiNote::parse(&out_path).unwrap();
        assert_eq!(parsed.title, "Test note");
        assert_eq!(parsed.confidence, Confidence::Verified);
        assert_eq!(
            parsed.last_updated,
            Some(NaiveDate::from_ymd_opt(2025, 12, 1).unwrap())
        );
        assert_eq!(parsed.related_files, vec!["src/foo.ts".to_string()]);
        assert!(parsed.deprecated);
        assert!(parsed.content.contains("Hello world."));
        assert!(parsed.memory_items.is_empty());
    }

    #[test]
    fn confidence_display() {
        assert_eq!(Confidence::Confirmed.to_string(), "confirmed");
        assert_eq!(Confidence::Verified.to_string(), "verified");
        assert_eq!(Confidence::SeenInCode.to_string(), "seen-in-code");
        assert_eq!(Confidence::Inferred.to_string(), "inferred");
        assert_eq!(Confidence::NeedsValidation.to_string(), "needs-validation");
    }

    #[test]
    fn confidence_default_is_inferred() {
        assert_eq!(Confidence::default(), Confidence::Inferred);
    }

    #[test]
    fn parse_deprecated_field_true() {
        let dir = TempDir::new().unwrap();
        let content = r#"---
title: Old note
deprecated: true
---
Deprecated content.
"#;
        let path = write_note(&dir, "old/deprecated.md", content);
        let note = WikiNote::parse(&path).unwrap();
        assert!(note.deprecated);
    }

    #[test]
    fn parse_deprecated_field_false() {
        let dir = TempDir::new().unwrap();
        let content = r#"---
title: Current note
deprecated: false
---
Active content.
"#;
        let path = write_note(&dir, "current/active.md", content);
        let note = WikiNote::parse(&path).unwrap();
        assert!(!note.deprecated);
    }

    // ── Memory Item type tests ──────────────────────────────────────

    #[test]
    fn memory_item_type_serialization() {
        assert_eq!(
            serde_yml::to_string(&MemoryItemType::Decision)
                .unwrap()
                .trim(),
            "decision"
        );
        assert_eq!(
            serde_yml::to_string(&MemoryItemType::BusinessRule)
                .unwrap()
                .trim(),
            "business_rule"
        );
        assert_eq!(
            serde_yml::to_string(&MemoryItemType::Exception)
                .unwrap()
                .trim(),
            "exception"
        );
    }

    #[test]
    fn memory_item_type_deserialization() {
        let d: MemoryItemType = serde_yml::from_str("decision").unwrap();
        assert_eq!(d, MemoryItemType::Decision);
        let b: MemoryItemType = serde_yml::from_str("business_rule").unwrap();
        assert_eq!(b, MemoryItemType::BusinessRule);
        let e: MemoryItemType = serde_yml::from_str("exception").unwrap();
        assert_eq!(e, MemoryItemType::Exception);
    }

    #[test]
    fn memory_item_type_display() {
        assert_eq!(MemoryItemType::Decision.to_string(), "decision");
        assert_eq!(MemoryItemType::BusinessRule.to_string(), "business_rule");
        assert_eq!(MemoryItemType::Exception.to_string(), "exception");
    }

    #[test]
    fn memory_item_status_serialization() {
        assert_eq!(
            serde_yml::to_string(&MemoryItemStatus::Active)
                .unwrap()
                .trim(),
            "active"
        );
        assert_eq!(
            serde_yml::to_string(&MemoryItemStatus::Deprecated)
                .unwrap()
                .trim(),
            "deprecated"
        );
    }

    #[test]
    fn memory_item_status_deserialization() {
        let a: MemoryItemStatus = serde_yml::from_str("active").unwrap();
        assert_eq!(a, MemoryItemStatus::Active);
        let d: MemoryItemStatus = serde_yml::from_str("deprecated").unwrap();
        assert_eq!(d, MemoryItemStatus::Deprecated);
    }

    #[test]
    fn memory_item_status_default_is_active() {
        assert_eq!(MemoryItemStatus::default(), MemoryItemStatus::Active);
    }

    #[test]
    fn memory_item_source_ref_rename() {
        let src = MemoryItemSource {
            kind: "file".to_string(),
            ref_: "src/billing/invoice.ts".to_string(),
            line: None,
        };
        let yaml = serde_yml::to_string(&src).unwrap();
        assert!(yaml.contains("ref:"), "YAML must use 'ref' not 'ref_'");
        assert!(!yaml.contains("ref_:"), "YAML must not contain 'ref_:'");

        // Roundtrip
        let parsed: MemoryItemSource = serde_yml::from_str(&yaml).unwrap();
        assert_eq!(parsed.ref_, "src/billing/invoice.ts");
        assert_eq!(parsed.kind, "file");
    }

    #[test]
    fn memory_item_source_with_line() {
        let src = MemoryItemSource {
            kind: "comment".to_string(),
            ref_: "src/billing/invoice.ts".to_string(),
            line: Some(42),
        };
        let yaml = serde_yml::to_string(&src).unwrap();
        assert!(yaml.contains("line: 42") || yaml.contains("line: '42'"));

        let parsed: MemoryItemSource = serde_yml::from_str(&yaml).unwrap();
        assert_eq!(parsed.line, Some(42));
    }

    #[test]
    fn memory_item_source_without_line() {
        let src = MemoryItemSource {
            kind: "test".to_string(),
            ref_: "tests/billing.test.ts".to_string(),
            line: None,
        };
        let yaml = serde_yml::to_string(&src).unwrap();
        assert!(!yaml.contains("line"), "line should be omitted when None");

        let parsed: MemoryItemSource = serde_yml::from_str(&yaml).unwrap();
        assert_eq!(parsed.line, None);
    }

    fn make_full_memory_item() -> MemoryItem {
        MemoryItem {
            id: "billing-001".to_string(),
            type_: MemoryItemType::Exception,
            text: "Le client X utilise encore l'ancien calcul".to_string(),
            confidence: Confidence::Confirmed,
            related_files: vec!["src/billing/legacy_pricing.ts".to_string()],
            sources: vec![
                MemoryItemSource {
                    kind: "file".to_string(),
                    ref_: "src/billing/legacy_pricing.ts".to_string(),
                    line: None,
                },
                MemoryItemSource {
                    kind: "test".to_string(),
                    ref_: "tests/billing/legacy_pricing.test.ts".to_string(),
                    line: None,
                },
            ],
            status: MemoryItemStatus::Active,
            last_reviewed: Some("2026-03-29".to_string()),
        }
    }

    #[test]
    fn memory_item_full_roundtrip() {
        let item = make_full_memory_item();
        let yaml = serde_yml::to_string(&item).unwrap();

        // Verify key fields in YAML output
        assert!(yaml.contains("id: billing-001"));
        assert!(yaml.contains("type: exception"));
        assert!(yaml.contains("confidence: confirmed"));
        assert!(yaml.contains("status: active"));

        // Roundtrip
        let parsed: MemoryItem = serde_yml::from_str(&yaml).unwrap();
        assert_eq!(parsed.id, item.id);
        assert_eq!(parsed.type_, item.type_);
        assert_eq!(parsed.text, item.text);
        assert_eq!(parsed.confidence, item.confidence);
        assert_eq!(parsed.related_files, item.related_files);
        assert_eq!(parsed.sources.len(), 2);
        assert_eq!(parsed.status, item.status);
        assert_eq!(parsed.last_reviewed, item.last_reviewed);
    }

    #[test]
    fn memory_item_type_rename_in_yaml() {
        let item = make_full_memory_item();
        let yaml = serde_yml::to_string(&item).unwrap();
        assert!(yaml.contains("type:"), "YAML must use 'type' not 'type_'");
        assert!(!yaml.contains("type_:"), "YAML must not contain 'type_:'");
    }

    #[test]
    fn memory_item_without_last_reviewed() {
        let item = MemoryItem {
            id: "auth-001".to_string(),
            type_: MemoryItemType::Decision,
            text: "Use JWT for auth".to_string(),
            confidence: Confidence::Inferred,
            related_files: Vec::new(),
            sources: Vec::new(),
            status: MemoryItemStatus::Active,
            last_reviewed: None,
        };
        let yaml = serde_yml::to_string(&item).unwrap();
        assert!(
            !yaml.contains("last_reviewed"),
            "last_reviewed should be omitted when None"
        );

        let parsed: MemoryItem = serde_yml::from_str(&yaml).unwrap();
        assert_eq!(parsed.last_reviewed, None);
    }

    #[test]
    fn memory_item_empty_optional_vecs_omitted() {
        let item = MemoryItem {
            id: "auth-002".to_string(),
            type_: MemoryItemType::BusinessRule,
            text: "Passwords must be 8+ chars".to_string(),
            confidence: Confidence::SeenInCode,
            related_files: Vec::new(),
            sources: Vec::new(),
            status: MemoryItemStatus::Active,
            last_reviewed: None,
        };
        let yaml = serde_yml::to_string(&item).unwrap();
        assert!(
            !yaml.contains("related_files"),
            "empty related_files should be omitted"
        );
        assert!(!yaml.contains("sources"), "empty sources should be omitted");
    }

    #[test]
    fn memory_item_last_reviewed_date_helper() {
        let item = make_full_memory_item();
        assert_eq!(
            item.last_reviewed_date(),
            Some(NaiveDate::from_ymd_opt(2026, 3, 29).unwrap())
        );

        let item_no_date = MemoryItem {
            last_reviewed: None,
            ..make_full_memory_item()
        };
        assert_eq!(item_no_date.last_reviewed_date(), None);

        let item_bad_date = MemoryItem {
            last_reviewed: Some("not-a-date".to_string()),
            ..make_full_memory_item()
        };
        assert_eq!(item_bad_date.last_reviewed_date(), None);
    }

    #[test]
    fn memory_item_is_high_confidence() {
        let confirmed = MemoryItem {
            confidence: Confidence::Confirmed,
            ..make_full_memory_item()
        };
        assert!(confirmed.is_high_confidence());

        let verified = MemoryItem {
            confidence: Confidence::Verified,
            ..make_full_memory_item()
        };
        assert!(verified.is_high_confidence());

        let inferred = MemoryItem {
            confidence: Confidence::Inferred,
            ..make_full_memory_item()
        };
        assert!(!inferred.is_high_confidence());

        let needs = MemoryItem {
            confidence: Confidence::NeedsValidation,
            ..make_full_memory_item()
        };
        assert!(!needs.is_high_confidence());
    }

    // ── WikiNote with memory_items ──────────────────────────────────

    #[test]
    fn parse_note_without_memory_items_compat() {
        // Existing notes without memory_items must still parse
        let dir = TempDir::new().unwrap();
        let content = r#"---
title: Billing overview
confidence: confirmed
last_updated: "2025-06-15"
related_files:
  - src/billing/invoice.ts
deprecated: false
---
# Billing

This domain handles invoicing.
"#;
        let path = write_note(&dir, "billing/_overview.md", content);
        let note = WikiNote::parse(&path).unwrap();

        assert_eq!(note.title, "Billing overview");
        assert_eq!(note.confidence, Confidence::Confirmed);
        assert!(note.memory_items.is_empty());
    }

    #[test]
    fn parse_note_with_memory_items() {
        let dir = TempDir::new().unwrap();
        let content = r#"---
title: Billing overview
confidence: verified
last_updated: "2026-03-29"
related_files:
  - src/billing/invoice.ts
deprecated: false
memory_items:
  - id: billing-001
    type: exception
    text: Le client X utilise encore l'ancien calcul
    confidence: confirmed
    related_files:
      - src/billing/legacy_pricing.ts
    sources:
      - kind: file
        ref: src/billing/legacy_pricing.ts
      - kind: test
        ref: tests/billing/legacy_pricing.test.ts
    status: active
    last_reviewed: "2026-03-29"
  - id: billing-002
    type: decision
    text: Pas de deduplication des lignes importees
    confidence: verified
    sources:
      - kind: file
        ref: src/billing/import.ts
    status: active
---
# Billing

This domain handles invoicing.
"#;
        let path = write_note(&dir, "billing/_overview.md", content);
        let note = WikiNote::parse(&path).unwrap();

        assert_eq!(note.memory_items.len(), 2);

        let item0 = &note.memory_items[0];
        assert_eq!(item0.id, "billing-001");
        assert_eq!(item0.type_, MemoryItemType::Exception);
        assert_eq!(item0.text, "Le client X utilise encore l'ancien calcul");
        assert_eq!(item0.confidence, Confidence::Confirmed);
        assert_eq!(item0.related_files, vec!["src/billing/legacy_pricing.ts"]);
        assert_eq!(item0.sources.len(), 2);
        assert_eq!(item0.sources[0].kind, "file");
        assert_eq!(item0.sources[0].ref_, "src/billing/legacy_pricing.ts");
        assert_eq!(item0.sources[1].kind, "test");
        assert_eq!(item0.status, MemoryItemStatus::Active);
        assert_eq!(item0.last_reviewed, Some("2026-03-29".to_string()));

        let item1 = &note.memory_items[1];
        assert_eq!(item1.id, "billing-002");
        assert_eq!(item1.type_, MemoryItemType::Decision);
        assert!(item1.related_files.is_empty());
        assert_eq!(item1.last_reviewed, None);

        // Markdown content preserved
        assert!(note.content.contains("This domain handles invoicing."));
    }

    #[test]
    fn write_and_read_roundtrip_with_memory_items() {
        let dir = TempDir::new().unwrap();
        let note = WikiNote {
            path: "test.md".to_string(),
            domain: "billing".to_string(),
            confidence: Confidence::Verified,
            last_updated: Some(NaiveDate::from_ymd_opt(2026, 3, 29).unwrap()),
            related_files: vec!["src/billing/invoice.ts".to_string()],
            deprecated: false,
            title: "Billing overview".to_string(),
            content: "# Billing\n\nHandles invoicing.\n".to_string(),
            memory_items: vec![
                make_full_memory_item(),
                MemoryItem {
                    id: "billing-002".to_string(),
                    type_: MemoryItemType::Decision,
                    text: "No dedup on import".to_string(),
                    confidence: Confidence::Verified,
                    related_files: Vec::new(),
                    sources: Vec::new(),
                    status: MemoryItemStatus::Active,
                    last_reviewed: None,
                },
            ],
        };

        let out_path = dir.path().join("billing/_overview.md");
        std::fs::create_dir_all(out_path.parent().unwrap()).unwrap();
        note.write(&out_path).unwrap();

        let parsed = WikiNote::parse(&out_path).unwrap();
        assert_eq!(parsed.memory_items.len(), 2);
        assert_eq!(parsed.memory_items[0].id, "billing-001");
        assert_eq!(parsed.memory_items[0].type_, MemoryItemType::Exception);
        assert_eq!(parsed.memory_items[0].sources.len(), 2);
        assert_eq!(parsed.memory_items[1].id, "billing-002");
        assert_eq!(parsed.memory_items[1].type_, MemoryItemType::Decision);
        assert!(parsed.memory_items[1].sources.is_empty());
        assert!(parsed.content.contains("Handles invoicing."));
    }

    #[test]
    fn write_note_without_memory_items_omits_field() {
        let dir = TempDir::new().unwrap();
        let note = WikiNote {
            path: "test.md".to_string(),
            domain: "auth".to_string(),
            confidence: Confidence::Inferred,
            last_updated: None,
            related_files: Vec::new(),
            deprecated: false,
            title: "Auth".to_string(),
            content: "# Auth\n".to_string(),
            memory_items: Vec::new(),
        };

        let out_path = dir.path().join("auth/_overview.md");
        std::fs::create_dir_all(out_path.parent().unwrap()).unwrap();
        note.write(&out_path).unwrap();

        let raw = std::fs::read_to_string(&out_path).unwrap();
        assert!(
            !raw.contains("memory_items"),
            "memory_items should not appear when empty"
        );
    }

    // ── Schema stability tests (task 028) ──────────────────────────

    #[test]
    fn memory_item_forward_compat_unknown_fields_ignored() {
        // YAML with fields added by a future version should parse without error.
        let yaml = r#"
id: billing-001
type: exception
text: Future-proof item
confidence: confirmed
status: active
future_field: "some new data"
priority: 99
tags:
  - hot
  - v2
"#;
        let parsed: MemoryItem = serde_yml::from_str(yaml).unwrap();
        assert_eq!(parsed.id, "billing-001");
        assert_eq!(parsed.type_, MemoryItemType::Exception);
        assert_eq!(parsed.text, "Future-proof item");
        assert_eq!(parsed.confidence, Confidence::Confirmed);
        // Unknown fields are silently ignored
    }

    #[test]
    fn memory_item_source_forward_compat_unknown_fields_ignored() {
        let yaml = r#"
kind: file
ref: src/billing/invoice.ts
line: 42
context: "surrounding code"
"#;
        let parsed: MemoryItemSource = serde_yml::from_str(yaml).unwrap();
        assert_eq!(parsed.kind, "file");
        assert_eq!(parsed.ref_, "src/billing/invoice.ts");
        assert_eq!(parsed.line, Some(42));
    }

    #[test]
    fn memory_item_backward_compat_minimal_fields() {
        // Only the strictly required fields — no optional ones.
        let yaml = r#"
id: auth-001
type: decision
text: Use JWT for auth
confidence: inferred
"#;
        let parsed: MemoryItem = serde_yml::from_str(yaml).unwrap();
        assert_eq!(parsed.id, "auth-001");
        assert_eq!(parsed.type_, MemoryItemType::Decision);
        assert!(parsed.related_files.is_empty());
        assert!(parsed.sources.is_empty());
        assert_eq!(parsed.status, MemoryItemStatus::Active); // default
        assert_eq!(parsed.last_reviewed, None); // default
    }

    #[test]
    fn note_forward_compat_unknown_frontmatter_fields_ignored() {
        // FrontMatter with fields added by a future version should parse OK.
        let dir = TempDir::new().unwrap();
        let content = r#"---
title: Billing overview
confidence: confirmed
last_updated: "2025-06-15"
related_files:
  - src/billing/invoice.ts
deprecated: false
schema_version: "2"
future_section:
  key: value
---
# Billing

Content here.
"#;
        let path = write_note(&dir, "billing/_overview.md", content);
        let note = WikiNote::parse(&path).unwrap();
        assert_eq!(note.title, "Billing overview");
        assert_eq!(note.confidence, Confidence::Confirmed);
    }

    #[test]
    fn json_output_schema_version_serializes_correctly() {
        // Verify that schema_version appears as a top-level field in JSON output.
        // The actual value "1" is tested end-to-end in cli_tests.rs.
        use crate::wiki::check_diff::CheckDiffResult;
        use crate::wiki::check_diff::Sensitivity;
        use crate::wiki::context::ContextJsonOutput;

        let context_output = ContextJsonOutput {
            schema_version: "1".to_string(),
            domain: None,
            confidence: None,
            last_updated: None,
            memory_items: Vec::new(),
            warnings: Vec::new(),
            fallback_mode: false,
        };
        let json: serde_json::Value = serde_json::to_value(&context_output).unwrap();
        assert_eq!(json["schema_version"], "1");
        assert!(json.get("schema_version").is_some(), "schema_version must be a top-level JSON field");

        let check_diff_output = CheckDiffResult {
            schema_version: "1".to_string(),
            files_analyzed: 0,
            sensitivity: Sensitivity::Low,
            domains: Vec::new(),
            unresolved_files: Vec::new(),
            suggested_actions: Vec::new(),
        };
        let json: serde_json::Value = serde_json::to_value(&check_diff_output).unwrap();
        assert_eq!(json["schema_version"], "1");
        assert!(json.get("schema_version").is_some(), "schema_version must be a top-level JSON field");
    }

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn parse_note_never_panics(content in "\\PC*") {
            let dir = tempfile::TempDir::new().unwrap();
            let path = dir.path().join("test.md");
            std::fs::write(&path, &content).unwrap();
            let _ = WikiNote::parse(&path); // Should never panic
        }
    }
}
