use std::collections::{HashMap, HashSet};
use std::fmt;
use std::net::IpAddr;
use std::time::Duration;

use clap::ValueEnum;

/// CPU% thresholds shared between CLI output and TUI rendering
pub const CPU_WARN_THRESHOLD: f32 = 20.0;
pub const CPU_ERROR_THRESHOLD: f32 = 50.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceCategory {
    DevServer,
    Database,
    Cache,
    Container,
    Browser,
    System,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct PortEntry {
    pub protocol: Protocol,
    pub local_addr: IpAddr,
    pub port: u16,
    pub pid: u32,
    pub process_name: String,
    pub process_cmd: Vec<String>,
    pub cpu_percent: f32,
    pub memory_mb: f64,
    pub uptime: Duration,
    pub known_service: Option<&'static str>,
    pub category: ServiceCategory,
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
    /// Display address as compact string
    pub fn addr_display(&self) -> String {
        match self.local_addr {
            IpAddr::V4(addr) if addr.is_unspecified() => format!("*:{}", self.port),
            IpAddr::V6(addr) if addr.is_unspecified() => format!("*:{}", self.port),
            IpAddr::V4(addr) if addr.is_loopback() => format!("127…:{}", self.port),
            _ => format!("{}:{}", self.local_addr, self.port),
        }
    }

    /// Format memory as human-readable string
    pub fn memory_display(&self) -> String {
        format_memory(self.memory_mb)
    }

    /// Format uptime as human-readable string
    pub fn uptime_display(&self) -> String {
        format_uptime(self.uptime)
    }
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
pub fn count_browser_groups(entries: &[PortEntry]) -> HashMap<(u32, &'static str), usize> {
    let mut counts: HashMap<(u32, &'static str), usize> = HashMap::new();
    for e in entries {
        if e.category == ServiceCategory::Browser {
            if let Some(svc) = e.known_service {
                *counts.entry((e.pid, svc)).or_insert(0) += 1;
            }
        }
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
    counts: &HashMap<(u32, &'static str), usize>,
) -> Vec<usize> {
    let mut seen: HashSet<(u32, &'static str)> = HashSet::new();
    entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            if e.category == ServiceCategory::Browser {
                if let Some(svc) = e.known_service {
                    // Only include the first entry per (pid, service) group
                    if counts.get(&(e.pid, svc)).copied().unwrap_or(0) > 1
                        && !seen.insert((e.pid, svc))
                    {
                        return None;
                    }
                }
            }
            Some(i)
        })
        .collect()
}

/// Sort a slice of PortEntry in-place by the given field.
/// Shared by CLI (main.rs) and TUI (ui.rs) to avoid duplication.
pub fn sort_entries(entries: &mut [PortEntry], field: SortField) {
    match field {
        SortField::Port => entries.sort_by_key(|e| e.port),
        SortField::ProcessName => entries.sort_by(|a, b| {
            a.process_name
                .to_lowercase()
                .cmp(&b.process_name.to_lowercase())
        }),
        SortField::Cpu => entries.sort_by(|a, b| {
            b.cpu_percent
                .partial_cmp(&a.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        SortField::Memory => entries.sort_by(|a, b| {
            b.memory_mb
                .partial_cmp(&a.memory_mb)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn make_entry(addr: IpAddr, port: u16, memory_mb: f64, uptime_secs: u64) -> PortEntry {
        PortEntry {
            protocol: Protocol::Tcp,
            local_addr: addr,
            port,
            pid: 1234,
            process_name: "test".to_string(),
            process_cmd: vec![],
            cpu_percent: 0.0,
            memory_mb,
            uptime: Duration::from_secs(uptime_secs),
            known_service: None,
            category: ServiceCategory::Unknown,
        }
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

    // --- addr_display ---

    #[test]
    fn addr_display_ipv4_unspecified() {
        let entry = make_entry(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 3000, 0.0, 0);
        assert_eq!(entry.addr_display(), "*:3000");
    }

    #[test]
    fn addr_display_ipv6_unspecified() {
        let entry = make_entry(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 8080, 0.0, 0);
        assert_eq!(entry.addr_display(), "*:8080");
    }

    #[test]
    fn addr_display_ipv4_loopback() {
        let entry = make_entry(IpAddr::V4(Ipv4Addr::LOCALHOST), 5432, 0.0, 0);
        assert_eq!(entry.addr_display(), "127…:5432");
    }

    #[test]
    fn addr_display_specific_addr() {
        let addr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));
        let entry = make_entry(addr, 9000, 0.0, 0);
        assert_eq!(entry.addr_display(), "192.168.1.100:9000");
    }

    // --- memory_display / uptime_display delegate correctly ---

    #[test]
    fn memory_display_delegates() {
        let entry = make_entry(IpAddr::V4(Ipv4Addr::LOCALHOST), 80, 2048.0, 0);
        assert_eq!(entry.memory_display(), "2.0 GB");
    }

    #[test]
    fn uptime_display_delegates() {
        let entry = make_entry(IpAddr::V4(Ipv4Addr::LOCALHOST), 80, 0.0, 3661);
        assert_eq!(entry.uptime_display(), "1h 1m");
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
}
