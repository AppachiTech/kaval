use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;

use anyhow::Result;
use netstat2::{get_sockets_info, AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState};
use sysinfo::System;

use crate::models::{PortEntry, Protocol};
use crate::util::identify_service;

/// Scan the system for all listening ports and map them to process info.
pub fn scan_ports(show_tcp: bool, show_udp: bool) -> Result<Vec<PortEntry>> {
    let mut proto_flags = ProtocolFlags::empty();
    if show_tcp {
        proto_flags |= ProtocolFlags::TCP;
    }
    if show_udp {
        proto_flags |= ProtocolFlags::UDP;
    }

    let af_flags = AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6;

    let sockets = get_sockets_info(af_flags, proto_flags)?;

    // Build a sysinfo System for process lookups
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    // Collect PIDs we care about, then look them up
    let mut entries: Vec<PortEntry> = Vec::new();
    let mut seen: HashMap<(u16, u32), bool> = HashMap::new();

    for socket in &sockets {
        let (protocol, local_addr, port, is_listening) = match &socket.protocol_socket_info {
            ProtocolSocketInfo::Tcp(tcp) => {
                if tcp.state != TcpState::Listen {
                    continue;
                }
                (Protocol::Tcp, tcp.local_addr, tcp.local_port, true)
            }
            ProtocolSocketInfo::Udp(udp) => (Protocol::Udp, udp.local_addr, udp.local_port, true),
        };

        if !is_listening {
            continue;
        }

        // Get associated PIDs
        for &pid in &socket.associated_pids {
            // Deduplicate by (port, pid)
            if seen.contains_key(&(port, pid)) {
                continue;
            }
            seen.insert((port, pid), true);

            let pid_obj = sysinfo::Pid::from_u32(pid);
            let (process_name, process_cmd, cpu_percent, memory_mb, uptime) =
                if let Some(proc) = sys.process(pid_obj) {
                    let name = proc.name().to_string_lossy().to_string();
                    let cmd = proc
                        .cmd()
                        .iter()
                        .map(|s| s.to_string_lossy().to_string())
                        .collect::<Vec<_>>()
                        .join(" ");
                    let cpu = proc.cpu_usage();
                    let mem = proc.memory() as f64 / (1024.0 * 1024.0);
                    let up = Duration::from_secs(proc.run_time());
                    (name, cmd, cpu, mem, up)
                } else {
                    (String::from("?"), String::new(), 0.0, 0.0, Duration::ZERO)
                };

            let (known_service, category) = identify_service(port, &process_name);

            entries.push(PortEntry {
                protocol,
                local_addr: IpAddr::from(local_addr),
                port,
                pid,
                process_name,
                process_cmd,
                cpu_percent,
                memory_mb,
                uptime,
                known_service,
                category,
            });
        }
    }

    // Default sort by port number
    entries.sort_by_key(|e| e.port);

    Ok(entries)
}

/// Kill a process by PID (cross-platform: macOS, Linux, Windows)
pub fn kill_process(pid: u32, force: bool) -> Result<()> {
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

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
