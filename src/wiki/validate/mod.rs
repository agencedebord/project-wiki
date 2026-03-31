mod domains;
mod links;
mod memory_items;
mod migration_status;
mod notes;

#[cfg(test)]
mod tests;

use anyhow::{Result, bail};

use crate::ui;
use crate::wiki::common;
use crate::wiki::config;

use domains::{
    check_domain_name_coherence, check_missing_dependencies, check_undocumented_domains,
};
use links::{
    check_broken_links, check_dead_references, check_deprecated_references, check_orphan_notes,
    collect_all_md_files,
};
use memory_items::check_memory_items;
use migration_status::check_migration_status;
use notes::{check_confidence_ratio, check_staleness};

pub fn run(strict: bool) -> Result<()> {
    let wiki_dir = common::find_wiki_root()?;
    let wiki_config = config::load(&wiki_dir);

    ui::app_header(env!("CARGO_PKG_VERSION"));
    if strict {
        ui::action("Validating wiki (strict mode)");
    } else {
        ui::action("Validating wiki");
    }

    let mut errors: usize = 0;
    let mut warnings: usize = 0;
    let mut passes: usize = 0;

    let notes = common::collect_all_notes(&wiki_dir)?;
    let md_files = collect_all_md_files(&wiki_dir)?;

    // ─── 1. Broken links ───
    ui::header("Broken links");
    let broken = check_broken_links(&md_files, &wiki_dir)?;
    if broken.is_empty() {
        ui::resolved("No broken links found.");
        passes += 1;
    } else {
        for (file, target) in &broken {
            ui::unresolved(&format!("{} -> {} (not found)", file, target));
        }
        errors += broken.len();
    }

    // ─── 2. Undocumented domains ───
    ui::header("Undocumented domains");
    let undocumented = check_undocumented_domains()?;
    if undocumented.is_empty() {
        ui::resolved("All code domains are documented.");
        passes += 1;
    } else {
        for domain in &undocumented {
            ui::warn(&format!(
                "Domain '{}' found in code but not in wiki",
                domain
            ));
        }
        warnings += undocumented.len();
    }

    // ─── 3. Dead references ───
    ui::header("Dead references");
    let dead_refs = check_dead_references(&notes);
    if dead_refs.is_empty() {
        ui::resolved("All related_files references are valid.");
        passes += 1;
    } else {
        for (note_path, ref_path) in &dead_refs {
            ui::unresolved(&format!(
                "{} references {} (not found)",
                note_path, ref_path
            ));
        }
        errors += dead_refs.len();
    }

    // ─── 4. Deprecated references ───
    ui::header("Deprecated references");
    let deprecated_refs = check_deprecated_references(&notes, &md_files, &wiki_dir)?;
    if deprecated_refs.is_empty() {
        ui::resolved("No active notes link to deprecated notes.");
        passes += 1;
    } else {
        for (source, target) in &deprecated_refs {
            ui::warn(&format!("{} links to deprecated note {}", source, target));
        }
        warnings += deprecated_refs.len();
    }

    // ─── 5. Confidence ratio ───
    ui::header("Confidence ratio");
    let (low_confidence_count, total_count, low_pct) = check_confidence_ratio(&notes);
    if total_count == 0 {
        ui::resolved("No notes to check.");
        passes += 1;
    } else if low_pct > 40.0 {
        ui::warn(&format!(
            "{}/{} notes ({:.0}%) are inferred or needs-validation (threshold: 40%)",
            low_confidence_count, total_count, low_pct
        ));
        warnings += 1;
    } else {
        ui::resolved(&format!(
            "{}/{} notes ({:.0}%) are inferred or needs-validation — within threshold",
            low_confidence_count, total_count, low_pct
        ));
        passes += 1;
    }

    // ─── 6. Staleness ───
    ui::header("Staleness");
    let staleness_days = wiki_config.staleness_days;
    let stale = check_staleness(&notes, staleness_days);
    if stale.is_empty() {
        ui::resolved(&format!(
            "No stale notes (all updated within {} days).",
            staleness_days
        ));
        passes += 1;
    } else {
        for (path, days) in &stale {
            ui::warn(&format!("{} — last updated {} days ago", path, days));
        }
        warnings += stale.len();
    }

    // ─── 7. Orphan notes ───
    ui::header("Orphan notes");
    let orphans = check_orphan_notes(&wiki_dir)?;
    if orphans.is_empty() {
        ui::resolved("All domain notes are referenced in _index.md.");
        passes += 1;
    } else {
        for path in &orphans {
            ui::warn(&format!("{} is not referenced in _index.md", path));
        }
        warnings += orphans.len();
    }

    // ─── 8. Domain name coherence ───
    ui::header("Domain name coherence");
    let incoherent = check_domain_name_coherence(&notes);
    if incoherent.is_empty() {
        ui::resolved("All domain folder names match note domain fields.");
        passes += 1;
    } else {
        for msg in &incoherent {
            ui::unresolved(msg);
        }
        errors += incoherent.len();
    }

    // ─── 9. Cross-domain dependencies ───
    ui::header("Cross-domain dependencies");
    let missing_deps = check_missing_dependencies(&notes);
    if missing_deps.is_empty() {
        ui::resolved("All referenced dependencies exist in wiki.");
        passes += 1;
    } else {
        for (note_path, dep) in &missing_deps {
            ui::warn(&format!(
                "{} depends on '{}' but no such domain exists in wiki",
                note_path, dep
            ));
        }
        warnings += missing_deps.len();
    }

    // ─── 10. Memory items ───
    ui::header("Memory items");
    let (mi_errors, mi_warnings) = check_memory_items(&notes);
    if mi_errors.is_empty() && mi_warnings.is_empty() {
        let total_items: usize = notes.iter().map(|n| n.memory_items.len()).sum();
        if total_items > 0 {
            ui::resolved(&format!(
                "{} memory item(s) across {} note(s) — all valid.",
                total_items,
                notes.iter().filter(|n| !n.memory_items.is_empty()).count()
            ));
        } else {
            ui::resolved("No memory items to validate.");
        }
        passes += 1;
    } else {
        for err in &mi_errors {
            ui::unresolved(err);
        }
        for warn in &mi_warnings {
            ui::warn(warn);
        }
        errors += mi_errors.len();
        warnings += mi_warnings.len();
    }

    // ─── 11. Migration status ───
    ui::header("Migration status");
    let migration = check_migration_status(&notes);
    // info_warnings tracks warnings that are purely informational
    // and should NOT be promoted to errors in strict mode.
    let mut info_warnings: usize = 0;
    if migration.total == 0 {
        ui::resolved("No notes to check.");
        passes += 1;
    } else if migration.without_items == 0 {
        ui::resolved(&format!(
            "All {} note(s) have memory_items.",
            migration.total
        ));
        passes += 1;
    } else {
        let list = migration.legacy_paths.join(", ");
        ui::warn(&format!(
            "{} note(s) without memory_items: {}",
            migration.without_items, list
        ));
        info_warnings += 1;
    }

    // In strict mode, warnings are promoted to errors
    // (but info_warnings are excluded — they stay as warnings)
    if strict {
        errors += warnings;
        warnings = info_warnings;
    } else {
        warnings += info_warnings;
    }

    // ─── Summary ───
    ui::header("Summary");
    let summary_lines = vec![format!(
        "{} passed  {} warnings  {} errors",
        passes, warnings, errors
    )];
    let summary_strings: Vec<String> = summary_lines;
    ui::summary_box(&summary_strings);

    if errors > 0 {
        eprintln!();
        ui::error("Validation failed.");
        eprintln!();
        bail!(
            "Validation failed with {} error(s) and {} warning(s).",
            errors,
            warnings
        );
    } else if warnings > 0 {
        eprintln!();
        ui::done("Validation passed with warnings.");
    } else {
        eprintln!();
        ui::done("Validation passed.");
    }
    eprintln!();

    Ok(())
}
