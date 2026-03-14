use std::collections::HashSet;
use std::net::IpAddr;
use std::time::Duration;

use anyhow::Result;
use netstat2::{get_sockets_info, AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState};
use sysinfo::System;

use crate::models::{PortEntry, Protocol};
use crate::util::identify_service;

/// Create a System with the two-refresh dance needed for accurate CPU usage.
/// Use this for one-shot commands. For the TUI, reuse a persistent System instead.
pub fn new_system_with_cpu() -> System {
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    sys
}

/// Construct a PortEntry from raw socket + process data.
/// Pure function — usable in unit tests without OS calls.
#[allow(clippy::too_many_arguments)]
pub(crate) fn make_port_entry(
    protocol: Protocol,
    local_addr: IpAddr,
    port: u16,
    pid: u32,
    process_name: String,
    process_cmd: Vec<String>,
    cpu_percent: f32,
    memory_mb: f64,
    uptime: Duration,
) -> PortEntry {
    let (known_service, category) = identify_service(port, &process_name);
    PortEntry {
        protocol,
        local_addr,
        port,
        pid,
        process_name,
        process_cmd,
        cpu_percent,
        memory_mb,
        uptime,
        known_service,
        category,
    }
}

/// Scan the system for all listening ports and map them to process info.
/// Pass a persistent `System` for accurate CPU readings across refreshes.
pub fn scan_ports(sys: &mut System, show_tcp: bool, show_udp: bool) -> Result<Vec<PortEntry>> {
    let mut proto_flags = ProtocolFlags::empty();
    if show_tcp {
        proto_flags |= ProtocolFlags::TCP;
    }
    if show_udp {
        proto_flags |= ProtocolFlags::UDP;
    }

    let af_flags = AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6;

    let sockets = get_sockets_info(af_flags, proto_flags)?;

    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let mut entries: Vec<PortEntry> = Vec::new();
    let mut seen: HashSet<(u16, u32)> = HashSet::new();

    for socket in &sockets {
        let (protocol, local_addr, port) = match &socket.protocol_socket_info {
            ProtocolSocketInfo::Tcp(tcp) => {
                if tcp.state != TcpState::Listen {
                    continue;
                }
                (Protocol::Tcp, tcp.local_addr, tcp.local_port)
            }
            ProtocolSocketInfo::Udp(udp) => (Protocol::Udp, udp.local_addr, udp.local_port),
        };

        for &pid in &socket.associated_pids {
            if !seen.insert((port, pid)) {
                continue;
            }

            let pid_obj = sysinfo::Pid::from_u32(pid);
            let (process_name, process_cmd, cpu_percent, memory_mb, uptime) =
                if let Some(proc) = sys.process(pid_obj) {
                    let name = proc.name().to_string_lossy().to_string();
                    let cmd: Vec<String> = proc
                        .cmd()
                        .iter()
                        .map(|s| s.to_string_lossy().into_owned())
                        .collect();
                    let cpu = proc.cpu_usage();
                    let mem = proc.memory() as f64 / (1024.0 * 1024.0);
                    let up = Duration::from_secs(proc.run_time());
                    (name, cmd, cpu, mem, up)
                } else {
                    // Race condition: process exited between socket enumeration
                    // and process lookup. Use a clear placeholder rather than "?".
                    (
                        String::from("<exited>"),
                        Vec::new(),
                        0.0,
                        0.0,
                        Duration::ZERO,
                    )
                };

            entries.push(make_port_entry(
                protocol,
                local_addr,
                port,
                pid,
                process_name,
                process_cmd,
                cpu_percent,
                memory_mb,
                uptime,
            ));
        }
    }

    Ok(entries)
}

/// Kill a process by PID. Pass a `System` that has been refreshed.
pub fn kill_process(sys: &System, pid: u32, force: bool) -> Result<()> {
    let pid_obj = sysinfo::Pid::from_u32(pid);
    let proc = sys
        .process(pid_obj)
        .ok_or_else(|| anyhow::anyhow!("Process with PID {} not found", pid))?;

    let signal = if force {
        sysinfo::Signal::Kill // SIGKILL on Unix, TerminateProcess on Windows
    } else {
        sysinfo::Signal::Term // SIGTERM on Unix, TerminateProcess on Windows
    };

    if proc.kill_with(signal).unwrap_or(false) {
        Ok(())
    } else {
        anyhow::bail!("Failed to kill PID {}. Try running with sudo.", pid,)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Protocol, ServiceCategory};
    use std::net::{IpAddr, Ipv4Addr};

    fn localhost() -> IpAddr {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    }

    #[test]
    fn make_port_entry_identifies_postgres() {
        let entry = make_port_entry(
            Protocol::Tcp,
            localhost(),
            5432,
            1000,
            "postmaster".to_string(),
            vec![],
            0.0,
            0.0,
            Duration::ZERO,
        );
        assert_eq!(entry.category, ServiceCategory::Database);
        assert_eq!(entry.known_service, Some("PostgreSQL"));
        assert_eq!(entry.port, 5432);
        assert_eq!(entry.pid, 1000);
    }

    #[test]
    fn make_port_entry_identifies_redis() {
        let entry = make_port_entry(
            Protocol::Tcp,
            localhost(),
            6379,
            2000,
            "redis-server".to_string(),
            vec![],
            0.0,
            0.0,
            Duration::ZERO,
        );
        assert_eq!(entry.category, ServiceCategory::Cache);
        assert_eq!(entry.known_service, Some("Redis"));
    }

    #[test]
    fn make_port_entry_identifies_node_by_port() {
        let entry = make_port_entry(
            Protocol::Tcp,
            localhost(),
            3000,
            3000,
            "node".to_string(),
            vec![],
            0.0,
            0.0,
            Duration::ZERO,
        );
        assert_eq!(entry.category, ServiceCategory::DevServer);
    }

    #[test]
    fn make_port_entry_unknown_service() {
        let entry = make_port_entry(
            Protocol::Tcp,
            localhost(),
            9999,
            4000,
            "my-custom-app".to_string(),
            vec![],
            0.0,
            0.0,
            Duration::ZERO,
        );
        assert_eq!(entry.category, ServiceCategory::Unknown);
        assert_eq!(entry.known_service, None);
    }

    #[test]
    fn make_port_entry_unknown_process_preserves_fields() {
        let entry = make_port_entry(
            Protocol::Udp,
            localhost(),
            0,
            9999,
            "?".to_string(),
            vec![],
            0.0,
            0.0,
            Duration::ZERO,
        );
        assert_eq!(entry.process_name, "?");
        assert_eq!(entry.protocol, Protocol::Udp);
        assert_eq!(entry.known_service, None);
    }

    #[test]
    fn make_port_entry_identifies_by_process_name_prefix() {
        // postgres process detected even on a non-standard port
        let entry = make_port_entry(
            Protocol::Tcp,
            localhost(),
            54321,
            5000,
            "postgres".to_string(),
            vec![],
            0.0,
            0.0,
            Duration::ZERO,
        );
        assert_eq!(entry.category, ServiceCategory::Database);
        assert_eq!(entry.known_service, Some("PostgreSQL"));
    }
}
