use std::collections::HashMap;
use std::io::{self, Write};

use anyhow::Result;
use crossterm::style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor};

use crate::models::{
    self, count_browser_groups, dedup_browser_indices, ServiceCategory, CPU_ERROR_THRESHOLD,
    CPU_WARN_THRESHOLD,
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
    browser_counts: Option<HashMap<(u32, &'static str), usize>>,
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
        let _ = write!(w, "{} {:<22}", a.fg(cat_col), truncate(&e.process_name, 22));

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
        let services = entries.iter().filter(|e| e.known_service.is_some()).count();
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

pub fn print_json(entries: &[models::PortEntry]) -> Result<()> {
    write_json(entries, &mut io::stdout())
}

fn write_json(entries: &[models::PortEntry], w: &mut impl Write) -> Result<()> {
    if entries.is_empty() {
        writeln!(w, "[]")?;
        return Ok(());
    }
    // Manual JSON to avoid serde dependency
    writeln!(w, "[")?;
    for (i, e) in entries.iter().enumerate() {
        let comma = if i < entries.len() - 1 { "," } else { "" };
        writeln!(
            w,
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
        )?;
    }
    writeln!(w, "]")?;
    Ok(())
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

    #[test]
    fn print_json_empty_entries() {
        // Should not panic — regression test for empty entry list
        let entries: Vec<models::PortEntry> = vec![];
        let result = print_json(&entries);
        assert!(result.is_ok());
    }

    // --- JSON round-trip / structural tests ---

    fn make_json_entry(
        port: u16,
        name: &str,
        service: Option<&'static str>,
        cpu: f32,
        memory_mb: f64,
        uptime_secs: u64,
    ) -> models::PortEntry {
        use std::net::{IpAddr, Ipv4Addr};
        models::PortEntry {
            protocol: models::Protocol::Tcp,
            local_addr: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port,
            pid: port as u32,
            process_name: name.to_string(),
            process_cmd: vec![name.to_string(), "--daemon".to_string()],
            cpu_percent: cpu,
            memory_mb,
            uptime: std::time::Duration::from_secs(uptime_secs),
            known_service: service,
            category: models::ServiceCategory::Unknown,
        }
    }

    #[test]
    fn print_json_empty_outputs_empty_array() {
        let mut buf = Vec::new();
        write_json(&[], &mut buf).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap().trim(), "[]");
    }

    #[test]
    fn print_json_single_entry_valid_structure() {
        let entries = vec![make_json_entry(
            3000,
            "node",
            Some("Next.js"),
            5.5,
            100.0,
            3600,
        )];
        let mut buf = Vec::new();
        write_json(&entries, &mut buf).unwrap();
        let json = String::from_utf8(buf).unwrap();

        // Outer structure
        assert!(json.trim_start().starts_with('['), "should start with [");
        assert!(json.trim_end().ends_with(']'), "should end with ]");

        // Required fields present
        assert!(json.contains("\"port\":3000"), "port field missing");
        assert!(
            json.contains("\"protocol\":\"TCP\""),
            "protocol field missing"
        );
        assert!(
            json.contains("\"process\":\"node\""),
            "process field missing"
        );
        assert!(
            json.contains("\"service\":\"Next.js\""),
            "service field missing"
        );
        assert!(json.contains("\"pid\":3000"), "pid field missing");
        assert!(json.contains("\"cpu\":5.5"), "cpu field missing");
        assert!(
            json.contains("\"memory_mb\":100.0"),
            "memory_mb field missing"
        );
        assert!(
            json.contains("\"uptime_secs\":3600"),
            "uptime_secs field missing"
        );
    }

    #[test]
    fn print_json_null_service_for_unknown() {
        let entries = vec![make_json_entry(9999, "custom", None, 0.0, 50.0, 60)];
        let mut buf = Vec::new();
        write_json(&entries, &mut buf).unwrap();
        let json = String::from_utf8(buf).unwrap();
        assert!(
            json.contains("\"service\":null"),
            "null service should appear as null"
        );
    }

    #[test]
    fn print_json_multiple_entries_no_trailing_comma() {
        let entries = vec![
            make_json_entry(3000, "node", Some("Next.js"), 0.0, 100.0, 0),
            make_json_entry(5432, "postgres", Some("PostgreSQL"), 0.0, 200.0, 0),
        ];
        let mut buf = Vec::new();
        write_json(&entries, &mut buf).unwrap();
        let json = String::from_utf8(buf).unwrap();

        // Last object line must not end with ","
        let last_obj_line = json
            .lines()
            .rfind(|l| l.trim_start().starts_with('{'))
            .unwrap();
        assert!(
            !last_obj_line.trim_end().ends_with(','),
            "last entry must not have trailing comma, got: {}",
            last_obj_line
        );
    }

    #[test]
    fn print_json_process_name_with_special_chars_is_escaped() {
        let mut entry = make_json_entry(8080, r#"my"app"#, None, 0.0, 0.0, 0);
        entry.process_name = r#"my"app\path"#.to_string();
        let mut buf = Vec::new();
        write_json(&[entry], &mut buf).unwrap();
        let json = String::from_utf8(buf).unwrap();
        // Quotes and backslash must be escaped
        assert!(
            json.contains(r#"my\"app\\path"#),
            "special chars must be escaped"
        );
    }
}
