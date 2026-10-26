//! Planning and carrying out kills: protection rules, parent targeting,
//! signals and verified exits. Shared by `kav kill` and the TUI.

use std::fmt;
use std::str::FromStr;
use std::time::{Duration, Instant};

use crate::models::{ContainerInfo, PortEntry, ProcessRef};
use crate::scanner::Scanner;
use crate::services::is_container_proxy;

/// Signals kav can send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Term,
    Kill,
    Int,
    Hup,
    Quit,
    Usr1,
    Usr2,
}

impl Signal {
    const ALL: [Signal; 7] = [
        Signal::Term,
        Signal::Kill,
        Signal::Int,
        Signal::Hup,
        Signal::Quit,
        Signal::Usr1,
        Signal::Usr2,
    ];

    fn raw(self) -> libc::c_int {
        match self {
            Signal::Term => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
            Signal::Int => libc::SIGINT,
            Signal::Hup => libc::SIGHUP,
            Signal::Quit => libc::SIGQUIT,
            Signal::Usr1 => libc::SIGUSR1,
            Signal::Usr2 => libc::SIGUSR2,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Signal::Term => "TERM",
            Signal::Kill => "KILL",
            Signal::Int => "INT",
            Signal::Hup => "HUP",
            Signal::Quit => "QUIT",
            Signal::Usr1 => "USR1",
            Signal::Usr2 => "USR2",
        }
    }
}

impl fmt::Display for Signal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SIG{}", self.name())
    }
}

impl FromStr for Signal {
    type Err = String;

    /// Accepts `TERM`, `SIGTERM`, `term` or the platform's signal number (`15`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let upper = s.trim().to_ascii_uppercase();
        let name = upper.strip_prefix("SIG").unwrap_or(&upper);
        let found = match name.parse::<libc::c_int>() {
            Ok(n) => Self::ALL.into_iter().find(|sig| sig.raw() == n),
            Err(_) => Self::ALL.into_iter().find(|sig| sig.name() == name),
        };
        found.ok_or_else(|| {
            format!("unknown signal '{s}' (use TERM, KILL, INT, HUP, QUIT, USR1 or USR2)")
        })
    }
}

/// Why sending a signal failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    PermissionDenied,
    NoSuchProcess,
    Other(String),
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SendError::PermissionDenied => write!(f, "permission denied — try sudo"),
            SendError::NoSuchProcess => write!(f, "the process already exited"),
            SendError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

/// Send `signal` to one process.
pub fn send(pid: u32, signal: Signal) -> Result<(), SendError> {
    // kill(0) signals our process group and kill(-1) every process we may
    // signal: only ever pass a single, positive PID.
    let raw_pid = libc::pid_t::try_from(pid)
        .ok()
        .filter(|p| *p > 0)
        .ok_or_else(|| SendError::Other(format!("refusing to signal PID {pid}")))?;
    // SAFETY: kill(2) with a positive PID targets exactly that process.
    if unsafe { libc::kill(raw_pid, signal.raw()) } == 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    Err(match err.raw_os_error() {
        Some(libc::EPERM) => SendError::PermissionDenied,
        Some(libc::ESRCH) => SendError::NoSuchProcess,
        _ => SendError::Other(err.to_string()),
    })
}

/// Facts about the running kav process that protection rules depend on.
#[derive(Debug, Clone, Copy)]
pub struct KillEnv {
    pub own_pid: u32,
    /// kav runs inside an SSH session (killing sshd could lock the user out).
    pub in_ssh: bool,
}

impl KillEnv {
    pub fn current() -> Self {
        Self {
            own_pid: std::process::id(),
            in_ssh: std::env::var_os("SSH_CONNECTION").is_some()
                || std::env::var_os("SSH_TTY").is_some(),
        }
    }
}

/// Whether a process may be killed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Protection {
    None,
    /// Allowed, but the user should know something (e.g. it respawns).
    Warn(&'static str),
    /// Not killed unless the user passes `--allow-protected`.
    Refuse(String),
}

const AIRPLAY_RESPAWNS: &str = "ControlCenter (AirPlay Receiver) restarts on its own and reclaims ports 5000/7000 — turn off AirPlay Receiver in System Settings → General → AirDrop & Handoff instead.";

/// Never signalled, whatever the options: init and kav itself.
fn untouchable(pid: u32, env: &KillEnv) -> Option<String> {
    if pid <= 1 {
        Some(format!("PID {pid} is the system's init process"))
    } else if pid == env.own_pid {
        Some("that is kav itself".to_string())
    } else {
        None
    }
}

/// Protection rules for killing the owner of `e`.
pub fn protection(e: &PortEntry, env: &KillEnv) -> Protection {
    let Some(pid) = e.pid else {
        return Protection::Refuse(format!(
            "port {} is held by a process kav can't see — try sudo",
            e.port
        ));
    };
    if let Some(reason) = untouchable(pid, env) {
        return Protection::Refuse(reason);
    }
    let name = e.process_name.to_lowercase();
    const CRITICAL: &[&str] = &[
        "launchd",
        "systemd",
        "init",
        "kernel_task",
        "windowserver",
        "loginwindow",
    ];
    if CRITICAL.contains(&name.as_str()) {
        return Protection::Refuse(format!("{} is a critical system process", e.process_name));
    }
    if is_container_proxy(&e.process_name, &e.process_cmd)
        || matches!(name.as_str(), "dockerd" | "containerd")
        || name.starts_with("rancher-desktop")
    {
        return Protection::Refuse(format!(
            "{} forwards ports for every container — stopping it stops them all; stop the container instead",
            e.process_name
        ));
    }
    if env.in_ssh && name == "sshd" {
        return Protection::Refuse("sshd may be serving your current SSH session".to_string());
    }
    if name == "controlcenter" {
        return Protection::Warn(AIRPLAY_RESPAWNS);
    }
    Protection::None
}

/// Whether `--parent` may target this process: not init, not a shell, a
/// terminal or an editor hosting the user's session.
pub fn parent_eligible(name: &str, pid: u32) -> bool {
    if pid <= 1 {
        return false;
    }
    let n = name.trim_start_matches('-').to_lowercase();
    const NEVER: &[&str] = &[
        "sh",
        "bash",
        "zsh",
        "fish",
        "dash",
        "ksh",
        "tcsh",
        "csh",
        "nu",
        "tmux",
        "screen",
        "login",
        "sshd",
        "launchd",
        "systemd",
        "init",
        "terminal",
        "iterm2",
        "kitty",
        "alacritty",
        "wezterm-gui",
        "ghostty",
        "warp",
    ];
    !(NEVER.contains(&n.as_str())
        || n.starts_with("tmux")
        || n.starts_with("code helper")
        || n.starts_with("code - ")
        || (n.contains("cursor") && n.contains("helper")))
}

/// What kav will do to free a port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Signal a process. `start_time` guards against PID reuse (0 = unknown).
    Signal { pid: u32, start_time: u64 },
    /// Stop the container that publishes the port.
    DockerStop(ContainerInfo),
    /// Do nothing, for this reason.
    Refuse(String),
}

/// One process (or container) to act on, with every requested port it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KillTarget {
    pub pid: Option<u32>,
    pub name: String,
    pub ports: Vec<u16>,
    pub action: Action,
    pub warning: Option<&'static str>,
    /// Set when `--parent` redirected the kill from the listener to its parent.
    pub via_parent: Option<ProcessRef>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct KillOptions {
    /// Kill the listener's parent (e.g. a reloader that would respawn it).
    pub target_parent: bool,
    /// Kill protected processes anyway (never init or kav itself).
    pub allow_protected: bool,
}

/// Turn the rows holding the requested ports into one target per process or
/// container, merging ports.
pub fn plan(entries: &[&PortEntry], o: &KillOptions, env: &KillEnv) -> Vec<KillTarget> {
    let mut targets: Vec<KillTarget> = Vec::new();
    for e in entries {
        let target = target_for(e, o, env);
        let same = targets
            .iter_mut()
            .find(|t| match (&t.action, &target.action) {
                (Action::Signal { pid: a, .. }, Action::Signal { pid: b, .. }) => a == b,
                (Action::DockerStop(a), Action::DockerStop(b)) => a.id == b.id,
                (Action::Refuse(_), Action::Refuse(_)) => t.pid.is_some() && t.pid == target.pid,
                _ => false,
            });
        match same {
            Some(t) => {
                if !t.ports.contains(&e.port) {
                    t.ports.push(e.port);
                }
            }
            None => targets.push(target),
        }
    }
    targets
}

fn target_for(e: &PortEntry, o: &KillOptions, env: &KillEnv) -> KillTarget {
    let mut target = KillTarget {
        pid: e.pid,
        name: e.process_name.clone(),
        ports: vec![e.port],
        action: Action::Refuse(String::new()),
        warning: None,
        via_parent: None,
    };
    // A container's port is always freed with `docker stop`, even with --allow-protected:
    // killing the runtime's port forwarder would stop every container.
    if let Some(container) = e.container.as_ref() {
        target.name = container.name.clone();
        target.action = Action::DockerStop(container.clone());
        return target;
    }
    let Some(pid) = e.pid else {
        if let Protection::Refuse(reason) = protection(e, env) {
            target.action = Action::Refuse(reason);
        }
        return target;
    };
    if o.target_parent {
        target.action = match &e.parent {
            Some(p) if parent_eligible(&p.name, p.pid) && untouchable(p.pid, env).is_none() => {
                target.pid = Some(p.pid);
                target.name = p.name.clone();
                target.via_parent = Some(p.clone());
                Action::Signal {
                    pid: p.pid,
                    start_time: p.start_time,
                }
            }
            Some(p) => Action::Refuse(format!(
                "parent {} (PID {}) is a shell, terminal or system process",
                p.name, p.pid
            )),
            None => Action::Refuse(format!("{} has no parent process", e.process_name)),
        };
        return target;
    }
    target.action = match protection(e, env) {
        Protection::Refuse(reason) if !o.allow_protected || untouchable(pid, env).is_some() => {
            Action::Refuse(reason)
        }
        Protection::Warn(warning) => {
            target.warning = Some(warning);
            Action::Signal {
                pid,
                start_time: e.start_time,
            }
        }
        _ => Action::Signal {
            pid,
            start_time: e.start_time,
        },
    };
    target
}

/// Poll until each `(pid, start_time)` has exited (zombies count) or `timeout`
/// passes. Returns which ones exited, in order.
pub fn wait_for_exit(
    scanner: &mut Scanner,
    targets: &[(u32, u64)],
    timeout: Duration,
) -> Vec<bool> {
    let deadline = Instant::now() + timeout;
    let mut exited = vec![false; targets.len()];
    loop {
        for (done, &(pid, start)) in exited.iter_mut().zip(targets) {
            if !*done && !scanner.process_alive(pid, start) {
                *done = true;
            }
        }
        if exited.iter().all(|d| *d) || Instant::now() >= deadline {
            return exited;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::testutil::entry;
    use crate::models::Protocol;

    fn env() -> KillEnv {
        KillEnv {
            own_pid: 4242,
            in_ssh: false,
        }
    }

    fn opts() -> KillOptions {
        KillOptions::default()
    }

    fn qdrant() -> ContainerInfo {
        ContainerInfo {
            id: "517d48f91bc3".into(),
            name: "roo-qdrant".into(),
            image: "qdrant/qdrant".into(),
            compose_project: None,
        }
    }

    #[test]
    fn signal_parsing_variants() {
        for s in ["TERM", "SIGTERM", "term", "15"] {
            assert_eq!(s.parse::<Signal>(), Ok(Signal::Term), "{s}");
        }
        assert_eq!("kill".parse::<Signal>(), Ok(Signal::Kill));
        assert_eq!("9".parse::<Signal>(), Ok(Signal::Kill));
        assert_eq!("SIGHUP".parse::<Signal>(), Ok(Signal::Hup));
        assert!("bogus".parse::<Signal>().is_err());
        assert!("0".parse::<Signal>().is_err());
        assert_eq!(Signal::Term.to_string(), "SIGTERM");
    }

    #[test]
    fn plan_merges_ports_per_pid() {
        let a = entry(3000, 10, "node");
        let b = entry(3001, 10, "node");
        let mut c = entry(3000, 10, "node");
        c.protocol = Protocol::Udp;
        let targets = plan(&[&a, &b, &c], &opts(), &env());
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].ports, vec![3000, 3001]);
        assert_eq!(targets[0].pid, Some(10));
        assert!(matches!(targets[0].action, Action::Signal { pid: 10, .. }));
    }

    #[test]
    fn plan_refuses_docker_backend_without_container() {
        let e = entry(6333, 21743, "com.docker.backend");
        let targets = plan(&[&e], &opts(), &env());
        match &targets[0].action {
            Action::Refuse(reason) => assert!(reason.contains("container"), "{reason}"),
            other => panic!("expected refusal, got {other:?}"),
        }
        let allowed = KillOptions {
            allow_protected: true,
            ..opts()
        };
        assert!(matches!(
            plan(&[&e], &allowed, &env())[0].action,
            Action::Signal { pid: 21743, .. }
        ));
    }

    #[test]
    fn plan_uses_docker_stop_when_attributed() {
        let mut a = entry(6333, 21743, "com.docker.backend");
        a.container = Some(qdrant());
        let mut b = entry(6334, 21743, "com.docker.backend");
        b.container = Some(qdrant());
        let targets = plan(&[&a, &b], &opts(), &env());
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].action, Action::DockerStop(qdrant()));
        assert_eq!(targets[0].ports, vec![6333, 6334]);
        assert_eq!(targets[0].name, "roo-qdrant");
        // --allow-protected (perhaps meant for another port) must not turn this into
        // killing the runtime, which would stop every container.
        let allowed = KillOptions {
            allow_protected: true,
            ..opts()
        };
        assert_eq!(
            plan(&[&a], &allowed, &env())[0].action,
            Action::DockerStop(qdrant())
        );
    }

    #[test]
    fn plan_refuses_pid_one_and_self_even_when_allowed() {
        let init = entry(8021, 1, "launchd");
        let me = entry(9000, 4242, "kav");
        let allowed = KillOptions {
            allow_protected: true,
            ..opts()
        };
        for o in [opts(), allowed] {
            let targets = plan(&[&init, &me], &o, &env());
            assert!(matches!(targets[0].action, Action::Refuse(_)), "pid 1");
            assert!(matches!(targets[1].action, Action::Refuse(_)), "self");
        }
    }

    #[test]
    fn plan_refuses_sshd_in_ssh_only() {
        let e = entry(22, 500, "sshd");
        let ssh = KillEnv {
            in_ssh: true,
            ..env()
        };
        assert!(matches!(
            plan(&[&e], &opts(), &ssh)[0].action,
            Action::Refuse(_)
        ));
        assert!(matches!(
            plan(&[&e], &opts(), &env())[0].action,
            Action::Signal { pid: 500, .. }
        ));
    }

    #[test]
    fn plan_parent_replaces_target_when_eligible() {
        let mut e = entry(3000, 200, "node");
        e.parent = Some(ProcessRef {
            pid: 100,
            name: "nodemon".into(),
            start_time: 1_700_000_123,
        });
        let o = KillOptions {
            target_parent: true,
            ..opts()
        };
        let t = &plan(&[&e], &o, &env())[0];
        assert_eq!(t.pid, Some(100));
        assert_eq!(t.name, "nodemon");
        assert_eq!(t.via_parent.as_ref().map(|p| p.pid), Some(100));
        // The parent's start time travels with the plan, so a reused PID is never signalled.
        assert!(
            matches!(
                t.action,
                Action::Signal {
                    pid: 100,
                    start_time: 1_700_000_123
                }
            ),
            "{:?}",
            t.action
        );
    }

    #[test]
    fn plan_parent_refused_for_shell() {
        let mut e = entry(3000, 200, "node");
        e.parent = Some(ProcessRef {
            pid: 90,
            name: "zsh".into(),
            start_time: 0,
        });
        let o = KillOptions {
            target_parent: true,
            ..opts()
        };
        assert!(matches!(
            plan(&[&e], &o, &env())[0].action,
            Action::Refuse(_)
        ));
        assert!(!parent_eligible("-zsh", 91));
        assert!(!parent_eligible("Code Helper (Plugin)", 92));
        assert!(!parent_eligible("launchd", 1));
        assert!(parent_eligible("npm", 93));
    }

    #[test]
    fn ownerless_is_refused() {
        let mut e = entry(47005, 1, "?");
        e.pid = None;
        match &plan(&[&e], &opts(), &env())[0].action {
            Action::Refuse(reason) => assert!(reason.contains("sudo"), "{reason}"),
            other => panic!("expected refusal, got {other:?}"),
        }
    }

    #[test]
    fn controlcenter_warns() {
        let e = entry(5000, 1239, "ControlCenter");
        let t = &plan(&[&e], &opts(), &env())[0];
        assert!(matches!(t.action, Action::Signal { pid: 1239, .. }));
        assert!(t.warning.unwrap_or_default().contains("AirPlay"));
    }

    #[test]
    fn send_to_exited_pid_is_no_such_process() {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert_eq!(send(pid, Signal::Term), Err(SendError::NoSuchProcess));
    }

    #[test]
    fn send_refuses_group_pids() {
        assert!(matches!(send(0, Signal::Term), Err(SendError::Other(_))));
        assert!(matches!(
            send(u32::MAX, Signal::Term),
            Err(SendError::Other(_))
        ));
    }

    #[test]
    fn process_alive_false_on_start_time_mismatch() {
        let mut scanner = Scanner::new();
        let me = std::process::id();
        assert!(
            scanner.process_alive(me, 0),
            "unknown start time accepts the pid"
        );
        assert!(
            !scanner.process_alive(me, 1),
            "different start time is PID reuse"
        );
    }

    #[test]
    fn wait_for_exit_sees_terminated_child() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut scanner = Scanner::new();
        assert_eq!(
            wait_for_exit(&mut scanner, &[(pid, 0)], Duration::from_millis(200)),
            vec![false],
            "still running"
        );
        send(pid, Signal::Term).unwrap();
        assert_eq!(
            wait_for_exit(&mut scanner, &[(pid, 0)], Duration::from_secs(5)),
            vec![true]
        );
        let _ = child.wait();
    }
}
