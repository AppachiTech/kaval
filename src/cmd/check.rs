//! `kav check`: is anything listening on these ports?

use std::collections::HashSet;
use std::io::{self, IsTerminal, Write};
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::Result;

use crate::models::{PortEntry, Protocol};
use crate::output::{self, json, table, ColorChoice};
use crate::probe::{self, BindCheck, Health, HealthResult};
use crate::scanner::{is_root, ContainerMode, ScanOptions, Scanner};
use crate::services::is_http_like;

const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

pub struct CheckArgs {
    pub ports: Vec<u16>,
    pub json: bool,
    pub free: bool,
    pub quiet: bool,
    pub probe: bool,
}

/// What kav knows about one requested port.
pub(crate) struct PortStatus<'a> {
    pub port: u16,
    pub listeners: Vec<&'a PortEntry>,
    pub in_use: bool,
}

impl PortStatus<'_> {
    /// False when the port is busy but its owner is hidden from this user.
    pub fn owner_visible(&self) -> bool {
        self.listeners.iter().any(|e| e.pid.is_some())
    }
}

/// Requested ports without repeats, in the order given.
pub(crate) fn dedup_ports(ports: &[u16]) -> Vec<u16> {
    let mut seen = HashSet::new();
    ports.iter().copied().filter(|p| seen.insert(*p)).collect()
}

/// Listeners on `port`. When none are visible, ask `hidden_in_use` whether a
/// process kav can't see holds the port.
pub(crate) fn port_status(
    port: u16,
    entries: &[PortEntry],
    hidden_in_use: impl Fn(u16) -> bool,
) -> PortStatus<'_> {
    let listeners: Vec<&PortEntry> = entries.iter().filter(|e| e.port == port).collect();
    let in_use = !listeners.is_empty() || hidden_in_use(port);
    PortStatus {
        port,
        listeners,
        in_use,
    }
}

/// Without root, other users' sockets are invisible: confirm with a passive
/// loopback connect (TCP) or a bind attempt (UDP).
fn hidden_in_use(port: u16) -> bool {
    !is_root()
        && (probe::tcp_accepts(port, Duration::from_millis(300))
            || probe::bind_check(port, Protocol::Udp) == BindCheck::InUse)
}

fn run_probe(status: &PortStatus<'_>) -> Option<Health> {
    if !status.in_use {
        return None;
    }
    let tcp = status
        .listeners
        .iter()
        .find(|e| e.protocol == Protocol::Tcp);
    Some(match tcp {
        Some(e) => probe::health(
            e.addrs
                .first()
                .copied()
                .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            status.port,
            is_http_like(status.port, e.service.as_ref()),
            PROBE_TIMEOUT,
        ),
        // Hidden owner: the TCP fallback found it, so probe loopback.
        None if status.listeners.is_empty() => probe::health(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            status.port,
            is_http_like(status.port, None),
            PROBE_TIMEOUT,
        ),
        None => Health {
            result: HealthResult::Unsupported,
            latency: Duration::ZERO,
        },
    })
}

fn probe_text(h: &Health) -> String {
    let ms = h.latency.as_millis();
    match h.result {
        HealthResult::Http(code) => format!("HTTP {code} in {ms} ms"),
        HealthResult::TcpOpen => format!("accepts TCP connections ({ms} ms)"),
        HealthResult::Refused => "connection refused".to_string(),
        HealthResult::Timeout => "no response (timed out)".to_string(),
        HealthResult::Unsupported => "not probed (UDP)".to_string(),
    }
}

fn probe_json(h: &Health) -> json::ProbeJson {
    let (result, http_status) = match h.result {
        HealthResult::Http(code) => ("http", Some(code)),
        HealthResult::TcpOpen => ("tcp_open", None),
        HealthResult::Refused => ("refused", None),
        HealthResult::Timeout => ("timeout", None),
        HealthResult::Unsupported => ("unsupported", None),
    };
    json::ProbeJson {
        result,
        http_status,
        latency_ms: h.latency.as_millis() as u64,
    }
}

pub fn run(args: &CheckArgs, color: ColorChoice) -> Result<ExitCode> {
    let ports = dedup_ports(&args.ports);
    let detailed = !args.quiet && !args.free;
    let snapshot = Scanner::new().scan(&ScanOptions {
        tcp: true,
        udp: true,
        sample_cpu: detailed,
        containers: if detailed {
            ContainerMode::Blocking
        } else {
            ContainerMode::Off
        },
    })?;
    let statuses: Vec<PortStatus<'_>> = ports
        .iter()
        .map(|&p| port_status(p, &snapshot.entries, hidden_in_use))
        .collect();
    let probes: Vec<Option<Health>> = statuses
        .iter()
        .map(|s| if args.probe { run_probe(s) } else { None })
        .collect();

    let active = statuses.iter().filter(|s| s.in_use).count();
    let ok = if args.free { active == 0 } else { active > 0 };
    let code = if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    };
    if args.quiet {
        return Ok(code);
    }

    let mut out = io::stdout().lock();
    if args.json {
        let doc = json::CheckDoc::new(
            statuses
                .iter()
                .zip(&probes)
                .map(|(s, h)| json::CheckPortJson {
                    port: s.port,
                    in_use: s.in_use,
                    owner_visible: s.owner_visible(),
                    listeners: s
                        .listeners
                        .iter()
                        .map(|e| json::PortJson::from_entry(e))
                        .collect(),
                    probe: h.as_ref().map(probe_json),
                })
                .collect(),
        );
        json::write_pretty(&doc, &mut out)?;
        return Ok(code);
    }

    let color = output::color_enabled(color, io::stdout().is_terminal());
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut check_table = table::CheckTable::new(color, home.as_deref());
    for (status, health) in statuses.iter().zip(&probes) {
        for (i, e) in status.listeners.iter().enumerate() {
            let probe = health.as_ref().filter(|_| i == 0).map(probe_text);
            check_table.entry(&mut out, e, probe.as_deref())?;
        }
    }
    for (status, health) in statuses.iter().zip(&probes) {
        if !status.listeners.is_empty() {
            continue;
        }
        if status.in_use {
            writeln!(
                out,
                "Port {} is in use by a process kav can't see (another user's) — try sudo",
                status.port
            )?;
        } else {
            writeln!(out, "Nothing listening on port {}", status.port)?;
        }
        if let Some(h) = health {
            writeln!(out, "    probe: {}", probe_text(h))?;
        }
    }
    if ports.len() > 1 {
        if args.free {
            let free = ports.len() - active;
            writeln!(out, "\n{free} of {} ports free", ports.len())?;
        } else {
            writeln!(out, "\n{active} of {} ports active", ports.len())?;
        }
    }
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::testutil::entry;

    #[test]
    fn hidden_owner_fallback_reports_in_use() {
        let status = port_status(47005, &[], |_| true);
        assert!(status.in_use);
        assert!(!status.owner_visible());
        let status = port_status(47005, &[], |_| false);
        assert!(!status.in_use);
    }

    #[test]
    fn visible_listener_skips_the_fallback() {
        let e = entry(3000, 11, "node");
        let entries = vec![e];
        let status = port_status(3000, &entries, |_| panic!("no fallback needed"));
        assert!(status.in_use);
        assert!(status.owner_visible());
        assert_eq!(status.listeners.len(), 1);
    }

    #[test]
    fn ownerless_rows_are_in_use_but_not_visible() {
        let mut e = entry(3000, 11, "?");
        e.pid = None;
        let entries = vec![e];
        let status = port_status(3000, &entries, |_| false);
        assert!(status.in_use);
        assert!(!status.owner_visible());
    }

    #[test]
    fn requested_ports_are_deduplicated_in_order() {
        assert_eq!(dedup_ports(&[3000, 80, 3000, 443, 80]), vec![3000, 80, 443]);
    }
}
