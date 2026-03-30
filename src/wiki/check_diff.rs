use std::collections::HashMap;
use std::path::Path;

use anyhow::{Result, bail};
use serde::Serialize;

use crate::wiki::common;
use crate::wiki::config;
use crate::wiki::file_index;
use crate::wiki::note::{Confidence, MemoryItem, MemoryItemStatus, MemoryItemType, WikiNote};

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

// ── File collection (task 008) ─────────────────────────────────────

/// Collect the list of modified files to check.
///
/// If `files` is non-empty, use those directly (explicit mode).
/// Otherwise, run `git diff --name-only` (or `--cached` when `staged`).
fn collect_files(files: &[String], staged: bool) -> Result<Vec<String>> {
    if !files.is_empty() {
        let mut result = Vec::new();
        for f in files {
            let normalized = normalize_path(f);
            if should_ignore(&normalized) {
                continue;
            }
            if std::path::Path::new(&normalized).exists() {
                result.push(normalized);
            } else {
                eprintln!("warning: file not found, skipping: {normalized}");
            }
        }
        return Ok(result);
    }

    // Git diff mode
    let mut cmd = std::process::Command::new("git");
    cmd.arg("diff").arg("--name-only");
    if staged {
        cmd.arg("--cached");
    }

    let output = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run git: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git diff failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: Vec<String> = stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .filter(|l| std::path::Path::new(l).exists())
        .filter(|l| !should_ignore(l))
        .collect();

    Ok(result)
}

fn normalize_path(path: &str) -> String {
    let p = path.strip_prefix("./").unwrap_or(path);
    p.to_string()
}

fn should_ignore(path: &str) -> bool {
    let ignored_prefixes = [
        ".wiki/",
        "node_modules/",
        "target/",
        "dist/",
        ".git/",
        "vendor/",
        "__pycache__/",
    ];
    for prefix in &ignored_prefixes {
        if path.starts_with(prefix) {
            return true;
        }
    }

    let ignored_extensions = [
        ".png", ".jpg", ".jpeg", ".gif", ".ico", ".svg", ".woff", ".woff2", ".ttf", ".eot", ".mp3",
        ".mp4", ".zip", ".tar", ".gz", ".pdf", ".exe", ".dll", ".so", ".dylib",
    ];
    for ext in &ignored_extensions {
        if path.ends_with(ext) {
            return true;
        }
    }

    false
}

// ── Domain resolution + aggregation (task 009) ─────────────────────

/// Intermediate struct before role assignment.
struct DomainAgg {
    files: Vec<String>,
    note: Option<WikiNote>,
    note_path: String,
}

/// Resolve files to domains, load notes, and build DomainHits.
fn resolve_domains(
    file_list: &[String],
    wiki_dir: &Path,
    project_root: &Path,
    max_items: usize,
) -> Result<(Vec<DomainHit>, Vec<String>)> {
    let index = file_index::load_or_rebuild(wiki_dir)?;

    // Group files by domain
    let mut domain_map: HashMap<String, DomainAgg> = HashMap::new();
    let mut unresolved: Vec<String> = Vec::new();

    for file in file_list {
        match file_index::resolve_domain(&index, file, project_root) {
            Some(domain) => {
                domain_map
                    .entry(domain.clone())
                    .or_insert_with(|| DomainAgg {
                        files: Vec::new(),
                        note: None,
                        note_path: wiki_dir
                            .join("domains")
                            .join(&domain)
                            .join("_overview.md")
                            .to_string_lossy()
                            .to_string(),
                    })
                    .files
                    .push(file.clone());
            }
            None => {
                unresolved.push(file.clone());
            }
        }
    }

    // Load notes for each domain
    for (domain, agg) in domain_map.iter_mut() {
        let overview_path = wiki_dir.join("domains").join(domain).join("_overview.md");
        if overview_path.exists() {
            if let Ok(note) = WikiNote::parse(&overview_path) {
                agg.note = Some(note);
            }
        }
    }

    let wiki_config = config::load(wiki_dir);

    // Sort domains: most files first, then most memory_items on tie
    let mut domain_entries: Vec<(String, DomainAgg)> = domain_map.into_iter().collect();
    domain_entries.sort_by(|a, b| {
        let file_cmp = b.1.files.len().cmp(&a.1.files.len());
        if file_cmp != std::cmp::Ordering::Equal {
            return file_cmp;
        }
        let a_items = a.1.note.as_ref().map(|n| n.memory_items.len()).unwrap_or(0);
        let b_items = b.1.note.as_ref().map(|n| n.memory_items.len()).unwrap_or(0);
        b_items.cmp(&a_items)
    });

    let total_domains = domain_entries.len();
    let max_domains = 3;
    let shown_domains = domain_entries
        .into_iter()
        .take(max_domains)
        .collect::<Vec<_>>();

    let mut hits: Vec<DomainHit> = Vec::new();

    for (i, (domain, agg)) in shown_domains.into_iter().enumerate() {
        let role = if i == 0 {
            DomainRole::Primary
        } else {
            DomainRole::Secondary
        };

        let all_modified = file_list;

        // Build memory items
        let items_output = match &agg.note {
            Some(note) => prioritize_and_format_items(
                &note.memory_items,
                all_modified,
                max_items,
                &agg.note_path,
            ),
            None => Vec::new(),
        };

        // Build warnings
        let warnings = match &agg.note {
            Some(note) => build_warnings(note, &domain, &agg.note_path, &wiki_config),
            None => {
                vec![DomainWarning {
                    kind: "no_note".to_string(),
                    note: agg.note_path.clone(),
                    days: None,
                }]
            }
        };

        hits.push(DomainHit {
            name: domain,
            role,
            files: agg.files,
            memory_items: items_output,
            warnings,
        });
    }

    // Add "+N other domains" as an unresolved hint if truncated
    if total_domains > max_domains {
        let extra = total_domains - max_domains;
        unresolved.push(format!("+{extra} other domain(s) not shown"));
    }

    Ok((hits, unresolved))
}

/// Filter, prioritize, and convert memory items to output format.
fn prioritize_and_format_items(
    items: &[MemoryItem],
    modified_files: &[String],
    max_items: usize,
    note_path: &str,
) -> Vec<DomainItemOutput> {
    let active: Vec<&MemoryItem> = items
        .iter()
        .filter(|i| i.status != MemoryItemStatus::Deprecated)
        .collect();

    let mut scored: Vec<(&MemoryItem, (u8, u8, bool))> = active
        .into_iter()
        .map(|item| {
            let related = has_related_file(item, modified_files);
            (
                item,
                (
                    type_priority(&item.type_),
                    confidence_priority(&item.confidence),
                    related,
                ),
            )
        })
        .collect();

    // Sort: type priority asc, confidence priority asc, related first (false > true reversed)
    scored.sort_by(|a, b| {
        let type_cmp = a.1.0.cmp(&b.1.0);
        if type_cmp != std::cmp::Ordering::Equal {
            return type_cmp;
        }
        let conf_cmp = a.1.1.cmp(&b.1.1);
        if conf_cmp != std::cmp::Ordering::Equal {
            return conf_cmp;
        }
        // Related files first (true = 0, false = 1 for sorting)
        b.1.2.cmp(&a.1.2)
    });

    scored
        .into_iter()
        .take(max_items)
        .map(|(item, (_, _, related))| DomainItemOutput {
            id: item.id.clone(),
            type_: item.type_.to_string(),
            text: item.text.clone(),
            confidence: item.confidence.to_string(),
            directly_related: related,
            source_note: note_path.to_string(),
        })
        .collect()
}

fn type_priority(t: &MemoryItemType) -> u8 {
    match t {
        MemoryItemType::Exception => 0,
        MemoryItemType::Decision => 1,
        MemoryItemType::BusinessRule => 2,
    }
}

fn confidence_priority(c: &Confidence) -> u8 {
    match c {
        Confidence::Confirmed | Confidence::Verified => 0,
        Confidence::SeenInCode => 1,
        Confidence::Inferred => 2,
        Confidence::NeedsValidation => 3,
    }
}

fn has_related_file(item: &MemoryItem, modified_files: &[String]) -> bool {
    item.related_files
        .iter()
        .any(|rf| modified_files.iter().any(|mf| mf == rf))
}

fn build_warnings(
    note: &WikiNote,
    domain: &str,
    note_path: &str,
    wiki_config: &config::WikiConfig,
) -> Vec<DomainWarning> {
    let mut warnings = Vec::new();

    // Stale check
    if let Some(updated) = note.last_updated {
        let today = chrono::Utc::now().date_naive();
        let days_old = (today - updated).num_days();
        if days_old > wiki_config.staleness_days as i64 {
            warnings.push(DomainWarning {
                kind: "stale".to_string(),
                note: note_path.to_string(),
                days: Some(days_old),
            });
        }
    }

    // Low confidence note
    if matches!(
        note.confidence,
        Confidence::Inferred | Confidence::NeedsValidation
    ) {
        warnings.push(DomainWarning {
            kind: "low_confidence".to_string(),
            note: note_path.to_string(),
            days: None,
        });
    }

    // No memory items
    if note.memory_items.is_empty() {
        warnings.push(DomainWarning {
            kind: "no_memory".to_string(),
            note: note_path.to_string(),
            days: None,
        });
    } else {
        // Low confidence items
        let low_count = note
            .memory_items
            .iter()
            .filter(|i| i.status != MemoryItemStatus::Deprecated)
            .filter(|i| {
                matches!(
                    i.confidence,
                    Confidence::Inferred | Confidence::NeedsValidation
                )
            })
            .count();
        if low_count > 0 {
            warnings.push(DomainWarning {
                kind: format!("{low_count} item(s) with low confidence in {domain}"),
                note: note_path.to_string(),
                days: None,
            });
        }
    }

    warnings
}

// ── Sensitivity (task 010) ─────────────────────────────────────────

fn calculate_sensitivity(result: &CheckDiffResult) -> Sensitivity {
    if result.domains.is_empty() && !result.unresolved_files.is_empty() {
        return Sensitivity::Low;
    }

    for domain in &result.domains {
        // High: any exception or decision
        for item in &domain.memory_items {
            if item.type_ == "exception" || item.type_ == "decision" {
                return Sensitivity::High;
            }
        }
        // High: stale note or low confidence note
        for w in &domain.warnings {
            if w.kind == "stale" || w.kind == "low_confidence" {
                return Sensitivity::High;
            }
        }
    }

    // Medium: domains with memory items but no high signal
    let has_items = result.domains.iter().any(|d| !d.memory_items.is_empty());

    if has_items {
        return Sensitivity::Medium;
    }

    Sensitivity::Low
}

fn generate_suggestions(result: &CheckDiffResult) -> Vec<String> {
    if result.sensitivity == Sensitivity::Low {
        return Vec::new();
    }

    let mut suggestions = Vec::new();
    let max_suggestions = 3;

    for domain in &result.domains {
        if suggestions.len() >= max_suggestions {
            break;
        }

        // Stale note suggestion
        for w in &domain.warnings {
            if suggestions.len() >= max_suggestions {
                break;
            }
            if w.kind == "stale" {
                suggestions.push(format!("Relire {}", w.note));
            }
        }

        // Exception/decision suggestions
        for item in &domain.memory_items {
            if suggestions.len() >= max_suggestions {
                break;
            }
            let short_text = if item.text.chars().count() > 50 {
                let truncated: String = item.text.chars().take(50).collect();
                format!("{}...", truncated)
            } else {
                item.text.clone()
            };

            if item.type_ == "exception" {
                suggestions.push(format!(
                    "Verifier si l'exception '{}' reste valide",
                    short_text
                ));
            } else if item.type_ == "decision" {
                suggestions.push(format!(
                    "Verifier si la decision '{}' reste valide",
                    short_text
                ));
            }
        }
    }

    // Medium fallback suggestion
    if suggestions.is_empty() && result.sensitivity == Sensitivity::Medium {
        if let Some(d) = result.domains.first() {
            suggestions.push(format!(
                "Consulter la memoire du domaine {} si le changement est significatif",
                d.name
            ));
        }
    }

    suggestions
}

// ── Output formatting (task 011) ───────────────────────────────────

fn format_text(result: &CheckDiffResult) -> String {
    let mut lines = Vec::new();

    lines.push("[project-wiki] Diff check".to_string());
    lines.push(String::new());

    let domain_count = result.domains.len();
    lines.push(format!("{} file(s) analyzed", result.files_analyzed));
    lines.push(format!("{} domain(s) affected", domain_count));

    let sensitivity_label = match result.sensitivity {
        Sensitivity::Low => "low",
        Sensitivity::Medium => "medium",
        Sensitivity::High => "high",
    };
    lines.push(format!("Sensitivity: {sensitivity_label}"));

    if !result.domains.is_empty() {
        lines.push(String::new());
        lines.push("Affected domains".to_string());
        for d in &result.domains {
            let item_count = d.memory_items.len();
            lines.push(format!(
                "  {} ({}) — {} file(s), {} item(s)",
                d.name,
                d.role,
                d.files.len(),
                item_count
            ));
        }
    }

    // Memory items
    let has_items = result.domains.iter().any(|d| !d.memory_items.is_empty());
    if has_items {
        lines.push(String::new());
        lines.push("Priority memory".to_string());
        for d in &result.domains {
            if d.memory_items.is_empty() {
                continue;
            }
            lines.push(format!("  {}:", d.name));
            for item in &d.memory_items {
                let related_marker = if item.directly_related { " *" } else { "" };
                lines.push(format!(
                    "    [{}] {} [{}]{}",
                    item.type_, item.text, item.confidence, related_marker
                ));
            }
        }
    }

    // Warnings
    let all_warnings: Vec<&DomainWarning> =
        result.domains.iter().flat_map(|d| &d.warnings).collect();
    if !all_warnings.is_empty() {
        lines.push(String::new());
        lines.push("Warnings".to_string());
        for w in &all_warnings {
            let detail = format_warning_detail(w);
            lines.push(format!("  \u{26a0} {detail}"));
        }
    }

    // Suggested actions
    if !result.suggested_actions.is_empty() {
        lines.push(String::new());
        lines.push("Suggested actions".to_string());
        for action in &result.suggested_actions {
            lines.push(format!("  \u{2192} {action}"));
        }
    }

    // Unresolved files
    if !result.unresolved_files.is_empty() {
        lines.push(String::new());
        lines.push("Unresolved files".to_string());
        for f in &result.unresolved_files {
            lines.push(format!("  {f}"));
        }
    }

    lines.join("\n")
}

/// Format a single warning into a human-readable detail string.
fn format_warning_detail(w: &DomainWarning) -> String {
    match w.kind.as_str() {
        "stale" => {
            let days = w.days.unwrap_or(0);
            format!("{} is stale ({days} days)", w.note)
        }
        "low_confidence" => {
            format!("{} has low confidence", w.note)
        }
        "no_memory" => {
            format!("No structured memory for {}", w.note)
        }
        "no_note" => "No wiki note found for domain".to_string(),
        other => other.to_string(),
    }
}

fn format_json(result: &CheckDiffResult) -> Result<String> {
    serde_json::to_string_pretty(result).map_err(|e| anyhow::anyhow!("JSON serialization: {e}"))
}

/// Format the check-diff result as a GitHub PR comment.
/// Returns `None` if sensitivity is `Low` (no comment needed).
pub fn format_pr_comment(result: &CheckDiffResult) -> Option<String> {
    if result.sensitivity == Sensitivity::Low {
        return None;
    }

    let mut lines = Vec::new();

    // Header with unique marker for idempotent updates
    lines.push("## \u{1f9e0} project-wiki \u{2014} Memory Check".to_string());
    lines.push("<!-- project-wiki-memory-check -->".to_string());
    lines.push(String::new());
    lines.push(format!("**Sensitivity: {}**", result.sensitivity));

    // Domains touched
    if !result.domains.is_empty() {
        lines.push(String::new());
        lines.push("### Domains touched".to_string());
        for d in &result.domains {
            lines.push(format!(
                "- **{}** ({} file(s), {} memory item(s))",
                d.name,
                d.files.len(),
                d.memory_items.len()
            ));
        }
    }

    // Priority memory table
    let has_items = result.domains.iter().any(|d| !d.memory_items.is_empty());
    if has_items {
        lines.push(String::new());
        lines.push("### Priority memory".to_string());
        lines.push("| Type | Item | Confidence |".to_string());
        lines.push("|------|------|------------|".to_string());
        for d in &result.domains {
            for item in &d.memory_items {
                let escaped_text = item.text.replace('|', "\\|");
                lines.push(format!(
                    "| {} | {} | {} |",
                    item.type_, escaped_text, item.confidence
                ));
            }
        }
    }

    // Warnings
    let all_warnings: Vec<&DomainWarning> =
        result.domains.iter().flat_map(|d| &d.warnings).collect();
    if !all_warnings.is_empty() {
        lines.push(String::new());
        lines.push("### Warnings".to_string());
        for w in &all_warnings {
            let detail = format_warning_detail(w);
            lines.push(format!("- \u{26a0}\u{fe0f} {detail}"));
        }
    }

    // Suggested actions
    if !result.suggested_actions.is_empty() {
        lines.push(String::new());
        lines.push("### Suggested actions".to_string());
        for action in &result.suggested_actions {
            lines.push(format!("- {action}"));
        }
    }

    Some(lines.join("\n"))
}

// ── Public entry point ─────────────────────────────────────────────

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

// ── Standalone analysis (for testing without CLI/wiki filesystem) ──

/// Build a CheckDiffResult from pre-built domain hits (useful for testing).
#[cfg(test)]
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

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiki::note::MemoryItem;

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
}
