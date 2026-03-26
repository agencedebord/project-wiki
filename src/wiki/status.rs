use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use console::style;

use crate::ui;
use crate::wiki::common;
use crate::wiki::note::{Confidence, WikiNote};

pub fn run() -> Result<()> {
    let wiki_dir = common::find_wiki_root()?;

    let domains = common::list_domain_names(&wiki_dir)?;
    let notes = common::collect_all_notes(&wiki_dir)?;
    let total_notes = notes.len();

    let confirmed = notes
        .iter()
        .filter(|n| matches!(n.confidence, Confidence::Confirmed | Confidence::Verified))
        .count();
    let seen_in_code = notes
        .iter()
        .filter(|n| matches!(n.confidence, Confidence::SeenInCode))
        .count();
    let inferred = notes
        .iter()
        .filter(|n| matches!(n.confidence, Confidence::Inferred))
        .count();
    let needs_validation = notes
        .iter()
        .filter(|n| matches!(n.confidence, Confidence::NeedsValidation))
        .count();

    let decisions = count_decisions()?;
    let stale_notes = find_stale(&notes);
    let open_questions = count_open_questions()?;

    // ─── Header ───
    ui::app_header(env!("CARGO_PKG_VERSION"));
    ui::action("Wiki status");

    // ─── Health gauge ───
    let _confirmed_pct = if total_notes > 0 {
        (confirmed + seen_in_code) as f64 / total_notes as f64
    } else {
        0.0
    };
    ui::header("Health");
    ui::stat_bar("Confirmed", confirmed + seen_in_code, total_notes);

    // ─── Overview ───
    ui::header("Overview");
    ui::stat("Domains", &domains.len().to_string());
    ui::stat("Notes", &total_notes.to_string());
    ui::stat("Decisions", &decisions.to_string());

    // ─── Confidence breakdown ───
    ui::header("Confidence");
    if total_notes > 0 {
        let bar_width = 15;
        let conf_bar = ui::gradient_bar(
            confirmed as f64 / total_notes as f64,
            bar_width,
            (46, 204, 113),
            (46, 204, 113),
        );
        let code_bar = ui::gradient_bar(
            seen_in_code as f64 / total_notes as f64,
            bar_width,
            (52, 152, 219),
            (52, 152, 219),
        );
        let inf_bar = ui::gradient_bar(
            inferred as f64 / total_notes as f64,
            bar_width,
            (241, 196, 15),
            (241, 196, 15),
        );

        eprintln!(
            "{}  {:<20} {} {:>3}",
            style("│").dim(),
            style("Confirmed/Verified").dim(),
            conf_bar,
            style(confirmed).green().bold()
        );
        eprintln!(
            "{}  {:<20} {} {:>3}",
            style("│").dim(),
            style("Seen in code").dim(),
            code_bar,
            style(seen_in_code).cyan().bold()
        );
        eprintln!(
            "{}  {:<20} {} {:>3}",
            style("│").dim(),
            style("Inferred").dim(),
            inf_bar,
            style(inferred).yellow().bold()
        );
        if needs_validation > 0 {
            let val_bar = ui::gradient_bar(
                needs_validation as f64 / total_notes as f64,
                bar_width,
                (231, 76, 60),
                (231, 76, 60),
            );
            eprintln!(
                "{}  {:<20} {} {:>3}",
                style("│").dim(),
                style("Needs validation").dim(),
                val_bar,
                style(needs_validation).red().bold()
            );
        }
    } else {
        ui::info("No notes yet.");
    }

    // ─── Domains ───
    if !domains.is_empty() {
        ui::header("Domains");
        let today = Utc::now().date_naive();
        for domain_name in &domains {
            let domain_notes: Vec<&WikiNote> = notes
                .iter()
                .filter(|n| n.domain == *domain_name)
                .collect();

            let latest_update = domain_notes
                .iter()
                .filter_map(|n| n.last_updated)
                .max();

            let (detail, is_stale) = match latest_update {
                Some(date) => {
                    let days = (today - date).num_days();
                    if days > 30 {
                        (format!("Updated {} days ago", days), true)
                    } else if days == 0 {
                        ("Updated today".to_string(), false)
                    } else if days == 1 {
                        ("Updated yesterday".to_string(), false)
                    } else {
                        (format!("Updated {} days ago", days), false)
                    }
                }
                None => ("No update date".to_string(), false),
            };

            ui::domain_entry(domain_name, &detail, is_stale);
        }
    }

    // ─── Alerts ───
    let has_alerts = !stale_notes.is_empty() || open_questions > 0 || needs_validation > 0;
    if has_alerts {
        ui::header("Alerts");
        if !stale_notes.is_empty() {
            ui::warn(&format!(
                "{} stale note(s) — not updated in 30+ days",
                stale_notes.len()
            ));
            for note_path in &stale_notes {
                eprintln!(
                    "{}    {}",
                    style("│").dim(),
                    style(note_path).dim()
                );
            }
        }
        if open_questions > 0 {
            ui::warn(&format!(
                "{} open question(s) in _needs-review.md",
                open_questions
            ));
        }
        if needs_validation > 0 {
            ui::warn(&format!(
                "{} note(s) marked [needs-validation]",
                needs_validation
            ));
        }
    }

    eprintln!();
    ui::done("Status complete.");
    eprintln!();

    Ok(())
}

fn count_decisions() -> Result<usize> {
    let decisions_dir = Path::new(".wiki/decisions");
    if !decisions_dir.exists() {
        return Ok(0);
    }

    let count = fs::read_dir(decisions_dir)
        .context("Failed to read .wiki/decisions")?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map_or(false, |ext| ext == "md")
        })
        .count();

    Ok(count)
}

fn find_stale(notes: &[WikiNote]) -> Vec<String> {
    let today = Utc::now().date_naive();
    notes
        .iter()
        .filter(|n| {
            n.last_updated
                .map_or(false, |date| (today - date).num_days() > 30)
        })
        .map(|n| n.path.clone())
        .collect()
}

fn count_open_questions() -> Result<usize> {
    let needs_review_path = Path::new(".wiki/_needs-review.md");
    if !needs_review_path.exists() {
        return Ok(0);
    }

    let content =
        fs::read_to_string(needs_review_path).context("Failed to read _needs-review.md")?;

    Ok(content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("- [ ]")
        })
        .count())
}
