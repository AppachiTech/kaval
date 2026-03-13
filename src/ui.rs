use std::io;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap},
    Frame, Terminal,
};

use std::collections::{HashMap, HashSet};

use sysinfo::System;

use crate::models::{PortEntry, ServiceCategory, SortField};
use crate::scanner::{kill_process, scan_ports};
use crate::theme::theme;

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

struct App {
    sys: System,
    entries: Vec<PortEntry>,
    filtered: Vec<usize>,                   // indices into entries
    browser_counts: HashMap<(u32, String), usize>, // (pid, service) -> port count
    table_state: TableState,
    filter_text: String,
    filter_active: bool,
    show_tcp: bool,
    show_udp: bool,
    sort_field: SortField,
    show_detail: bool,
    detail_scroll: u16,
    confirm_kill: Option<(usize, bool)>, // (index, force) of entry to confirm kill
    status_msg: Option<(String, Instant)>,
    should_quit: bool,
}

impl App {
    fn new() -> Self {
        let mut sys = System::new();
        // First refresh establishes baseline for CPU calculation
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        Self {
            sys,
            entries: Vec::new(),
            filtered: Vec::new(),
            browser_counts: HashMap::new(),
            table_state: TableState::default(),
            filter_text: String::new(),
            filter_active: false,
            show_tcp: true,
            show_udp: true,
            sort_field: SortField::Port,
            show_detail: true,
            detail_scroll: 0,
            confirm_kill: None, // (index, force)
            status_msg: None,
            should_quit: false,
        }
    }

    fn refresh(&mut self) {
        match scan_ports(&mut self.sys, self.show_tcp, self.show_udp) {
            Ok(entries) => {
                self.entries = entries;
                self.sort_entries();
                self.apply_filter();
            }
            Err(e) => {
                self.status_msg = Some((format!("Scan error: {}", e), Instant::now()));
            }
        }
    }

    fn sort_entries(&mut self) {
        match self.sort_field {
            SortField::Port => self.entries.sort_by_key(|e| e.port),
            SortField::ProcessName => self.entries.sort_by(|a, b| {
                a.process_name
                    .to_lowercase()
                    .cmp(&b.process_name.to_lowercase())
            }),
            SortField::Cpu => self.entries.sort_by(|a, b| {
                b.cpu_percent
                    .partial_cmp(&a.cpu_percent)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            SortField::Memory => self.entries.sort_by(|a, b| {
                b.memory_mb
                    .partial_cmp(&a.memory_mb)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
        }
    }

    fn apply_filter(&mut self) {
        let query = self.filter_text.to_lowercase();

        // Count browser entries per (pid, service) for grouping
        self.browser_counts.clear();
        for e in &self.entries {
            if e.category == ServiceCategory::Browser {
                if let Some(svc) = e.known_service {
                    *self
                        .browser_counts
                        .entry((e.pid, svc.to_string()))
                        .or_insert(0) += 1;
                }
            }
        }

        // Track which browser (pid, service) groups we've already included
        let mut browser_seen: HashSet<(u32, String)> = HashSet::new();

        self.filtered = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                if !query.is_empty()
                    && !e.port.to_string().contains(&query)
                    && !e.process_name.to_lowercase().contains(&query)
                    && !e
                        .known_service
                        .map(|s| s.to_lowercase().contains(&query))
                        .unwrap_or(false)
                {
                    return false;
                }

                // Deduplicate browser entries: keep only first per (pid, service)
                if e.category == ServiceCategory::Browser {
                    if let Some(svc) = e.known_service {
                        if !browser_seen.insert((e.pid, svc.to_string())) {
                            return false;
                        }
                    }
                }

                true
            })
            .map(|(i, _)| i)
            .collect();

        // Keep selection in bounds
        if let Some(selected) = self.table_state.selected() {
            if selected >= self.filtered.len() {
                self.table_state.select(if self.filtered.is_empty() {
                    None
                } else {
                    Some(self.filtered.len() - 1)
                });
            }
        } else if !self.filtered.is_empty() {
            self.table_state.select(Some(0));
        }
    }

    fn selected_entry(&self) -> Option<&PortEntry> {
        self.table_state
            .selected()
            .and_then(|i| self.filtered.get(i))
            .map(|&idx| &self.entries[idx])
    }

    fn move_selection(&mut self, delta: i32) {
        if self.filtered.is_empty() {
            return;
        }
        let current = self.table_state.selected().unwrap_or(0) as i32;
        let new = (current + delta).clamp(0, self.filtered.len() as i32 - 1) as usize;
        if self.table_state.selected() != Some(new) {
            self.detail_scroll = 0;
        }
        self.table_state.select(Some(new));
    }
}

pub fn run_tui() -> Result<()> {
    // Install panic hook to restore terminal on crash
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    app.refresh();

    let mut last_refresh = Instant::now();

    loop {
        terminal.draw(|f| draw(f, &mut app))?;

        // Poll for events with timeout for auto-refresh
        let timeout = REFRESH_INTERVAL
            .checked_sub(last_refresh.elapsed())
            .unwrap_or(Duration::ZERO);

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                handle_key(&mut app, key);
            }
        }

        // Auto-refresh
        if last_refresh.elapsed() >= REFRESH_INTERVAL {
            app.refresh();
            last_refresh = Instant::now();
        }

        if app.should_quit {
            break;
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent) {
    // Kill confirmation dialog takes priority
    if let Some((idx, force)) = app.confirm_kill {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let entry = &app.entries[app.filtered[idx]];
                let pid = entry.pid;
                let name = entry.process_name.clone();
                let port = entry.port;
                match kill_process(&app.sys, pid, force) {
                    Ok(()) => {
                        app.status_msg = Some((
                            format!(
                                "{}Killed {} (PID {}) on port {}",
                                if force { "Force " } else { "" },
                                name,
                                pid,
                                port
                            ),
                            Instant::now(),
                        ));
                        app.refresh();
                    }
                    Err(e) => {
                        app.status_msg = Some((format!("Kill failed: {}", e), Instant::now()));
                    }
                }
                app.confirm_kill = None;
            }
            _ => {
                app.confirm_kill = None;
            }
        }
        return;
    }

    // Ctrl+key shortcuts (work in ALL modes: normal + filter)
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('c') | KeyCode::Char('q') => {
                app.should_quit = true;
                return;
            }
            KeyCode::Char('x') => {
                // Kill with confirmation
                if let Some(selected) = app.table_state.selected() {
                    if selected < app.filtered.len() {
                        app.confirm_kill = Some((selected, false));
                    }
                }
                return;
            }
            KeyCode::Char('k') => {
                // Force kill (SIGKILL), with confirmation
                if let Some(selected) = app.table_state.selected() {
                    if selected < app.filtered.len() {
                        app.confirm_kill = Some((selected, true));
                    }
                }
                return;
            }
            KeyCode::Char('d') => {
                app.show_detail = !app.show_detail;
                return;
            }
            KeyCode::Char('s') => {
                app.sort_field = app.sort_field.next();
                app.sort_entries();
                app.apply_filter();
                return;
            }
            KeyCode::Char('t') => {
                // Cycle: TCP+UDP → TCP only → UDP only → TCP+UDP
                match (app.show_tcp, app.show_udp) {
                    (true, true) => {
                        app.show_udp = false;
                    }
                    (true, false) => {
                        app.show_tcp = false;
                        app.show_udp = true;
                    }
                    (false, true) => {
                        app.show_tcp = true;
                    }
                    (false, false) => {
                        app.show_tcp = true;
                        app.show_udp = true;
                    }
                }
                app.refresh();
                return;
            }
            KeyCode::Char('r') => {
                app.refresh();
                app.status_msg = Some(("Refreshed".to_string(), Instant::now()));
                return;
            }
            _ => {}
        }
    }

    // Filter input mode
    if app.filter_active {
        match key.code {
            KeyCode::Esc => {
                app.filter_active = false;
                app.filter_text.clear();
                app.apply_filter();
            }
            KeyCode::Enter => {
                app.filter_active = false;
            }
            KeyCode::Backspace => {
                app.filter_text.pop();
                app.apply_filter();
            }
            KeyCode::Char(c) => {
                app.filter_text.push(c);
                app.apply_filter();
            }
            _ => {}
        }
        return;
    }

    // Normal mode
    match key.code {
        KeyCode::Esc => app.should_quit = true,
        KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
        KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
        KeyCode::PageUp | KeyCode::Left => {
            app.detail_scroll = app.detail_scroll.saturating_sub(3);
        }
        KeyCode::PageDown | KeyCode::Right => {
            app.detail_scroll = app.detail_scroll.saturating_add(3);
        }
        KeyCode::Char('/') => {
            app.filter_active = true;
        }
        _ => {}
    }
}

fn draw(f: &mut Frame, app: &mut App) {
    let size = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header + filter
            Constraint::Min(5),    // table
            Constraint::Length(1), // status bar
        ])
        .split(size);

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

    // Kill confirmation overlay
    if let Some((idx, force)) = app.confirm_kill {
        if let Some(&entry_idx) = app.filtered.get(idx) {
            let entry = &app.entries[entry_idx];
            draw_kill_confirm(f, entry, force);
        }
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
            format!("{}{}", &app.filter_text, filter_indicator),
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

    let paragraph = Paragraph::new(header).block(block);
    f.render_widget(paragraph, area);
}

fn draw_table(f: &mut Frame, app: &mut App, area: Rect) {
    let t = theme();

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
            let cpu_color = if e.cpu_percent > 50.0 {
                t.error
            } else if e.cpu_percent > 20.0 {
                t.warning
            } else {
                t.text
            };

            // For grouped browser entries, look up how many ports this (pid, service) has
            let browser_count = if e.category == ServiceCategory::Browser {
                e.known_service
                    .and_then(|svc| app.browser_counts.get(&(e.pid, svc.to_string())).copied())
                    .unwrap_or(1)
            } else {
                1
            };

            let service_text = match e.known_service {
                Some(svc) if browser_count > 1 => format!("{} ×{}", svc, browser_count),
                Some(svc) => svc.to_string(),
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
                Cell::from(e.process_name.clone()).style(Style::default().fg(cat_color)),
                Cell::from(service_text).style(Style::default().fg(cat_color)),
                Cell::from(e.pid.to_string()).style(Style::default().fg(t.text_muted)),
                Cell::from(format!("{:.1}%", e.cpu_percent)).style(Style::default().fg(cpu_color)),
                Cell::from(e.memory_display()).style(Style::default().fg(t.text)),
                Cell::from(e.uptime_display()).style(Style::default().fg(t.text_muted)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(7),  // PORT
        Constraint::Length(6),  // PROTO
        Constraint::Length(14), // PROCESS
        Constraint::Length(16), // SERVICE
        Constraint::Length(7),  // PID
        Constraint::Length(7),  // CPU
        Constraint::Length(9),  // MEM
        Constraint::Length(8),  // UPTIME
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(t.border)),
        )
        .row_highlight_style(
            Style::default()
                .bg(t.selection_bg)
                .fg(t.selection_fg)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_detail(f: &mut Frame, app: &App, area: Rect) {
    let t = theme();

    let content = if let Some(entry) = app.selected_entry() {
        vec![
            Line::from(vec![
                Span::styled("Port: ", Style::default().fg(t.text_secondary)),
                Span::styled(
                    format!("{} ({})", entry.port, entry.protocol),
                    Style::default().fg(t.text).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Address: ", Style::default().fg(t.text_secondary)),
                Span::styled(entry.addr_display(), Style::default().fg(t.text)),
            ]),
            Line::from(vec![
                Span::styled("Process: ", Style::default().fg(t.text_secondary)),
                Span::styled(
                    &entry.process_name,
                    Style::default().fg(t.category_color(entry.category)),
                ),
            ]),
            Line::from(vec![
                Span::styled("PID: ", Style::default().fg(t.text_secondary)),
                Span::styled(entry.pid.to_string(), Style::default().fg(t.text)),
            ]),
            Line::from(vec![
                Span::styled("Service: ", Style::default().fg(t.text_secondary)),
                Span::styled(
                    entry.known_service.unwrap_or("Unknown"),
                    Style::default().fg(t.category_color(entry.category)),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("CPU: ", Style::default().fg(t.text_secondary)),
                Span::styled(
                    format!("{:.1}%", entry.cpu_percent),
                    Style::default().fg(t.text),
                ),
            ]),
            Line::from(vec![
                Span::styled("Memory: ", Style::default().fg(t.text_secondary)),
                Span::styled(entry.memory_display(), Style::default().fg(t.text)),
            ]),
            Line::from(vec![
                Span::styled("Uptime: ", Style::default().fg(t.text_secondary)),
                Span::styled(entry.uptime_display(), Style::default().fg(t.text)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Command:",
                Style::default().fg(t.text_secondary),
            )),
            Line::from(Span::styled(
                &entry.process_cmd,
                Style::default().fg(t.text_muted),
            )),
        ]
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

    // Show status message if recent (within 3 seconds)
    if let Some((ref msg, ref when)) = app.status_msg {
        if when.elapsed() < Duration::from_secs(3) {
            let line = Line::from(Span::styled(
                format!(" {}", msg),
                Style::default().fg(t.success),
            ));
            f.render_widget(Paragraph::new(line), area);
            return;
        }
    }

    let shortcuts = Line::from(vec![
        Span::styled(
            " /",
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Filter  ", Style::default().fg(t.text_muted)),
        Span::styled(
            "^X",
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Kill  ", Style::default().fg(t.text_muted)),
        Span::styled(
            "^K",
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Force  ", Style::default().fg(t.text_muted)),
        Span::styled(
            "^D",
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Detail  ", Style::default().fg(t.text_muted)),
        Span::styled(
            "^S",
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Sort  ", Style::default().fg(t.text_muted)),
        Span::styled(
            "^T",
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Proto  ", Style::default().fg(t.text_muted)),
        Span::styled(
            "^R",
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Refresh  ", Style::default().fg(t.text_muted)),
        Span::styled(
            "^Q",
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Quit", Style::default().fg(t.text_muted)),
    ]);

    f.render_widget(Paragraph::new(shortcuts), area);
}

fn draw_kill_confirm(f: &mut Frame, entry: &PortEntry, force: bool) {
    let t = theme();
    let area = f.area();

    let title = if force {
        " Confirm Force Kill (SIGKILL) "
    } else {
        " Confirm Kill "
    };
    let action = if force { "  Force kill " } else { "  Kill " };

    // Center a dialog box
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
                entry.process_name.to_string(),
                Style::default().fg(t.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" (PID {}) on port {}?", entry.pid, entry.port),
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

    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, dialog_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Protocol, ServiceCategory};
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::Duration;

    fn make_entry(
        port: u16,
        name: &str,
        service: Option<&'static str>,
        category: ServiceCategory,
        cpu: f32,
        memory: f64,
    ) -> PortEntry {
        PortEntry {
            protocol: Protocol::Tcp,
            local_addr: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port,
            pid: port as u32,
            process_name: name.to_string(),
            process_cmd: String::new(),
            cpu_percent: cpu,
            memory_mb: memory,
            uptime: Duration::from_secs(100),
            known_service: service,
            category,
        }
    }

    fn app_with_entries(entries: Vec<PortEntry>) -> App {
        let mut app = App::new();
        app.entries = entries;
        app.sort_entries();
        app.apply_filter();
        app
    }

    // --- Filter tests ---

    #[test]
    fn filter_empty_shows_all() {
        let app = app_with_entries(vec![
            make_entry(3000, "node", Some("Next.js"), ServiceCategory::DevServer, 0.0, 100.0),
            make_entry(5432, "postgres", Some("PostgreSQL"), ServiceCategory::Database, 0.0, 200.0),
        ]);
        assert_eq!(app.filtered.len(), 2);
    }

    #[test]
    fn filter_by_port_number() {
        let mut app = app_with_entries(vec![
            make_entry(3000, "node", Some("Next.js"), ServiceCategory::DevServer, 0.0, 100.0),
            make_entry(5432, "postgres", Some("PostgreSQL"), ServiceCategory::Database, 0.0, 200.0),
        ]);
        app.filter_text = "5432".to_string();
        app.apply_filter();
        assert_eq!(app.filtered.len(), 1);
        assert_eq!(app.entries[app.filtered[0]].port, 5432);
    }

    #[test]
    fn filter_by_partial_port() {
        let mut app = app_with_entries(vec![
            make_entry(3000, "node", None, ServiceCategory::DevServer, 0.0, 100.0),
            make_entry(3001, "node", None, ServiceCategory::DevServer, 0.0, 100.0),
            make_entry(5432, "postgres", None, ServiceCategory::Database, 0.0, 200.0),
        ]);
        app.filter_text = "300".to_string();
        app.apply_filter();
        assert_eq!(app.filtered.len(), 2);
    }

    #[test]
    fn filter_by_process_name_case_insensitive() {
        let mut app = app_with_entries(vec![
            make_entry(3000, "node", None, ServiceCategory::DevServer, 0.0, 100.0),
            make_entry(5432, "postgres", Some("PostgreSQL"), ServiceCategory::Database, 0.0, 200.0),
        ]);
        app.filter_text = "POST".to_string();
        app.apply_filter();
        assert_eq!(app.filtered.len(), 1);
        assert_eq!(app.entries[app.filtered[0]].process_name, "postgres");
    }

    #[test]
    fn filter_by_service_name() {
        let mut app = app_with_entries(vec![
            make_entry(3000, "node", Some("Next.js"), ServiceCategory::DevServer, 0.0, 100.0),
            make_entry(5432, "postmaster", Some("PostgreSQL"), ServiceCategory::Database, 0.0, 200.0),
        ]);
        app.filter_text = "next".to_string();
        app.apply_filter();
        assert_eq!(app.filtered.len(), 1);
        assert_eq!(app.entries[app.filtered[0]].known_service, Some("Next.js"));
    }

    #[test]
    fn filter_no_match() {
        let mut app = app_with_entries(vec![
            make_entry(3000, "node", None, ServiceCategory::DevServer, 0.0, 100.0),
        ]);
        app.filter_text = "zzzzz".to_string();
        app.apply_filter();
        assert_eq!(app.filtered.len(), 0);
    }

    #[test]
    fn filter_selection_stays_in_bounds() {
        let mut app = app_with_entries(vec![
            make_entry(3000, "node", None, ServiceCategory::DevServer, 0.0, 100.0),
            make_entry(5432, "postgres", None, ServiceCategory::Database, 0.0, 200.0),
            make_entry(6379, "redis", None, ServiceCategory::Cache, 0.0, 50.0),
        ]);
        // Select last item
        app.table_state.select(Some(2));
        // Filter to 1 result — selection must clamp
        app.filter_text = "node".to_string();
        app.apply_filter();
        assert_eq!(app.filtered.len(), 1);
        assert!(app.table_state.selected().unwrap() < app.filtered.len());
    }

    // --- Sort tests ---

    #[test]
    fn sort_by_port_ascending() {
        let app = app_with_entries(vec![
            make_entry(8080, "nginx", None, ServiceCategory::System, 0.0, 100.0),
            make_entry(3000, "node", None, ServiceCategory::DevServer, 0.0, 100.0),
            make_entry(5432, "postgres", None, ServiceCategory::Database, 0.0, 100.0),
        ]);
        let ports: Vec<u16> = app.filtered.iter().map(|&i| app.entries[i].port).collect();
        assert_eq!(ports, vec![3000, 5432, 8080]);
    }

    #[test]
    fn sort_by_cpu_descending() {
        let mut app = app_with_entries(vec![
            make_entry(3000, "node", None, ServiceCategory::DevServer, 10.0, 100.0),
            make_entry(5432, "postgres", None, ServiceCategory::Database, 80.0, 100.0),
            make_entry(6379, "redis", None, ServiceCategory::Cache, 5.0, 100.0),
        ]);
        app.sort_field = SortField::Cpu;
        app.sort_entries();
        app.apply_filter();
        let cpus: Vec<f32> = app.filtered.iter().map(|&i| app.entries[i].cpu_percent).collect();
        assert_eq!(cpus, vec![80.0, 10.0, 5.0]);
    }

    #[test]
    fn sort_by_memory_descending() {
        let mut app = app_with_entries(vec![
            make_entry(3000, "node", None, ServiceCategory::DevServer, 0.0, 100.0),
            make_entry(5432, "postgres", None, ServiceCategory::Database, 0.0, 500.0),
            make_entry(6379, "redis", None, ServiceCategory::Cache, 0.0, 50.0),
        ]);
        app.sort_field = SortField::Memory;
        app.sort_entries();
        app.apply_filter();
        let mems: Vec<f64> = app.filtered.iter().map(|&i| app.entries[i].memory_mb).collect();
        assert_eq!(mems, vec![500.0, 100.0, 50.0]);
    }

    #[test]
    fn sort_by_process_name_case_insensitive() {
        let mut app = app_with_entries(vec![
            make_entry(3000, "Nginx", None, ServiceCategory::System, 0.0, 100.0),
            make_entry(5432, "apache", None, ServiceCategory::System, 0.0, 100.0),
            make_entry(6379, "Redis", None, ServiceCategory::Cache, 0.0, 100.0),
        ]);
        app.sort_field = SortField::ProcessName;
        app.sort_entries();
        app.apply_filter();
        let names: Vec<&str> = app
            .filtered
            .iter()
            .map(|&i| app.entries[i].process_name.as_str())
            .collect();
        assert_eq!(names, vec!["apache", "Nginx", "Redis"]);
    }

    // --- Move selection ---

    #[test]
    fn move_selection_clamps_at_top() {
        let mut app = app_with_entries(vec![
            make_entry(3000, "node", None, ServiceCategory::DevServer, 0.0, 100.0),
            make_entry(5432, "postgres", None, ServiceCategory::Database, 0.0, 100.0),
        ]);
        app.table_state.select(Some(0));
        app.move_selection(-1);
        assert_eq!(app.table_state.selected(), Some(0));
    }

    #[test]
    fn move_selection_clamps_at_bottom() {
        let mut app = app_with_entries(vec![
            make_entry(3000, "node", None, ServiceCategory::DevServer, 0.0, 100.0),
            make_entry(5432, "postgres", None, ServiceCategory::Database, 0.0, 100.0),
        ]);
        app.table_state.select(Some(1));
        app.move_selection(1);
        assert_eq!(app.table_state.selected(), Some(1));
    }

    #[test]
    fn move_selection_empty_list() {
        let mut app = app_with_entries(vec![]);
        app.move_selection(1);
        // Should not panic
        assert_eq!(app.table_state.selected(), None);
    }

    // --- Browser grouping tests ---

    fn make_browser_entry(port: u16, pid: u32, service: &'static str) -> PortEntry {
        PortEntry {
            protocol: Protocol::Tcp,
            local_addr: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port,
            pid,
            process_name: "Google Chrome Helper".to_string(),
            process_cmd: String::new(),
            cpu_percent: 1.0,
            memory_mb: 50.0,
            uptime: Duration::from_secs(100),
            known_service: Some(service),
            category: ServiceCategory::Browser,
        }
    }

    #[test]
    fn browser_entries_grouped_by_pid_and_service() {
        let app = app_with_entries(vec![
            make_browser_entry(5000, 100, "Chrome"),
            make_browser_entry(5001, 100, "Chrome"),
            make_browser_entry(5002, 100, "Chrome"),
            make_entry(3000, "node", Some("Next.js"), ServiceCategory::DevServer, 0.0, 100.0),
        ]);
        // 3 Chrome entries collapsed to 1, plus 1 node entry = 2
        assert_eq!(app.filtered.len(), 2);
        // The browser group count should be 3
        assert_eq!(
            app.browser_counts.get(&(100, "Chrome".to_string())),
            Some(&3)
        );
    }

    #[test]
    fn different_browser_pids_not_grouped() {
        let app = app_with_entries(vec![
            make_browser_entry(5000, 100, "Chrome"),
            make_browser_entry(5001, 200, "Chrome"),
        ]);
        // Different PIDs — should be 2 separate entries
        assert_eq!(app.filtered.len(), 2);
    }

    #[test]
    fn different_browser_services_not_grouped() {
        let app = app_with_entries(vec![
            make_browser_entry(5000, 100, "Chrome"),
            make_browser_entry(5001, 100, "Brave"),
        ]);
        // Different services — should be 2 separate entries
        assert_eq!(app.filtered.len(), 2);
    }

    #[test]
    fn single_browser_entry_not_grouped() {
        let app = app_with_entries(vec![
            make_browser_entry(5000, 100, "Chrome"),
            make_entry(3000, "node", None, ServiceCategory::DevServer, 0.0, 100.0),
        ]);
        // Only 1 Chrome entry — no grouping, count is 1
        assert_eq!(app.filtered.len(), 2);
        assert_eq!(
            app.browser_counts.get(&(100, "Chrome".to_string())),
            Some(&1)
        );
    }
}
