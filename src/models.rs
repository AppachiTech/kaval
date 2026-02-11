use std::fmt;
use std::net::IpAddr;
use std::time::Duration;

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
    pub process_cmd: String,
    pub cpu_percent: f32,
    pub memory_mb: f64,
    pub uptime: Duration,
    pub known_service: Option<&'static str>,
    pub category: ServiceCategory,
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
        if self.memory_mb >= 1024.0 {
            format!("{:.1} GB", self.memory_mb / 1024.0)
        } else {
            format!("{:.0} MB", self.memory_mb)
        }
    }

    /// Format uptime as human-readable string
    pub fn uptime_display(&self) -> String {
        let secs = self.uptime.as_secs();
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    Port,
    ProcessName,
    Cpu,
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
