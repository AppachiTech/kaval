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
use crate::models::{
    count_browser_groups, matches_filter, sort_entries, EntryKey, PortEntry, SortField,
};
use crate::scanner::{Snapshot, Visibility};

/// How long a status-bar message stays visible.
pub(super) const STATUS_TTL: Duration = Duration::from_secs(3);
/// How long a row that appeared after startup stays marked as new.
pub(super) const NEW_ROW_TTL: Duration = Duration::from_secs(10);

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

/// What a table row stands for, so the selection survives refreshes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RowId {
    Entry(EntryKey),
    /// A collapsed browser group: (pid, service).
    Group(u32, String),
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
    /// How many entries match the filter (before browser groups collapse).
    pub(super) matching: usize,
    /// Identity of the selected row, followed across refreshes and re-sorts.
    selected_id: Option<RowId>,
    pub(super) browser_counts: HashMap<(u32, String), usize>,
    pub(super) table_state: TableState,
    pub(super) filter_text: String,
    pub(super) filter_active: bool,
    pub(super) show_tcp: bool,
    pub(super) show_udp: bool,
    /// Show only listeners reachable from other machines.
    pub(super) exposed_only: bool,
    pub(super) sort_field: SortField,
    pub(super) sort_reverse: bool,
    /// Keys seen in the previous scan; anything else is new.
    known: HashSet<EntryKey>,
    /// When each row that appeared after the first scan was first seen.
    first_seen: HashMap<EntryKey, Instant>,
    /// Browser groups (pid, service) shown expanded.
    expanded: HashSet<(u32, String)>,
    /// Rows marked with Space for a multi-row kill.
    pub(super) marked: HashSet<EntryKey>,
    pub(super) show_detail: bool,
    pub(super) detail_scroll: u16,
    pub(super) confirm: Option<Confirm>,
    pub(super) show_help: bool,
    pub(super) help_scroll: u16,
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
            matching: 0,
            selected_id: None,
            browser_counts: HashMap::new(),
            table_state: TableState::default(),
            filter_text: String::new(),
            filter_active: false,
            show_tcp: true,
            show_udp: true,
            exposed_only: false,
            sort_field: SortField::Port,
            sort_reverse: false,
            known: HashSet::new(),
            first_seen: HashMap::new(),
            expanded: HashSet::new(),
            marked: HashSet::new(),
            show_detail: true,
            detail_scroll: 0,
            confirm: None,
            show_help: false,
            help_scroll: 0,
            loading: true,
            status: None,
            table_area: Rect::default(),
            env: KillEnv::current(),
        }
    }

    /// Replace the rows with a fresh scan. Rows that were not in the previous
    /// scan are marked as new (not on the very first scan); a new TCP listener
    /// reachable from the network is announced.
    pub fn apply_snapshot(&mut self, snapshot: Snapshot, now: Instant) {
        let keys: HashSet<EntryKey> = snapshot.entries.iter().map(PortEntry::key).collect();
        if !self.loading {
            let mut exposed = None;
            for e in snapshot
                .entries
                .iter()
                .filter(|e| !self.known.contains(&e.key()))
            {
                self.first_seen.insert(e.key(), now);
                if e.is_exposed() && e.protocol == crate::models::Protocol::Tcp {
                    exposed = Some(format!(
                        "New listener exposed to the network: {} (PID {}) on {}:{}",
                        e.process_name,
                        e.pid.map_or_else(|| "?".to_string(), |p| p.to_string()),
                        e.bind_display(),
                        e.port
                    ));
                }
            }
            if let Some(text) = exposed {
                self.set_status(text, true);
            }
        }
        self.first_seen.retain(|k, _| keys.contains(k));
        self.marked.retain(|k| keys.contains(k));
        self.known = keys;
        self.entries = snapshot.entries;
        self.visibility = snapshot.visibility;
        self.loading = false;
        self.resort();
    }

    pub(super) fn is_expanded(&self, key: &(u32, String)) -> bool {
        self.expanded.contains(key)
    }

    /// True for rows that appeared less than [`NEW_ROW_TTL`] ago.
    pub(super) fn is_new(&self, key: &EntryKey, now: Instant) -> bool {
        self.first_seen
            .get(key)
            .is_some_and(|seen| now.saturating_duration_since(*seen) < NEW_ROW_TTL)
    }

    pub(super) fn set_status(&mut self, text: impl Into<String>, error: bool) {
        self.status = Some(Status {
            text: text.into(),
            at: Instant::now(),
            error,
        });
    }

    fn resort(&mut self) {
        sort_entries(&mut self.entries, self.sort_field, self.sort_reverse);
        self.apply_filter();
    }

    /// Identity of the row at `index` in `filtered`.
    fn row_id(&self, index: usize) -> Option<RowId> {
        let e = &self.entries[*self.filtered.get(index)?];
        Some(match e.browser_key() {
            Some(key)
                if !self.expanded.contains(&key)
                    && self.browser_counts.get(&key).copied().unwrap_or(0) > 1 =>
            {
                RowId::Group(key.0, key.1)
            }
            _ => RowId::Entry(e.key()),
        })
    }

    pub(super) fn apply_filter(&mut self) {
        let query = self.filter_text.to_lowercase();
        let matching: Vec<usize> = (0..self.entries.len())
            .filter(|&i| matches_filter(&self.entries[i], &query))
            .filter(|&i| !self.exposed_only || self.entries[i].is_exposed())
            .collect();
        self.matching = matching.len();
        // Group only what the filter shows, so a lone match keeps its port.
        let shown: Vec<PortEntry> = matching.iter().map(|&i| self.entries[i].clone()).collect();
        self.browser_counts = count_browser_groups(&shown);
        // Browser rows collapse to the first per (pid, service).
        let mut browser_seen: HashSet<(u32, String)> = HashSet::new();
        self.filtered = matching
            .into_iter()
            .filter(|&i| match self.entries[i].browser_key() {
                Some(key) if !self.expanded.contains(&key) => browser_seen.insert(key),
                _ => true,
            })
            .collect();

        // Follow the selected row; if it is gone, stay at the same position.
        let followed = self
            .selected_id
            .clone()
            .and_then(|id| (0..self.filtered.len()).find(|&i| self.row_id(i) == Some(id.clone())));
        let index = followed.or_else(|| match self.table_state.selected() {
            Some(i) => self.filtered.len().checked_sub(1).map(|last| i.min(last)),
            None => (!self.filtered.is_empty()).then_some(0),
        });
        self.table_state.select(index);
        self.selected_id = index.and_then(|i| self.row_id(i));
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
        self.select_index(new);
    }

    fn select_index(&mut self, index: usize) {
        if index < self.filtered.len() {
            if self.table_state.selected() != Some(index) {
                self.detail_scroll = 0;
            }
            self.table_state.select(Some(index));
            self.selected_id = self.row_id(index);
        }
    }

    /// Rows that fit in the table (borders and header excluded).
    fn page_size(&self) -> i32 {
        i32::from(self.table_area.height.saturating_sub(3)).max(1)
    }

    /// TCP+UDP → TCP → UDP → TCP+UDP
    fn cycle_protocols(&mut self) {
        (self.show_tcp, self.show_udp) = match (self.show_tcp, self.show_udp) {
            (true, true) => (true, false),
            (true, false) => (false, true),
            _ => (true, true),
        };
    }

    /// Expand a collapsed browser group, or collapse the group the row belongs to.
    fn toggle_group(&mut self) {
        let Some(key) = self.selected_entry().and_then(PortEntry::browser_key) else {
            return;
        };
        if !self.expanded.remove(&key) {
            if self.browser_counts.get(&key).copied().unwrap_or(0) < 2 {
                return;
            }
            self.expanded.insert(key);
        }
        self.apply_filter();
    }

    /// Mark (or unmark) the selected row for a multi-row kill and move down.
    fn toggle_mark(&mut self) {
        let Some(key) = self.selected_entry().map(PortEntry::key) else {
            return;
        };
        if !self.marked.remove(&key) {
            self.marked.insert(key);
        }
        self.move_selection(1);
    }

    /// Plan a kill of the marked rows (or the selected one) and ask for
    /// confirmation, or explain why it is refused.
    fn begin_kill(&mut self, signal: Signal) {
        let rows: Vec<&PortEntry> = if self.marked.is_empty() {
            self.selected_entry().into_iter().collect()
        } else {
            self.entries
                .iter()
                .filter(|e| self.marked.contains(&e.key()))
                .collect()
        };
        if rows.is_empty() {
            return;
        }
        let targets = killer::plan(&rows, &KillOptions::default(), &self.env);
        if targets.len() > 1 {
            self.confirm = Some(Confirm { targets, signal });
            return;
        }
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

        // Help overlay: arrows scroll it, any other key dismisses it
        if self.show_help {
            match key.code {
                KeyCode::Down | KeyCode::Char('j') => self.help_scroll += 1,
                KeyCode::Up | KeyCode::Char('k') => {
                    self.help_scroll = self.help_scroll.saturating_sub(1);
                }
                _ => {
                    self.show_help = false;
                    self.help_scroll = 0;
                }
            }
            return vec![];
        }

        // Kill confirmation takes priority: `y` confirms, anything else cancels.
        if let Some(confirm) = self.confirm.take() {
            if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                self.marked.clear();
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
                KeyCode::Up => self.move_selection(-1),
                KeyCode::Down => self.move_selection(1),
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
            // Esc clears an applied filter, then marks; it never quits.
            KeyCode::Esc => {
                if !self.filter_text.is_empty() {
                    self.filter_text.clear();
                    self.apply_filter();
                } else {
                    self.marked.clear();
                }
            }
            KeyCode::Enter => self.toggle_group(),
            KeyCode::Char(' ') => self.toggle_mark(),
            KeyCode::Char('e') => {
                self.exposed_only = !self.exposed_only;
                self.apply_filter();
            }
            KeyCode::Char('s') => {
                self.sort_field = self.sort_field.next();
                self.resort();
            }
            KeyCode::Char('S') => {
                self.sort_reverse = !self.sort_reverse;
                self.resort();
            }
            KeyCode::Char('t') => {
                self.cycle_protocols();
                return vec![Effect::Refresh];
            }
            KeyCode::Char('d') => self.show_detail = !self.show_detail,
            KeyCode::Char('r') => {
                self.set_status("Refreshed", false);
                return vec![Effect::Refresh];
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::PageUp => self.move_selection(-self.page_size()),
            KeyCode::PageDown => self.move_selection(self.page_size()),
            KeyCode::Char('[') | KeyCode::Left => {
                self.detail_scroll = self.detail_scroll.saturating_sub(3);
            }
            KeyCode::Char(']') | KeyCode::Right => {
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
    use std::time::Duration;

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

    // --- Task 14: selection identity, Esc, paging, filter-mode arrows ---

    #[test]
    fn selection_follows_identity_across_refresh() {
        let entries = vec![
            browser(5001, 100, "Chrome"),
            browser(5002, 100, "Chrome"),
            browser(5003, 100, "Chrome"),
            make_entry(3000, "node", None, ServiceCategory::DevServer, 0.0, 1.0),
            make_entry(5432, "postgres", None, ServiceCategory::Database, 0.0, 1.0),
        ];
        let mut app = app_with(entries.clone());
        app.handle_key(key(KeyCode::Down)); // 3000 node → Chrome group
        assert_eq!(app.selected_entry().unwrap().pid, Some(100));
        let mut next = entries;
        next.push(make_entry(
            2000,
            "ruby",
            None,
            ServiceCategory::Unknown,
            0.0,
            1.0,
        ));
        app.apply_snapshot(
            Snapshot {
                entries: next,
                visibility: Visibility::Complete,
            },
            Instant::now(),
        );
        assert_eq!(
            app.selected_entry().unwrap().pid,
            Some(100),
            "selection stays on Chrome when a row appears above it"
        );
    }

    #[test]
    fn selection_stays_in_place_when_its_row_disappears() {
        let mut app = sample();
        app.handle_key(key(KeyCode::Down)); // postgres
        let remaining = vec![
            make_entry(3000, "node", None, ServiceCategory::DevServer, 0.0, 1.0),
            make_entry(6379, "redis", None, ServiceCategory::Cache, 0.0, 1.0),
        ];
        app.apply_snapshot(
            Snapshot {
                entries: remaining,
                visibility: Visibility::Complete,
            },
            Instant::now(),
        );
        assert_eq!(app.table_state.selected(), Some(1));
        assert_eq!(app.selected_entry().unwrap().port, 6379);
    }

    #[test]
    fn esc_clears_filter_then_noop() {
        let mut app = sample();
        app.handle_key(key(KeyCode::Char('/')));
        for c in "red".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        app.handle_key(key(KeyCode::Enter));
        assert!(!app.filter_active);
        assert_eq!(app.filtered.len(), 1, "filter kept after Enter");
        assert_eq!(app.handle_key(key(KeyCode::Esc)), vec![]);
        assert!(app.filter_text.is_empty());
        assert_eq!(app.filtered.len(), 3, "Esc cleared the filter");
        assert_eq!(
            app.handle_key(key(KeyCode::Esc)),
            vec![],
            "second Esc does nothing"
        );
    }

    #[test]
    fn pgdn_pages_table() {
        let entries = (0..30)
            .map(|i| make_entry(4000 + i, "svc", None, ServiceCategory::Unknown, 0.0, 1.0))
            .collect();
        let mut app = app_with(entries);
        app.table_area = Rect::new(0, 0, 80, 13); // 10 visible rows
        app.handle_key(key(KeyCode::PageDown));
        assert_eq!(app.table_state.selected(), Some(10));
        app.handle_key(key(KeyCode::PageDown));
        app.handle_key(key(KeyCode::PageDown));
        assert_eq!(app.table_state.selected(), Some(29), "clamped at the end");
        app.handle_key(key(KeyCode::PageUp));
        assert_eq!(app.table_state.selected(), Some(19));
    }

    #[test]
    fn arrows_navigate_in_filter_mode() {
        let mut app = sample();
        app.handle_key(key(KeyCode::Char('/')));
        app.handle_key(key(KeyCode::Down));
        assert!(app.filter_active, "still filtering");
        assert_eq!(app.table_state.selected(), Some(1));
        app.handle_key(key(KeyCode::Up));
        assert_eq!(app.table_state.selected(), Some(0));
    }

    #[test]
    fn brackets_scroll_the_detail_pane() {
        let mut app = sample();
        app.handle_key(key(KeyCode::Char(']')));
        assert_eq!(app.detail_scroll, 3);
        app.handle_key(key(KeyCode::Char('[')));
        assert_eq!(app.detail_scroll, 0);
    }

    // --- Task 15a: exposure, new rows, groups, multi-select, sort, aliases ---

    fn exposed(port: u16, pid: u32, name: &str) -> PortEntry {
        let mut e = entry(port, pid, name);
        e.addrs = vec![std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)];
        e
    }

    fn snapshot(entries: Vec<PortEntry>) -> Snapshot {
        Snapshot {
            entries,
            visibility: Visibility::Complete,
        }
    }

    #[test]
    fn exposed_toggle_filters_rows() {
        let mut app = app_with(vec![entry(3000, 1, "node"), exposed(8080, 2, "caddy")]);
        app.handle_key(key(KeyCode::Char('e')));
        assert_eq!(shown_ports(&app), vec![8080]);
        app.handle_key(key(KeyCode::Char('e')));
        assert_eq!(shown_ports(&app), vec![3000, 8080]);
    }

    #[test]
    fn new_rows_are_marked_for_ten_seconds() {
        let t0 = Instant::now();
        let mut app = App::new();
        app.apply_snapshot(snapshot(vec![entry(3000, 1, "node")]), t0);
        let old_key = app.entries[0].key();
        assert!(!app.is_new(&old_key, t0), "the first scan marks nothing");
        app.apply_snapshot(
            snapshot(vec![entry(3000, 1, "node"), exposed(9000, 2, "python")]),
            t0 + Duration::from_secs(2),
        );
        let new_key = (crate::models::Protocol::Tcp, 9000, Some(2));
        assert!(app.is_new(&new_key, t0 + Duration::from_secs(3)));
        assert!(!app.is_new(&old_key, t0 + Duration::from_secs(3)));
        assert!(
            !app.is_new(&new_key, t0 + Duration::from_secs(13)),
            "mark expires"
        );
        let status = app.status.as_ref().expect("new exposed listener announced");
        assert!(
            status.error && status.text.contains("exposed"),
            "{}",
            status.text
        );
    }

    #[test]
    fn enter_expands_browser_group() {
        let mut app = app_with(vec![
            browser(5001, 100, "Chrome"),
            browser(5002, 100, "Chrome"),
            browser(5003, 100, "Chrome"),
            entry(9999, 7, "node"),
        ]);
        assert_eq!(app.filtered.len(), 2);
        app.handle_key(key(KeyCode::Enter)); // first row is the Chrome group
        assert_eq!(shown_ports(&app), vec![5001, 5002, 5003, 9999]);
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(
            app.filtered.len(),
            2,
            "Enter on a member collapses the group again"
        );
    }

    #[test]
    fn space_marks_and_kill_targets_marked() {
        let mut app = sample();
        app.handle_key(key(KeyCode::Char(' '))); // mark 3000, move down
        app.handle_key(key(KeyCode::Char(' '))); // mark 5432
        assert_eq!(app.marked.len(), 2);
        app.handle_key(ctrl('x'));
        match app.handle_key(key(KeyCode::Char('y'))).as_slice() {
            [Effect::Kill { targets, .. }] => {
                let pids: Vec<Option<u32>> = targets.iter().map(|t| t.pid).collect();
                assert_eq!(pids, vec![Some(3000), Some(5432)]);
            }
            other => panic!("expected a kill, got {other:?}"),
        }
        assert!(app.marked.is_empty(), "marks clear after the kill is sent");
    }

    #[test]
    fn esc_clears_marks_when_no_filter() {
        let mut app = sample();
        app.handle_key(key(KeyCode::Char(' ')));
        app.handle_key(key(KeyCode::Esc));
        assert!(app.marked.is_empty());
    }

    #[test]
    fn reverse_sort_with_shift_s() {
        let mut app = sample();
        app.handle_key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT));
        assert!(app.sort_reverse);
        assert_eq!(shown_ports(&app), vec![6379, 5432, 3000]);
    }

    #[test]
    fn plain_letter_aliases() {
        let mut app = sample();
        app.handle_key(key(KeyCode::Char('s')));
        assert_eq!(app.sort_field, SortField::ProcessName);
        assert_eq!(
            app.handle_key(key(KeyCode::Char('t'))),
            vec![Effect::Refresh]
        );
        assert!(!app.show_udp);
        let shown = app.show_detail;
        app.handle_key(key(KeyCode::Char('d')));
        assert_ne!(app.show_detail, shown);
        assert_eq!(
            app.handle_key(key(KeyCode::Char('r'))),
            vec![Effect::Refresh]
        );
        assert!(app.confirm.is_none(), "no plain-letter kill");
        app.handle_key(key(KeyCode::Char('x')));
        assert!(app.confirm.is_none(), "x alone never kills");
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
