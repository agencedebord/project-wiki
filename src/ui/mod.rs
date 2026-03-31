use console::style;

// ─── Unicode markers (no emojis) ───

const MARKER_SUCCESS: &str = "✓";
const MARKER_WARNING: &str = "▲";
const MARKER_ERROR: &str = "✗";
const MARKER_INFO: &str = "●";
const MARKER_STEP: &str = "├";
#[allow(dead_code)] // Reserved for future use in multi-step flows
const MARKER_STEP_LAST: &str = "└";
const MARKER_START: &str = "┌";
const MARKER_DIAMOND: &str = "◆";

// ─── Gradient helpers ───

/// Interpolate between two RGB colors.
fn lerp_color(from: (u8, u8, u8), to: (u8, u8, u8), t: f64) -> (u8, u8, u8) {
    let r = from.0 as f64 + (to.0 as f64 - from.0 as f64) * t;
    let g = from.1 as f64 + (to.1 as f64 - from.1 as f64) * t;
    let b = from.2 as f64 + (to.2 as f64 - from.2 as f64) * t;
    (r as u8, g as u8, b as u8)
}

/// Apply a gradient to a string, character by character.
pub fn gradient_text(text: &str, from: (u8, u8, u8), to: (u8, u8, u8)) -> String {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len().max(1) as f64;
    let mut result = String::new();
    for (i, ch) in chars.iter().enumerate() {
        let t = i as f64 / (len - 1.0).max(1.0);
        let (r, g, b) = lerp_color(from, to, t);
        result.push_str(&format!("\x1b[38;2;{};{};{}m{}\x1b[0m", r, g, b, ch));
    }
    result
}

/// Render a progress bar with gradient colors.
pub fn gradient_bar(progress: f64, width: usize, from: (u8, u8, u8), to: (u8, u8, u8)) -> String {
    let filled = (progress * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    let mut result = String::new();

    for i in 0..filled {
        let t = i as f64 / (width as f64 - 1.0).max(1.0);
        let (r, g, b) = lerp_color(from, to, t);
        result.push_str(&format!("\x1b[38;2;{};{};{}m█\x1b[0m", r, g, b));
    }
    for _ in 0..empty {
        result.push_str(&format!("{}", style("░").dim()));
    }

    result
}

/// Render a health bar (red → yellow → green based on percentage).
pub fn health_bar(progress: f64, width: usize) -> String {
    let red = (231, 76, 60); // #e74c3c
    let yellow = (241, 196, 15); // #f1c40f
    let green = (46, 204, 113); // #2ecc71

    let filled = (progress * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    let mut result = String::new();

    for i in 0..filled {
        let t = i as f64 / (width as f64 - 1.0).max(1.0);
        let color = if t < 0.5 {
            lerp_color(red, yellow, t * 2.0)
        } else {
            lerp_color(yellow, green, (t - 0.5) * 2.0)
        };
        result.push_str(&format!(
            "\x1b[38;2;{};{};{}m█\x1b[0m",
            color.0, color.1, color.2
        ));
    }
    for _ in 0..empty {
        result.push_str(&format!("{}", style("░").dim()));
    }

    result
}

// ─── Color palette ───

const VIOLET: (u8, u8, u8) = (138, 43, 226); // gradient start for titles
const BLUE: (u8, u8, u8) = (52, 152, 219); // gradient end for titles
const CYAN_COLOR: (u8, u8, u8) = (26, 188, 216);
#[cfg(feature = "notion")]
const ROSE: (u8, u8, u8) = (224, 102, 153);
#[cfg(feature = "enrich")]
const AMBER: (u8, u8, u8) = (255, 191, 0);

// ─── Public output functions ───

/// Print the app header with gradient.
pub fn app_header(version: &str) {
    let title = format!("codefidence v{}", version);
    eprintln!(
        "{} {}",
        style(MARKER_START).cyan(),
        gradient_text(&title, VIOLET, BLUE)
    );
    eprintln!("{}", style("│").dim());
}

/// Print a major action start.
pub fn action(msg: &str) {
    eprintln!("{} {}", style(MARKER_DIAMOND).cyan(), style(msg).bold());
}

/// Print a step within an action.
pub fn step(msg: &str) {
    eprintln!("{}  {}", style(MARKER_STEP).dim(), msg);
}

/// Print the last step within an action.
#[allow(dead_code)] // Reserved for future multi-step flows
pub fn step_last(msg: &str) {
    eprintln!("{}  {}", style(MARKER_STEP_LAST).dim(), msg);
}

/// Print a success message.
pub fn success(msg: &str) {
    eprintln!(
        "{} {}",
        style(MARKER_SUCCESS).green().bold(),
        style(msg).green()
    );
}

/// Print an informational message.
pub fn info(msg: &str) {
    eprintln!("{} {}", style(MARKER_INFO).cyan(), style(msg).dim());
}

/// Print a warning message.
pub fn warn(msg: &str) {
    eprintln!("{} {}", style(MARKER_WARNING).yellow(), style(msg).yellow());
}

/// Print an error message.
pub fn error(msg: &str) {
    eprintln!("{} {}", style(MARKER_ERROR).red().bold(), style(msg).red());
}

/// Print a section header.
pub fn header(msg: &str) {
    eprintln!();
    eprintln!(
        "{}  {}",
        style(MARKER_STEP).dim(),
        gradient_text(msg, VIOLET, BLUE)
    );
}

/// Print a stat line.
pub fn stat(label: &str, value: &str) {
    eprintln!(
        "{}  {:<24} {}",
        style("│").dim(),
        style(label).dim(),
        style(value).cyan().bold()
    );
}

/// Print a stat line with warning color.
#[allow(dead_code)] // Reserved for future validate/status enhancements
pub fn stat_warn(label: &str, value: &str) {
    eprintln!(
        "{}  {:<24} {}",
        style("│").dim(),
        style(label).dim(),
        style(value).yellow().bold()
    );
}

/// Print a stat line with error color.
#[allow(dead_code)] // Reserved for future validate/status enhancements
pub fn stat_error(label: &str, value: &str) {
    eprintln!(
        "{}  {:<24} {}",
        style("│").dim(),
        style(label).dim(),
        style(value).red().bold()
    );
}

/// Print a stat with a health/progress bar.
pub fn stat_bar(label: &str, count: usize, total: usize) {
    let pct = if total > 0 {
        count as f64 / total as f64
    } else {
        0.0
    };
    let bar = health_bar(pct, 20);
    eprintln!(
        "{}  {:<24} {} {:>3}%",
        style("│").dim(),
        style(label).dim(),
        bar,
        (pct * 100.0).round() as u32
    );
}

/// Print a progress bar for scan operations (blue → cyan gradient).
pub fn scan_progress(msg: &str, progress: f64) {
    let bar = gradient_bar(progress, 30, BLUE, CYAN_COLOR);
    let pct = (progress * 100.0).round() as u32;
    eprintln!(
        "{}  {} {:>3}%  {}",
        style("│").dim(),
        bar,
        pct,
        style(msg).dim()
    );
}

/// Print a progress bar for Notion operations (violet → rose gradient).
#[cfg(feature = "notion")]
pub fn notion_progress(msg: &str, progress: f64) {
    let bar = gradient_bar(progress, 30, VIOLET, ROSE);
    let pct = (progress * 100.0).round() as u32;
    eprintln!(
        "{}  {} {:>3}%  {}",
        style("│").dim(),
        bar,
        pct,
        style(msg).dim()
    );
}

/// Print a progress bar for LLM enrichment operations (amber → cyan gradient).
#[cfg(feature = "enrich")]
pub fn enrich_progress(msg: &str, progress: f64) {
    let bar = gradient_bar(progress, 30, AMBER, CYAN_COLOR);
    let pct = (progress * 100.0).round() as u32;
    eprintln!(
        "{}  {} {:>3}%  {}",
        style("│").dim(),
        bar,
        pct,
        style(msg).dim()
    );
}

/// Print a resolved contradiction.
pub fn resolved(msg: &str) {
    eprintln!(
        "{}  {} {}",
        style("│").dim(),
        style(MARKER_INFO).green(),
        msg
    );
}

/// Print an unresolved issue.
pub fn unresolved(msg: &str) {
    eprintln!(
        "{}  {} {}",
        style("│").dim(),
        style(MARKER_WARNING).red(),
        style(msg).red()
    );
}

/// Print a domain name in the domain list.
pub fn domain_entry(name: &str, detail: &str, is_stale: bool) {
    let marker = if is_stale {
        style(MARKER_WARNING).yellow().to_string()
    } else {
        style(MARKER_SUCCESS).green().to_string()
    };
    let detail_styled = if is_stale {
        style(detail).yellow().to_string()
    } else {
        style(detail).dim().to_string()
    };
    eprintln!(
        "{}  {} {:<20} {}",
        style("│").dim(),
        marker,
        style(name).white().bold(),
        detail_styled
    );
}

/// Print a box around summary content.
pub fn summary_box(lines: &[String]) {
    let max_len = lines
        .iter()
        .map(|l| console::measure_text_width(l))
        .max()
        .unwrap_or(40);
    let border_len = max_len + 4;

    eprintln!("{}  ┌{}┐", style("│").dim(), "─".repeat(border_len));
    for line in lines {
        let padding = max_len - console::measure_text_width(line);
        eprintln!(
            "{}  │  {}{}  │",
            style("│").dim(),
            line,
            " ".repeat(padding)
        );
    }
    eprintln!("{}  └{}┘", style("│").dim(), "─".repeat(border_len));
}

/// Print a message only when verbose mode is enabled.
pub fn verbose(msg: &str) {
    if crate::verbosity::is_verbose() {
        eprintln!("  {} {}", style(MARKER_INFO).dim(), style(msg).dim());
    }
}

/// Print a debug message (only at -vv or higher).
#[allow(dead_code)] // Available for use at -vv verbosity
pub fn debug(msg: &str) {
    if crate::verbosity::is_debug() {
        eprintln!("  {} {}", style("DBG").dim().cyan(), style(msg).dim());
    }
}

/// Print the closing line.
pub fn done(msg: &str) {
    eprintln!();
    eprintln!(
        "{} {}",
        style(MARKER_SUCCESS).green().bold(),
        style(msg).green().bold()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_text_returns_non_empty_string() {
        let result = gradient_text("hello", (255, 0, 0), (0, 0, 255));
        assert!(!result.is_empty());
        // Should contain the original characters
        assert!(result.contains('h'));
        assert!(result.contains('o'));
    }

    #[test]
    fn gradient_text_handles_empty_string() {
        let result = gradient_text("", (255, 0, 0), (0, 0, 255));
        assert!(result.is_empty());
    }

    #[test]
    fn gradient_text_handles_single_char() {
        let result = gradient_text("x", (255, 0, 0), (0, 0, 255));
        assert!(!result.is_empty());
        assert!(result.contains('x'));
    }

    #[test]
    fn gradient_bar_returns_correct_visual_length() {
        let width = 10;
        let result = gradient_bar(0.5, width, (0, 255, 0), (0, 255, 0));
        // The bar should contain filled and empty characters
        let filled_count = result.matches('\u{2588}').count(); // '█'
        assert_eq!(
            filled_count, 5,
            "50% of width 10 should give 5 filled blocks"
        );
    }

    #[test]
    fn gradient_bar_zero_progress() {
        let width = 10;
        let result = gradient_bar(0.0, width, (0, 255, 0), (0, 255, 0));
        let filled_count = result.matches('\u{2588}').count();
        assert_eq!(filled_count, 0);
    }

    #[test]
    fn gradient_bar_full_progress() {
        let width = 10;
        let result = gradient_bar(1.0, width, (0, 255, 0), (0, 255, 0));
        let filled_count = result.matches('\u{2588}').count();
        assert_eq!(filled_count, 10);
    }

    #[test]
    fn health_bar_returns_correct_visual_length() {
        let width = 20;
        let result = health_bar(0.5, width);
        let filled_count = result.matches('\u{2588}').count();
        assert_eq!(
            filled_count, 10,
            "50% of width 20 should give 10 filled blocks"
        );
    }

    #[test]
    fn health_bar_zero_progress() {
        let width = 20;
        let result = health_bar(0.0, width);
        let filled_count = result.matches('\u{2588}').count();
        assert_eq!(filled_count, 0);
    }

    #[test]
    fn health_bar_full_progress() {
        let width = 20;
        let result = health_bar(1.0, width);
        let filled_count = result.matches('\u{2588}').count();
        assert_eq!(filled_count, 20);
    }

    #[test]
    fn lerp_color_at_zero_returns_from() {
        let result = lerp_color((100, 150, 200), (200, 250, 50), 0.0);
        assert_eq!(result, (100, 150, 200));
    }

    #[test]
    fn lerp_color_at_one_returns_to() {
        let result = lerp_color((100, 150, 200), (200, 250, 50), 1.0);
        assert_eq!(result, (200, 250, 50));
    }

    #[test]
    fn lerp_color_at_half_returns_midpoint() {
        let result = lerp_color((0, 0, 0), (200, 100, 50), 0.5);
        assert_eq!(result, (100, 50, 25));
    }
}
