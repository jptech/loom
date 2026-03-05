use std::time::Duration;

use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};

// ── Unicode constants ────────────────────────────────────────────────

pub const CHECK: &str = "\u{2713}"; // ✓
pub const CROSS: &str = "\u{2717}"; // ✗
pub const DOT: &str = "\u{25CF}"; // ●
pub const WARNING: &str = "\u{26A0}"; // ⚠
pub const BLOCK_FULL: char = '\u{2588}'; // █
pub const BLOCK_EMPTY: char = '\u{2591}'; // ░
pub const TREE_BRANCH: &str = "\u{251C}\u{2500}\u{2500}"; // ├──
pub const TREE_LAST: &str = "\u{2514}\u{2500}\u{2500}"; // └──
pub const TREE_VERT: &str = "\u{2502}"; // │
pub const CONNECTOR: &str = "\u{2570}\u{2500}"; // ╰─
pub const DASH: &str = "\u{2500}"; // ─

// ── Icon enum ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub enum Icon {
    Check,
    Cross,
    Dot,
    Warning,
}

impl Icon {
    pub fn render(self) -> String {
        match self {
            Icon::Check => CHECK.green().to_string(),
            Icon::Cross => CROSS.red().to_string(),
            Icon::Dot => DOT.cyan().to_string(),
            Icon::Warning => WARNING.yellow().to_string(),
        }
    }
}

// ── Header ───────────────────────────────────────────────────────────

/// Print a styled header line:  `  loom v0.1.0 (abc1234) · part1 → part2 · part3`
pub fn header(parts: &[(&str, &str)]) {
    let version = env!("CARGO_PKG_VERSION");
    let git_hash = env!("LOOM_GIT_HASH");
    let mut line = format!(
        "  {} {} {}",
        "loom".cyan().bold(),
        format!("v{}", version).dimmed(),
        format!("({})", git_hash).dimmed()
    );
    for (sep, value) in parts {
        line.push_str(&format!(" {} {}", sep.dimmed(), value.bold()));
    }
    eprintln!("{}", line);
    eprintln!();
}

// ── Status lines ─────────────────────────────────────────────────────

/// Print a status line: `  ✓ Label         detail`
pub fn status(icon: Icon, label: &str, detail: &str) {
    if detail.is_empty() {
        eprintln!("  {} {:<14}", icon.render(), label);
    } else {
        eprintln!("  {} {:<14} {}", icon.render(), label, detail);
    }
}

/// Print a status line with timing and memory: `  ✓ Label         27s    512 MB`
pub fn status_with_metrics(icon: Icon, label: &str, secs: f64, mb: u64) {
    eprintln!(
        "  {} {:<14} {:>6}   {:>4} MB",
        icon.render(),
        label,
        format_duration(secs),
        mb
    );
}

// ── Sub-items ────────────────────────────────────────────────────────

/// Print a tree sub-item: `    ├ msg` or `    └ msg`
pub fn sub_item(msg: &str, is_last: bool) {
    let prefix = if is_last { "\u{2514}" } else { "\u{251C}" }; // └ or ├
    eprintln!("    {} {}", prefix, msg);
}

/// Print a warning sub-item: `    ⚠ msg`
pub fn sub_warning(msg: &str) {
    eprintln!("    {} {}", WARNING.yellow(), msg.yellow());
}

// ── Utilization bars ─────────────────────────────────────────────────

/// Generate a 20-char utilization bar: `████████░░░░░░░░░░░░`
pub fn util_bar(pct: f64) -> String {
    let width = 20;
    let filled = ((pct / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    let empty = width - filled;
    format!(
        "{}{}",
        BLOCK_FULL.to_string().repeat(filled),
        BLOCK_EMPTY.to_string().repeat(empty)
    )
}

/// Print side-by-side utilization pairs:
/// `    ├ LUT  45.2%  ████████░░░░  FF  32.1%  ██████░░░░░░`
pub fn util_pair(n1: &str, p1: f64, n2: &str, p2: f64, is_last: bool) {
    let prefix = if is_last { "\u{2514}" } else { "\u{251C}" };
    eprintln!(
        "    {} {:<4} {:>5.1}%  {}  {:<4} {:>5.1}%  {}",
        prefix,
        n1,
        p1,
        util_bar(p1),
        n2,
        p2,
        util_bar(p2),
    );
}

// ── Timing ───────────────────────────────────────────────────────────

/// Print a timing line with colored pass/fail indicators.
pub fn timing_line(label: &str, wns: f64, whs: f64, is_last: bool) {
    let prefix = if is_last { "\u{2514}" } else { "\u{251C}" };
    let wns_icon = if wns >= 0.0 {
        CHECK.green().to_string()
    } else {
        CROSS.red().to_string()
    };
    let whs_icon = if whs >= 0.0 {
        CHECK.green().to_string()
    } else {
        CROSS.red().to_string()
    };
    eprintln!(
        "    {} {}  WNS {:+.3}ns {}  WHS {:+.3}ns {}",
        prefix, label, wns, wns_icon, whs, whs_icon
    );
}

// ── Spinner ──────────────────────────────────────────────────────────

/// Create a spinner with custom tick chars and elapsed time display.
pub fn create_spinner(msg: &str) -> ProgressBar {
    let sp = ProgressBar::new_spinner();
    sp.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("\u{25D0}\u{25D3}\u{25D1}\u{25D2} ") // ◐◓◑◒
            .template("  {spinner:.cyan} {msg}  [{elapsed:.dim}]")
            .unwrap(),
    );
    sp.enable_steady_tick(Duration::from_millis(120));
    sp.set_message(msg.to_string());
    sp
}

// ── Duration formatting ──────────────────────────────────────────────

/// Format seconds as a compact duration string.
/// `5.3s`, `27s`, `2m29s`, `1h03m`
pub fn format_duration(secs: f64) -> String {
    let s = secs.round() as u64;
    if s >= 3600 {
        format!("{}h{:02}m", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{}m{:02}s", s / 60, s % 60)
    } else if secs < 10.0 && secs > 0.0 {
        format!("{:.1}s", secs)
    } else {
        format!("{}s", s)
    }
}

// ── Error block ──────────────────────────────────────────────────────

/// Print a styled error block:
/// ```text
///   error[E2] Configuration error
///   ╰─ message
/// ```
pub fn error_block(code: i32, prefix: &str, message: &str) {
    eprintln!(
        "  {} {}",
        format!("error[E{}]", code).red().bold(),
        prefix.red()
    );
    eprintln!("  {} {}", CONNECTOR, message);
}

// ── Summary ──────────────────────────────────────────────────────────

/// Print a success summary: `  ✓ Build passed · 2m29s`
pub fn summary_pass(label: &str, duration_secs: Option<f64>) {
    let dur = duration_secs
        .map(|s| format!(" \u{00B7} {}", format_duration(s)))
        .unwrap_or_default();
    eprintln!();
    eprintln!(
        "  {} {}{}",
        CHECK.green(),
        label.green().bold(),
        dur.dimmed()
    );
}

/// Print a failure summary: `  ✗ Build failed · phase`
pub fn summary_fail(label: &str, detail: &str) {
    eprintln!();
    eprintln!(
        "  {} {} \u{00B7} {}",
        CROSS.red(),
        label.red().bold(),
        detail
    );
}

/// Print a summary detail line: `    Key: value`
pub fn summary_detail(key: &str, value: &str) {
    eprintln!("    {}: {}", key, value);
}

// ── Dependency tree ──────────────────────────────────────────────────

/// Print the tree root label.
pub fn tree_root(text: &str) {
    eprintln!("  {}", text.bold());
}

/// Print a tree item with the appropriate connector.
pub fn tree_item(text: &str, is_last: bool) {
    let prefix = if is_last { TREE_LAST } else { TREE_BRANCH };
    eprintln!("  {} {}", prefix, text);
}

/// Print a detail line under a tree item (for verbose mode).
pub fn tree_detail(text: &str, is_last_parent: bool) {
    let prefix = if is_last_parent { " " } else { TREE_VERT };
    eprintln!("  {}     {}", prefix, text.dimmed());
}

// ── Section headers (for loom status) ────────────────────────────────

/// Print a section header: `  Title`
pub fn section_header(title: &str) {
    eprintln!("  {}", title.bold());
}

/// Print a detail key-value line: `    Name:        value`
pub fn detail_line(key: &str, value: &str) {
    eprintln!("    {:<12} {}", format!("{}:", key), value);
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Capitalize the first letter of a string.
pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + c.as_str(),
    }
}

/// Print a blank line.
pub fn blank() {
    eprintln!();
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(0.0), "0s");
        assert_eq!(format_duration(5.3), "5.3s");
        assert_eq!(format_duration(27.0), "27s");
        assert_eq!(format_duration(10.0), "10s");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(60.0), "1m00s");
        assert_eq!(format_duration(149.0), "2m29s");
        assert_eq!(format_duration(90.0), "1m30s");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(3600.0), "1h00m");
        assert_eq!(format_duration(3780.0), "1h03m");
    }

    #[test]
    fn test_util_bar_empty() {
        let bar = util_bar(0.0);
        assert_eq!(bar.chars().count(), 20);
        assert!(!bar.contains(BLOCK_FULL));
    }

    #[test]
    fn test_util_bar_full() {
        let bar = util_bar(100.0);
        assert_eq!(bar.chars().count(), 20);
        assert!(!bar.contains(BLOCK_EMPTY));
    }

    #[test]
    fn test_util_bar_half() {
        let bar = util_bar(50.0);
        assert_eq!(bar.chars().count(), 20);
        let full_count = bar.chars().filter(|&c| c == BLOCK_FULL).count();
        assert_eq!(full_count, 10);
    }

    #[test]
    fn test_capitalize() {
        assert_eq!(capitalize("hello"), "Hello");
        assert_eq!(capitalize(""), "");
        assert_eq!(capitalize("a"), "A");
        assert_eq!(capitalize("synthesize"), "Synthesize");
    }

    #[test]
    fn test_icon_render() {
        // Just ensure they don't panic and produce non-empty strings
        assert!(!Icon::Check.render().is_empty());
        assert!(!Icon::Cross.render().is_empty());
        assert!(!Icon::Dot.render().is_empty());
        assert!(!Icon::Warning.render().is_empty());
    }
}
