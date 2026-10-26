//! JSON output, schema version 2.
//!
//! Every document carries `"schema": 2` and the kav version. Field names are
//! part of kav's scripting contract (documented in the README): add fields
//! freely, but rename or remove only with a schema bump.

use std::io::Write;

use anyhow::Result;
use serde::Serialize;

use crate::models::{PortEntry, Protocol, ServiceCategory, ServiceSource};
use crate::scanner::Visibility;

/// Version of the JSON documents kav emits.
pub const SCHEMA: u32 = 2;

pub fn category_name(c: ServiceCategory) -> &'static str {
    match c {
        ServiceCategory::DevServer => "dev_server",
        ServiceCategory::Database => "database",
        ServiceCategory::Cache => "cache",
        ServiceCategory::Queue => "queue",
        ServiceCategory::Container => "container",
        ServiceCategory::Browser => "browser",
        ServiceCategory::System => "system",
        ServiceCategory::Unknown => "unknown",
    }
}

pub fn source_name(s: ServiceSource) -> &'static str {
    match s {
        ServiceSource::Process => "process",
        ServiceSource::Command => "command",
        ServiceSource::Port => "port",
        ServiceSource::Container => "container",
    }
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// `kav list --json` (one frame per line with `--watch`).
#[derive(Serialize)]
pub struct ListDoc<'a> {
    pub schema: u32,
    pub version: &'static str,
    /// Unix seconds; only in `--watch` frames.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
    /// False when some listeners' owners are hidden (run with sudo to see all).
    pub complete: bool,
    pub ports: Vec<PortJson<'a>>,
}

#[derive(Serialize)]
pub struct ParentJson<'a> {
    pub pid: u32,
    pub name: &'a str,
}

#[derive(Serialize)]
pub struct ContainerJson<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub image: &'a str,
    pub compose_project: Option<&'a str>,
}

/// One listener.
#[derive(Serialize)]
pub struct PortJson<'a> {
    pub port: u16,
    pub protocol: &'static str,
    pub addresses: Vec<String>,
    pub exposed: bool,
    pub pid: Option<u32>,
    pub process: Option<&'a str>,
    pub command: &'a [String],
    pub cwd: Option<String>,
    pub project: Option<&'a str>,
    pub user: Option<&'a str>,
    pub parent: Option<ParentJson<'a>>,
    pub service: Option<&'a str>,
    pub service_source: Option<&'static str>,
    pub category: &'static str,
    pub container: Option<ContainerJson<'a>>,
    pub cpu: f64,
    pub memory_mb: f64,
    pub uptime_secs: u64,
    /// Established connections (TCP only).
    pub connections: Option<usize>,
}

impl<'a> PortJson<'a> {
    pub fn from_entry(e: &'a PortEntry) -> Self {
        Self {
            port: e.port,
            protocol: match e.protocol {
                Protocol::Tcp => "TCP",
                Protocol::Udp => "UDP",
            },
            addresses: e.addrs.iter().map(|a| a.to_string()).collect(),
            exposed: e.is_exposed(),
            pid: e.pid,
            process: e.pid.map(|_| e.process_name.as_str()),
            command: &e.process_cmd,
            cwd: e.cwd.as_ref().map(|c| c.display().to_string()),
            project: e.project.as_deref(),
            user: e.user.as_deref(),
            parent: e.parent.as_ref().map(|p| ParentJson {
                pid: p.pid,
                name: &p.name,
            }),
            service: e.service_name(),
            service_source: e.service.as_ref().map(|s| source_name(s.source)),
            category: category_name(e.category),
            container: e.container.as_ref().map(|c| ContainerJson {
                id: &c.id,
                name: &c.name,
                image: &c.image,
                compose_project: c.compose_project.as_deref(),
            }),
            cpu: round1(f64::from(e.cpu_percent)),
            memory_mb: round1(e.memory_mb),
            uptime_secs: e.uptime.as_secs(),
            connections: (e.protocol == Protocol::Tcp).then_some(e.clients.len()),
        }
    }
}

pub fn list_doc(
    entries: &[PortEntry],
    visibility: Visibility,
    timestamp: Option<u64>,
) -> ListDoc<'_> {
    ListDoc {
        schema: SCHEMA,
        version: env!("CARGO_PKG_VERSION"),
        timestamp,
        complete: visibility == Visibility::Complete,
        ports: entries.iter().map(PortJson::from_entry).collect(),
    }
}

/// A health probe result in `kav check --probe --json`.
#[derive(Serialize)]
pub struct ProbeJson {
    /// `http`, `tcp_open`, `refused`, `timeout` or `unsupported`.
    pub result: &'static str,
    pub http_status: Option<u16>,
    pub latency_ms: u64,
}

/// One requested port in `kav check --json`.
#[derive(Serialize)]
pub struct CheckPortJson<'a> {
    pub port: u16,
    pub in_use: bool,
    /// False when the port is in use but its owner is hidden (another user's process).
    pub owner_visible: bool,
    pub listeners: Vec<PortJson<'a>>,
    pub probe: Option<ProbeJson>,
}

#[derive(Serialize)]
pub struct CheckDoc<'a> {
    pub schema: u32,
    pub version: &'static str,
    pub ports: Vec<CheckPortJson<'a>>,
}

impl<'a> CheckDoc<'a> {
    pub fn new(ports: Vec<CheckPortJson<'a>>) -> Self {
        Self {
            schema: SCHEMA,
            version: env!("CARGO_PKG_VERSION"),
            ports,
        }
    }
}

/// One process or container acted on by `kav kill --json`.
#[derive(Serialize)]
pub struct KillResultJson {
    pub ports: Vec<u16>,
    pub pid: Option<u32>,
    pub process: Option<String>,
    /// `signal`, `docker_stop` or `none`.
    pub action: &'static str,
    pub signal: Option<String>,
    /// `exited`, `signal_sent`, `stopped`, `still_running`, `respawned`, `failed`, `refused`, `declined` or `not_found`.
    pub outcome: &'static str,
    pub message: Option<String>,
}

#[derive(Serialize)]
pub struct KillDoc {
    pub schema: u32,
    pub version: &'static str,
    pub results: Vec<KillResultJson>,
}

impl KillDoc {
    pub fn new(results: Vec<KillResultJson>) -> Self {
        Self {
            schema: SCHEMA,
            version: env!("CARGO_PKG_VERSION"),
            results,
        }
    }
}

/// Pretty-printed document followed by a newline.
pub fn write_pretty<T: Serialize>(doc: &T, w: &mut impl Write) -> Result<()> {
    serde_json::to_writer_pretty(&mut *w, doc)?;
    writeln!(w)?;
    Ok(())
}

/// Compact single-line document (NDJSON frame) followed by a newline.
pub fn write_line<T: Serialize>(doc: &T, w: &mut impl Write) -> Result<()> {
    serde_json::to_writer(&mut *w, doc)?;
    writeln!(w)?;
    w.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::testutil::{entry, with_service};
    use crate::models::{
        Client, ContainerInfo, PortEntry, ProcessRef, Protocol, ServiceCategory, ServiceSource,
    };
    use crate::scanner::Visibility;
    use serde_json::{json, Value};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
    use std::path::PathBuf;
    use std::time::Duration;

    fn full_entry() -> PortEntry {
        let mut e = with_service(
            entry(5432, 1337, "postgres"),
            "PostgreSQL",
            ServiceCategory::Database,
            ServiceSource::Process,
        );
        e.addrs = vec![
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ];
        e.process_cmd = vec!["postgres".into(), "-D".into(), "/data".into()];
        e.cwd = Some(PathBuf::from("/data"));
        e.project = Some("shop".into());
        e.user = Some("madhu".into());
        e.parent = Some(ProcessRef {
            pid: 1,
            name: "launchd".into(),
        });
        e.cpu_percent = 12.345;
        e.memory_mb = 21.04;
        e.uptime = Duration::from_secs(3600);
        e.container = Some(ContainerInfo {
            id: "abc".into(),
            name: "shop-db-1".into(),
            image: "postgres:16".into(),
            compose_project: Some("shop".into()),
        });
        e.clients = vec![Client {
            addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 60000),
            pid: Some(44),
            process: Some("psql".into()),
        }];
        e
    }

    fn to_value<T: serde::Serialize>(doc: &T) -> Value {
        let mut buf = Vec::new();
        write_pretty(doc, &mut buf).unwrap();
        serde_json::from_slice(&buf).unwrap()
    }

    #[test]
    fn list_doc_has_schema_2_and_fields() {
        let entries = vec![full_entry()];
        let v = to_value(&list_doc(&entries, Visibility::Complete, None));
        assert_eq!(v["schema"], 2);
        assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(v["complete"], true);
        assert!(
            v.get("timestamp").is_none(),
            "no timestamp outside watch mode"
        );
        assert_eq!(
            v["ports"][0],
            json!({
                "port": 5432,
                "protocol": "TCP",
                "addresses": ["127.0.0.1", "::1"],
                "exposed": false,
                "pid": 1337,
                "process": "postgres",
                "command": ["postgres", "-D", "/data"],
                "cwd": "/data",
                "project": "shop",
                "user": "madhu",
                "parent": {"pid": 1, "name": "launchd"},
                "service": "PostgreSQL",
                "service_source": "process",
                "category": "database",
                "container": {
                    "id": "abc",
                    "name": "shop-db-1",
                    "image": "postgres:16",
                    "compose_project": "shop"
                },
                "cpu": 12.3,
                "memory_mb": 21.0,
                "uptime_secs": 3600,
                "connections": 1
            })
        );
    }

    #[test]
    fn null_fields_serialize_as_null() {
        let mut e = entry(47005, 1, "?");
        e.pid = None;
        e.protocol = Protocol::Udp;
        e.addrs = vec![IpAddr::V4(Ipv4Addr::UNSPECIFIED)];
        let entries = vec![e];
        let v = to_value(&list_doc(&entries, Visibility::Unattributed(1), None));
        assert_eq!(v["complete"], false);
        let p = &v["ports"][0];
        for key in [
            "pid",
            "process",
            "cwd",
            "project",
            "user",
            "parent",
            "service",
            "service_source",
            "container",
            "connections",
        ] {
            assert!(p[key].is_null(), "{key} should be null: {p}");
        }
        assert_eq!(p["category"], "unknown");
        assert_eq!(p["exposed"], true);
        assert_eq!(p["protocol"], "UDP");
    }

    #[test]
    fn ndjson_frame_is_single_line_with_timestamp() {
        let entries = vec![full_entry(), full_entry()];
        let mut buf = Vec::new();
        write_line(
            &list_doc(&entries, Visibility::Complete, Some(1_761_500_000)),
            &mut buf,
        )
        .unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert_eq!(text.matches('\n').count(), 1, "one line per frame");
        assert!(text.ends_with('\n'));
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["timestamp"], 1_761_500_000u64);
        assert_eq!(v["ports"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn categories_and_sources_have_stable_names() {
        assert_eq!(category_name(ServiceCategory::DevServer), "dev_server");
        assert_eq!(category_name(ServiceCategory::Queue), "queue");
        assert_eq!(source_name(ServiceSource::Container), "container");
        assert_eq!(source_name(ServiceSource::Command), "command");
    }
}
