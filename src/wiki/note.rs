use std::fmt;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Confidence {
    Confirmed,
    Verified,
    SeenInCode,
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

impl Default for Confidence {
    fn default() -> Self {
        Confidence::Inferred
    }
}

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
}

impl WikiNote {
    pub fn parse(path: &Path) -> Result<Self> {
        let raw =
            fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;

        let parsed = gray_matter::Matter::<gray_matter::engine::YAML>::new()
            .parse(&raw);

        let front_matter: FrontMatter = if let Some(pod) = parsed.data {
            pod.deserialize()
                .unwrap_or_else(|_| FrontMatter {
                    title: String::new(),
                    confidence: Confidence::default(),
                    last_updated: None,
                    related_files: Vec::new(),
                    deprecated: false,
                })
        } else {
            FrontMatter {
                title: String::new(),
                confidence: Confidence::default(),
                last_updated: None,
                related_files: Vec::new(),
                deprecated: false,
            }
        };

        let last_updated = front_matter.last_updated.as_ref().and_then(|s| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
        });

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
        };

        let yaml = serde_yml::to_string(&front).context("Failed to serialize front matter")?;

        let output = format!("---\n{}---\n{}", yaml, self.content);
        fs::write(path, output)
            .with_context(|| format!("Failed to write {}", path.display()))?;

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
