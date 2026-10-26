//! Rendering for the TUI.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
    Frame,
};

use super::app::{App, STATUS_TTL};
use crate::killer::Action;
use crate::models::{CPU_ERROR_THRESHOLD, CPU_WARN_THRESHOLD};
use crate::output::sanitize;
use crate::theme::theme;

// Table column widths (characters). Changing these affects both the header and rows.
// COL_PROCESS is 20 to fit "Google Chrome Helper" (20 chars) without truncation.
const COL_PORT: u16 = 7;
const COL_PROTO: u16 = 6;
const COL_PROCESS: u16 = 20;
const COL_SERVICE: u16 = 16;
const COL_PID: u16 = 7;
const COL_CPU: u16 = 7;
const COL_MEM: u16 = 9;
const COL_UPTIME: u16 = 8;

/// PID for display; `-` when the owner is not visible.
fn pid_text(pid: Option<u32>) -> String {
    pid.map_or_else(|| "-".to_string(), |p| p.to_string())
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header + filter
            Constraint::Min(5),    // table
            Constraint::Length(1), // status bar
        ])
        .split(f.area());

    draw_header(f, app, chunks[0]);

    if app.show_detail {
        let detail_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(chunks[1]);
        draw_table(f, app, detail_layout[0]);
        draw_detail(f, app, detail_layout[1]);
    } else {
        draw_table(f, app, chunks[1]);
    }

    draw_status_bar(f, app, chunks[2]);

    if app.confirm.is_some() {
        draw_kill_confirm(f, app);
    }
    if app.show_help {
        draw_help_overlay(f);
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let t = theme();

    let tcp_label = if app.show_tcp { "TCP ✓" } else { "TCP ✗" };
    let udp_label = if app.show_udp { "UDP ✓" } else { "UDP ✗" };
    let filter_indicator = if app.filter_active { "▌" } else { "" };

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
        Span::styled(
            format!("  {} ports", app.filtered.len()),
            Style::default().fg(t.text_secondary),
        ),
        Span::styled(
            format!("  Sort: {}", app.sort_field.label()),
            Style::default().fg(t.text_muted),
        ),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.border));
    f.render_widget(Paragraph::new(header).block(block), area);
}

fn draw_table(f: &mut Frame, app: &mut App, area: Rect) {
    let t = theme();
    app.table_area = area;

    // Show loading indicator on first scan
    if app.loading && app.filtered.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(t.border));
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

    let header_cells = [
        "PORT", "PROTO", "PROCESS", "SERVICE", "PID", "CPU", "MEM", "UPTIME",
    ]
    .iter()
    .map(|h| {
        Cell::from(*h).style(
            Style::default()
                .fg(t.text_secondary)
                .add_modifier(Modifier::BOLD),
        )
    });
    let header = Row::new(header_cells).height(1);

    let rows: Vec<Row> = app
        .filtered
        .iter()
        .map(|&idx| {
            let e = &app.entries[idx];
            let cat_color = t.category_color(e.category);
            let cpu_color = if e.cpu_percent > CPU_ERROR_THRESHOLD {
                t.error
            } else if e.cpu_percent > CPU_WARN_THRESHOLD {
                t.warning
            } else {
                t.text
            };

            // For grouped browser entries, look up how many ports this (pid, service) has
            let browser_count = e
                .browser_key()
                .and_then(|key| app.browser_counts.get(&key).copied())
                .unwrap_or(1);

            let service_text = match e.service_name() {
                Some(svc) if browser_count > 1 => format!("{} ×{}", sanitize(svc), browser_count),
                Some(_) => sanitize(&e.service_display()).into_owned(),
                None => "—".to_string(),
            };

            let port_text = if browser_count > 1 {
                "···".to_string()
            } else {
                e.port.to_string()
            };

            Row::new(vec![
                Cell::from(port_text).style(Style::default().fg(t.text)),
                Cell::from(e.protocol.to_string()).style(Style::default().fg(t.text_secondary)),
                Cell::from(sanitize(&e.process_name).into_owned())
                    .style(Style::default().fg(cat_color)),
                Cell::from(service_text).style(Style::default().fg(cat_color)),
                Cell::from(pid_text(e.pid)).style(Style::default().fg(t.text_muted)),
                Cell::from(format!("{:.1}%", e.cpu_percent)).style(Style::default().fg(cpu_color)),
                Cell::from(e.memory_display()).style(Style::default().fg(t.text)),
                Cell::from(e.uptime_display()).style(Style::default().fg(t.text_muted)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(COL_PORT),
        Constraint::Length(COL_PROTO),
        Constraint::Length(COL_PROCESS),
        Constraint::Length(COL_SERVICE),
        Constraint::Length(COL_PID),
        Constraint::Length(COL_CPU),
        Constraint::Length(COL_MEM),
        Constraint::Length(COL_UPTIME),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(t.border)),
        )
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

    let content = if let Some(entry) = app.selected_entry() {
        let exited = entry.process_name == "<exited>";
        let mut lines = vec![
            Line::from(vec![
                label("Port: "),
                Span::styled(
                    format!("{} ({})", entry.port, entry.protocol),
                    Style::default().fg(t.text).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                label("Address: "),
                Span::styled(entry.addr_display(), Style::default().fg(t.text)),
            ]),
            Line::from(vec![
                label("Process: "),
                Span::styled(
                    sanitize(&entry.process_name).into_owned(),
                    Style::default().fg(if exited {
                        t.text_muted
                    } else {
                        t.category_color(entry.category)
                    }),
                ),
            ]),
            Line::from(vec![
                label("PID: "),
                Span::styled(pid_text(entry.pid), Style::default().fg(t.text)),
            ]),
            Line::from(vec![
                label("Service: "),
                Span::styled(
                    sanitize(entry.service_name().unwrap_or("Unknown")).into_owned(),
                    Style::default().fg(t.category_color(entry.category)),
                ),
            ]),
            Line::from(""),
        ];

        if exited {
            lines.push(Line::from(Span::styled(
                "Process exited during scan. The socket was still",
                Style::default().fg(t.text_muted),
            )));
            lines.push(Line::from(Span::styled(
                "open when ports were enumerated. Refresh to clear.",
                Style::default().fg(t.text_muted),
            )));
        } else {
            lines.extend([
                Line::from(vec![
                    label("CPU: "),
                    Span::styled(
                        format!("{:.1}%", entry.cpu_percent),
                        Style::default().fg(t.text),
                    ),
                ]),
                Line::from(vec![
                    label("Memory: "),
                    Span::styled(entry.memory_display(), Style::default().fg(t.text)),
                ]),
                Line::from(vec![
                    label("Uptime: "),
                    Span::styled(entry.uptime_display(), Style::default().fg(t.text)),
                ]),
                Line::from(""),
                Line::from(label("Command:")),
                Line::from(Span::styled(
                    sanitize(&entry.cmd_display()).into_owned(),
                    Style::default().fg(t.text_muted),
                )),
            ]);
        }
        lines
    } else {
        vec![Line::from(Span::styled(
            "No port selected",
            Style::default().fg(t.text_muted),
        ))]
    };

    let scroll_hint = if app.detail_scroll > 0 {
        " Detail (PgUp/PgDn to scroll) ▲ "
    } else {
        " Detail (PgUp/PgDn to scroll) "
    };
    let block = Block::default()
        .title(scroll_hint)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.border));
    let paragraph = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));
    f.render_widget(paragraph, area);
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
            key("ESC"),
            desc(" cancel  "),
            key("Enter"),
            desc(" confirm"),
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

fn draw_help_overlay(f: &mut Frame) {
    let t = theme();
    let area = f.area();

    let rows = [
        ("↑/↓  j/k", "Navigate list"),
        ("Home / End", "Jump to top / bottom"),
        ("/", "Enter filter mode"),
        ("Ctrl+X", "Kill selected (SIGTERM, confirm)"),
        ("Ctrl+K", "Force kill (SIGKILL, confirm)"),
        ("Ctrl+D", "Toggle detail pane"),
        ("Ctrl+S", "Cycle sort: Port → Name → CPU → Mem"),
        ("Ctrl+T", "Cycle protocol filter: TCP+UDP → TCP → UDP"),
        ("Ctrl+R", "Force refresh"),
        ("y", "Yank port number to clipboard"),
        ("o", "Open port in browser"),
        ("PgUp/PgDn ←/→", "Scroll detail pane"),
        ("Ctrl+Z", "Suspend"),
        ("q / Ctrl+Q", "Quit"),
        ("?", "Show / dismiss this help"),
    ];

    let dialog_width = 58u16.min(area.width.saturating_sub(4));
    let dialog_height = rows.len() as u16 + 4;
    let x = (area.width.saturating_sub(dialog_width)) / 2;
    let y = (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    let mut lines = vec![Line::from(""), Line::from("")];
    for (key, desc) in &rows {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<18}", key),
                Style::default().fg(t.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(*desc, Style::default().fg(t.text_muted)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Press any key to dismiss",
        Style::default().fg(t.text_muted),
    )));

    let block = Block::default()
        .title(" Keyboard Shortcuts ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.primary));
    f.render_widget(Paragraph::new(lines).block(block), dialog_area);
}

fn draw_kill_confirm(f: &mut Frame, app: &App) {
    let Some(confirm) = &app.confirm else { return };
    let Some(target) = confirm.targets.first() else {
        return;
    };
    let t = theme();
    let area = f.area();

    let ports: Vec<String> = target.ports.iter().map(u16::to_string).collect();
    let (title, action) = match &target.action {
        Action::DockerStop(_) => (" Confirm docker stop ", "  Stop container "),
        _ if confirm.signal == crate::killer::Signal::Kill => {
            (" Confirm Force Kill (SIGKILL) ", "  Force kill ")
        }
        _ => (" Confirm Kill ", "  Kill "),
    };

    let dialog_width = 55u16.min(area.width.saturating_sub(4));
    let dialog_height = 5u16;
    let x = (area.width.saturating_sub(dialog_width)) / 2;
    let y = (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    let text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                action,
                Style::default().fg(t.error).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                sanitize(&target.name).into_owned(),
                Style::default().fg(t.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    " (PID {}) on port {}?",
                    pid_text(target.pid),
                    ports.join(", ")
                ),
                Style::default().fg(t.text_secondary),
            ),
        ]),
        Line::from(Span::styled(
            "  y = confirm, any other key = cancel",
            Style::default().fg(t.text_muted),
        )),
    ];

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.error));
    f.render_widget(Paragraph::new(text).block(block), dialog_area);
}
