mod cli;
mod models;
mod scanner;
mod theme;
mod ui;
mod util;

use std::io::{self, Write};

use anyhow::Result;
use clap::Parser;
use crossterm::style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor};
use models::{format_memory, format_uptime};

use cli::{Cli, Command};
use models::ServiceCategory;
use scanner::{kill_process, scan_ports};
use theme::theme;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            // Default: launch TUI
            ui::run_tui()?;
        }

        Some(Command::List { json }) => {
            let entries = scan_ports(true, true)?;
            if json {
                print_json(&entries)?;
            } else {
                print_table(&entries);
            }
        }

        Some(Command::Check { port }) => {
            let entries = scan_ports(true, true)?;
            let matches: Vec<_> = entries.iter().filter(|e| e.port == port).collect();
            if matches.is_empty() {
                println!("Nothing listening on port {}", port);
                std::process::exit(1);
            } else {
                for entry in &matches {
                    println!(
                        "Port {} ({}) — {} (PID {}){}",
                        entry.port,
                        entry.protocol,
                        entry.process_name,
                        entry.pid,
                        entry
                            .known_service
                            .map(|s| format!(" [{}]", s))
                            .unwrap_or_default(),
                    );
                    if !entry.process_cmd.is_empty() {
                        println!("  Command: {}", entry.process_cmd);
                    }
                    println!(
                        "  CPU: {:.1}%  Memory: {}  Uptime: {}",
                        entry.cpu_percent,
                        entry.memory_display(),
                        entry.uptime_display()
                    );
                }
            }
        }

        Some(Command::Kill { port, force }) => {
            let entries = scan_ports(true, true)?;
            let matches: Vec<_> = entries.iter().filter(|e| e.port == port).collect();
            if matches.is_empty() {
                println!("Nothing listening on port {}", port);
            } else {
                for entry in &matches {
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

    Ok(())
}

/// Grouped browser entry for display
struct BrowserGroup {
    service: &'static str,
    category: ServiceCategory,
    protocol: models::Protocol,
    pid: u32,
    process_name: String,
    cpu_percent: f32,
    memory_mb: f64,
    uptime: std::time::Duration,
    count: usize,
}

/// Row to render — either a single entry or a grouped browser
enum DisplayRow<'a> {
    Single(&'a models::PortEntry),
    Grouped(BrowserGroup),
}

fn print_table(entries: &[models::PortEntry]) {
    if entries.is_empty() {
        println!("No listening ports found.");
        return;
    }

    // Build display rows: group browser entries by (pid, service)
    let mut rows: Vec<DisplayRow> = Vec::new();
    let mut browser_groups: std::collections::HashMap<
        (u32, &'static str),
        Vec<&models::PortEntry>,
    > = std::collections::HashMap::new();

    for e in entries {
        if e.category == ServiceCategory::Browser {
            if let Some(svc) = e.known_service {
                browser_groups.entry((e.pid, svc)).or_default().push(e);
                continue;
            }
        }
        rows.push(DisplayRow::Single(e));
    }

    // Convert browser groups to display rows, sorted by first entry's port
    let mut grouped: Vec<DisplayRow> = browser_groups
        .into_iter()
        .map(|((pid, svc), group)| {
            let first = group[0];
            DisplayRow::Grouped(BrowserGroup {
                service: svc,
                category: first.category,
                protocol: first.protocol,
                pid,
                process_name: first.process_name.clone(),
                cpu_percent: first.cpu_percent,
                memory_mb: first.memory_mb,
                uptime: first.uptime,
                count: group.len(),
            })
        })
        .collect();
    // Sort grouped rows by process name for consistent ordering
    grouped.sort_by(|a, b| {
        let a_name = match a {
            DisplayRow::Grouped(g) => &g.process_name,
            _ => unreachable!(),
        };
        let b_name = match b {
            DisplayRow::Grouped(g) => &g.process_name,
            _ => unreachable!(),
        };
        a_name.cmp(b_name)
    });
    rows.extend(grouped);

    let t = theme();
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
        "{}{}  {:<6} {:<5} {:<22} {:<20} {:<7} {:<7} {:<9} UPTIME{}{}",
        SetForegroundColor(hdr),
        SetAttribute(Attribute::Bold),
        "PORT",
        "PROTO",
        "PROCESS",
        "SERVICE",
        "PID",
        "CPU",
        "MEM",
        SetAttribute(Attribute::Reset),
        ResetColor,
    );
    let _ = writeln!(
        w,
        "{}{}{}",
        SetForegroundColor(divider),
        "─".repeat(100),
        ResetColor,
    );

    for row in &rows {
        match row {
            DisplayRow::Single(e) => {
                let cat_col = t.category_ct_color(e.category);
                let port_col = if e.known_service.is_some() {
                    Color::White
                } else {
                    light
                };

                let _ = write!(
                    w,
                    "{}{}  {:<6}{}",
                    SetForegroundColor(port_col),
                    SetAttribute(Attribute::Bold),
                    e.port,
                    SetAttribute(Attribute::Reset)
                );
                let _ = write!(
                    w,
                    "{} {:<5}",
                    SetForegroundColor(t.proto_ct_color(e.protocol)),
                    e.protocol
                );
                let _ = write!(
                    w,
                    "{} {:<22}",
                    SetForegroundColor(cat_col),
                    truncate(&e.process_name, 22)
                );

                let service = e.known_service.unwrap_or("");
                if service.is_empty() {
                    let _ = write!(w, "{} {:<20}", SetForegroundColor(divider), "·");
                } else {
                    let _ = write!(
                        w,
                        "{}{} {:<20}{}",
                        SetForegroundColor(cat_col),
                        SetAttribute(Attribute::Bold),
                        service,
                        SetAttribute(Attribute::Reset)
                    );
                }

                let _ = write!(w, "{} {:<7}", SetForegroundColor(dim), e.pid);
                let cpu_str = format!("{:.1}%", e.cpu_percent);
                let cpu_col = if e.cpu_percent > 50.0 {
                    error
                } else if e.cpu_percent > 10.0 {
                    warning
                } else {
                    dim
                };
                let _ = write!(w, "{} {:<7}", SetForegroundColor(cpu_col), cpu_str);
                let _ = write!(w, "{} {:<9}", SetForegroundColor(light), e.memory_display());
                let _ = write!(w, "{} {}", SetForegroundColor(dim), e.uptime_display());
                let _ = writeln!(w, "{}", ResetColor);
            }
            DisplayRow::Grouped(g) => {
                let cat_col = t.category_ct_color(g.category);
                let svc_label = format!("{} ×{}", g.service, g.count);

                let _ = write!(
                    w,
                    "{}{}  {:<6}{}",
                    SetForegroundColor(dim),
                    SetAttribute(Attribute::Bold),
                    "···",
                    SetAttribute(Attribute::Reset)
                );
                let _ = write!(
                    w,
                    "{} {:<5}",
                    SetForegroundColor(t.proto_ct_color(g.protocol)),
                    g.protocol
                );
                let _ = write!(
                    w,
                    "{} {:<22}",
                    SetForegroundColor(cat_col),
                    truncate(&g.process_name, 22)
                );
                let _ = write!(
                    w,
                    "{}{} {:<20}{}",
                    SetForegroundColor(cat_col),
                    SetAttribute(Attribute::Bold),
                    svc_label,
                    SetAttribute(Attribute::Reset)
                );
                let _ = write!(w, "{} {:<7}", SetForegroundColor(dim), g.pid);
                let _ = write!(
                    w,
                    "{} {:<7}",
                    SetForegroundColor(dim),
                    format!("{:.1}%", g.cpu_percent)
                );
                let _ = write!(
                    w,
                    "{} {:<9}",
                    SetForegroundColor(light),
                    format_memory(g.memory_mb)
                );
                let _ = write!(
                    w,
                    "{} {}",
                    SetForegroundColor(dim),
                    format_uptime(g.uptime)
                );
                let _ = writeln!(w, "{}", ResetColor);
            }
        }
    }

    // Summary
    let total = entries.len();
    let tcp = entries
        .iter()
        .filter(|e| e.protocol == models::Protocol::Tcp)
        .count();
    let udp = total - tcp;
    let services = entries.iter().filter(|e| e.known_service.is_some()).count();
    let _ = write!(
        w,
        "\n{}{}{} ports{} ({} TCP, {} UDP)",
        SetForegroundColor(hdr),
        SetAttribute(Attribute::Bold),
        total,
        SetAttribute(Attribute::Reset),
        tcp,
        udp,
    );
    if services > 0 {
        let _ = write!(w, " · {} known services", services);
    }
    let _ = writeln!(w, "{}", ResetColor);
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
            e.process_name.replace('"', "\\\""),
            e.known_service
                .map(|s| format!("\"{}\"", s))
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
}
