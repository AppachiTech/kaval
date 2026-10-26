//! TUI state and input handling. Input produces [`Effect`]s that the event
//! loop carries out, so key handling is testable without a terminal.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use ratatui::widgets::TableState;

use crate::killer::{self, Action, KillEnv, KillOptions, KillTarget, Signal};
use crate::models::{count_browser_groups, matches_filter, sort_entries, PortEntry, SortField};
use crate::scanner::{Snapshot, Visibility};

/// How long a status-bar message stays visible.
pub(super) const STATUS_TTL: Duration = Duration::from_secs(3);

/// Something the event loop should do in response to input.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Quit,
    Refresh,
    Copy {
        text: String,
        what: &'static str,
    },
    Open(String),
    Kill {
        targets: Vec<KillTarget>,
        signal: Signal,
    },
    #[allow(dead_code)] // TODO(v0.2 Task 15): `p` health probe
    Probe {
        ip: IpAddr,
        port: u16,
        http: bool,
    },
    Suspend,
}

/// A kill waiting for the user's confirmation.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Confirm {
    pub targets: Vec<KillTarget>,
    pub signal: Signal,
}

pub(super) struct Status {
    pub text: String,
    pub at: Instant,
    pub error: bool,
}

pub struct App {
    pub(super) entries: Vec<PortEntry>,
    pub(super) visibility: Visibility,
    /// Indices into `entries` that are shown (filtered, browser groups collapsed).
    pub(super) filtered: Vec<usize>,
    pub(super) browser_counts: HashMap<(u32, String), usize>,
    pub(super) table_state: TableState,
    pub(super) filter_text: String,
    pub(super) filter_active: bool,
    pub(super) show_tcp: bool,
    pub(super) show_udp: bool,
    pub(super) sort_field: SortField,
    pub(super) show_detail: bool,
    pub(super) detail_scroll: u16,
    pub(super) confirm: Option<Confirm>,
    pub(super) show_help: bool,
    pub(super) loading: bool,
    pub(super) status: Option<Status>,
    /// Where the table was last drawn (for mouse clicks).
    pub(super) table_area: Rect,
    env: KillEnv,
}

impl App {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            visibility: Visibility::Complete,
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
            confirm: None,
            show_help: false,
            loading: true,
            status: None,
            table_area: Rect::default(),
            env: KillEnv::current(),
        }
    }

    /// Replace the rows with a fresh scan.
    pub fn apply_snapshot(&mut self, snapshot: Snapshot, _now: Instant) {
        self.entries = snapshot.entries;
        self.visibility = snapshot.visibility;
        self.loading = false;
        self.resort();
    }

    pub(super) fn set_status(&mut self, text: impl Into<String>, error: bool) {
        self.status = Some(Status {
            text: text.into(),
            at: Instant::now(),
            error,
        });
    }

    fn resort(&mut self) {
        sort_entries(&mut self.entries, self.sort_field, false);
        self.browser_counts = count_browser_groups(&self.entries);
        self.apply_filter();
    }

    pub(super) fn apply_filter(&mut self) {
        let query = self.filter_text.to_lowercase();
        // Browser rows collapse to the first per (pid, service).
        let mut browser_seen: HashSet<(u32, String)> = HashSet::new();
        self.filtered = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                if !matches_filter(e, &query) {
                    return false;
                }
                match e.browser_key() {
                    Some(key) => browser_seen.insert(key),
                    None => true,
                }
            })
            .map(|(i, _)| i)
            .collect();

        // Keep selection in bounds
        match self.table_state.selected() {
            Some(selected) if selected >= self.filtered.len() => {
                self.table_state.select(self.filtered.len().checked_sub(1));
            }
            None if !self.filtered.is_empty() => self.table_state.select(Some(0)),
            _ => {}
        }
    }

    pub(super) fn selected_entry(&self) -> Option<&PortEntry> {
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

    fn select_index(&mut self, index: usize) {
        if index < self.filtered.len() {
            if self.table_state.selected() != Some(index) {
                self.detail_scroll = 0;
            }
            self.table_state.select(Some(index));
        }
    }

    /// TCP+UDP → TCP → UDP → TCP+UDP
    fn cycle_protocols(&mut self) {
        (self.show_tcp, self.show_udp) = match (self.show_tcp, self.show_udp) {
            (true, true) => (true, false),
            (true, false) => (false, true),
            _ => (true, true),
        };
    }

    /// Plan a kill of the selected row and ask for confirmation, or explain
    /// why it is refused.
    fn begin_kill(&mut self, signal: Signal) {
        let Some(entry) = self.selected_entry() else {
            return;
        };
        let targets = killer::plan(&[entry], &KillOptions::default(), &self.env);
        match targets.first().map(|t| &t.action) {
            Some(Action::Refuse(reason)) => {
                let reason = reason.clone();
                self.set_status(format!("Not killing: {reason}"), true);
            }
            Some(_) => self.confirm = Some(Confirm { targets, signal }),
            None => {}
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        // Terminals that report key releases (or Windows) send two events per press.
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return vec![];
        }

        // Help overlay: any key dismisses it
        if self.show_help {
            self.show_help = false;
            return vec![];
        }

        // Kill confirmation takes priority: `y` confirms, anything else cancels.
        if let Some(confirm) = self.confirm.take() {
            if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                return vec![Effect::Kill {
                    targets: confirm.targets,
                    signal: confirm.signal,
                }];
            }
            self.set_status("Kill cancelled", false);
            return vec![];
        }

        // Ctrl+key shortcuts work in every mode, including while filtering.
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') | KeyCode::Char('q') => return vec![Effect::Quit],
                KeyCode::Char('z') => return vec![Effect::Suspend],
                KeyCode::Char('x') => self.begin_kill(Signal::Term),
                KeyCode::Char('k') => self.begin_kill(Signal::Kill),
                KeyCode::Char('d') => self.show_detail = !self.show_detail,
                KeyCode::Char('s') => {
                    self.sort_field = self.sort_field.next();
                    self.resort();
                }
                KeyCode::Char('t') => {
                    self.cycle_protocols();
                    return vec![Effect::Refresh];
                }
                KeyCode::Char('r') => {
                    self.set_status("Refreshed", false);
                    return vec![Effect::Refresh];
                }
                _ => {}
            }
            return vec![];
        }

        if self.filter_active {
            match key.code {
                KeyCode::Esc => {
                    self.filter_active = false;
                    self.filter_text.clear();
                    self.apply_filter();
                }
                KeyCode::Enter => self.filter_active = false,
                KeyCode::Backspace => {
                    self.filter_text.pop();
                    self.apply_filter();
                }
                KeyCode::Char(c) => {
                    self.filter_text.push(c);
                    self.apply_filter();
                }
                _ => {}
            }
            return vec![];
        }

        match key.code {
            KeyCode::Char('q') => return vec![Effect::Quit],
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::PageUp | KeyCode::Left => {
                self.detail_scroll = self.detail_scroll.saturating_sub(3);
            }
            KeyCode::PageDown | KeyCode::Right => {
                self.detail_scroll = self.detail_scroll.saturating_add(3);
            }
            KeyCode::Home => self.select_index(0),
            KeyCode::End => {
                if let Some(last) = self.filtered.len().checked_sub(1) {
                    self.select_index(last);
                }
            }
            KeyCode::Char('/') => self.filter_active = true,
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char('o') => {
                if let Some(e) = self.selected_entry() {
                    return vec![Effect::Open(format!("http://localhost:{}", e.port))];
                }
            }
            KeyCode::Char('y') => {
                if let Some(e) = self.selected_entry() {
                    return vec![Effect::Copy {
                        text: e.port.to_string(),
                        what: "port",
                    }];
                }
            }
            _ => {}
        }
        vec![]
    }

    /// Wheel scrolls the selection; a left click selects the row under the pointer.
    pub fn handle_mouse(&mut self, m: MouseEvent, table_area: Rect) -> Vec<Effect> {
        match m.kind {
            MouseEventKind::ScrollDown => self.move_selection(1),
            MouseEventKind::ScrollUp => self.move_selection(-1),
            MouseEventKind::Down(MouseButton::Left) => {
                // Rows start below the border and the header line.
                let first_row = table_area.y + 2;
                if table_area.contains(Position::new(m.column, m.row)) && m.row >= first_row {
                    let index = self.table_state.offset() + usize::from(m.row - first_row);
                    self.select_index(index);
                }
            }
            _ => {}
        }
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::testutil::{entry, with_service};
    use crate::models::{ServiceCategory, ServiceSource};
    use ratatui::crossterm::event::{KeyEventState, MouseEvent};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn make_entry(
        port: u16,
        name: &str,
        service: Option<&'static str>,
        category: ServiceCategory,
        cpu: f32,
        memory: f64,
    ) -> PortEntry {
        let mut e = entry(port, port as u32, name);
        e.cpu_percent = cpu;
        e.memory_mb = memory;
        e.category = category;
        match service {
            Some(svc) => with_service(e, svc, category, ServiceSource::Process),
            None => e,
        }
    }

    fn browser(port: u16, pid: u32, service: &'static str) -> PortEntry {
        with_service(
            entry(port, pid, "Google Chrome Helper"),
            service,
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

    fn sample() -> App {
        app_with(vec![
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
            make_entry(6379, "redis", None, ServiceCategory::Cache, 0.0, 50.0),
        ])
    }

    fn set_filter(app: &mut App, text: &str) {
        app.filter_text = text.to_string();
        app.apply_filter();
    }

    fn shown_ports(app: &App) -> Vec<u16> {
        app.filtered.iter().map(|&i| app.entries[i].port).collect()
    }

    // --- quitting & key kinds ---

    #[test]
    fn ctrl_c_and_q_quit() {
        let mut app = sample();
        assert_eq!(app.handle_key(ctrl('c')), vec![Effect::Quit]);
        assert_eq!(app.handle_key(ctrl('q')), vec![Effect::Quit]);
        assert_eq!(app.handle_key(key(KeyCode::Char('q'))), vec![Effect::Quit]);
    }

    #[test]
    fn esc_does_not_quit() {
        let mut app = sample();
        assert!(!app.handle_key(key(KeyCode::Esc)).contains(&Effect::Quit));
    }

    #[test]
    fn q_while_filtering_is_text() {
        let mut app = sample();
        app.handle_key(key(KeyCode::Char('/')));
        assert_eq!(app.handle_key(key(KeyCode::Char('q'))), vec![]);
        assert_eq!(app.filter_text, "q");
    }

    #[test]
    fn release_events_are_ignored() {
        let mut app = sample();
        let mut release = key(KeyCode::Char('q'));
        release.kind = KeyEventKind::Release;
        release.state = KeyEventState::NONE;
        assert_eq!(app.handle_key(release), vec![]);
        let mut down = key(KeyCode::Down);
        down.kind = KeyEventKind::Release;
        app.handle_key(down);
        assert_eq!(app.table_state.selected(), Some(0), "release does not move");
    }

    #[test]
    fn ctrl_z_suspends() {
        assert_eq!(sample().handle_key(ctrl('z')), vec![Effect::Suspend]);
    }

    // --- filtering ---

    #[test]
    fn filter_empty_shows_all() {
        assert_eq!(sample().filtered.len(), 3);
    }

    #[test]
    fn filter_by_port_number() {
        let mut app = sample();
        set_filter(&mut app, "5432");
        assert_eq!(shown_ports(&app), vec![5432]);
    }

    #[test]
    fn filter_by_partial_port_is_exact_match() {
        let mut app = app_with(vec![
            make_entry(300, "node", None, ServiceCategory::DevServer, 0.0, 1.0),
            make_entry(3000, "node", None, ServiceCategory::DevServer, 0.0, 1.0),
            make_entry(3001, "node", None, ServiceCategory::DevServer, 0.0, 1.0),
        ]);
        set_filter(&mut app, "300");
        assert_eq!(shown_ports(&app), vec![300]);
    }

    #[test]
    fn filter_by_process_name_case_insensitive() {
        let mut app = sample();
        set_filter(&mut app, "POST");
        assert_eq!(shown_ports(&app), vec![5432]);
    }

    #[test]
    fn filter_by_service_name() {
        let mut app = sample();
        set_filter(&mut app, "next");
        assert_eq!(shown_ports(&app), vec![3000]);
    }

    #[test]
    fn filter_no_match() {
        let mut app = sample();
        set_filter(&mut app, "zzzzz");
        assert!(app.filtered.is_empty());
    }

    #[test]
    fn filter_selection_stays_in_bounds() {
        let mut app = sample();
        app.table_state.select(Some(2));
        set_filter(&mut app, "node");
        assert_eq!(app.filtered.len(), 1);
        assert!(app.table_state.selected().unwrap() < app.filtered.len());
    }

    #[test]
    fn typing_in_filter_mode_filters_live() {
        let mut app = sample();
        app.handle_key(key(KeyCode::Char('/')));
        for c in "redis".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(shown_ports(&app), vec![6379]);
        app.handle_key(key(KeyCode::Esc));
        assert!(!app.filter_active);
        assert_eq!(app.filtered.len(), 3, "Esc clears the filter");
    }

    // --- sorting ---

    #[test]
    fn sort_by_port_ascending() {
        let app = app_with(vec![
            make_entry(8080, "nginx", None, ServiceCategory::System, 0.0, 1.0),
            make_entry(3000, "node", None, ServiceCategory::DevServer, 0.0, 1.0),
            make_entry(5432, "postgres", None, ServiceCategory::Database, 0.0, 1.0),
        ]);
        assert_eq!(shown_ports(&app), vec![3000, 5432, 8080]);
    }

    #[test]
    fn ctrl_s_cycles_sort_and_reorders() {
        let mut app = app_with(vec![
            make_entry(3000, "node", None, ServiceCategory::DevServer, 10.0, 100.0),
            make_entry(
                5432,
                "postgres",
                None,
                ServiceCategory::Database,
                80.0,
                500.0,
            ),
            make_entry(6379, "redis", None, ServiceCategory::Cache, 5.0, 50.0),
        ]);
        app.handle_key(ctrl('s')); // Name
        app.handle_key(ctrl('s')); // CPU
        assert_eq!(app.sort_field, SortField::Cpu);
        assert_eq!(shown_ports(&app), vec![5432, 3000, 6379]);
        app.handle_key(ctrl('s')); // Mem
        assert_eq!(shown_ports(&app), vec![5432, 3000, 6379]);
    }

    // --- selection & navigation ---

    #[test]
    fn move_selection_clamps() {
        let mut app = sample();
        app.handle_key(key(KeyCode::Up));
        assert_eq!(app.table_state.selected(), Some(0));
        app.handle_key(key(KeyCode::End));
        app.handle_key(key(KeyCode::Down));
        assert_eq!(app.table_state.selected(), Some(2));
    }

    #[test]
    fn move_selection_empty_list() {
        let mut app = app_with(vec![]);
        app.handle_key(key(KeyCode::Down));
        assert_eq!(app.table_state.selected(), None);
    }

    #[test]
    fn mouse_click_selects_row_and_wheel_scrolls() {
        let mut app = sample();
        let area = Rect::new(0, 3, 80, 10);
        let click = |row| MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row,
            modifiers: KeyModifiers::NONE,
        };
        app.handle_mouse(click(3 + 2 + 2), area); // third row
        assert_eq!(app.table_state.selected(), Some(2));
        app.handle_mouse(click(3), area); // border: ignored
        assert_eq!(app.table_state.selected(), Some(2));
        let wheel_up = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        };
        app.handle_mouse(wheel_up, area);
        assert_eq!(app.table_state.selected(), Some(1));
    }

    // --- protocol filter ---

    #[test]
    fn ctrl_t_cycles_protocols_and_refreshes() {
        let mut app = sample();
        let mut seen = Vec::new();
        for _ in 0..3 {
            assert_eq!(app.handle_key(ctrl('t')), vec![Effect::Refresh]);
            seen.push((app.show_tcp, app.show_udp));
        }
        assert_eq!(seen, vec![(true, false), (false, true), (true, true)]);
    }

    // --- effects ---

    #[test]
    fn y_copies_port_and_o_opens() {
        let mut app = sample();
        assert_eq!(
            app.handle_key(key(KeyCode::Char('y'))),
            vec![Effect::Copy {
                text: "3000".into(),
                what: "port"
            }]
        );
        assert_eq!(
            app.handle_key(key(KeyCode::Char('o'))),
            vec![Effect::Open("http://localhost:3000".into())]
        );
    }

    #[test]
    fn kill_needs_confirmation() {
        let mut app = sample();
        assert_eq!(app.handle_key(ctrl('x')), vec![]);
        assert!(app.confirm.is_some());
        match app.handle_key(key(KeyCode::Char('y'))).as_slice() {
            [Effect::Kill { targets, signal }] => {
                assert_eq!(*signal, Signal::Term);
                assert_eq!(targets[0].pid, Some(3000));
            }
            other => panic!("expected a kill, got {other:?}"),
        }
        app.handle_key(ctrl('k'));
        assert_eq!(
            app.handle_key(key(KeyCode::Char('n'))),
            vec![],
            "any other key cancels"
        );
        assert!(app.confirm.is_none());
    }

    #[test]
    fn protected_rows_are_refused_with_a_reason() {
        let mut app = app_with(vec![entry(6333, 21743, "com.docker.backend")]);
        app.handle_key(ctrl('x'));
        assert!(app.confirm.is_none());
        let status = app.status.as_ref().expect("status message");
        assert!(
            status.error && status.text.contains("container"),
            "{}",
            status.text
        );
    }

    // --- browser grouping ---

    #[test]
    fn browser_entries_grouped_by_pid_and_service() {
        let app = app_with(vec![
            browser(5000, 100, "Chrome"),
            browser(5001, 100, "Chrome"),
            browser(5002, 100, "Chrome"),
            make_entry(
                3000,
                "node",
                Some("Next.js"),
                ServiceCategory::DevServer,
                0.0,
                1.0,
            ),
        ]);
        assert_eq!(app.filtered.len(), 2);
        assert_eq!(
            app.browser_counts.get(&(100, "Chrome".to_string())),
            Some(&3)
        );
    }

    #[test]
    fn different_browser_pids_and_services_not_grouped() {
        let app = app_with(vec![
            browser(5000, 100, "Chrome"),
            browser(5001, 200, "Chrome"),
            browser(5002, 100, "Brave"),
        ]);
        assert_eq!(app.filtered.len(), 3);
    }
}
