use std::collections::{HashMap, HashSet};
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use clap::ValueEnum;

/// CPU% thresholds shared between CLI output and TUI rendering
pub const CPU_WARN_THRESHOLD: f32 = 20.0;
pub const CPU_ERROR_THRESHOLD: f32 = 50.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Protocol {
    Tcp,
    Udp,
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Protocol::Tcp => write!(f, "TCP"),
            Protocol::Udp => write!(f, "UDP"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceCategory {
    DevServer,
    Database,
    Cache,
    Queue,
    Container,
    Browser,
    System,
    Unknown,
}

/// How a service name was determined. Port-only matches are guesses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceSource {
    Process,
    Command,
    Port,
    Container,
}

/// A recognised service (database, dev server, container, ...).
#[derive(Debug, Clone, PartialEq)]
pub struct Service {
    pub name: String,
    pub category: ServiceCategory,
    pub source: ServiceSource,
    /// Extra context shown in details (e.g. how to free a macOS system port).
    pub note: Option<&'static str>,
}

/// Who can reach a listener, derived from its bind addresses.
#[allow(dead_code)] // TODO(v0.2 Task 11a): BIND column / --exposed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exposure {
    /// Bound to loopback only (127.0.0.1 / ::1).
    Loopback,
    /// Bound to a specific non-loopback address (e.g. a LAN IP).
    Network(IpAddr),
    /// Bound to all interfaces (0.0.0.0 / ::).
    AllInterfaces,
}

/// A process referenced from a row (e.g. the listener's parent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRef {
    pub pid: u32,
    pub name: String,
}

/// The container that publishes a port through a container runtime's proxy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub compose_project: Option<String>,
}

/// A peer with an established TCP connection to a listener.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Client {
    pub addr: SocketAddr,
    pub pid: Option<u32>,
    pub process: Option<String>,
}

/// Identity of a row across refreshes: protocol, port and owning PID.
#[allow(dead_code)] // TODO(v0.2 Task 14): selection identity
pub type EntryKey = (Protocol, u16, Option<u32>);

/// One listening socket owner: a (protocol, port, pid) with all its bind addresses.
#[derive(Debug, Clone)]
pub struct PortEntry {
    pub protocol: Protocol,
    pub port: u16,
    /// Every local address this owner listens on for the port (sorted).
    pub addrs: Vec<IpAddr>,
    /// `None` when the socket's owner is not visible (another user's process).
    pub pid: Option<u32>,
    pub process_name: String,
    pub process_cmd: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub project: Option<String>,
    pub user: Option<String>,
    pub parent: Option<ProcessRef>,
    /// Process start time (seconds since the epoch), used to detect PID reuse.
    pub start_time: u64,
    pub cpu_percent: f32,
    pub memory_mb: f64,
    pub uptime: Duration,
    pub service: Option<Service>,
    pub category: ServiceCategory,
    pub container: Option<ContainerInfo>,
    #[allow(dead_code)] // TODO(v0.2 Task 15): connections in the detail pane
    pub clients: Vec<Client>,
}

/// Format memory (in MB) as human-readable string
pub fn format_memory(memory_mb: f64) -> String {
    if memory_mb >= 1024.0 {
        format!("{:.1} GB", memory_mb / 1024.0)
    } else {
        format!("{:.0} MB", memory_mb)
    }
}

/// Format uptime duration as human-readable string
pub fn format_uptime(uptime: Duration) -> String {
    let secs = uptime.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d {}h", secs / 86400, (secs % 86400) / 3600)
    }
}

impl PortEntry {
    /// Identity used to keep selections and highlights stable across refreshes.
    #[allow(dead_code)] // TODO(v0.2 Task 14)
    pub fn key(&self) -> EntryKey {
        (self.protocol, self.port, self.pid)
    }

    pub fn service_name(&self) -> Option<&str> {
        self.service.as_ref().map(|s| s.name.as_str())
    }

    /// Service name for display; port-only guesses get a trailing `?`.
    #[allow(dead_code)] // TODO(v0.2 Task 11a)
    pub fn service_display(&self) -> String {
        match &self.service {
            Some(s) if s.source == ServiceSource::Port => format!("{}?", s.name),
            Some(s) => s.name.clone(),
            None => String::new(),
        }
    }

    /// The widest exposure among the bind addresses.
    #[allow(dead_code)] // TODO(v0.2 Task 11a)
    pub fn exposure(&self) -> Exposure {
        let mut exposure = Exposure::Loopback;
        for addr in &self.addrs {
            let addr = addr.to_canonical();
            if addr.is_unspecified() {
                return Exposure::AllInterfaces;
            }
            if !addr.is_loopback() && exposure == Exposure::Loopback {
                exposure = Exposure::Network(addr);
            }
        }
        exposure
    }

    /// True when the listener is reachable from other machines.
    #[allow(dead_code)] // TODO(v0.2 Task 11a)
    pub fn is_exposed(&self) -> bool {
        self.exposure() != Exposure::Loopback
    }

    /// Compact bind column: `localhost`, `*` (all interfaces) or the address.
    #[allow(dead_code)] // TODO(v0.2 Task 11a)
    pub fn bind_display(&self) -> String {
        match self.exposure() {
            Exposure::Loopback => "localhost".to_string(),
            Exposure::AllInterfaces => "*".to_string(),
            Exposure::Network(ip) => ip.to_string(),
        }
    }

    /// Every bind address with its port; IPv6 is bracketed (`[::1]:5432`).
    pub fn addr_display(&self) -> String {
        self.addrs
            .iter()
            .map(|ip| SocketAddr::new(*ip, self.port).to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Format memory as human-readable string
    pub fn memory_display(&self) -> String {
        format_memory(self.memory_mb)
    }

    /// Format uptime as human-readable string
    pub fn uptime_display(&self) -> String {
        format_uptime(self.uptime)
    }

    /// Format command line with shell-style quoting for arguments containing spaces
    pub fn cmd_display(&self) -> String {
        shell_join(&self.process_cmd)
    }

    /// `(pid, service)` for browser rows that can be grouped.
    pub fn browser_key(&self) -> Option<(u32, String)> {
        if self.category != ServiceCategory::Browser {
            return None;
        }
        Some((self.pid?, self.service_name()?.to_string()))
    }
}

/// Join command-line arguments with shell-style quoting.
/// Arguments containing spaces or double quotes are wrapped in double quotes,
/// with inner quotes escaped.
pub fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if a.contains(' ') || a.contains('"') {
                format!("\"{}\"", a.replace('"', "\\\""))
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SortField {
    /// Sort by port number (default)
    #[value(name = "port")]
    Port,
    /// Sort by process name
    #[value(name = "name")]
    ProcessName,
    /// Sort by CPU usage (descending)
    #[value(name = "cpu")]
    Cpu,
    /// Sort by memory usage (descending)
    #[value(name = "mem")]
    Memory,
}

impl SortField {
    pub fn next(self) -> Self {
        match self {
            SortField::Port => SortField::ProcessName,
            SortField::ProcessName => SortField::Cpu,
            SortField::Cpu => SortField::Memory,
            SortField::Memory => SortField::Port,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortField::Port => "Port",
            SortField::ProcessName => "Name",
            SortField::Cpu => "CPU",
            SortField::Memory => "Mem",
        }
    }
}

/// Build a map of `(pid, service) → port count` for browser entries.
///
/// Browser processes (Chrome, Firefox, Brave, Safari) typically open dozens of
/// ephemeral ports. This function groups them by `(pid, service)` so the UI can
/// collapse all ports for one browser instance into a single row.
///
/// Used by both the CLI renderer (`output.rs`) and the TUI (`ui.rs`) to avoid
/// duplicating the same logic.
pub fn count_browser_groups(entries: &[PortEntry]) -> HashMap<(u32, String), usize> {
    let mut counts: HashMap<(u32, String), usize> = HashMap::new();
    for key in entries.iter().filter_map(PortEntry::browser_key) {
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
}

/// Build the deduplicated list of entries for display, keeping only the first
/// browser entry per `(pid, service)` group.
///
/// Returns indices into `entries` that should be shown. Pass the result of
/// `count_browser_groups` as `counts` to avoid recomputing it.
pub fn dedup_browser_indices(
    entries: &[PortEntry],
    counts: &HashMap<(u32, String), usize>,
) -> Vec<usize> {
    let mut seen: HashSet<(u32, String)> = HashSet::new();
    entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e.browser_key() {
            Some(key) if counts.get(&key).copied().unwrap_or(0) > 1 => {
                seen.insert(key).then_some(i)
            }
            _ => Some(i),
        })
        .collect()
}

/// Sort entries in-place by `field` (`reverse` flips the primary order).
/// Ties fall back to port, protocol and PID so rows keep a stable order
/// across refreshes. Shared by the CLI and the TUI.
pub fn sort_entries(entries: &mut [PortEntry], field: SortField, reverse: bool) {
    use std::cmp::Ordering;
    entries.sort_by(|a, b| {
        let primary = match field {
            SortField::Port => a.port.cmp(&b.port),
            SortField::ProcessName => a
                .process_name
                .to_lowercase()
                .cmp(&b.process_name.to_lowercase()),
            SortField::Cpu => b
                .cpu_percent
                .partial_cmp(&a.cpu_percent)
                .unwrap_or(Ordering::Equal),
            SortField::Memory => b
                .memory_mb
                .partial_cmp(&a.memory_mb)
                .unwrap_or(Ordering::Equal),
        };
        let primary = if reverse { primary.reverse() } else { primary };
        primary
            .then_with(|| a.port.cmp(&b.port))
            .then_with(|| a.protocol.cmp(&b.protocol))
            .then_with(|| a.pid.cmp(&b.pid))
    })
}

/// Match an entry against a lowercase filter query.
///
/// Numeric queries match the port or PID exactly (`80` never matches 8080).
/// Other queries are case-insensitive substrings of the process name, service,
/// command line, project, container name/image or user.
pub fn matches_filter(e: &PortEntry, q: &str) -> bool {
    if q.is_empty() {
        return true;
    }
    if let Ok(n) = q.parse::<u32>() {
        return u32::from(e.port) == n || e.pid == Some(n);
    }
    let hit = |s: &str| s.to_lowercase().contains(q);
    hit(&e.process_name)
        || e.service_name().is_some_and(hit)
        || e.process_cmd.iter().any(|a| hit(a))
        || e.project.as_deref().is_some_and(hit)
        || e.container
            .as_ref()
            .is_some_and(|c| hit(&c.name) || hit(&c.image))
        || e.user.as_deref().is_some_and(hit)
}

/// Retain only entries matching `pattern` (see [`matches_filter`]).
pub fn filter_entries(entries: &mut Vec<PortEntry>, pattern: &str) {
    let q = pattern.to_lowercase();
    entries.retain(|e| matches_filter(e, &q));
}

/// Test-only builders shared by unit tests across modules.
#[cfg(test)]
pub(crate) mod testutil {
    use super::*;
    use std::net::Ipv4Addr;

    /// A TCP entry on 127.0.0.1 owned by `pid`, with neutral defaults.
    pub fn entry(port: u16, pid: u32, name: &str) -> PortEntry {
        PortEntry {
            protocol: Protocol::Tcp,
            port,
            addrs: vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
            pid: Some(pid),
            process_name: name.to_string(),
            process_cmd: vec![],
            cwd: None,
            project: None,
            user: None,
            parent: None,
            start_time: 0,
            cpu_percent: 0.0,
            memory_mb: 0.0,
            uptime: Duration::from_secs(100),
            service: None,
            category: ServiceCategory::Unknown,
            container: None,
            clients: vec![],
        }
    }

    /// Attach a service (and its category) to an entry.
    pub fn with_service(
        mut e: PortEntry,
        name: &str,
        category: ServiceCategory,
        source: ServiceSource,
    ) -> PortEntry {
        e.service = Some(Service {
            name: name.to_string(),
            category,
            source,
            note: None,
        });
        e.category = category;
        e
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::{entry, with_service};
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    // --- Protocol Display ---

    #[test]
    fn protocol_display() {
        assert_eq!(Protocol::Tcp.to_string(), "TCP");
        assert_eq!(Protocol::Udp.to_string(), "UDP");
    }

    // --- format_memory ---

    #[test]
    fn format_memory_mb() {
        assert_eq!(format_memory(512.0), "512 MB");
        assert_eq!(format_memory(0.0), "0 MB");
        assert_eq!(format_memory(1023.9), "1024 MB");
    }

    #[test]
    fn format_memory_gb() {
        assert_eq!(format_memory(1024.0), "1.0 GB");
        assert_eq!(format_memory(2048.0), "2.0 GB");
        assert_eq!(format_memory(1536.0), "1.5 GB");
    }

    // --- format_uptime ---

    #[test]
    fn format_uptime_seconds() {
        assert_eq!(format_uptime(Duration::from_secs(0)), "0s");
        assert_eq!(format_uptime(Duration::from_secs(30)), "30s");
        assert_eq!(format_uptime(Duration::from_secs(59)), "59s");
    }

    #[test]
    fn format_uptime_minutes() {
        assert_eq!(format_uptime(Duration::from_secs(60)), "1m");
        assert_eq!(format_uptime(Duration::from_secs(3599)), "59m");
    }

    #[test]
    fn format_uptime_hours() {
        assert_eq!(format_uptime(Duration::from_secs(3600)), "1h 0m");
        assert_eq!(format_uptime(Duration::from_secs(7260)), "2h 1m");
        assert_eq!(format_uptime(Duration::from_secs(86399)), "23h 59m");
    }

    #[test]
    fn format_uptime_days() {
        assert_eq!(format_uptime(Duration::from_secs(86400)), "1d 0h");
        assert_eq!(format_uptime(Duration::from_secs(90000)), "1d 1h");
    }

    // --- addresses & exposure ---

    #[test]
    fn exposure_prefers_all_interfaces() {
        let mut e = entry(3000, 1, "node");
        e.addrs = vec![v4(127, 0, 0, 1), IpAddr::V6(Ipv6Addr::UNSPECIFIED)];
        assert_eq!(e.exposure(), Exposure::AllInterfaces);
        e.addrs = vec![v4(127, 0, 0, 1), v4(192, 168, 1, 5)];
        assert_eq!(e.exposure(), Exposure::Network(v4(192, 168, 1, 5)));
        e.addrs = vec![v4(127, 0, 0, 1), IpAddr::V6(Ipv6Addr::LOCALHOST)];
        assert_eq!(e.exposure(), Exposure::Loopback);
    }

    #[test]
    fn ipv4_mapped_loopback_is_not_exposed() {
        let mut e = entry(3000, 1, "node");
        e.addrs = vec!["::ffff:127.0.0.1".parse().unwrap()];
        assert!(!e.is_exposed());
    }

    #[test]
    fn bind_display_values() {
        let mut e = entry(3000, 1, "node");
        e.addrs = vec![v4(127, 0, 0, 1), IpAddr::V6(Ipv6Addr::LOCALHOST)];
        assert_eq!(e.bind_display(), "localhost");
        e.addrs = vec![v4(0, 0, 0, 0)];
        assert_eq!(e.bind_display(), "*");
        e.addrs = vec![v4(192, 168, 1, 5)];
        assert_eq!(e.bind_display(), "192.168.1.5");
    }

    #[test]
    fn addr_display_brackets_ipv6() {
        let mut e = entry(47004, 1, "Python");
        e.addrs = vec![IpAddr::V6(Ipv6Addr::LOCALHOST)];
        assert_eq!(e.addr_display(), "[::1]:47004");
        e.addrs = vec![v4(127, 0, 0, 1), IpAddr::V6(Ipv6Addr::LOCALHOST)];
        assert_eq!(e.addr_display(), "127.0.0.1:47004, [::1]:47004");
    }

    #[test]
    fn addr_display_specific_addr() {
        let mut e = entry(9000, 1, "test");
        e.addrs = vec![v4(192, 168, 1, 100)];
        assert_eq!(e.addr_display(), "192.168.1.100:9000");
    }

    // --- memory_display / uptime_display delegate correctly ---

    #[test]
    fn memory_display_delegates() {
        let mut e = entry(80, 1, "test");
        e.memory_mb = 2048.0;
        assert_eq!(e.memory_display(), "2.0 GB");
    }

    #[test]
    fn uptime_display_delegates() {
        let mut e = entry(80, 1, "test");
        e.uptime = Duration::from_secs(3661);
        assert_eq!(e.uptime_display(), "1h 1m");
    }

    #[test]
    fn service_display_marks_port_guesses() {
        let e = with_service(
            entry(8000, 1, "Python"),
            "Django / FastAPI",
            ServiceCategory::DevServer,
            ServiceSource::Port,
        );
        assert_eq!(e.service_display(), "Django / FastAPI?");
        let e = with_service(
            entry(5432, 1, "postgres"),
            "PostgreSQL",
            ServiceCategory::Database,
            ServiceSource::Process,
        );
        assert_eq!(e.service_display(), "PostgreSQL");
    }

    // --- SortField ---

    #[test]
    fn sort_field_cycles() {
        let field = SortField::Port;
        assert_eq!(field.next(), SortField::ProcessName);
        assert_eq!(field.next().next(), SortField::Cpu);
        assert_eq!(field.next().next().next(), SortField::Memory);
        assert_eq!(field.next().next().next().next(), SortField::Port);
    }

    #[test]
    fn sort_field_labels() {
        assert_eq!(SortField::Port.label(), "Port");
        assert_eq!(SortField::ProcessName.label(), "Name");
        assert_eq!(SortField::Cpu.label(), "CPU");
        assert_eq!(SortField::Memory.label(), "Mem");
    }

    #[test]
    fn sort_ties_are_stable_by_port_protocol_pid() {
        let mut a = entry(8000, 20, "python");
        a.memory_mb = 50.0;
        let mut b = entry(8000, 10, "python");
        b.memory_mb = 50.0;
        let mut c = entry(8000, 10, "python");
        c.protocol = Protocol::Udp;
        c.memory_mb = 50.0;
        let mut v = vec![a, c, b];
        sort_entries(&mut v, SortField::Memory, false);
        let keys: Vec<(Protocol, Option<u32>)> = v.iter().map(|e| (e.protocol, e.pid)).collect();
        assert_eq!(
            keys,
            vec![
                (Protocol::Tcp, Some(10)),
                (Protocol::Tcp, Some(20)),
                (Protocol::Udp, Some(10))
            ]
        );
    }

    #[test]
    fn reverse_sort_flips_primary_order() {
        let mut v = vec![entry(80, 1, "a"), entry(443, 2, "b"), entry(22, 3, "c")];
        sort_entries(&mut v, SortField::Port, true);
        let ports: Vec<u16> = v.iter().map(|e| e.port).collect();
        assert_eq!(ports, vec![443, 80, 22]);
    }

    #[test]
    fn sort_by_cpu_is_descending() {
        let mut low = entry(1, 1, "a");
        low.cpu_percent = 5.0;
        let mut high = entry(2, 2, "b");
        high.cpu_percent = 80.0;
        let mut v = vec![low, high];
        sort_entries(&mut v, SortField::Cpu, false);
        assert_eq!(v[0].cpu_percent, 80.0);
    }

    // --- filtering ---

    #[test]
    fn filter_matches_pid_command_project_container() {
        let mut e = entry(3000, 4242, "node");
        e.process_cmd = vec!["node".into(), "server.js".into()];
        e.project = Some("shop-web".into());
        e.container = Some(ContainerInfo {
            id: "abc123".into(),
            name: "roo-qdrant".into(),
            image: "qdrant/qdrant".into(),
            compose_project: None,
        });
        assert!(matches_filter(&e, "4242"), "numeric query matches pid");
        assert!(matches_filter(&e, "server.js"), "matches command line");
        assert!(matches_filter(&e, "shop"), "matches project");
        assert!(matches_filter(&e, "qdrant"), "matches container name");
        assert!(
            !matches_filter(&e, "424"),
            "numeric query is exact, not substring"
        );
        assert!(!matches_filter(&e, "zzz"));
    }

    #[test]
    fn numeric_filter_is_exact_port() {
        assert!(matches_filter(&entry(80, 1, "nginx"), "80"));
        assert!(!matches_filter(&entry(8080, 2, "caddy"), "80"));
        assert!(!matches_filter(&entry(3800, 3, "node"), "80"));
        assert!(!matches_filter(&entry(18080, 4, "x80"), "80"));
    }

    #[test]
    fn filter_entries_is_case_insensitive_on_names_and_services() {
        let mut v = vec![
            with_service(
                entry(3000, 1, "node"),
                "Next.js",
                ServiceCategory::DevServer,
                ServiceSource::Command,
            ),
            entry(5432, 2, "postgres"),
        ];
        filter_entries(&mut v, "NEXT");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].port, 3000);
    }

    // --- browser grouping ---

    fn browser(port: u16, pid: u32, svc: &str) -> PortEntry {
        with_service(
            entry(port, pid, "Google Chrome Helper"),
            svc,
            ServiceCategory::Browser,
            ServiceSource::Process,
        )
    }

    #[test]
    fn browser_groups_count_by_pid_and_service() {
        let v = vec![
            browser(5000, 100, "Chrome"),
            browser(5001, 100, "Chrome"),
            browser(5002, 200, "Chrome"),
            browser(5003, 100, "Brave"),
        ];
        let counts = count_browser_groups(&v);
        assert_eq!(counts.get(&(100, "Chrome".to_string())), Some(&2));
        assert_eq!(counts.get(&(200, "Chrome".to_string())), Some(&1));
        assert_eq!(counts.get(&(100, "Brave".to_string())), Some(&1));
        assert_eq!(dedup_browser_indices(&v, &counts), vec![0, 2, 3]);
    }

    // --- shell_join ---

    #[test]
    fn shell_join_simple_args() {
        let args = vec!["node".to_string(), "server.js".to_string()];
        assert_eq!(shell_join(&args), "node server.js");
    }

    #[test]
    fn shell_join_quotes_spaces() {
        let args = vec!["node".to_string(), "my script.js".to_string()];
        assert_eq!(shell_join(&args), r#"node "my script.js""#);
    }

    #[test]
    fn shell_join_empty() {
        let args: Vec<String> = vec![];
        assert_eq!(shell_join(&args), "");
    }

    #[test]
    fn shell_join_escapes_inner_quotes() {
        let args = vec!["node".to_string(), "file\"name with space.js".to_string()];
        assert_eq!(shell_join(&args), r#"node "file\"name with space.js""#);
    }

    #[test]
    fn shell_join_quotes_only_no_space() {
        let args = vec!["echo".to_string(), "say\"hello".to_string()];
        assert_eq!(shell_join(&args), r#"echo "say\"hello""#);
    }

    #[test]
    fn shell_join_mixed() {
        let args = vec![
            "/usr/bin/python3".to_string(),
            "-m".to_string(),
            "my module".to_string(),
            "--flag".to_string(),
        ];
        assert_eq!(
            shell_join(&args),
            r#"/usr/bin/python3 -m "my module" --flag"#
        );
    }
}
