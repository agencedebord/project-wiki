mod collect;
mod prioritize;
mod render;
mod resolve;
mod sensitivity;
mod warnings;

#[cfg(test)]
mod tests;

use anyhow::Result;
use serde::Serialize;

use crate::wiki::common;

use collect::collect_files;
use render::{format_json, format_text};
use resolve::resolve_domains;
use sensitivity::{calculate_sensitivity, generate_suggestions};

// ── Data types ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Sensitivity {
    Low,
    Medium,
    High,
}

impl std::fmt::Display for Sensitivity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Sensitivity::Low => write!(f, "low"),
            Sensitivity::Medium => write!(f, "medium"),
            Sensitivity::High => write!(f, "high"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DomainRole {
    Primary,
    Secondary,
}

impl std::fmt::Display for DomainRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DomainRole::Primary => write!(f, "primary"),
            DomainRole::Secondary => write!(f, "secondary"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DomainWarning {
    pub kind: String,
    pub note: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DomainItemOutput {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub text: String,
    pub confidence: String,
    pub directly_related: bool,
    pub source_note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DomainHit {
    pub name: String,
    pub role: DomainRole,
    pub files: Vec<String>,
    pub memory_items: Vec<DomainItemOutput>,
    pub warnings: Vec<DomainWarning>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckDiffResult {
    pub schema_version: String,
    pub files_analyzed: usize,
    pub sensitivity: Sensitivity,
    pub domains: Vec<DomainHit>,
    pub unresolved_files: Vec<String>,
    pub suggested_actions: Vec<String>,
}

// ── Public API ─────────────────────────────────────────────────────

pub use render::format_pr_comment;

pub fn run(
    files: &[String],
    staged: bool,
    json: bool,
    pr_comment: bool,
    max_items: usize,
) -> Result<()> {
    let wiki_dir = common::find_wiki_root()?;
    let project_root = wiki_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Wiki directory has no parent"))?;

    let file_list = collect_files(files, staged)?;

    if file_list.is_empty() {
        let empty = CheckDiffResult {
            schema_version: "1".to_string(),
            files_analyzed: 0,
            sensitivity: Sensitivity::Low,
            domains: Vec::new(),
            unresolved_files: Vec::new(),
            suggested_actions: Vec::new(),
        };

        if json {
            println!("{}", format_json(&empty)?);
        } else if pr_comment {
            // Low sensitivity → no output (silent exit)
        } else {
            println!("[project-wiki] Diff check\n\nNo modified files detected.");
        }
        return Ok(());
    }

    let (domains, unresolved) = resolve_domains(&file_list, &wiki_dir, project_root, max_items)?;

    let mut result = CheckDiffResult {
        schema_version: "1".to_string(),
        files_analyzed: file_list.len(),
        sensitivity: Sensitivity::Low, // placeholder
        domains,
        unresolved_files: unresolved,
        suggested_actions: Vec::new(),
    };

    result.sensitivity = calculate_sensitivity(&result);
    result.suggested_actions = generate_suggestions(&result);

    if json {
        println!("{}", format_json(&result)?);
    } else if pr_comment {
        if let Some(comment) = format_pr_comment(&result) {
            println!("{}", comment);
        }
    } else {
        println!("{}", format_text(&result));
    }

    Ok(())
}
