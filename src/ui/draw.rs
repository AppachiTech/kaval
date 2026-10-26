//! Rendering for the TUI.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
    Frame,
};

use super::app::{App, STATUS_TTL};
use crate::killer::{Action, Signal};
use crate::models::{
    Exposure, PortEntry, Protocol, ServiceSource, CPU_ERROR_THRESHOLD, CPU_WARN_THRESHOLD,
};
use crate::output::sanitize;
use crate::probe::describe;
use crate::project::display_path;
use crate::scanner::Visibility;
use crate::theme::theme;

/// Below this width the detail pane goes under the table instead of beside it.
const SIDE_DETAIL_MIN_WIDTH: u16 = 110;
const SIDE_DETAIL_WIDTH: u16 = 44;
const BOTTOM_DETAIL_HEIGHT: u16 = 11;

/// PID for display; `-` when the owner is not visible.
fn pid_text(pid: Option<u32>) -> String {
    pid.map_or_else(|| "-".to_string(), |p| p.to_string())
}

/// A rectangle of at most `width`×`height`, centred in `area`.
fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header + filter
            Constraint::Min(5),    // table (+ detail)
            Constraint::Length(1), // status bar
        ])
        .split(f.area());

    draw_header(f, app, chunks[0]);

    let body = chunks[1];
    if app.show_detail && body.width >= SIDE_DETAIL_MIN_WIDTH {
        let parts = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(40), Constraint::Length(SIDE_DETAIL_WIDTH)])
            .split(body);
        draw_table(f, app, parts[0]);
        draw_detail(f, app, parts[1]);
    } else if app.show_detail && body.height > BOTTOM_DETAIL_HEIGHT + 4 {
        let parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(4), Constraint::Length(BOTTOM_DETAIL_HEIGHT)])
            .split(body);
        draw_table(f, app, parts[0]);
        draw_detail(f, app, parts[1]);
    } else {
        draw_table(f, app, body);
    }

    draw_status_bar(f, app, chunks[2]);

    if app.confirm.is_some() {
        draw_kill_confirm(f, app);
    }
    if app.yank_menu {
        draw_yank_menu(f);
    }
    if app.show_help {
        draw_help_overlay(f, app);
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let t = theme();

    let tcp_label = if app.show_tcp { "TCP ✓" } else { "TCP ✗" };
    let udp_label = if app.show_udp { "UDP ✓" } else { "UDP ✗" };
    let filter_indicator = if app.filter_active { "▌" } else { "" };
    let count = if app.filter_text.is_empty() {
        format!("  {} ports", app.entries.len())
    } else {
        format!("  {} of {} ports", app.matching, app.entries.len())
    };

    let header = Line::from(vec![
        Span::styled(
            "  Kaval",
            Style::default().fg(t.primary).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" — Guard your ports  ", Style::default().fg(t.text_muted)),
        Span::styled("Filter: ", Style::default().fg(t.text_secondary)),
        Span::styled(
            format!("{}{}", sanitize(&app.filter_text), filter_indicator),
            Style::default().fg(if app.filter_active { t.primary } else { t.text }),
        ),
        Span::styled("  ", Style::default()),
        Span::styled(
            format!("[{}]", tcp_label),
            Style::default().fg(if app.show_tcp {
                t.success
            } else {
                t.text_muted
            }),
        ),
        Span::styled(" ", Style::default()),
        Span::styled(
            format!("[{}]", udp_label),
            Style::default().fg(if app.show_udp {
                t.success
            } else {
                t.text_muted
            }),
        ),
        Span::styled(count, Style::default().fg(t.text_secondary)),
        Span::styled(
            format!(
                "  Sort: {}{}",
                app.sort_field.label(),
                if app.sort_reverse { " (reversed)" } else { "" }
            ),
            Style::default().fg(t.text_muted),
        ),
        Span::styled(
            if app.exposed_only {
                "  [exposed only]"
            } else {
                ""
            },
            Style::default().fg(t.warning),
        ),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.border));
    f.render_widget(Paragraph::new(header).block(block), area);
}

/// Table columns, most important first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Col {
    /// `*` marked for a multi-row kill, `+` appeared recently.
    Mark,
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

const COLUMNS: [Col; 11] = [
    Col::Mark,
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
/// Dropped in this order when the table is too narrow.
const DROP_ORDER: [Col; 6] = [
    Col::Uptime,
    Col::Project,
    Col::Mem,
    Col::Proto,
    Col::Cpu,
    Col::Bind,
];

impl Col {
    fn header(self) -> &'static str {
        match self {
            Col::Mark => "",
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

    /// Minimum width; PROCESS and SERVICE also share any spare space.
    fn width(self) -> u16 {
        match self {
            Col::Mark => 1,
            Col::Port => 6,
            Col::Proto => 5,
            Col::Bind => 10,
            Col::Process => 16,
            Col::Service => 14,
            Col::Project => 14,
            Col::Pid => 7,
            Col::Cpu => 6,
            Col::Mem => 8,
            Col::Uptime => 7,
        }
    }

    fn constraint(self) -> Constraint {
        match self {
            Col::Process | Col::Service => Constraint::Min(self.width()),
            _ => Constraint::Length(self.width()),
        }
    }
}

/// The columns that fit in `width` characters (one space between columns).
fn columns_for(width: u16) -> Vec<Col> {
    let total = |cols: &[Col]| cols.iter().map(|c| c.width() + 1).sum::<u16>();
    let mut cols = COLUMNS.to_vec();
    for drop in DROP_ORDER {
        if total(&cols) <= width {
            break;
        }
        cols.retain(|c| *c != drop);
    }
    cols
}

/// Per-row state that is not part of the entry itself.
#[derive(Clone, Copy)]
struct RowFlags {
    group: usize,
    marked: bool,
    new: bool,
}

fn cell(col: Col, e: &PortEntry, flags: RowFlags) -> Cell<'static> {
    let t = theme();
    let style = |c| Style::default().fg(c);
    let category = t.category_color(e.category);
    let group = flags.group;
    match col {
        Col::Mark if flags.marked => {
            Cell::from("*").style(Style::default().fg(t.warning).add_modifier(Modifier::BOLD))
        }
        Col::Mark if flags.new => Cell::from("+").style(style(t.success)),
        Col::Mark => Cell::from(" "),
        Col::Port => Cell::from(if group > 1 {
            "···".to_string()
        } else {
            e.port.to_string()
        })
        .style(style(t.text)),
        Col::Proto => Cell::from(e.protocol.to_string()).style(style(t.text_secondary)),
        Col::Bind => Cell::from(e.bind_display()).style(style(if e.is_exposed() {
            t.warning
        } else {
            t.text_muted
        })),
        Col::Process => Cell::from(sanitize(&e.process_name).into_owned()).style(style(category)),
        Col::Service => Cell::from(match e.service_name() {
            Some(svc) if group > 1 => format!("{} ×{group}", sanitize(svc)),
            Some(_) => sanitize(&e.service_display()).into_owned(),
            None => "—".to_string(),
        })
        .style(style(category)),
        Col::Project => Cell::from(
            e.project
                .as_deref()
                .map_or_else(|| "—".to_string(), |p| sanitize(p).into_owned()),
        )
        .style(style(t.text_secondary)),
        Col::Pid => Cell::from(pid_text(e.pid)).style(style(t.text_muted)),
        Col::Cpu => Cell::from(format!("{:.1}%", e.cpu_percent)).style(style(
            if e.cpu_percent > CPU_ERROR_THRESHOLD {
                t.error
            } else if e.cpu_percent > CPU_WARN_THRESHOLD {
                t.warning
            } else {
                t.text
            },
        )),
        Col::Mem => Cell::from(e.memory_display()).style(style(t.text)),
        Col::Uptime => Cell::from(e.uptime_display()).style(style(t.text_muted)),
    }
}

/// Why some listeners are missing or anonymous (other users' processes, without root).
fn hidden_owner_note(visibility: Visibility) -> Option<String> {
    match visibility {
        Visibility::Complete => None,
        Visibility::OtherUsersHidden => {
            Some(" only your processes are shown — run sudo kav to see all ".to_string())
        }
        Visibility::Unattributed(n) => Some(format!(
            " {n} listener{} with hidden owners — run sudo kav to see who ",
            if n == 1 { "" } else { "s" }
        )),
    }
}

fn draw_table(f: &mut Frame, app: &mut App, area: Rect) {
    let t = theme();
    app.table_area = area;
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.border));
    if let Some(note) = hidden_owner_note(app.visibility) {
        block = block.title_bottom(Line::from(Span::styled(
            note,
            Style::default().fg(t.text_muted),
        )));
    }

    // Show loading indicator on first scan
    if app.loading && app.filtered.is_empty() {
        let loading = Paragraph::new(Line::from(Span::styled(
            "Scanning ports…",
            Style::default()
                .fg(t.text_muted)
                .add_modifier(Modifier::ITALIC),
        )))
        .block(block)
        .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(loading, area);
        return;
    }

    let cols = columns_for(area.width.saturating_sub(2));
    let header = Row::new(cols.iter().map(|c| {
        Cell::from(c.header()).style(
            Style::default()
                .fg(t.text_secondary)
                .add_modifier(Modifier::BOLD),
        )
    }));

    let now = std::time::Instant::now();
    let rows: Vec<Row> = app
        .filtered
        .iter()
        .map(|&idx| {
            let e = &app.entries[idx];
            let flags = RowFlags {
                // For collapsed browser groups, how many ports this (pid, service) has
                group: e
                    .browser_key()
                    .filter(|key| !app.is_expanded(key))
                    .and_then(|key| app.browser_counts.get(&key).copied())
                    .unwrap_or(1),
                marked: app.marked.contains(&e.key()),
                new: app.is_new(&e.key(), now),
            };
            Row::new(cols.iter().map(|c| cell(*c, e, flags)))
        })
        .collect();

    let table = Table::new(rows, cols.iter().map(|c| c.constraint()))
        .header(header)
        .block(block)
        .row_highlight_style(if t.mono {
            Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default()
                .bg(t.selection_bg)
                .fg(t.selection_fg)
                .add_modifier(Modifier::BOLD)
        });

    f.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_detail(f: &mut Frame, app: &App, area: Rect) {
    let t = theme();
    let label = |s: &'static str| Span::styled(s, Style::default().fg(t.text_secondary));
    let value = |s: String| Span::styled(s, Style::default().fg(t.text));
    let muted = |s: String| Span::styled(s, Style::default().fg(t.text_muted));

    let mut lines: Vec<Line> = Vec::new();
    match app.selected_entry() {
        None => lines.push(Line::from(muted("No port selected".into()))),
        Some(e) => {
            let category = Style::default().fg(t.category_color(e.category));
            let exited = e.process_name == "<exited>";
            lines.push(Line::from(vec![
                label("Port: "),
                Span::styled(
                    format!("{} ({})", e.port, e.protocol),
                    Style::default().fg(t.text).add_modifier(Modifier::BOLD),
                ),
            ]));
            let (bind, warn) = match e.exposure() {
                Exposure::Loopback => ("localhost only".to_string(), false),
                Exposure::AllInterfaces => (
                    "all interfaces — reachable from other machines".to_string(),
                    true,
                ),
                Exposure::Network(ip) => (format!("{ip} — reachable on that network"), true),
            };
            lines.push(Line::from(vec![
                label("Bind: "),
                Span::styled(
                    bind,
                    Style::default().fg(if warn { t.warning } else { t.text }),
                ),
            ]));
            lines.push(Line::from(vec![
                label("Address: "),
                value(e.addr_display()),
            ]));
            lines.push(Line::from(vec![
                label("Process: "),
                Span::styled(
                    sanitize(&e.process_name).into_owned(),
                    if exited {
                        Style::default().fg(t.text_muted)
                    } else {
                        category
                    },
                ),
                muted(format!(" (PID {})", pid_text(e.pid))),
            ]));
            if let Some(user) = &e.user {
                lines.push(Line::from(vec![
                    label("User: "),
                    value(sanitize(user).into_owned()),
                ]));
            }
            let source = match e.service.as_ref().map(|s| s.source) {
                Some(ServiceSource::Port) => " (guessed from the port)",
                Some(ServiceSource::Command) => " (from the command line)",
                _ => "",
            };
            lines.push(Line::from(vec![
                label("Service: "),
                Span::styled(
                    sanitize(e.service_name().unwrap_or("Unknown")).into_owned(),
                    category,
                ),
                muted(source.into()),
            ]));
            if let Some(project) = &e.project {
                lines.push(Line::from(vec![
                    label("Project: "),
                    value(sanitize(project).into_owned()),
                ]));
            }
            if let Some(cwd) = &e.cwd {
                lines.push(Line::from(vec![
                    label("Directory: "),
                    muted(sanitize(&display_path(cwd, app.home.as_deref())).into_owned()),
                ]));
            }
            if let Some(parent) = &e.parent {
                lines.push(Line::from(vec![
                    label("Parent: "),
                    value(format!("{} (PID {})", sanitize(&parent.name), parent.pid)),
                ]));
            }
            if let Some(c) = &e.container {
                let compose = c
                    .compose_project
                    .as_deref()
                    .map(|p| format!(", compose {p}"))
                    .unwrap_or_default();
                lines.push(Line::from(vec![
                    label("Container: "),
                    value(sanitize(&format!("{} ({}{compose})", c.name, c.image)).into_owned()),
                ]));
            }
            if e.protocol == Protocol::Tcp {
                let clients = if e.clients.is_empty() {
                    "none".to_string()
                } else {
                    let names: Vec<String> = e
                        .clients
                        .iter()
                        .take(5)
                        .map(|c| match (&c.process, c.pid) {
                            (Some(name), Some(pid)) => format!("{} ({pid})", sanitize(name)),
                            _ => c.addr.to_string(),
                        })
                        .collect();
                    format!("{} — {}", e.clients.len(), names.join(", "))
                };
                lines.push(Line::from(vec![label("Connections: "), value(clients)]));
                lines.push(Line::from(match app.probes.get(&e.port) {
                    Some((health, at)) => vec![
                        label("Health: "),
                        value(describe(health)),
                        muted(format!(" ({}s ago)", at.elapsed().as_secs())),
                    ],
                    None => vec![label("Health: "), muted("press p to probe".into())],
                }));
            }
            if let Some(note) = e.service.as_ref().and_then(|s| s.note) {
                lines.push(Line::from(Span::styled(
                    note,
                    Style::default().fg(t.warning),
                )));
            }
            lines.push(Line::from(""));
            if exited {
                lines.push(Line::from(muted(
                    "The process exited during the scan. Refresh to clear.".into(),
                )));
            } else {
                lines.push(Line::from(vec![
                    label("CPU: "),
                    value(format!("{:.1}%", e.cpu_percent)),
                    label("   Memory: "),
                    value(e.memory_display()),
                    label("   Uptime: "),
                    value(e.uptime_display()),
                ]));
                lines.push(Line::from(""));
                lines.push(Line::from(label("Command:")));
                lines.push(Line::from(muted(sanitize(&e.cmd_display()).into_owned())));
            }
        }
    }

    let scroll_hint = if app.detail_scroll > 0 {
        " Detail ([ ] to scroll) ▲ "
    } else {
        " Detail ([ ] to scroll) "
    };
    let block = Block::default()
        .title(scroll_hint)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.border));
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));
    f.render_widget(paragraph, area);
}

/// The menu shown after `y`: what to copy.
fn draw_yank_menu(f: &mut Frame) {
    let t = theme();
    let rows = [
        ("p / y", "port"),
        ("i", "PID"),
        ("u", "URL"),
        ("c", "command line"),
        ("d", "working directory"),
    ];
    let mut lines = vec![Line::from("")];
    for (key, what) in rows {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {key:<7}"),
                Style::default().fg(t.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(what, Style::default().fg(t.text_muted)),
        ]));
    }
    let area = centered(f.area(), 34, lines.len() as u16 + 2);
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(" Copy… ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.primary));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let t = theme();

    if let Some(status) = app.status.as_ref().filter(|s| s.at.elapsed() < STATUS_TTL) {
        let color = if status.error { t.error } else { t.success };
        let line = Line::from(Span::styled(
            format!(" {}", sanitize(&status.text)),
            Style::default().fg(color),
        ));
        f.render_widget(Paragraph::new(line), area);
        return;
    }

    let key =
        |k: &'static str| Span::styled(k, Style::default().fg(t.text).add_modifier(Modifier::BOLD));
    let desc = |d: &'static str| Span::styled(d, Style::default().fg(t.text_muted));

    let line = if app.filter_active {
        Line::from(vec![
            Span::styled(
                " Filter mode",
                Style::default().fg(t.primary).add_modifier(Modifier::BOLD),
            ),
            desc(" — type to search  "),
            key("↑↓"),
            desc(" move  "),
            key("Enter"),
            desc(" keep  "),
            key("Esc"),
            desc(" clear"),
        ])
    } else {
        Line::from(vec![
            key(" /"),
            desc(" Filter  "),
            key("^X"),
            desc(" Kill  "),
            key("^K"),
            desc(" Force  "),
            key("^D"),
            desc(" Detail  "),
            key("^S"),
            desc(" Sort  "),
            key("^T"),
            desc(" Proto  "),
            key("^R"),
            desc(" Refresh  "),
            key("q"),
            desc(" Quit  "),
            key("?"),
            desc(" Help"),
        ])
    };
    f.render_widget(Paragraph::new(line), area);
}

const HELP_ROWS: [(&str, &str); 20] = [
    ("↑/↓  j/k", "Move selection (mouse wheel too)"),
    ("PgUp / PgDn", "Page the list"),
    ("Home / End", "Jump to top / bottom"),
    ("/", "Filter: port, PID, name, service, command…"),
    ("Esc", "Clear the filter, then the marks"),
    ("Enter", "Expand / collapse a browser group"),
    ("Space", "Mark rows for a multi-row kill"),
    ("Ctrl+X", "Kill selected/marked (SIGTERM, confirm)"),
    ("Ctrl+K", "Force kill (SIGKILL, confirm)"),
    ("e", "Only listeners exposed to the network"),
    ("d / Ctrl+D", "Toggle detail pane"),
    ("[ / ]  ← / →", "Scroll detail pane"),
    ("s / Ctrl+S", "Cycle sort: Port → Name → CPU → Mem"),
    ("S", "Reverse the sort"),
    ("t / Ctrl+T", "Cycle protocols: TCP+UDP → TCP → UDP"),
    ("r / Ctrl+R", "Refresh now"),
    ("y  o  p", "Copy… (port/PID/URL/cmd/dir) · Open · Probe"),
    ("Ctrl+Z", "Suspend"),
    ("q / Ctrl+Q", "Quit"),
    ("?", "Show / dismiss this help"),
];

fn draw_help_overlay(f: &mut Frame, app: &mut App) {
    let t = theme();
    let mut lines = Vec::new();
    for (key, desc) in HELP_ROWS {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {key:<15}"),
                Style::default().fg(t.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(desc, Style::default().fg(t.text_muted)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Press any key to dismiss (↑/↓ scroll)",
        Style::default().fg(t.text_muted),
    )));

    let content_height = lines.len() as u16;
    let area = centered(f.area(), 64, content_height + 2);
    let inner_height = area.height.saturating_sub(2);
    let max_scroll = content_height.saturating_sub(inner_height);
    app.help_scroll = app.help_scroll.min(max_scroll);
    let title = if app.help_scroll < max_scroll {
        " Keyboard Shortcuts ↓ "
    } else {
        " Keyboard Shortcuts "
    };

    f.render_widget(Clear, area);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.primary));
    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .scroll((app.help_scroll, 0)),
        area,
    );
}

fn draw_kill_confirm(f: &mut Frame, app: &App) {
    let Some(confirm) = &app.confirm else { return };
    let Some(target) = confirm.targets.first() else {
        return;
    };
    let t = theme();

    let ports: Vec<String> = target.ports.iter().map(u16::to_string).collect();
    let (title, verb, how) = match &target.action {
        Action::DockerStop(c) => (
            " Confirm docker stop ",
            "Stop container",
            format!("docker stop {}", sanitize(&c.name)),
        ),
        _ if confirm.signal == Signal::Kill => (
            " Confirm Force Kill ",
            "Force kill",
            format!("signal {}", confirm.signal),
        ),
        _ => (
            " Confirm Kill ",
            "Kill",
            format!("signal {}", confirm.signal),
        ),
    };
    let bold =
        |s: String| Span::styled(s, Style::default().fg(t.text).add_modifier(Modifier::BOLD));
    let muted = |s: String| Span::styled(s, Style::default().fg(t.text_secondary));

    let mut text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("  {verb} "),
                Style::default().fg(t.error).add_modifier(Modifier::BOLD),
            ),
            bold(sanitize(&target.name).into_owned()),
        ]),
        Line::from(muted(format!(
            "  PID {} · port {} · {how}",
            pid_text(target.pid),
            ports.join(", ")
        ))),
    ];
    if confirm.targets.len() > 1 {
        text.push(Line::from(muted(format!(
            "  and {} more marked row(s)",
            confirm.targets.len() - 1
        ))));
    }
    if let Some(warning) = target.warning {
        text.push(Line::from(Span::styled(
            format!("  {warning}"),
            Style::default().fg(t.warning),
        )));
    }
    let parent = confirm
        .parent_targets
        .as_ref()
        .and_then(|p| p.first())
        .and_then(|p| p.via_parent.as_ref());
    text.push(Line::from(""));
    text.push(Line::from(Span::styled(
        match parent {
            Some(p) => format!(
                "  y confirm · p stop the parent {} (PID {}) instead · other keys cancel",
                sanitize(&p.name),
                p.pid
            ),
            None => "  y confirm · any other key cancels".to_string(),
        },
        Style::default().fg(t.text_muted),
    )));

    let height = 7
        + u16::from(target.warning.is_some()) * 3
        + u16::from(confirm.targets.len() > 1)
        + u16::from(parent.is_some());
    let area = centered(f.area(), 64, height);
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.error));
    f.render_widget(
        Paragraph::new(text).block(block).wrap(Wrap { trim: false }),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::testutil::{entry, with_service};
    use crate::models::{PortEntry, ServiceCategory, ServiceSource};
    use crate::scanner::{Snapshot, Visibility};
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;
    use std::time::Instant;

    fn render(app: &mut App, w: u16, h: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    fn text(lines: &[String]) -> String {
        lines.join("\n")
    }

    fn row_of(lines: &[String], needle: &str) -> Option<usize> {
        lines.iter().position(|l| l.contains(needle))
    }

    fn browser(port: u16) -> PortEntry {
        with_service(
            entry(port, 100, "Google Chrome Helper"),
            "Chrome",
            ServiceCategory::Browser,
            ServiceSource::Process,
        )
    }

    fn app_with(entries: Vec<PortEntry>) -> App {
        let mut app = App::new();
        app.apply_snapshot(
            Snapshot {
                entries,
                visibility: Visibility::Complete,
            },
            Instant::now(),
        );
        app
    }

    fn seven() -> Vec<PortEntry> {
        vec![
            browser(5001),
            browser(5002),
            browser(5003),
            entry(3000, 11, "node"),
            entry(5432, 22, "postgres"),
            entry(6379, 33, "redis-server"),
            entry(8080, 44, "caddy"),
        ]
    }

    #[test]
    fn hidden_owners_are_explained() {
        let mut app = app_with(seven());
        assert!(!text(&render(&mut app, 100, 20)).contains("sudo"));
        for (visibility, needle) in [
            (Visibility::OtherUsersHidden, "only your processes"),
            (Visibility::Unattributed(2), "2 listeners"),
        ] {
            app.apply_snapshot(
                Snapshot {
                    entries: seven(),
                    visibility,
                },
                Instant::now(),
            );
            let out = text(&render(&mut app, 100, 20));
            assert!(out.contains(needle) && out.contains("sudo"), "{out}");
        }
    }

    #[test]
    fn gutter_marks_new_and_marked_rows() {
        let mut app = app_with(vec![entry(3000, 11, "node"), entry(5432, 22, "postgres")]);
        app.show_detail = false;
        app.apply_snapshot(
            Snapshot {
                entries: vec![
                    entry(3000, 11, "node"),
                    entry(5432, 22, "postgres"),
                    entry(7777, 33, "newcomer"),
                ],
                visibility: Visibility::Complete,
            },
            Instant::now(),
        );
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)); // mark 3000
        let out = render(&mut app, 120, 10);
        let row = |port: &str| out.iter().find(|l| l.contains(port)).unwrap().clone();
        assert!(row("3000").starts_with("│*"), "{}", row("3000"));
        assert!(row("7777").starts_with("│+"), "{}", row("7777"));
        assert!(row("5432").starts_with("│ "), "{}", row("5432"));
    }

    #[test]
    fn header_shows_exposed_only_and_reversed_sort() {
        let mut app = app_with(seven());
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT));
        let out = render(&mut app, 160, 12);
        assert!(out[1].contains("exposed only"), "{}", out[1]);
        assert!(out[1].contains("reversed"), "{}", out[1]);
    }

    #[test]
    fn detail_shows_project_parent_container_clients() {
        let mut e = entry(6333, 21743, "com.docker.backend");
        e.addrs = vec![std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)];
        e.project = Some("shop".into());
        e.user = Some("madhu".into());
        e.parent = Some(crate::models::ProcessRef {
            pid: 1,
            name: "launchd".into(),
            start_time: 0,
        });
        e.container = Some(crate::models::ContainerInfo {
            id: "517d48f91bc3".into(),
            name: "roo-qdrant".into(),
            image: "qdrant/qdrant".into(),
            compose_project: None,
        });
        e.clients = vec![crate::models::Client {
            addr: "127.0.0.1:60000".parse().unwrap(),
            pid: Some(4521),
            process: Some("psql".into()),
        }];
        let mut app = app_with(vec![e]);
        let out = text(&render(&mut app, 150, 40));
        for needle in [
            "shop",
            "madhu",
            "launchd",
            "roo-qdrant",
            "qdrant/qdrant",
            "psql",
            "reachable",
        ] {
            assert!(out.contains(needle), "detail lacks {needle}:\n{out}");
        }
    }

    #[test]
    fn kill_dialog_shows_parent_and_container_actions() {
        let mut node = entry(3000, 200, "node");
        node.parent = Some(crate::models::ProcessRef {
            pid: 100,
            name: "nodemon".into(),
            start_time: 0,
        });
        let mut app = app_with(vec![node]);
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
        let out = text(&render(&mut app, 100, 20));
        assert!(out.contains("nodemon") && out.contains("parent"), "{out}");

        let mut proxy = entry(6333, 21743, "com.docker.backend");
        proxy.container = Some(crate::models::ContainerInfo {
            id: "517d48f91bc3".into(),
            name: "roo-qdrant".into(),
            image: "qdrant/qdrant".into(),
            compose_project: None,
        });
        let mut app = app_with(vec![proxy]);
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
        let out = text(&render(&mut app, 100, 20));
        assert!(out.contains("docker stop roo-qdrant"), "{out}");
    }

    #[test]
    fn loading_frame_drawn_before_first_scan() {
        let out = text(&render(&mut App::new(), 100, 20));
        assert!(out.contains("Scanning ports"), "{out}");
    }

    #[test]
    fn header_counts_ports_not_rows() {
        let mut app = app_with(seven());
        let out = render(&mut app, 140, 20);
        assert!(out[1].contains("7 ports"), "{}", out[1]);
        app.filter_text = "chrome".into();
        app.apply_filter();
        let out = render(&mut app, 140, 20);
        assert!(out[1].contains("3 of 7 ports"), "{}", out[1]);
    }

    #[test]
    fn browser_filter_shows_actual_port() {
        let mut app = app_with(seven());
        app.show_detail = false;
        app.filter_text = "5002".into();
        app.apply_filter();
        let out = text(&render(&mut app, 120, 12));
        assert!(out.contains("5002"), "{out}");
        assert!(
            !out.contains("···"),
            "a lone match is not shown as a group:\n{out}"
        );
    }

    #[test]
    fn kill_dialog_shows_pid_and_port_for_long_names() {
        let mut app = app_with(vec![entry(62662, 62662, "Code - Insiders Helper (Plugin)")]);
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
        assert!(app.confirm.is_some());
        let out = text(&render(&mut app, 100, 14));
        assert!(out.contains("PID 62662"), "{out}");
        assert!(out.contains("port 62662"), "{out}");
        assert!(out.contains("SIGTERM"), "{out}");
    }

    #[test]
    fn help_overlay_fits_80x24() {
        let mut app = app_with(seven());
        app.show_help = true;
        let out = text(&render(&mut app, 80, 24));
        assert!(out.contains("Press any key"), "{out}");
        assert!(
            out.contains("TCP+UDP → TCP → UDP"),
            "long descriptions are not cut:\n{out}"
        );
    }

    #[test]
    fn help_overlay_scrolls_40x10_without_panic() {
        let mut app = app_with(seven());
        app.show_help = true;
        let first = text(&render(&mut app, 40, 10));
        assert!(first.contains("Keyboard"), "{first}");
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert!(app.show_help, "arrows scroll help instead of closing it");
        let scrolled = text(&render(&mut app, 40, 10));
        assert_ne!(first, scrolled, "help content scrolled");
    }

    #[test]
    fn narrow_layout_moves_detail_below() {
        let mut app = app_with(seven());
        let narrow = render(&mut app, 90, 30);
        let header = row_of(&narrow, "PORT").unwrap();
        let detail = row_of(&narrow, " Detail").unwrap();
        assert!(detail > header + 2, "detail below the table at 90 columns");
        let wide = render(&mut app, 140, 30);
        let header = row_of(&wide, "PORT").unwrap();
        let detail = row_of(&wide, " Detail").unwrap();
        assert!(detail < header, "detail beside the table at 140 columns");
    }

    #[test]
    fn columns_hide_by_priority() {
        let mut app = app_with(seven());
        app.show_detail = false;
        let narrow = render(&mut app, 80, 12);
        let header = &narrow[row_of(&narrow, "PORT").unwrap()];
        for col in ["PORT", "BIND", "PROCESS", "SERVICE", "PID"] {
            assert!(header.contains(col), "{col} kept at 80 columns: {header}");
        }
        assert!(
            !header.contains("UPTIME") && !header.contains("PROJECT"),
            "{header}"
        );
        let wide = render(&mut app, 170, 12);
        let header = &wide[row_of(&wide, "PORT").unwrap()];
        for col in ["UPTIME", "PROJECT", "MEM", "CPU", "PROTO"] {
            assert!(header.contains(col), "{col} shown at 170 columns: {header}");
        }
    }
}
