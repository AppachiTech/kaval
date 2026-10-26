use std::collections::HashMap;
use std::io::{self, Write};

use ratatui::crossterm::style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor};

use crate::models::{
    self, count_browser_groups, dedup_browser_indices, CPU_ERROR_THRESHOLD, CPU_WARN_THRESHOLD,
};
use crate::theme::theme;

/// Helper to conditionally emit ANSI color/attribute codes
struct Ansi(bool);

impl Ansi {
    fn fg(&self, c: Color) -> String {
        if self.0 {
            format!("{}", SetForegroundColor(c))
        } else {
            String::new()
        }
    }
    fn bold(&self) -> String {
        if self.0 {
            format!("{}", SetAttribute(Attribute::Bold))
        } else {
            String::new()
        }
    }
    fn reset(&self) -> String {
        if self.0 {
            format!("{}{}", SetAttribute(Attribute::Reset), ResetColor)
        } else {
            String::new()
        }
    }
}

/// Options for table rendering
struct TableOptions {
    /// Browser count per (pid, service) for grouping — None skips grouping
    browser_counts: Option<HashMap<(u32, String), usize>>,
    /// Show command line below each row
    show_cmd: bool,
    /// Show summary footer with these pre-computed counts (total, tcp, udp)
    summary_counts: Option<(usize, usize, usize)>,
}

pub fn print_table(entries: &[models::PortEntry], color: bool) {
    if entries.is_empty() {
        println!("No listening ports found.");
        return;
    }

    let browser_counts = count_browser_groups(entries);
    let display_entries: Vec<&models::PortEntry> = dedup_browser_indices(entries, &browser_counts)
        .into_iter()
        .map(|i| &entries[i])
        .collect();

    let total = entries.len();
    let tcp = entries
        .iter()
        .filter(|e| e.protocol == models::Protocol::Tcp)
        .count();
    let udp = total - tcp;

    let opts = TableOptions {
        browser_counts: Some(browser_counts),
        show_cmd: false,
        summary_counts: Some((total, tcp, udp)),
    };

    print_rows(&display_entries, color, &opts);
}

pub fn print_check_table(entries: &[&models::PortEntry], color: bool) {
    let opts = TableOptions {
        browser_counts: None,
        show_cmd: true,
        summary_counts: None,
    };

    print_rows(entries, color, &opts);
}

fn print_rows(entries: &[&models::PortEntry], color: bool, opts: &TableOptions) {
    let t = theme();
    let a = Ansi(color);
    let out = io::stdout();
    let mut w = out.lock();

    let hdr = t.ct(t.text_secondary);
    let dim = t.ct(t.text_muted);
    let divider = t.ct(t.border);
    let light = t.ct(t.text_secondary);
    let error = t.ct(t.error);
    let warning = t.ct(t.warning);

    // Header
    let _ = writeln!(
        w,
        "{}{}  {:<6} {:<5} {:<22} {:<20} {:<7} {:<7} {:<9} UPTIME{}",
        a.fg(hdr),
        a.bold(),
        "PORT",
        "PROTO",
        "PROCESS",
        "SERVICE",
        "PID",
        "CPU",
        "MEM",
        a.reset(),
    );
    let _ = writeln!(w, "{}{}{}", a.fg(divider), "─".repeat(100), a.reset(),);

    for e in entries {
        let cat_col = t.category_ct_color(e.category);

        // Resolve browser grouping
        let browser_count = opts
            .browser_counts
            .as_ref()
            .zip(e.browser_key())
            .and_then(|(counts, key)| counts.get(&key).copied())
            .unwrap_or(1);

        let port_text = if browser_count > 1 {
            "···".to_string()
        } else {
            e.port.to_string()
        };
        let port_col = if e.service.is_some() {
            Color::White
        } else {
            light
        };

        let _ = write!(
            w,
            "{}{}  {:<6}{}",
            a.fg(port_col),
            a.bold(),
            port_text,
            a.reset()
        );
        let _ = write!(
            w,
            "{} {:<5}",
            a.fg(t.proto_ct_color(e.protocol)),
            e.protocol
        );
        let _ = write!(w, "{} {:<22}", a.fg(cat_col), truncate(&e.process_name, 22));

        let service_text = match e.service_name() {
            Some(svc) if browser_count > 1 => format!("{} ×{}", svc, browser_count),
            Some(svc) => svc.to_string(),
            None => String::new(),
        };
        if service_text.is_empty() {
            let _ = write!(w, "{} {:<20}", a.fg(divider), "·");
        } else {
            let _ = write!(
                w,
                "{}{} {:<20}{}",
                a.fg(cat_col),
                a.bold(),
                service_text,
                a.reset()
            );
        }

        let pid_text = e.pid.map_or_else(|| "-".to_string(), |p| p.to_string());
        let _ = write!(w, "{} {:<7}", a.fg(dim), pid_text);
        let cpu_str = format!("{:.1}%", e.cpu_percent);
        let cpu_col = if e.cpu_percent > CPU_ERROR_THRESHOLD {
            error
        } else if e.cpu_percent > CPU_WARN_THRESHOLD {
            warning
        } else {
            dim
        };
        let _ = write!(w, "{} {:<7}", a.fg(cpu_col), cpu_str);
        let _ = write!(w, "{} {:<9}", a.fg(light), e.memory_display());
        let _ = write!(w, "{} {}", a.fg(dim), e.uptime_display());
        let _ = writeln!(w, "{}", a.reset());

        // Show command line below the row
        if opts.show_cmd && !e.process_cmd.is_empty() {
            let _ = writeln!(
                w,
                "{}    cmd: {}{}",
                a.fg(dim),
                truncate(&e.cmd_display(), 90),
                a.reset()
            );
        }
    }

    // Summary footer
    if let Some((total, tcp, udp)) = opts.summary_counts {
        let services = entries.iter().filter(|e| e.service.is_some()).count();
        let _ = write!(
            w,
            "\n{}{}{} ports{} ({} TCP, {} UDP)",
            a.fg(hdr),
            a.bold(),
            total,
            a.reset(),
            tcp,
            udp,
        );
        if services > 0 {
            let _ = write!(w, " · {} known services", services);
        }
        let _ = writeln!(w, "{}", a.reset());
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max - 1).collect();
        format!("{}…", truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_exact_length() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_long_string() {
        assert_eq!(truncate("hello world", 5), "hell…");
    }

    #[test]
    fn truncate_empty_string() {
        assert_eq!(truncate("", 5), "");
    }

    #[test]
    fn truncate_multibyte_utf8() {
        // Japanese characters (3 bytes each) — must not panic
        assert_eq!(truncate("日本語テスト", 4), "日本語…");
    }

    #[test]
    fn truncate_emoji() {
        // Emojis are multi-byte — must not panic
        assert_eq!(truncate("🚀🔥💡🎯🎉", 3), "🚀🔥…");
    }

    #[test]
    fn truncate_mixed_ascii_and_unicode() {
        assert_eq!(truncate("café résumé", 6), "café …");
    }
}
