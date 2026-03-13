mod cli;
mod models;
mod scanner;
mod theme;
mod ui;
mod util;

use std::collections::HashSet;
use std::io::{self, IsTerminal, Write};

use anyhow::Result;
use clap::Parser;
use crossterm::style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor};
use cli::{Cli, Command};
use models::ServiceCategory;
use scanner::{kill_process, new_system_with_cpu, scan_ports};
use theme::theme;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            // Default: launch TUI
            ui::run_tui()?;
        }

        Some(Command::List { json }) => {
            let mut sys = new_system_with_cpu();
            let entries = scan_ports(&mut sys, true, true)?;
            if json {
                print_json(&entries)?;
            } else {
                print_table(&entries, io::stdout().is_terminal());
            }
        }

        Some(Command::Check { ports }) => {
            let mut sys = new_system_with_cpu();
            let entries = scan_ports(&mut sys, true, true)?;
            let color = io::stdout().is_terminal();
            let matches: Vec<_> = entries
                .iter()
                .filter(|e| ports.contains(&e.port))
                .collect();
            let missing: Vec<_> = ports
                .iter()
                .filter(|p| !entries.iter().any(|e| e.port == **p))
                .collect();
            if !matches.is_empty() {
                print_check_table(&matches, color);
            }
            for port in &missing {
                println!("Nothing listening on port {}", port);
            }
            if matches.is_empty() {
                std::process::exit(1);
            }
        }

        Some(Command::Kill { ports, force }) => {
            let mut sys = new_system_with_cpu();
            let entries = scan_ports(&mut sys, true, true)?;
            let mut killed_pids: HashSet<u32> = HashSet::new();
            for port in &ports {
                let matches: Vec<_> = entries.iter().filter(|e| e.port == *port).collect();
                if matches.is_empty() {
                    println!("Nothing listening on port {}", port);
                } else {
                    for entry in &matches {
                        // Skip if we already killed this PID (same process on multiple ports)
                        if !killed_pids.insert(entry.pid) {
                            continue;
                        }
                        if !force {
                            println!(
                                "Killing {} (PID {}) on port {}...",
                                entry.process_name, entry.pid, entry.port
                            );
                        }
                        kill_process(entry.pid, force)?;
                        println!(
                            "{}Killed {} (PID {})",
                            if force { "Force " } else { "" },
                            entry.process_name,
                            entry.pid
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

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
    browser_counts: Option<std::collections::HashMap<(u32, &'static str), usize>>,
    /// Show command line below each row
    show_cmd: bool,
    /// Show summary footer
    show_summary: bool,
}

fn print_table(entries: &[models::PortEntry], color: bool) {
    if entries.is_empty() {
        println!("No listening ports found.");
        return;
    }

    // Count browser entries per (pid, service) and deduplicate
    let mut browser_counts: std::collections::HashMap<(u32, &str), usize> =
        std::collections::HashMap::new();
    for e in entries {
        if e.category == ServiceCategory::Browser {
            if let Some(svc) = e.known_service {
                *browser_counts.entry((e.pid, svc)).or_insert(0) += 1;
            }
        }
    }

    // Build deduplicated list: keep first browser entry per (pid, service)
    let mut browser_seen: std::collections::HashSet<(u32, &str)> =
        std::collections::HashSet::new();
    let display_entries: Vec<&models::PortEntry> = entries
        .iter()
        .filter(|e| {
            if e.category == ServiceCategory::Browser {
                if let Some(svc) = e.known_service {
                    return browser_seen.insert((e.pid, svc));
                }
            }
            true
        })
        .collect();

    let opts = TableOptions {
        browser_counts: Some(browser_counts),
        show_cmd: false,
        show_summary: true,
    };

    print_rows(&display_entries, entries.len(), color, &opts);
}

fn print_check_table(entries: &[&models::PortEntry], color: bool) {
    let opts = TableOptions {
        browser_counts: None,
        show_cmd: true,
        show_summary: false,
    };

    print_rows(entries, entries.len(), color, &opts);
}

fn print_rows(entries: &[&models::PortEntry], total_count: usize, color: bool, opts: &TableOptions) {
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
    let _ = writeln!(
        w,
        "{}{}{}",
        a.fg(divider),
        "─".repeat(100),
        a.reset(),
    );

    for e in entries {
        let cat_col = t.category_ct_color(e.category);

        // Resolve browser grouping
        let browser_count = opts
            .browser_counts
            .as_ref()
            .and_then(|counts| {
                if e.category == ServiceCategory::Browser {
                    e.known_service
                        .and_then(|svc| counts.get(&(e.pid, svc)).copied())
                } else {
                    None
                }
            })
            .unwrap_or(1);

        let port_text = if browser_count > 1 {
            "···".to_string()
        } else {
            e.port.to_string()
        };
        let port_col = if e.known_service.is_some() {
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
        let _ = write!(
            w,
            "{} {:<22}",
            a.fg(cat_col),
            truncate(&e.process_name, 22)
        );

        let service_text = match e.known_service {
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

        let _ = write!(w, "{} {:<7}", a.fg(dim), e.pid);
        let cpu_str = format!("{:.1}%", e.cpu_percent);
        let cpu_col = if e.cpu_percent > 50.0 {
            error
        } else if e.cpu_percent > 10.0 {
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
                truncate(&e.process_cmd, 90),
                a.reset()
            );
        }
    }

    // Summary footer
    if opts.show_summary {
        let tcp = entries
            .iter()
            .filter(|e| e.protocol == models::Protocol::Tcp)
            .count();
        let udp = total_count - tcp;
        let services = entries.iter().filter(|e| e.known_service.is_some()).count();
        let _ = write!(
            w,
            "\n{}{}{} ports{} ({} TCP, {} UDP)",
            a.fg(hdr),
            a.bold(),
            total_count,
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

/// Escape a string for JSON output (handles all required characters)
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                // Control characters
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

fn print_json(entries: &[models::PortEntry]) -> Result<()> {
    // Manual JSON to avoid serde dependency
    println!("[");
    for (i, e) in entries.iter().enumerate() {
        let comma = if i < entries.len() - 1 { "," } else { "" };
        println!(
            r#"  {{"port":{},"protocol":"{}","process":"{}","service":{},"pid":{},"cpu":{:.1},"memory_mb":{:.1},"uptime_secs":{}}}{}"#,
            e.port,
            e.protocol,
            json_escape(&e.process_name),
            e.known_service
                .map(|s| format!("\"{}\"", json_escape(s)))
                .unwrap_or_else(|| "null".to_string()),
            e.pid,
            e.cpu_percent,
            e.memory_mb,
            e.uptime.as_secs(),
            comma,
        );
    }
    println!("]");
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
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

    // --- JSON escaping tests ---

    #[test]
    fn json_escape_plain_string() {
        assert_eq!(json_escape("hello"), "hello");
    }

    #[test]
    fn json_escape_quotes() {
        assert_eq!(json_escape(r#"say "hi""#), r#"say \"hi\""#);
    }

    #[test]
    fn json_escape_backslash() {
        assert_eq!(json_escape(r"path\to\file"), r"path\\to\\file");
    }

    #[test]
    fn json_escape_newline_tab() {
        assert_eq!(json_escape("line1\nline2\tend"), "line1\\nline2\\tend");
    }

    #[test]
    fn json_escape_control_chars() {
        // Null byte and other control characters
        assert_eq!(json_escape("\x00\x01\x1f"), "\\u0000\\u0001\\u001f");
    }

    #[test]
    fn json_escape_carriage_return() {
        assert_eq!(json_escape("a\rb"), "a\\rb");
    }
}
