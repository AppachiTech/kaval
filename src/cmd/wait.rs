//! `kav wait`: block until ports are listening (or free) — for scripts and CI.

use std::io::{self, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::cmd::check::dedup_ports;
use crate::models::Protocol;
use crate::probe;
use crate::scanner::{is_root, listening_ports};

const POLL: Duration = Duration::from_millis(250);

pub struct WaitArgs {
    pub ports: Vec<u16>,
    pub free: bool,
    /// Zero means wait forever.
    pub timeout: Duration,
    pub udp: bool,
    pub quiet: bool,
}

/// Which of `ports` are busy right now. Passive: enumerates sockets and, for
/// TCP ports whose owner may be hidden, connects to loopback — it never binds,
/// so it cannot race a server that is starting up.
fn busy(ports: &[u16], proto: Protocol) -> Result<Vec<bool>> {
    let listening = listening_ports(proto == Protocol::Tcp, proto == Protocol::Udp)?;
    Ok(ports
        .iter()
        .map(|&p| {
            listening.contains(&(proto, p))
                || (proto == Protocol::Tcp
                    && !is_root()
                    && probe::tcp_accepts(p, Duration::from_millis(200)))
        })
        .collect())
}

fn list(ports: &[u16]) -> String {
    let ports: Vec<String> = ports.iter().map(u16::to_string).collect();
    ports.join(", ")
}

pub fn run(args: &WaitArgs) -> Result<ExitCode> {
    let ports = dedup_ports(&args.ports);
    let proto = if args.udp {
        Protocol::Udp
    } else {
        Protocol::Tcp
    };
    let start = Instant::now();
    let goal = if args.free { "free" } else { "listening" };
    loop {
        let state = busy(&ports, proto)?;
        let pending: Vec<u16> = ports
            .iter()
            .zip(&state)
            .filter(|(_, &busy)| busy == args.free)
            .map(|(&p, _)| p)
            .collect();
        if pending.is_empty() {
            if !args.quiet {
                writeln!(
                    io::stdout(),
                    "Port {} {} {goal} (after {:.1}s)",
                    list(&ports),
                    if ports.len() == 1 { "is" } else { "are" },
                    start.elapsed().as_secs_f64()
                )?;
            }
            return Ok(ExitCode::SUCCESS);
        }
        if !args.timeout.is_zero() && start.elapsed() >= args.timeout {
            if !args.quiet {
                eprintln!(
                    "Timed out after {}s: port {} not {goal} yet",
                    args.timeout.as_secs_f64(),
                    list(&pending)
                );
            }
            return Ok(ExitCode::from(1));
        }
        std::thread::sleep(POLL);
    }
}
