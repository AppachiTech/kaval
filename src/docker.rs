//! Container attribution: which container publishes a port that a container
//! runtime's proxy (Docker Desktop, OrbStack, Colima, docker-proxy) listens on.
//!
//! Uses the local `docker` CLI, so it works with any Docker-compatible runtime
//! the CLI is configured for. Every call has a timeout: a paused or updating
//! Docker Desktop must never hang kav.

use std::collections::HashMap;
use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::models::{ContainerInfo, Protocol};

/// Published host port → the container behind it.
pub type Mappings = HashMap<(Protocol, u16), ContainerInfo>;

/// How long a `docker ps` may take before kav gives up.
pub const DOCKER_TIMEOUT: Duration = Duration::from_secs(2);
/// After a failed lookup (no docker CLI, daemon down), wait this long before retrying.
const FAILURE_BACKOFF: Duration = Duration::from_secs(30);

const PS_FORMAT: &str =
    "{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Ports}}\t{{.Label \"com.docker.compose.project\"}}";

/// Published host ports in a `docker ps` Ports column, e.g.
/// `0.0.0.0:6333->6333/tcp, [::]:6333->6333/tcp` or `0.0.0.0:8000-8002->8000-8002/tcp`.
/// Unpublished ports (`5432/tcp`) are skipped.
pub fn parse_ports(s: &str) -> Vec<(u16, Protocol)> {
    let mut out = Vec::new();
    for part in s.split(',').map(str::trim) {
        let Some((host, container)) = part.split_once("->") else {
            continue;
        };
        let proto = if container.ends_with("/udp") {
            Protocol::Udp
        } else {
            Protocol::Tcp
        };
        let Some((_, ports)) = host.rsplit_once(':') else {
            continue;
        };
        let (start, end) = ports.split_once('-').unwrap_or((ports, ports));
        if let (Ok(start), Ok(end)) = (start.parse::<u16>(), end.parse::<u16>()) {
            for port in start..=end {
                if !out.contains(&(port, proto)) {
                    out.push((port, proto));
                }
            }
        }
    }
    out
}

/// One line of `docker ps --format PS_FORMAT`.
pub fn parse_ps_line(line: &str) -> Option<(ContainerInfo, Vec<(u16, Protocol)>)> {
    let mut fields = line.split('\t');
    let id = fields.next()?.trim();
    if id.is_empty() {
        return None;
    }
    let names = fields.next().unwrap_or_default().trim();
    let image = fields.next().unwrap_or_default().trim();
    let ports = fields.next().unwrap_or_default();
    let compose = fields.next().unwrap_or_default().trim();
    let info = ContainerInfo {
        id: id.to_string(),
        name: names.split(',').next().unwrap_or(names).to_string(),
        image: image.to_string(),
        compose_project: (!compose.is_empty() && compose != "<no value>")
            .then(|| compose.to_string()),
    };
    Some((info, parse_ports(ports)))
}

/// Run a command, killing it if it takes longer than `timeout`.
pub fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> Result<Output> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to start command")?;
    // Drain both pipes on threads so a chatty command cannot block on a full pipe.
    let drain = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    };
    let stdout = drain(Box::new(child.stdout.take().context("no stdout")?));
    let stderr = drain(Box::new(child.stderr.take().context("no stderr")?));
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("timed out after {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    Ok(Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// Ask the docker CLI which containers publish which host ports.
pub fn list_mappings(timeout: Duration) -> Result<Mappings> {
    let out = run_with_timeout(
        Command::new("docker").args(["ps", "--format", PS_FORMAT]),
        timeout,
    )?;
    if !out.status.success() {
        bail!(
            "docker ps failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let mut mappings = Mappings::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some((info, ports)) = parse_ps_line(line) {
            for (port, proto) in ports {
                mappings.insert((proto, port), info.clone());
            }
        }
    }
    Ok(mappings)
}

/// Stop a container (`docker stop`), waiting at most `timeout`.
pub fn stop(container: &ContainerInfo, timeout: Duration) -> Result<()> {
    let out = run_with_timeout(
        Command::new("docker").args(["stop", &container.id]),
        timeout,
    )?;
    if !out.status.success() {
        bail!(
            "docker stop {} failed: {}",
            container.name,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// Cached container mappings, refreshed at most once per `ttl`.
pub struct ContainerResolver {
    ttl: Duration,
    cached: Option<(Instant, Mappings)>,
    pending: Option<Receiver<Result<Mappings>>>,
    failed_at: Option<Instant>,
}

impl ContainerResolver {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            cached: None,
            pending: None,
            failed_at: None,
        }
    }

    fn wants_refresh(&self) -> bool {
        let fresh = self
            .cached
            .as_ref()
            .is_some_and(|(at, _)| at.elapsed() < self.ttl);
        let backing_off = self
            .failed_at
            .is_some_and(|at| at.elapsed() < FAILURE_BACKOFF);
        !fresh && !backing_off
    }

    fn store(&mut self, result: Result<Mappings>) {
        match result {
            Ok(m) => {
                self.cached = Some((Instant::now(), m));
                self.failed_at = None;
            }
            Err(_) => self.failed_at = Some(Instant::now()),
        }
    }

    /// Current mappings, fetching (and waiting) when stale. For one-shot commands.
    pub fn get_blocking(&mut self) -> Option<&Mappings> {
        if self.wants_refresh() {
            let result = list_mappings(DOCKER_TIMEOUT);
            self.store(result);
        }
        self.cached.as_ref().map(|(_, m)| m)
    }

    /// Latest known mappings without waiting; starts a background refresh when stale.
    /// For the TUI, where a slow `docker ps` must not stall input.
    pub fn get_background(&mut self) -> Option<&Mappings> {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    self.store(result);
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.pending = None,
            }
        }
        if self.pending.is_none() && self.wants_refresh() {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(list_mappings(DOCKER_TIMEOUT));
            });
            self.pending = Some(rx);
        }
        self.cached.as_ref().map(|(_, m)| m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_docker_desktop_ports() {
        assert_eq!(
            parse_ports("0.0.0.0:6333->6333/tcp, [::]:6333->6333/tcp"),
            vec![(6333, Protocol::Tcp)]
        );
        assert_eq!(
            parse_ports("0.0.0.0:5353->5353/udp"),
            vec![(5353, Protocol::Udp)]
        );
    }

    #[test]
    fn parses_port_ranges() {
        assert_eq!(
            parse_ports("0.0.0.0:8000-8002->8000-8002/tcp"),
            vec![
                (8000, Protocol::Tcp),
                (8001, Protocol::Tcp),
                (8002, Protocol::Tcp)
            ]
        );
    }

    #[test]
    fn skips_unpublished_ports() {
        assert_eq!(parse_ports("5432/tcp"), vec![]);
        assert_eq!(
            parse_ports("5432/tcp, 0.0.0.0:2222->22/tcp"),
            vec![(2222, Protocol::Tcp)]
        );
        assert_eq!(parse_ports(""), vec![]);
    }

    #[test]
    fn parses_compose_project() {
        let (info, ports) =
            parse_ps_line("abc123\tshop-db-1\tpostgres:16\t0.0.0.0:5432->5432/tcp\tshop").unwrap();
        assert_eq!(info.id, "abc123");
        assert_eq!(info.name, "shop-db-1");
        assert_eq!(info.image, "postgres:16");
        assert_eq!(info.compose_project.as_deref(), Some("shop"));
        assert_eq!(ports, vec![(5432, Protocol::Tcp)]);

        let (info, _) = parse_ps_line("def456\troo-qdrant\tqdrant/qdrant\t\t").unwrap();
        assert_eq!(info.compose_project, None);
        let (info, _) = parse_ps_line("def456\tx\ty\t\t<no value>").unwrap();
        assert_eq!(info.compose_project, None);
        assert!(parse_ps_line("").is_none());
    }

    #[test]
    fn run_with_timeout_kills_slow_commands() {
        let start = Instant::now();
        let result = run_with_timeout(Command::new("sleep").arg("5"), Duration::from_millis(300));
        assert!(result.is_err(), "slow command must time out");
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "took {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn run_with_timeout_returns_output() {
        let out =
            run_with_timeout(Command::new("echo").arg("hello"), Duration::from_secs(5)).unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hello");
    }
}
