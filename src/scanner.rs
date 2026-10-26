use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use anyhow::Result;
use netstat2::{get_sockets_info, AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState};
use sysinfo::{
    Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, Uid, UpdateKind, Users,
};

use crate::docker::{ContainerResolver, Mappings};
use crate::models::{
    Client, PortEntry, ProcessRef, Protocol, Service, ServiceCategory, ServiceSource,
};
use crate::project::ProjectCache;
use crate::services::{identify, is_container_proxy};

/// What every listener refresh collects. `refresh_processes()` alone skips the
/// command line, cwd and user, so request them explicitly (fetched once per process).
pub fn refresh_kind() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing()
        .with_cpu()
        .with_memory()
        .with_cmd(UpdateKind::OnlyIfNotSet)
        .with_cwd(UpdateKind::OnlyIfNotSet)
        .with_user(UpdateKind::OnlyIfNotSet)
}

/// How container attribution runs during a scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContainerMode {
    /// Never ask a container runtime.
    #[default]
    Off,
    /// Ask and wait (one-shot commands).
    Blocking,
    /// Ask in the background and use the latest answer (TUI).
    Background,
}

/// What a scan should collect.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub tcp: bool,
    pub udp: bool,
    /// Sample CPU over `MINIMUM_CPU_UPDATE_INTERVAL` before measuring (one-shot commands).
    pub sample_cpu: bool,
    pub containers: ContainerMode,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            tcp: true,
            udp: true,
            sample_cpu: false,
            containers: ContainerMode::Off,
        }
    }
}

/// Whether a snapshot includes every listening socket on the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Running as root: every socket and owner is visible.
    Complete,
    /// macOS without root: other users' processes (system daemons) are not listed.
    OtherUsersHidden,
    /// Linux without root: this many listeners have no visible owner.
    Unattributed(usize),
}

/// The result of one scan.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub entries: Vec<PortEntry>,
    pub visibility: Visibility,
}

/// Socket classification used by [`group_sockets`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SocketKind {
    Listen,
    Established,
    Other,
    Udp,
}

/// One socket as reported by the OS, before grouping.
#[derive(Debug, Clone)]
pub(crate) struct RawSocket {
    pub protocol: Protocol,
    pub local: IpAddr,
    pub port: u16,
    pub remote: Option<SocketAddr>,
    pub kind: SocketKind,
    pub pids: Vec<u32>,
    /// Owning user (Linux only); lets owner-less rows still show who holds them.
    pub uid: Option<u32>,
}

/// A connected peer before process names are resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawClient {
    pub addr: SocketAddr,
    pub pid: Option<u32>,
}

/// Listening sockets merged per `(protocol, port, pid)`.
#[derive(Debug, Clone)]
pub(crate) struct Group {
    pub protocol: Protocol,
    pub port: u16,
    pub pid: Option<u32>,
    pub addrs: Vec<IpAddr>,
    pub uid: Option<u32>,
    pub clients: Vec<RawClient>,
}

fn owners(pids: &[u32]) -> Vec<Option<u32>> {
    if pids.is_empty() {
        vec![None]
    } else {
        pids.iter().copied().map(Some).collect()
    }
}

/// Merge raw sockets into one group per `(protocol, port, pid)`, ordered by
/// port, protocol and PID. TCP listeners and bound UDP sockets become groups
/// (UDP port 0 means unbound and is skipped); established TCP sockets become
/// the clients of the listener they were accepted on.
pub(crate) fn group_sockets(raw: &[RawSocket]) -> Vec<Group> {
    let mut groups: BTreeMap<(u16, Protocol, Option<u32>), Group> = BTreeMap::new();
    for s in raw {
        let listening = match s.kind {
            SocketKind::Listen => true,
            SocketKind::Udp => s.port != 0,
            SocketKind::Established | SocketKind::Other => false,
        };
        if !listening {
            continue;
        }
        for pid in owners(&s.pids) {
            let g = groups
                .entry((s.port, s.protocol, pid))
                .or_insert_with(|| Group {
                    protocol: s.protocol,
                    port: s.port,
                    pid,
                    addrs: Vec::new(),
                    uid: s.uid,
                    clients: Vec::new(),
                });
            if !g.addrs.contains(&s.local) {
                g.addrs.push(s.local);
            }
            g.uid = g.uid.or(s.uid);
        }
    }

    // Established sockets by their local endpoint, so a loopback client's
    // own socket can be found from the server side's remote address.
    let established = || {
        raw.iter()
            .filter(|s| s.protocol == Protocol::Tcp && s.kind == SocketKind::Established)
    };
    let by_local: HashMap<SocketAddr, &[u32]> = established()
        .map(|s| {
            (
                SocketAddr::new(s.local.to_canonical(), s.port),
                s.pids.as_slice(),
            )
        })
        .collect();
    for s in established() {
        let Some(remote) = s.remote else { continue };
        let addr = SocketAddr::new(remote.ip().to_canonical(), remote.port());
        for pid in owners(&s.pids) {
            if let Some(g) = groups.get_mut(&(s.port, Protocol::Tcp, pid)) {
                let client_pid = by_local.get(&addr).and_then(|p| p.first().copied());
                g.clients.push(RawClient {
                    addr,
                    pid: client_pid,
                });
            }
        }
    }

    groups
        .into_values()
        .map(|mut g| {
            g.addrs.sort();
            g
        })
        .collect()
}

/// Worker processes (uvicorn/gunicorn workers, multiprocessing children) often
/// carry no framework hints in their own command line. When a row's service is
/// only a port guess (or unknown) and its parent listens on the same port with a
/// real detection, inherit the parent's service.
pub(crate) fn inherit_parent_services(entries: &mut [PortEntry]) {
    let known: HashMap<(u16, u32), Service> = entries
        .iter()
        .filter_map(|e| {
            let s = e
                .service
                .as_ref()
                .filter(|s| s.source != ServiceSource::Port)?;
            Some(((e.port, e.pid?), s.clone()))
        })
        .collect();
    for e in entries.iter_mut() {
        if e.service
            .as_ref()
            .is_some_and(|s| s.source != ServiceSource::Port)
        {
            continue;
        }
        if let Some(s) = e.parent.as_ref().and_then(|p| known.get(&(e.port, p.pid))) {
            e.service = Some(s.clone());
            e.category = s.category;
        }
    }
}

/// Name container-proxy rows after the container that publishes their port.
pub(crate) fn attribute_containers(entries: &mut [PortEntry], mappings: &Mappings) {
    for e in entries.iter_mut() {
        if !is_container_proxy(&e.process_name, &e.process_cmd) {
            continue;
        }
        if let Some(container) = mappings.get(&(e.protocol, e.port)) {
            e.service = Some(Service {
                name: container.name.clone(),
                category: ServiceCategory::Container,
                source: ServiceSource::Container,
                note: e.service.as_ref().and_then(|s| s.note),
            });
            e.category = ServiceCategory::Container;
            e.container = Some(container.clone());
        }
    }
}

/// True when kav runs with root privileges (sees every socket owner).
pub fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() == 0 }
}

fn visibility_for(entries: &[PortEntry], root: bool) -> Visibility {
    if root {
        return Visibility::Complete;
    }
    let unattributed = entries.iter().filter(|e| e.pid.is_none()).count();
    if unattributed > 0 {
        Visibility::Unattributed(unattributed)
    } else if cfg!(target_os = "macos") {
        Visibility::OtherUsersHidden
    } else {
        Visibility::Complete
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn socket_uid(s: &netstat2::SocketInfo) -> Option<u32> {
    Some(s.uid)
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn socket_uid(_s: &netstat2::SocketInfo) -> Option<u32> {
    None
}

/// Enumerate sockets from the OS.
fn collect_sockets(tcp: bool, udp: bool) -> Result<Vec<RawSocket>> {
    let mut proto_flags = ProtocolFlags::empty();
    if tcp {
        proto_flags |= ProtocolFlags::TCP;
    }
    if udp {
        proto_flags |= ProtocolFlags::UDP;
    }
    let af_flags = AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6;
    let sockets = get_sockets_info(af_flags, proto_flags)?;
    Ok(sockets
        .iter()
        .map(|s| match &s.protocol_socket_info {
            ProtocolSocketInfo::Tcp(t) => {
                let kind = match t.state {
                    TcpState::Listen => SocketKind::Listen,
                    TcpState::Established => SocketKind::Established,
                    _ => SocketKind::Other,
                };
                RawSocket {
                    protocol: Protocol::Tcp,
                    local: t.local_addr,
                    port: t.local_port,
                    remote: (kind == SocketKind::Established)
                        .then(|| SocketAddr::new(t.remote_addr, t.remote_port)),
                    kind,
                    pids: s.associated_pids.clone(),
                    uid: socket_uid(s),
                }
            }
            ProtocolSocketInfo::Udp(u) => RawSocket {
                protocol: Protocol::Udp,
                local: u.local_addr,
                port: u.local_port,
                remote: None,
                kind: SocketKind::Udp,
                pids: s.associated_pids.clone(),
                uid: socket_uid(s),
            },
        })
        .collect())
}

/// Listening `(protocol, port)` pairs only — no process details, so it is cheap
/// enough to poll (used by `kav wait` and `kav pick`).
pub fn listening_ports(tcp: bool, udp: bool) -> Result<HashSet<(Protocol, u16)>> {
    Ok(group_sockets(&collect_sockets(tcp, udp)?)
        .into_iter()
        .map(|g| (g.protocol, g.port))
        .collect())
}

/// Scans listening sockets and maps them to process details.
///
/// Keep one `Scanner` alive across refreshes (TUI, watch mode): CPU usage is
/// measured between consecutive refreshes of the same `System`.
pub struct Scanner {
    sys: System,
    /// Separate table for liveness checks, so polling never disturbs CPU accounting.
    probe_sys: System,
    users: Users,
    projects: ProjectCache,
    containers: ContainerResolver,
}

impl Scanner {
    pub fn new() -> Self {
        Self {
            sys: System::new(),
            probe_sys: System::new(),
            users: Users::new_with_refreshed_list(),
            projects: ProjectCache::from_env(),
            containers: ContainerResolver::new(Duration::from_secs(5)),
        }
    }

    fn refresh(&mut self, pids: &[Pid], kind: ProcessRefreshKind) {
        if !pids.is_empty() {
            self.sys
                .refresh_processes_specifics(ProcessesToUpdate::Some(pids), true, kind);
        }
    }

    /// Take a snapshot of every listening socket.
    ///
    /// Only socket-owning processes are refreshed (plus, by name only, their
    /// parents and loopback clients). With `sample_cpu`, sysinfo gets the three
    /// refreshes it needs for a process it has not seen before — discover,
    /// baseline, then measure after `MINIMUM_CPU_UPDATE_INTERVAL`.
    pub fn scan(&mut self, opts: &ScanOptions) -> Result<Snapshot> {
        let groups = group_sockets(&collect_sockets(opts.tcp, opts.udp)?);

        let mut pids: Vec<Pid> = groups
            .iter()
            .filter_map(|g| g.pid)
            .map(Pid::from_u32)
            .collect();
        pids.sort();
        pids.dedup();
        if opts.sample_cpu {
            self.refresh(&pids, refresh_kind()); // discover
            self.refresh(&pids, refresh_kind()); // baseline
            std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        }
        self.refresh(&pids, refresh_kind()); // measure

        let mut extra: HashSet<Pid> = groups
            .iter()
            .flat_map(|g| g.clients.iter().filter_map(|c| c.pid))
            .map(Pid::from_u32)
            .collect();
        extra.extend(
            pids.iter()
                .filter_map(|pid| self.sys.process(*pid).and_then(|p| p.parent())),
        );
        let extra: Vec<Pid> = extra.into_iter().filter(|p| !pids.contains(p)).collect();
        self.refresh(&extra, ProcessRefreshKind::nothing());

        let mut entries: Vec<PortEntry> = groups.into_iter().map(|g| self.build_entry(g)).collect();
        inherit_parent_services(&mut entries);
        for e in &mut entries {
            e.project = e.cwd.as_deref().and_then(|cwd| self.projects.resolve(cwd));
        }
        let has_proxy = entries
            .iter()
            .any(|e| is_container_proxy(&e.process_name, &e.process_cmd));
        let mappings = match opts.containers {
            _ if !has_proxy => None,
            ContainerMode::Off => None,
            ContainerMode::Blocking => self.containers.get_blocking(),
            ContainerMode::Background => self.containers.get_background(),
        };
        if let Some(mappings) = mappings {
            attribute_containers(&mut entries, mappings);
        }
        let visibility = visibility_for(&entries, is_root());
        Ok(Snapshot {
            entries,
            visibility,
        })
    }

    /// True while `pid` runs and is still the same process: a different start
    /// time means the PID was reused (`start_time` 0 = unknown). Zombies count as gone.
    pub fn process_alive(&mut self, pid: u32, start_time: u64) -> bool {
        let pid = Pid::from_u32(pid);
        self.probe_sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing(),
        );
        match self.probe_sys.process(pid) {
            None => false,
            Some(p) if p.status() == ProcessStatus::Zombie => false,
            Some(p) => start_time == 0 || p.start_time() == start_time,
        }
    }

    fn process_name(&self, pid: u32) -> Option<String> {
        self.sys
            .process(Pid::from_u32(pid))
            .map(|p| p.name().to_string_lossy().into_owned())
    }

    fn user_name(&self, uid: &Uid) -> Option<String> {
        self.users.get_user_by_id(uid).map(|u| u.name().to_string())
    }

    fn build_entry(&self, g: Group) -> PortEntry {
        let mut e = PortEntry {
            protocol: g.protocol,
            port: g.port,
            addrs: g.addrs,
            pid: g.pid,
            process_name: String::from("?"),
            process_cmd: Vec::new(),
            cwd: None,
            project: None,
            user: None,
            parent: None,
            start_time: 0,
            cpu_percent: 0.0,
            memory_mb: 0.0,
            uptime: Duration::ZERO,
            service: None,
            category: ServiceCategory::Unknown,
            container: None,
            clients: g
                .clients
                .into_iter()
                .map(|c| Client {
                    addr: c.addr,
                    pid: c.pid,
                    process: c.pid.and_then(|p| self.process_name(p)),
                })
                .collect(),
        };
        match g.pid.map(|pid| self.sys.process(Pid::from_u32(pid))) {
            Some(Some(p)) => {
                e.process_name = p.name().to_string_lossy().into_owned();
                e.process_cmd = p
                    .cmd()
                    .iter()
                    .map(|s| s.to_string_lossy().into_owned())
                    .collect();
                e.cwd = p.cwd().map(|c| c.to_path_buf());
                e.user = p.user_id().and_then(|uid| self.user_name(uid));
                e.parent = p.parent().map(|pp| ProcessRef {
                    pid: pp.as_u32(),
                    name: self.process_name(pp.as_u32()).unwrap_or_default(),
                });
                e.start_time = p.start_time();
                e.cpu_percent = p.cpu_usage();
                e.memory_mb = p.memory() as f64 / (1024.0 * 1024.0);
                e.uptime = Duration::from_secs(p.run_time());
            }
            // Race: the process exited between socket enumeration and lookup.
            Some(None) => e.process_name = String::from("<exited>"),
            // Owner not visible to this user; the kernel still tells us the uid on Linux.
            None => {
                e.user = g
                    .uid
                    .and_then(|uid| Uid::try_from(uid as usize).ok())
                    .and_then(|uid| self.user_name(&uid));
            }
        }
        e.service = identify(e.port, &e.process_name, &e.process_cmd);
        e.category = e
            .service
            .as_ref()
            .map_or(ServiceCategory::Unknown, |s| s.category);
        e
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

    const LO4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
    const LO6: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);

    fn sock(
        protocol: Protocol,
        local: IpAddr,
        port: u16,
        kind: SocketKind,
        pids: &[u32],
    ) -> RawSocket {
        RawSocket {
            protocol,
            local,
            port,
            remote: None,
            kind,
            pids: pids.to_vec(),
            uid: None,
        }
    }

    fn keys(groups: &[Group]) -> Vec<(Protocol, u16, Option<u32>)> {
        groups.iter().map(|g| (g.protocol, g.port, g.pid)).collect()
    }

    #[test]
    fn group_keeps_tcp_and_udp_separate() {
        let raw = [
            sock(Protocol::Tcp, LO4, 47001, SocketKind::Listen, &[7]),
            sock(Protocol::Udp, LO4, 47001, SocketKind::Udp, &[7]),
        ];
        assert_eq!(
            keys(&group_sockets(&raw)),
            vec![
                (Protocol::Tcp, 47001, Some(7)),
                (Protocol::Udp, 47001, Some(7))
            ]
        );
    }

    #[test]
    fn group_merges_addresses() {
        let raw = [
            sock(Protocol::Tcp, LO6, 5432, SocketKind::Listen, &[9]),
            sock(Protocol::Tcp, LO4, 5432, SocketKind::Listen, &[9]),
        ];
        let groups = group_sockets(&raw);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].addrs, vec![LO4, LO6]);
    }

    #[test]
    fn group_skips_udp_port_zero() {
        let raw = [sock(
            Protocol::Udp,
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            0,
            SocketKind::Udp,
            &[3],
        )];
        assert!(group_sockets(&raw).is_empty());
    }

    #[test]
    fn group_ignores_non_listening_tcp() {
        let raw = [
            sock(Protocol::Tcp, LO4, 5432, SocketKind::Other, &[9]),
            sock(Protocol::Tcp, LO4, 60000, SocketKind::Established, &[9]),
        ];
        assert!(group_sockets(&raw).is_empty());
    }

    #[test]
    fn group_keeps_ownerless_sockets() {
        let mut s = sock(
            Protocol::Tcp,
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            47005,
            SocketKind::Listen,
            &[],
        );
        s.uid = Some(501);
        let groups = group_sockets(&[s]);
        assert_eq!(keys(&groups), vec![(Protocol::Tcp, 47005, None)]);
        assert_eq!(groups[0].uid, Some(501));
    }

    #[test]
    fn clients_counted_for_established() {
        let mut server_side = sock(Protocol::Tcp, LO4, 5432, SocketKind::Established, &[9]);
        server_side.remote = Some(SocketAddr::new(LO4, 60000));
        let mut client_side = sock(Protocol::Tcp, LO4, 60000, SocketKind::Established, &[44]);
        client_side.remote = Some(SocketAddr::new(LO4, 5432));
        let raw = [
            sock(Protocol::Tcp, LO4, 5432, SocketKind::Listen, &[9]),
            server_side,
            client_side,
        ];
        let groups = group_sockets(&raw);
        assert_eq!(keys(&groups), vec![(Protocol::Tcp, 5432, Some(9))]);
        assert_eq!(
            groups[0].clients,
            vec![RawClient {
                addr: SocketAddr::new(LO4, 60000),
                pid: Some(44)
            }]
        );
    }

    #[test]
    fn clients_match_ipv4_mapped_addresses() {
        let mapped: IpAddr = "::ffff:127.0.0.1".parse().unwrap();
        let mut server_side = sock(Protocol::Tcp, mapped, 3000, SocketKind::Established, &[5]);
        server_side.remote = Some(SocketAddr::new(mapped, 61000));
        let mut client_side = sock(Protocol::Tcp, LO4, 61000, SocketKind::Established, &[77]);
        client_side.remote = Some(SocketAddr::new(LO4, 3000));
        let raw = [
            sock(
                Protocol::Tcp,
                IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                3000,
                SocketKind::Listen,
                &[5],
            ),
            server_side,
            client_side,
        ];
        let groups = group_sockets(&raw);
        assert_eq!(groups[0].clients.len(), 1);
        assert_eq!(groups[0].clients[0].pid, Some(77));
        assert_eq!(groups[0].clients[0].addr, SocketAddr::new(LO4, 61000));
    }

    #[test]
    fn visibility_reports_ownerless_rows_and_root() {
        let mut hidden = crate::models::testutil::entry(47005, 1, "?");
        hidden.pid = None;
        let visible = crate::models::testutil::entry(3000, 2, "node");
        assert_eq!(
            visibility_for(&[hidden.clone(), visible.clone()], false),
            Visibility::Unattributed(1)
        );
        assert_eq!(visibility_for(&[hidden], true), Visibility::Complete);
        let expected = if cfg!(target_os = "macos") {
            Visibility::OtherUsersHidden
        } else {
            Visibility::Complete
        };
        assert_eq!(visibility_for(&[visible], false), expected);
    }

    #[test]
    fn workers_inherit_parent_listener_service() {
        use crate::models::testutil::{entry, with_service};
        use crate::models::{ProcessRef, ServiceSource};
        let parent = with_service(
            entry(8000, 52009, "Python"),
            "Uvicorn",
            ServiceCategory::DevServer,
            ServiceSource::Command,
        );
        let mut worker = with_service(
            entry(8000, 52011, "Python"),
            "Django / FastAPI",
            ServiceCategory::DevServer,
            ServiceSource::Port,
        );
        worker.parent = Some(ProcessRef {
            pid: 52009,
            name: "Python".into(),
        });
        let mut other_port = worker.clone();
        other_port.port = 9000;
        other_port.service = None;
        let mut v = vec![parent, worker, other_port];
        inherit_parent_services(&mut v);
        assert_eq!(v[1].service_name(), Some("Uvicorn"));
        assert_eq!(v[2].service_name(), None, "only same-port parents count");
    }

    #[test]
    fn container_proxies_are_attributed() {
        use crate::models::testutil::entry;
        use crate::models::{ContainerInfo, ServiceSource};
        let qdrant = ContainerInfo {
            id: "517d48f91bc3".into(),
            name: "roo-qdrant".into(),
            image: "qdrant/qdrant".into(),
            compose_project: None,
        };
        let mut mappings = crate::docker::Mappings::new();
        mappings.insert((Protocol::Tcp, 6333), qdrant.clone());
        let mut v = vec![
            entry(6333, 21743, "com.docker.backend"),
            entry(6334, 21743, "com.docker.backend"),
            entry(6333, 999, "node"),
        ];
        attribute_containers(&mut v, &mappings);
        assert_eq!(v[0].container.as_ref(), Some(&qdrant));
        assert_eq!(v[0].service_name(), Some("roo-qdrant"));
        assert_eq!(
            v[0].service.as_ref().unwrap().source,
            ServiceSource::Container
        );
        assert_eq!(v[0].category, ServiceCategory::Container);
        assert_eq!(v[1].container, None, "unmapped port stays unattributed");
        assert_eq!(
            v[2].container, None,
            "only container proxies are attributed"
        );
    }

    #[test]
    fn scan_finds_own_listener_with_details() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let snap = Scanner::new().scan(&ScanOptions::default()).unwrap();
        let e = snap
            .entries
            .iter()
            .find(|e| e.port == port && e.protocol == Protocol::Tcp)
            .expect("own listener in snapshot");
        assert_eq!(e.pid, Some(std::process::id()));
        assert_eq!(e.addrs, vec![LO4]);
        assert!(!e.process_cmd.is_empty(), "command line collected");
        assert!(e.cwd.is_some(), "cwd collected");
        assert!(e.user.is_some(), "user resolved");
        assert!(e.parent.is_some(), "parent resolved");
        // cargo runs tests from the crate root, which is a git checkout under $HOME
        // on dev machines and CI runners alike.
        let cwd = std::env::current_dir().unwrap();
        if std::env::var_os("HOME").is_some_and(|h| cwd.starts_with(h)) {
            assert!(e.project.is_some(), "project resolved from cwd {cwd:?}");
        }
    }
}
