//! Port probes: does something accept connections, can a server bind the port,
//! and a lightweight health check. Everything stays on this machine.

use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use socket2::{Domain, Socket, Type};

use crate::models::Protocol;

/// True when a TCP connection to the port on loopback (IPv4, then IPv6) succeeds.
/// Passive: it never binds, so it cannot race a server that is starting up.
pub fn tcp_accepts(port: u16, timeout: Duration) -> bool {
    [
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(Ipv6Addr::LOCALHOST),
    ]
    .iter()
    .any(|ip| TcpStream::connect_timeout(&SocketAddr::new(*ip, port), timeout).is_ok())
}

/// Outcome of trying to bind a port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindCheck {
    Free,
    InUse,
    /// Could not tell (e.g. a privileged port without permission).
    Unknown,
}

/// Try to bind the port on all interfaces (IPv4, and IPv6-only) without
/// `SO_REUSEADDR` — the way a server starting up would meet it. The probe
/// socket is closed immediately.
pub fn bind_check(port: u16, proto: Protocol) -> BindCheck {
    let ty = match proto {
        Protocol::Tcp => Type::STREAM,
        Protocol::Udp => Type::DGRAM,
    };
    let mut result = BindCheck::Free;
    for (domain, ip) in [
        (Domain::IPV4, IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
        (Domain::IPV6, IpAddr::V6(Ipv6Addr::UNSPECIFIED)),
    ] {
        let Ok(socket) = Socket::new(domain, ty, None) else {
            continue; // address family unavailable (e.g. IPv6 disabled)
        };
        if domain == Domain::IPV6 && socket.set_only_v6(true).is_err() {
            continue;
        }
        match socket.bind(&SocketAddr::new(ip, port).into()) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::AddrInUse => return BindCheck::InUse,
            Err(e) if e.kind() == ErrorKind::AddrNotAvailable => {}
            Err(_) => result = BindCheck::Unknown,
        }
    }
    result
}

/// What a health probe saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthResult {
    /// An HTTP server answered with this status code.
    Http(u16),
    /// The port accepts TCP connections (no HTTP answer, or HTTP not tried).
    TcpOpen,
    Refused,
    Timeout,
    /// Probing is not possible (e.g. UDP).
    Unsupported,
}

/// A health probe result and how long it took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Health {
    pub result: HealthResult,
    pub latency: Duration,
}

/// Wildcard binds are probed on loopback.
fn probe_target(ip: IpAddr) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V4(v4) if v4.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(v6) if v6.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        other => other,
    }
}

/// `HTTP/1.1 204 No Content` → 204.
fn parse_status_line(bytes: &[u8]) -> Option<u16> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut parts = text.split_whitespace();
    parts.next().filter(|v| v.starts_with("HTTP/"))?;
    parts.next()?.parse().ok()
}

/// Connect to `ip:port`; with `http`, send `HEAD /` and read the status line.
pub fn health(ip: IpAddr, port: u16, http: bool, timeout: Duration) -> Health {
    let start = Instant::now();
    let done = |result| Health {
        result,
        latency: start.elapsed(),
    };
    let addr = SocketAddr::new(probe_target(ip), port);
    let mut stream = match TcpStream::connect_timeout(&addr, timeout) {
        Ok(stream) => stream,
        Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
            return done(HealthResult::Timeout)
        }
        Err(_) => return done(HealthResult::Refused),
    };
    if !http {
        return done(HealthResult::TcpOpen);
    }
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    if stream
        .write_all(b"HEAD / HTTP/1.0\r\nHost: localhost\r\nUser-Agent: kav\r\n\r\n")
        .is_err()
    {
        return done(HealthResult::TcpOpen);
    }
    let mut buf = [0u8; 64];
    let n = stream.read(&mut buf).unwrap_or(0);
    done(parse_status_line(&buf[..n]).map_or(HealthResult::TcpOpen, HealthResult::Http))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, UdpSocket};

    const LO: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

    /// Poll `check` until it holds or `timeout` passes. Other tests in this binary
    /// spawn child processes; on macOS std marks new sockets close-on-exec in a
    /// separate step, so a child spawned concurrently can inherit a test's socket
    /// and keep a dropped listener alive until it exits.
    fn eventually(timeout: Duration, mut check: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if check() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn tcp_accepts_true_for_listener_false_after_drop() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(tcp_accepts(port, Duration::from_millis(300)));
        drop(listener);
        assert!(eventually(Duration::from_secs(5), || !tcp_accepts(
            port,
            Duration::from_millis(300)
        )));
    }

    #[test]
    fn bind_check_in_use_then_free() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert_eq!(bind_check(port, Protocol::Tcp), BindCheck::InUse);
        drop(listener);
        assert!(eventually(Duration::from_secs(5), || bind_check(
            port,
            Protocol::Tcp
        ) == BindCheck::Free));
    }

    #[test]
    fn udp_bind_check_in_use() {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        assert_eq!(bind_check(port, Protocol::Udp), BindCheck::InUse);
    }

    #[test]
    fn health_reads_http_status() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut buf = [0u8; 256];
            let _ = conn.read(&mut buf);
            conn.write_all(b"HTTP/1.0 204 No Content\r\n\r\n").unwrap();
        });
        let h = health(LO, port, true, Duration::from_secs(2));
        assert_eq!(h.result, HealthResult::Http(204));
    }

    #[test]
    fn health_tcp_only_when_not_http() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = health(LO, port, false, Duration::from_secs(1));
        assert_eq!(h.result, HealthResult::TcpOpen);
        drop(listener);
    }

    #[test]
    fn health_refused_when_nothing_listens() {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert!(eventually(Duration::from_secs(5), || health(
            LO,
            port,
            true,
            Duration::from_millis(500)
        )
        .result
            == HealthResult::Refused));
    }

    #[test]
    fn health_targets_loopback_for_wildcard_binds() {
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = health(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port,
            false,
            Duration::from_secs(1),
        );
        assert_eq!(h.result, HealthResult::TcpOpen);
    }
}
