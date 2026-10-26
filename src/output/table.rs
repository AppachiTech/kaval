//! Human-readable tables for `kav list` and `kav check`.

use std::collections::HashSet;
use std::io::{self, Write};

use ratatui::crossterm::style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor};

use super::sanitize;
use crate::models::{
    self, count_browser_groups, dedup_browser_indices, EntryKey, PortEntry, Protocol,
    CPU_ERROR_THRESHOLD, CPU_WARN_THRESHOLD,
};
use crate::scanner::Visibility;
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

/// How to render a listing.
pub struct TableOpts<'a> {
    pub color: bool,
    /// Terminal width; `None` (piped output) never drops or squeezes columns.
    pub width: Option<u16>,
    /// Rows to mark with `+` (new since the previous `--watch` frame).
    pub mark_new: &'a HashSet<EntryKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Col {
    Port,
    Proto,
    Bind,
    Process,
    Service,
    Project,
    Pid,
    Cpu,
    Mem,
    Uptime,
}

const COLUMNS: [Col; 10] = [
    Col::Port,
    Col::Proto,
    Col::Bind,
    Col::Process,
    Col::Service,
    Col::Project,
    Col::Pid,
    Col::Cpu,
    Col::Mem,
    Col::Uptime,
];
/// Dropped in this order when the terminal is too narrow.
const DROP_ORDER: [Col; 4] = [Col::Uptime, Col::Project, Col::Mem, Col::Proto];
/// Width of the new-row marker column.
const GUTTER: usize = 2;
const MIN_PROCESS: usize = 10;

impl Col {
    fn header(self) -> &'static str {
        match self {
            Col::Port => "PORT",
            Col::Proto => "PROTO",
            Col::Bind => "BIND",
            Col::Process => "PROCESS",
            Col::Service => "SERVICE",
            Col::Project => "PROJECT",
            Col::Pid => "PID",
            Col::Cpu => "CPU",
            Col::Mem => "MEM",
            Col::Uptime => "UPTIME",
        }
    }

    fn width(self) -> usize {
        match self {
            Col::Port => 6,
            Col::Proto => 5,
            Col::Bind => 12,
            Col::Process => 20,
            Col::Service => 18,
            Col::Project => 14,
            Col::Pid => 7,
            Col::Cpu => 6,
            Col::Mem => 8,
            Col::Uptime => 7,
        }
    }
}

/// Columns to show and how wide PROCESS may be for a terminal of `width`.
struct Layout {
    cols: Vec<Col>,
    process_width: usize,
}

impl Layout {
    fn for_width(width: Option<u16>) -> Self {
        let mut layout = Layout {
            cols: COLUMNS.to_vec(),
            process_width: Col::Process.width(),
        };
        let Some(max) = width.map(usize::from) else {
            return layout;
        };
        for col in DROP_ORDER {
            if layout.total() <= max {
                break;
            }
            layout.cols.retain(|c| *c != col);
        }
        while layout.total() > max && layout.process_width > MIN_PROCESS {
            layout.process_width -= 1;
        }
        layout
    }

    fn col_width(&self, col: Col) -> usize {
        if col == Col::Process {
            self.process_width
        } else {
            col.width()
        }
    }

    /// Characters per line (each column plus a separating space).
    fn total(&self) -> usize {
        GUTTER
            + self
                .cols
                .iter()
                .map(|c| self.col_width(*c) + 1)
                .sum::<usize>()
    }
}

/// Truncate to `width` characters and pad with spaces.
fn fit(text: &str, width: usize) -> String {
    format!("{:<width$}", truncate(text, width))
}

/// One table cell: text plus colour and weight.
struct Cell {
    text: String,
    color: Color,
    bold: bool,
}

fn cell(col: Col, e: &PortEntry, group: usize) -> Cell {
    let t = theme();
    let plain = |text: String, color| Cell {
        text,
        color,
        bold: false,
    };
    let category = t.category_ct_color(e.category);
    match col {
        Col::Port => Cell {
            text: if group > 1 {
                "···".to_string()
            } else {
                e.port.to_string()
            },
            color: if e.service.is_some() {
                t.ct(t.text)
            } else {
                t.ct(t.text_secondary)
            },
            bold: true,
        },
        Col::Proto => plain(e.protocol.to_string(), t.proto_ct_color(e.protocol)),
        Col::Bind => plain(
            e.bind_display(),
            if e.is_exposed() {
                t.ct(t.warning)
            } else {
                t.ct(t.text_muted)
            },
        ),
        Col::Process => plain(sanitize(&e.process_name).into_owned(), category),
        Col::Service => match &e.service {
            Some(s) => Cell {
                text: if group > 1 {
                    format!("{} ×{group}", sanitize(&s.name))
                } else {
                    sanitize(&e.service_display()).into_owned()
                },
                color: category,
                bold: s.source != models::ServiceSource::Port,
            },
            None => plain("·".to_string(), t.ct(t.border)),
        },
        Col::Project => match &e.project {
            Some(p) => plain(sanitize(p).into_owned(), t.ct(t.text_secondary)),
            None => plain("·".to_string(), t.ct(t.border)),
        },
        Col::Pid => plain(
            e.pid.map_or_else(|| "-".to_string(), |p| p.to_string()),
            t.ct(t.text_muted),
        ),
        Col::Cpu => plain(
            format!("{:.1}%", e.cpu_percent),
            if e.cpu_percent > CPU_ERROR_THRESHOLD {
                t.ct(t.error)
            } else if e.cpu_percent > CPU_WARN_THRESHOLD {
                t.ct(t.warning)
            } else {
                t.ct(t.text_muted)
            },
        ),
        Col::Mem => plain(e.memory_display(), t.ct(t.text_secondary)),
        Col::Uptime => plain(e.uptime_display(), t.ct(t.text_muted)),
    }
}

fn write_header(w: &mut impl Write, layout: &Layout, a: &Ansi) -> io::Result<()> {
    let t = theme();
    let mut line = " ".repeat(GUTTER);
    for col in &layout.cols {
        line.push_str(&fit(col.header(), layout.col_width(*col)));
        line.push(' ');
    }
    writeln!(
        w,
        "{}{}{}{}",
        a.fg(t.ct(t.text_secondary)),
        a.bold(),
        line.trim_end(),
        a.reset()
    )?;
    writeln!(
        w,
        "{}{}{}",
        a.fg(t.ct(t.border)),
        "─".repeat(layout.total() - 1),
        a.reset()
    )
}

fn write_row(
    w: &mut impl Write,
    e: &PortEntry,
    group: usize,
    layout: &Layout,
    a: &Ansi,
    is_new: bool,
) -> io::Result<()> {
    let t = theme();
    let mut line = String::new();
    if is_new {
        line.push_str(&format!("{}+{} ", a.fg(t.ct(t.success)), a.reset()));
    } else {
        line.push_str(&" ".repeat(GUTTER));
    }
    let last = layout.cols.len() - 1;
    for (i, col) in layout.cols.iter().enumerate() {
        let c = cell(*col, e, group);
        let text = if i == last {
            truncate(&c.text, layout.col_width(*col))
        } else {
            fit(&c.text, layout.col_width(*col))
        };
        let bold = if c.bold { a.bold() } else { String::new() };
        line.push_str(&format!("{}{}{}{} ", a.fg(c.color), bold, text, a.reset()));
    }
    writeln!(w, "{}", line.trim_end())
}

fn write_hint(w: &mut impl Write, visibility: Visibility, a: &Ansi) -> io::Result<()> {
    let t = theme();
    let hint = match visibility {
        Visibility::Complete => return Ok(()),
        Visibility::OtherUsersHidden => {
            "only your own processes are shown — run `sudo kav list` to include system services"
                .to_string()
        }
        Visibility::Unattributed(n) => format!(
            "{n} listener{} belong to other users — run with sudo to see who owns them",
            if n == 1 { "" } else { "s" }
        ),
    };
    writeln!(w, "{}Note: {hint}{}", a.fg(t.ct(t.text_muted)), a.reset())
}

/// Print the `kav list` table.
pub fn print_list(
    entries: &[PortEntry],
    visibility: Visibility,
    opts: &TableOpts<'_>,
    w: &mut impl Write,
) -> io::Result<()> {
    let a = Ansi(opts.color);
    let t = theme();
    if entries.is_empty() {
        writeln!(w, "No listening ports found.")?;
        return write_hint(w, visibility, &a);
    }
    let layout = Layout::for_width(opts.width);
    let groups = count_browser_groups(entries);
    write_header(w, &layout, &a)?;
    for i in dedup_browser_indices(entries, &groups) {
        let e = &entries[i];
        let group = e
            .browser_key()
            .and_then(|k| groups.get(&k).copied())
            .unwrap_or(1);
        write_row(w, e, group, &layout, &a, opts.mark_new.contains(&e.key()))?;
    }

    let tcp = entries
        .iter()
        .filter(|e| e.protocol == Protocol::Tcp)
        .count();
    let udp = entries.len() - tcp;
    let services = entries.iter().filter(|e| e.service.is_some()).count();
    let exposed = entries.iter().filter(|e| e.is_exposed()).count();
    let mut footer = format!(
        "{} ports{} ({tcp} TCP, {udp} UDP)",
        entries.len(),
        a.reset()
    );
    if services > 0 {
        footer.push_str(&format!(" · {services} known services"));
    }
    if exposed > 0 {
        footer.push_str(&format!(
            " · {}{exposed} exposed{}",
            a.fg(t.ct(t.warning)),
            a.reset()
        ));
    }
    writeln!(w, "\n{}{}{footer}", a.fg(t.ct(t.text_secondary)), a.bold())?;
    write_hint(w, visibility, &a)
}

/// Rows for `kav check`, each followed by labelled detail lines.
pub struct CheckTable<'a> {
    ansi: Ansi,
    layout: Layout,
    home: Option<&'a std::path::Path>,
    header_done: bool,
}

impl<'a> CheckTable<'a> {
    pub fn new(color: bool, home: Option<&'a std::path::Path>) -> Self {
        Self {
            ansi: Ansi(color),
            layout: Layout::for_width(None),
            home,
            header_done: false,
        }
    }

    /// Write one listener (the header first, once) and its details.
    pub fn entry(
        &mut self,
        w: &mut impl Write,
        e: &PortEntry,
        probe: Option<&str>,
    ) -> io::Result<()> {
        if !self.header_done {
            write_header(w, &self.layout, &self.ansi)?;
            self.header_done = true;
        }
        write_row(w, e, 1, &self.layout, &self.ansi, false)?;
        let mut details: Vec<(&str, String)> = Vec::new();
        if !e.process_cmd.is_empty() {
            details.push(("cmd", truncate(&sanitize(&e.cmd_display()), 120)));
        }
        if let Some(cwd) = &e.cwd {
            details.push((
                "dir",
                sanitize(&crate::project::display_path(cwd, self.home)).into_owned(),
            ));
        }
        if let Some(c) = &e.container {
            let compose = c
                .compose_project
                .as_deref()
                .map(|p| format!(", compose project {p}"))
                .unwrap_or_default();
            details.push((
                "container",
                sanitize(&format!("{} ({}{compose})", c.name, c.image)).into_owned(),
            ));
        }
        if let Some(p) = &e.parent {
            details.push(("parent", format!("{} (PID {})", sanitize(&p.name), p.pid)));
        }
        if e.protocol == Protocol::Tcp && !e.clients.is_empty() {
            let names: Vec<String> = e
                .clients
                .iter()
                .take(5)
                .map(|c| match (&c.process, c.pid) {
                    (Some(name), Some(pid)) => format!("{} (PID {pid})", sanitize(name)),
                    _ => c.addr.to_string(),
                })
                .collect();
            details.push((
                "clients",
                format!("{} — {}", e.clients.len(), names.join(", ")),
            ));
        }
        if let Some(note) = e.service.as_ref().and_then(|s| s.note) {
            details.push(("note", note.to_string()));
        }
        if let Some(probe) = probe {
            details.push(("probe", probe.to_string()));
        }
        let t = theme();
        for (label, value) in details {
            writeln!(
                w,
                "{}    {:<11}{value}{}",
                self.ansi.fg(t.ct(t.text_muted)),
                format!("{label}:"),
                self.ansi.reset()
            )?;
        }
        Ok(())
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

    use crate::models::testutil::{entry, with_service};
    use crate::models::{EntryKey, ServiceCategory, ServiceSource};
    use crate::scanner::Visibility;
    use std::collections::HashSet;
    use std::net::{IpAddr, Ipv4Addr};

    fn render(
        entries: &[models::PortEntry],
        visibility: Visibility,
        width: Option<u16>,
        new: &HashSet<EntryKey>,
    ) -> String {
        let mut buf = Vec::new();
        let opts = TableOpts {
            color: false,
            width,
            mark_new: new,
        };
        print_list(entries, visibility, &opts, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn sample() -> Vec<models::PortEntry> {
        let mut web = with_service(
            entry(3000, 11, "node"),
            "Next.js",
            ServiceCategory::DevServer,
            ServiceSource::Command,
        );
        web.addrs = vec![IpAddr::V4(Ipv4Addr::UNSPECIFIED)];
        web.project = Some("shop".into());
        let guess = with_service(
            entry(8000, 12, "Python"),
            "Django / FastAPI",
            ServiceCategory::DevServer,
            ServiceSource::Port,
        );
        vec![web, guess]
    }

    #[test]
    fn table_shows_bind_project_and_exposure() {
        let out = render(&sample(), Visibility::Complete, None, &HashSet::new());
        let header = out.lines().next().unwrap();
        for col in [
            "PORT", "PROTO", "BIND", "PROCESS", "SERVICE", "PROJECT", "PID", "CPU", "MEM", "UPTIME",
        ] {
            assert!(header.contains(col), "missing {col} in {header:?}");
        }
        let row = out.lines().find(|l| l.contains("3000")).unwrap();
        assert!(
            row.contains(" * "),
            "all-interfaces bind shown as *: {row:?}"
        );
        assert!(row.contains("shop"), "project shown: {row:?}");
        assert!(
            out.contains("1 exposed"),
            "footer counts exposed listeners:\n{out}"
        );
    }

    #[test]
    fn table_drops_columns_when_narrow() {
        let out = render(&sample(), Visibility::Complete, Some(80), &HashSet::new());
        let header = out.lines().next().unwrap();
        assert!(
            !header.contains("UPTIME") && !header.contains("PROJECT"),
            "{header:?}"
        );
        assert!(
            header.contains("PORT") && header.contains("PROCESS") && header.contains("SERVICE")
        );
        for line in out.lines() {
            assert!(line.chars().count() <= 80, "line wider than 80: {line:?}");
        }
    }

    #[test]
    fn table_marks_port_guesses() {
        let out = render(&sample(), Visibility::Complete, None, &HashSet::new());
        assert!(out.contains("Django / FastAPI?"), "{out}");
        assert!(
            out.contains("Next.js") && !out.contains("Next.js?"),
            "{out}"
        );
    }

    #[test]
    fn table_marks_new_rows() {
        let entries = sample();
        let new: HashSet<EntryKey> = [entries[1].key()].into_iter().collect();
        let out = render(&entries, Visibility::Complete, None, &new);
        let row = out.lines().find(|l| l.contains("8000")).unwrap();
        assert!(row.starts_with('+'), "new row marked: {row:?}");
        let old = out.lines().find(|l| l.contains("3000")).unwrap();
        assert!(!old.starts_with('+'));
    }

    #[test]
    fn table_sanitizes_process_names() {
        let mut e = entry(4000, 13, "evil\x1b[2J");
        e.process_cmd = vec!["x".into()];
        let out = render(&[e], Visibility::Complete, None, &HashSet::new());
        assert!(out.contains("evil?[2J"), "{out:?}");
        assert!(!out.contains('\x1b'), "no escape bytes without colour");
    }

    #[test]
    fn table_hints_when_owners_hidden() {
        let out = render(
            &sample(),
            Visibility::OtherUsersHidden,
            None,
            &HashSet::new(),
        );
        assert!(out.contains("sudo"), "{out}");
        let out = render(
            &sample(),
            Visibility::Unattributed(2),
            None,
            &HashSet::new(),
        );
        assert!(out.contains("2 listeners") && out.contains("sudo"), "{out}");
        let out = render(&sample(), Visibility::Complete, None, &HashSet::new());
        assert!(!out.contains("sudo"), "{out}");
    }

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
