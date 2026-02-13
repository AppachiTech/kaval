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
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState},
    Frame, Terminal,
};

use crate::models::{PortEntry, SortField};
use crate::scanner::{kill_process, scan_ports};
use crate::theme::theme;

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

struct App {
    entries: Vec<PortEntry>,
    filtered: Vec<usize>, // indices into entries
    table_state: TableState,
    filter_text: String,
    filter_active: bool,
    show_tcp: bool,
    show_udp: bool,
    sort_field: SortField,
    show_detail: bool,
    confirm_kill: Option<usize>, // index of entry to confirm kill
    status_msg: Option<(String, Instant)>,
    should_quit: bool,
}

impl App {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            filtered: Vec::new(),
            table_state: TableState::default(),
            filter_text: String::new(),
            filter_active: false,
            show_tcp: true,
            show_udp: true,
            sort_field: SortField::Port,
            show_detail: false,
            confirm_kill: None,
            status_msg: None,
            should_quit: false,
        }
    }

    fn refresh(&mut self) {
        match scan_ports(self.show_tcp, self.show_udp) {
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
            SortField::Cpu => self
                .entries
                .sort_by(|a, b| b.cpu_percent.partial_cmp(&a.cpu_percent).unwrap()),
            SortField::Memory => self
                .entries
                .sort_by(|a, b| b.memory_mb.partial_cmp(&a.memory_mb).unwrap()),
        }
    }

    fn apply_filter(&mut self) {
        let query = self.filter_text.to_lowercase();
        self.filtered = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                if query.is_empty() {
                    return true;
                }
                e.port.to_string().contains(&query)
                    || e.process_name.to_lowercase().contains(&query)
                    || e.known_service
                        .map(|s| s.to_lowercase().contains(&query))
                        .unwrap_or(false)
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
        self.table_state.select(Some(new));
    }
}

pub fn run_tui() -> Result<()> {
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
    if let Some(idx) = app.confirm_kill {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let entry = &app.entries[app.filtered[idx]];
                let pid = entry.pid;
                let name = entry.process_name.clone();
                let port = entry.port;
                match kill_process(pid, false) {
                    Ok(()) => {
                        app.status_msg = Some((
                            format!("Killed {} (PID {}) on port {}", name, pid, port),
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
                        app.confirm_kill = Some(selected);
                    }
                }
                return;
            }
            KeyCode::Char('k') => {
                // Force kill (SIGKILL), no confirmation
                if let Some(selected) = app.table_state.selected() {
                    if selected < app.filtered.len() {
                        let entry = &app.entries[app.filtered[selected]];
                        let pid = entry.pid;
                        let name = entry.process_name.clone();
                        let port = entry.port;
                        match kill_process(pid, true) {
                            Ok(()) => {
                                app.status_msg = Some((
                                    format!("Force killed {} (PID {}) on port {}", name, pid, port),
                                    Instant::now(),
                                ));
                                app.refresh();
                            }
                            Err(e) => {
                                app.status_msg =
                                    Some((format!("Kill failed: {}", e), Instant::now()));
                            }
                        }
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
    if let Some(idx) = app.confirm_kill {
        if let Some(&entry_idx) = app.filtered.get(idx) {
            let entry = &app.entries[entry_idx];
            draw_kill_confirm(f, entry);
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
            let service_text = e.known_service.unwrap_or("—");
            let cat_color = t.category_color(e.category);
            let cpu_color = if e.cpu_percent > 50.0 {
                t.error
            } else if e.cpu_percent > 20.0 {
                t.warning
            } else {
                t.text
            };

            Row::new(vec![
                Cell::from(e.port.to_string()).style(Style::default().fg(t.text)),
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

    let block = Block::default()
        .title(" Detail ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.border));

    let paragraph = Paragraph::new(content).block(block);
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

fn draw_kill_confirm(f: &mut Frame, entry: &PortEntry) {
    let t = theme();
    let area = f.area();

    // Center a dialog box
    let dialog_width = 50u16.min(area.width.saturating_sub(4));
    let dialog_height = 5u16;
    let x = (area.width.saturating_sub(dialog_width)) / 2;
    let y = (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    let text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  Kill ",
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
        .title(" Confirm Kill ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.error));

    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, dialog_area);
}
