use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

use crate::wiki::config;
use crate::wiki::file_index;
use crate::wiki::note::WikiNote;

use super::prioritize::prioritize_and_format_items;
use super::warnings::build_warnings;
use super::{DomainHit, DomainRole, DomainWarning};

/// Intermediate struct before role assignment.
pub(super) struct DomainAgg {
    pub files: Vec<String>,
    pub note: Option<WikiNote>,
    pub note_path: String,
}

/// Resolve files to domains, load notes, and build DomainHits.
pub(super) fn resolve_domains(
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
