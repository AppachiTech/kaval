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

use crate::models::{
    count_browser_groups, port_matches, sort_entries, PortEntry, ServiceCategory, SortField,
    CPU_ERROR_THRESHOLD, CPU_WARN_THRESHOLD,
};
use crate::scanner::{kill_process, scan_ports};
use crate::theme::theme;

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

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

struct App {
    sys: System,
    entries: Vec<PortEntry>,
    filtered: Vec<usize>,                                // indices into entries
    browser_counts: HashMap<(u32, &'static str), usize>, // (pid, service) -> port count
    table_state: TableState,
    filter_text: String,
    filter_active: bool,
    show_tcp: bool,
    show_udp: bool,
    sort_field: SortField,
    show_detail: bool,
    detail_scroll: u16,
    confirm_kill: Option<(u32, String, u16, bool)>, // (pid, name, port, force)
    show_help: bool,
    loading: bool,
    status_msg: Option<(String, Instant, bool)>, // (message, when, is_error)
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
            confirm_kill: None,
            show_help: false,
            loading: true,
            status_msg: None,
            should_quit: false,
        }
    }

    fn refresh(&mut self) {
        match scan_ports(&mut self.sys, self.show_tcp, self.show_udp) {
            Ok(entries) => {
                self.entries = entries;
                self.sort_entries();
                self.browser_counts = count_browser_groups(&self.entries);
                self.apply_filter();
            }
            Err(e) => {
                self.status_msg = Some((format!("Scan error: {}", e), Instant::now(), true));
            }
        }
        self.loading = false;
    }

    fn sort_entries(&mut self) {
        sort_entries(&mut self.entries, self.sort_field);
    }

    fn apply_filter(&mut self) {
        // Pre-condition: self.browser_counts must be current.
        // It is recomputed by refresh() after entries change, not here —
        // browser counts depend on entry content, not sort order or filter text.
        //
        // Note: we reimplement browser dedup inline rather than calling
        // `dedup_browser_indices()` from models.rs because this method
        // interleaves filter-text matching with deduplication in a single
        // pass. Separating them would require two passes over entries.
        let query = self.filter_text.to_lowercase();

        // Track which browser (pid, service) groups we've already included
        let mut browser_seen: HashSet<(u32, &'static str)> = HashSet::new();

        self.filtered = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                if !query.is_empty()
                    && !port_matches(e.port, &query)
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
                        if !browser_seen.insert((e.pid, svc)) {
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

/// Copy text to the system clipboard. Returns true if successful.
/// Uses pbcopy on macOS, xclip/xsel on Linux. Silent no-op on other platforms.
#[cfg(target_os = "macos")]
fn clipboard_copy(text: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let Ok(mut child) = Command::new("pbcopy").stdin(Stdio::piped()).spawn() else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn clipboard_copy(text: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let candidates: &[(&str, &[&str])] = &[
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
    ];
    for &(prog, args) in candidates {
        let Ok(mut child) = Command::new(prog).args(args).stdin(Stdio::piped()).spawn() else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        if child.wait().map(|s| s.success()).unwrap_or(false) {
            return true;
        }
    }
    false
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn clipboard_copy(_text: &str) -> bool {
    false
}

/// Open a URL in the default browser. Returns true if the command spawned successfully.
#[cfg(target_os = "macos")]
fn open_in_browser(url: &str) -> bool {
    std::process::Command::new("open")
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

#[cfg(target_os = "linux")]
fn open_in_browser(url: &str) -> bool {
    std::process::Command::new("xdg-open")
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn open_in_browser(_url: &str) -> bool {
    false
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
    // Help overlay: any key dismisses it
    if app.show_help {
        app.show_help = false;
        return;
    }

    // Kill confirmation dialog takes priority
    if let Some((pid, name, port, force)) = app.confirm_kill.take() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => match kill_process(&app.sys, pid, force) {
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
                        false,
                    ));
                    app.refresh();
                }
                Err(e) => {
                    app.status_msg = Some((format!("Kill failed: {}", e), Instant::now(), true));
                }
            },
            _ => {}
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
                // Kill with confirmation — snapshot PID/name/port now to avoid stale index
                if let Some(selected) = app.table_state.selected() {
                    if let Some(&idx) = app.filtered.get(selected) {
                        let e = &app.entries[idx];
                        app.confirm_kill = Some((e.pid, e.process_name.clone(), e.port, false));
                    }
                }
                return;
            }
            KeyCode::Char('k') => {
                // Force kill (SIGKILL), with confirmation — snapshot PID/name/port now
                if let Some(selected) = app.table_state.selected() {
                    if let Some(&idx) = app.filtered.get(selected) {
                        let e = &app.entries[idx];
                        app.confirm_kill = Some((e.pid, e.process_name.clone(), e.port, true));
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
                app.status_msg = Some(("Refreshed".to_string(), Instant::now(), false));
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
        KeyCode::Home => {
            if !app.filtered.is_empty() {
                app.detail_scroll = 0;
                app.table_state.select(Some(0));
            }
        }
        KeyCode::End => {
            if !app.filtered.is_empty() {
                app.detail_scroll = 0;
                app.table_state.select(Some(app.filtered.len() - 1));
            }
        }
        KeyCode::Char('/') => {
            app.filter_active = true;
        }
        KeyCode::Char('?') => {
            app.show_help = true;
        }
        KeyCode::Char('o') => {
            if let Some(entry) = app.selected_entry() {
                let url = format!("http://localhost:{}", entry.port);
                if open_in_browser(&url) {
                    app.status_msg = Some((format!("Opening {}", url), Instant::now(), false));
                } else {
                    app.status_msg =
                        Some(("Browser not available".to_string(), Instant::now(), true));
                }
            }
        }
        KeyCode::Char('y') => {
            if let Some(entry) = app.selected_entry() {
                let text = entry.port.to_string();
                if clipboard_copy(&text) {
                    app.status_msg = Some((
                        format!("Copied port {} to clipboard", text),
                        Instant::now(),
                        false,
                    ));
                } else {
                    app.status_msg = Some((
                        "Clipboard unavailable (pbcopy/xclip/xsel not found)".to_string(),
                        Instant::now(),
                        true,
                    ));
                }
            }
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
    if let Some((pid, ref name, port, force)) = app.confirm_kill {
        draw_kill_confirm(f, pid, name, port, force);
    }

    // Help overlay (drawn on top of everything)
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
            let browser_count = if e.category == ServiceCategory::Browser {
                e.known_service
                    .and_then(|svc| app.browser_counts.get(&(e.pid, svc)).copied())
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
        let exited = entry.process_name == "<exited>";
        let mut lines = vec![
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
                    Style::default().fg(if exited {
                        t.text_muted
                    } else {
                        t.category_color(entry.category)
                    }),
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
                    entry.cmd_display(),
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

    // Show status message if recent (within 3 seconds)
    if let Some((ref msg, ref when, is_error)) = app.status_msg {
        if when.elapsed() < Duration::from_secs(3) {
            let color = if is_error { t.error } else { t.success };
            let line = Line::from(Span::styled(
                format!(" {}", msg),
                Style::default().fg(color),
            ));
            f.render_widget(Paragraph::new(line), area);
            return;
        }
    }

    // Filter mode: replace shortcuts with contextual hint
    if app.filter_active {
        let line = Line::from(vec![
            Span::styled(
                " Filter mode",
                Style::default().fg(t.primary).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" — type to search  ", Style::default().fg(t.text_muted)),
            Span::styled(
                "ESC",
                Style::default().fg(t.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" cancel  ", Style::default().fg(t.text_muted)),
            Span::styled(
                "Enter",
                Style::default().fg(t.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" confirm", Style::default().fg(t.text_muted)),
        ]);
        f.render_widget(Paragraph::new(line), area);
        return;
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
        Span::styled(" Quit  ", Style::default().fg(t.text_muted)),
        Span::styled(
            "?",
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Help", Style::default().fg(t.text_muted)),
    ]);

    f.render_widget(Paragraph::new(shortcuts), area);
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
        ("Ctrl+Q / Esc", "Quit"),
        ("?", "Show / dismiss this help"),
    ];

    let dialog_width = 58u16.min(area.width.saturating_sub(4));
    let dialog_height = rows.len() as u16 + 4; // header + rows + padding + border
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

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, dialog_area);
}

fn draw_kill_confirm(f: &mut Frame, pid: u32, name: &str, port: u16, force: bool) {
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
                name.to_string(),
                Style::default().fg(t.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" (PID {}) on port {}?", pid, port),
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
            process_cmd: vec![],
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
        app.browser_counts = count_browser_groups(&app.entries);
        app.apply_filter();
        app
    }

    // --- Filter tests ---

    #[test]
    fn filter_empty_shows_all() {
        let app = app_with_entries(vec![
            make_entry(
                3000,
                "node",
                Some("Next.js"),
                ServiceCategory::DevServer,
                0.0,
                100.0,
            ),
            make_entry(
                5432,
                "postgres",
                Some("PostgreSQL"),
                ServiceCategory::Database,
                0.0,
                200.0,
            ),
        ]);
        assert_eq!(app.filtered.len(), 2);
    }

    #[test]
    fn filter_by_port_number() {
        let mut app = app_with_entries(vec![
            make_entry(
                3000,
                "node",
                Some("Next.js"),
                ServiceCategory::DevServer,
                0.0,
                100.0,
            ),
            make_entry(
                5432,
                "postgres",
                Some("PostgreSQL"),
                ServiceCategory::Database,
                0.0,
                200.0,
            ),
        ]);
        app.filter_text = "5432".to_string();
        app.apply_filter();
        assert_eq!(app.filtered.len(), 1);
        assert_eq!(app.entries[app.filtered[0]].port, 5432);
    }

    #[test]
    fn filter_by_partial_port_is_exact_match() {
        // "300" is a valid u16 — must match only port 300, not 3000 or 3001
        let mut app = app_with_entries(vec![
            make_entry(300, "node", None, ServiceCategory::DevServer, 0.0, 100.0),
            make_entry(3000, "node", None, ServiceCategory::DevServer, 0.0, 100.0),
            make_entry(3001, "node", None, ServiceCategory::DevServer, 0.0, 100.0),
        ]);
        app.filter_text = "300".to_string();
        app.apply_filter();
        assert_eq!(app.filtered.len(), 1);
        assert_eq!(app.entries[app.filtered[0]].port, 300);
    }

    #[test]
    fn filter_by_process_name_case_insensitive() {
        let mut app = app_with_entries(vec![
            make_entry(3000, "node", None, ServiceCategory::DevServer, 0.0, 100.0),
            make_entry(
                5432,
                "postgres",
                Some("PostgreSQL"),
                ServiceCategory::Database,
                0.0,
                200.0,
            ),
        ]);
        app.filter_text = "POST".to_string();
        app.apply_filter();
        assert_eq!(app.filtered.len(), 1);
        assert_eq!(app.entries[app.filtered[0]].process_name, "postgres");
    }

    #[test]
    fn filter_by_service_name() {
        let mut app = app_with_entries(vec![
            make_entry(
                3000,
                "node",
                Some("Next.js"),
                ServiceCategory::DevServer,
                0.0,
                100.0,
            ),
            make_entry(
                5432,
                "postmaster",
                Some("PostgreSQL"),
                ServiceCategory::Database,
                0.0,
                200.0,
            ),
        ]);
        app.filter_text = "next".to_string();
        app.apply_filter();
        assert_eq!(app.filtered.len(), 1);
        assert_eq!(app.entries[app.filtered[0]].known_service, Some("Next.js"));
    }

    // --- port_matches unit tests ---

    #[test]
    fn port_matches_exact_numeric() {
        assert!(port_matches(80, "80"));
        assert!(port_matches(5432, "5432"));
        assert!(!port_matches(8080, "80")); // must NOT substring-match
        assert!(!port_matches(3800, "80"));
        assert!(!port_matches(18080, "80"));
    }

    #[test]
    fn port_matches_non_numeric_query_does_not_match_port() {
        // Non-numeric queries (e.g. service names) never match a port number —
        // they fall through to process name / service matching instead.
        assert!(!port_matches(3000, "node"));
        assert!(!port_matches(5432, "postgres"));
        assert!(!port_matches(80, "http"));
    }

    #[test]
    fn filter_by_exact_port_not_substring() {
        let mut app = app_with_entries(vec![
            make_entry(80, "nginx", None, ServiceCategory::System, 0.0, 100.0),
            make_entry(8080, "caddy", None, ServiceCategory::System, 0.0, 100.0),
            make_entry(3800, "node", None, ServiceCategory::DevServer, 0.0, 100.0),
        ]);
        app.filter_text = "80".to_string();
        app.apply_filter();
        // Only port 80 — not 8080 or 3800
        assert_eq!(app.filtered.len(), 1);
        assert_eq!(app.entries[app.filtered[0]].port, 80);
    }

    #[test]
    fn filter_by_exact_port_5432_not_54321() {
        let mut app = app_with_entries(vec![
            make_entry(
                5432,
                "postgres",
                Some("PostgreSQL"),
                ServiceCategory::Database,
                0.0,
                200.0,
            ),
            make_entry(54321, "custom", None, ServiceCategory::Unknown, 0.0, 50.0),
        ]);
        app.filter_text = "5432".to_string();
        app.apply_filter();
        assert_eq!(app.filtered.len(), 1);
        assert_eq!(app.entries[app.filtered[0]].port, 5432);
    }

    #[test]
    fn filter_no_match() {
        let mut app = app_with_entries(vec![make_entry(
            3000,
            "node",
            None,
            ServiceCategory::DevServer,
            0.0,
            100.0,
        )]);
        app.filter_text = "zzzzz".to_string();
        app.apply_filter();
        assert_eq!(app.filtered.len(), 0);
    }

    #[test]
    fn filter_selection_stays_in_bounds() {
        let mut app = app_with_entries(vec![
            make_entry(3000, "node", None, ServiceCategory::DevServer, 0.0, 100.0),
            make_entry(
                5432,
                "postgres",
                None,
                ServiceCategory::Database,
                0.0,
                200.0,
            ),
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
            make_entry(
                5432,
                "postgres",
                None,
                ServiceCategory::Database,
                0.0,
                100.0,
            ),
        ]);
        let ports: Vec<u16> = app.filtered.iter().map(|&i| app.entries[i].port).collect();
        assert_eq!(ports, vec![3000, 5432, 8080]);
    }

    #[test]
    fn sort_by_cpu_descending() {
        let mut app = app_with_entries(vec![
            make_entry(3000, "node", None, ServiceCategory::DevServer, 10.0, 100.0),
            make_entry(
                5432,
                "postgres",
                None,
                ServiceCategory::Database,
                80.0,
                100.0,
            ),
            make_entry(6379, "redis", None, ServiceCategory::Cache, 5.0, 100.0),
        ]);
        app.sort_field = SortField::Cpu;
        app.sort_entries();
        app.apply_filter();
        let cpus: Vec<f32> = app
            .filtered
            .iter()
            .map(|&i| app.entries[i].cpu_percent)
            .collect();
        assert_eq!(cpus, vec![80.0, 10.0, 5.0]);
    }

    #[test]
    fn sort_by_memory_descending() {
        let mut app = app_with_entries(vec![
            make_entry(3000, "node", None, ServiceCategory::DevServer, 0.0, 100.0),
            make_entry(
                5432,
                "postgres",
                None,
                ServiceCategory::Database,
                0.0,
                500.0,
            ),
            make_entry(6379, "redis", None, ServiceCategory::Cache, 0.0, 50.0),
        ]);
        app.sort_field = SortField::Memory;
        app.sort_entries();
        app.apply_filter();
        let mems: Vec<f64> = app
            .filtered
            .iter()
            .map(|&i| app.entries[i].memory_mb)
            .collect();
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
            make_entry(
                5432,
                "postgres",
                None,
                ServiceCategory::Database,
                0.0,
                100.0,
            ),
        ]);
        app.table_state.select(Some(0));
        app.move_selection(-1);
        assert_eq!(app.table_state.selected(), Some(0));
    }

    #[test]
    fn move_selection_clamps_at_bottom() {
        let mut app = app_with_entries(vec![
            make_entry(3000, "node", None, ServiceCategory::DevServer, 0.0, 100.0),
            make_entry(
                5432,
                "postgres",
                None,
                ServiceCategory::Database,
                0.0,
                100.0,
            ),
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
            process_cmd: vec![],
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
            make_entry(
                3000,
                "node",
                Some("Next.js"),
                ServiceCategory::DevServer,
                0.0,
                100.0,
            ),
        ]);
        // 3 Chrome entries collapsed to 1, plus 1 node entry = 2
        assert_eq!(app.filtered.len(), 2);
        // The browser group count should be 3
        assert_eq!(app.browser_counts.get(&(100, "Chrome")), Some(&3));
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
        assert_eq!(app.browser_counts.get(&(100, "Chrome")), Some(&1));
    }

    // --- Protocol cycling tests ---

    #[test]
    fn protocol_cycle_full_loop() {
        let mut app = app_with_entries(vec![]);
        // Start: both enabled
        assert!(app.show_tcp && app.show_udp);

        // Step 1: TCP+UDP → TCP only
        app.show_udp = false;
        assert!(app.show_tcp && !app.show_udp);

        // Step 2: TCP only → UDP only
        app.show_tcp = false;
        app.show_udp = true;
        assert!(!app.show_tcp && app.show_udp);

        // Step 3: UDP only → TCP+UDP
        app.show_tcp = true;
        assert!(app.show_tcp && app.show_udp);
    }

    #[test]
    fn protocol_cycle_recovers_from_both_false() {
        let mut app = app_with_entries(vec![]);
        // Simulate invalid state: both disabled
        app.show_tcp = false;
        app.show_udp = false;

        // The Ctrl+T handler treats (false, false) as a recovery: re-enable both
        match (app.show_tcp, app.show_udp) {
            (true, true) => app.show_udp = false,
            (true, false) => {
                app.show_tcp = false;
                app.show_udp = true;
            }
            (false, true) => app.show_tcp = true,
            (false, false) => {
                app.show_tcp = true;
                app.show_udp = true;
            }
        }
        assert!(app.show_tcp && app.show_udp);
    }
}
