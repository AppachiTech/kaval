//! Helper listener processes for end-to-end tests.
//!
//! The integration-test binary re-executes itself with `--exact helper_process` and
//! `KAV_HELPER=<mode>`. The `helper_process` test (in tests/cli.rs) calls
//! [`run_helper_from_env`], which opens the requested socket(s), prints `PORT=<n>`
//! and sleeps until it is killed. Running a separate process means `kav` sees a real
//! listener with its own PID, command line and CPU usage.

#![allow(dead_code)] // each test binary uses a different subset of these helpers

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// A running helper process that listens on `port`. Killed on drop.
pub struct Helper {
    pub pid: u32,
    pub port: u16,
    child: Child,
    /// Grandchild listener started by `orphaner`, killed on drop.
    orphan: Option<u32>,
}

impl Helper {
    /// Spawn a helper. Modes: `tcp`, `tcpudp`, `busy`, `noterm`, `v6`, `exposed`, and
    /// `supervisor` (keeps a child listener alive on one port, restarting it when it
    /// exits; on SIGTERM it stops the child and exits, like nodemon or pm2).
    pub fn spawn(mode: &str) -> Helper {
        Self::spawn_with(mode, &[])
    }

    /// Spawn a helper with extra environment (e.g. `fixed` + `KAV_HELPER_PORT`).
    pub fn spawn_with(mode: &str, env: &[(&str, String)]) -> Helper {
        let mut cmd = helper_command(mode);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn helper process");
        let stdout = child.stdout.take().expect("helper stdout");
        // libtest prints "test helper_process ... " before the test body runs, so the
        // marker can appear mid-line. Read on a thread so a broken helper can't hang us.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let field = |key: &str| {
                    line.split_whitespace()
                        .find_map(|w| w.strip_prefix(key))
                        .and_then(|v| v.parse::<u32>().ok())
                };
                if let Some(port) = field("PORT=") {
                    let _ = tx.send(Some((port as u16, field("CHILD="))));
                    return;
                }
            }
            let _ = tx.send(None);
        });
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Some((port, orphan))) => Helper {
                pid: child.id(),
                port,
                child,
                orphan,
            },
            other => {
                let _ = child.kill();
                panic!("helper ({mode}) did not report a port: {other:?}");
            }
        }
    }

    /// True while the helper process has not exited.
    pub fn is_running(&mut self) -> bool {
        self.child.try_wait().expect("try_wait").is_none()
    }

    /// Wait up to `timeout` for the helper to exit.
    pub fn wait_exit(&mut self, timeout: Duration) -> Option<ExitStatus> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                return Some(status);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        // SIGTERM first so a supervisor can stop its child; SIGKILL whatever ignores it.
        unsafe {
            libc::kill(self.pid as libc::pid_t, libc::SIGTERM);
        }
        if self.wait_exit(Duration::from_millis(500)).is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(orphan) = self.orphan {
            unsafe {
                libc::kill(orphan as libc::pid_t, libc::SIGKILL);
            }
        }
    }
}

fn helper_command(mode: &str) -> Command {
    let mut cmd = Command::new(std::env::current_exe().expect("current_exe"));
    cmd.args([
        "--exact",
        "helper_process",
        "--nocapture",
        "--test-threads=1",
    ])
    .env("KAV_HELPER", mode);
    cmd
}

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_sigterm(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

fn wait_listening(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "child never listened on {port}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Parent that starts a `fixed` child and then just sleeps: on SIGTERM it dies
/// without stopping the child, leaving the listener orphaned.
#[allow(clippy::zombie_processes)] // orphaning the child is the point; tests kill it
fn run_orphaner() -> ! {
    let port = TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("pick port")
        .port();
    let child = helper_command("fixed")
        .env("KAV_HELPER_PORT", port.to_string())
        .stdout(Stdio::null())
        .spawn()
        .expect("spawn child");
    wait_listening(port);
    println!("PORT={port} CHILD={}", child.id());
    std::io::stdout().flush().expect("flush");
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Parent process that keeps a `fixed` child listening on one port.
fn run_supervisor() -> ! {
    let port = TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("pick port")
        .port();
    unsafe {
        libc::signal(libc::SIGTERM, on_sigterm as *const () as libc::sighandler_t);
    }
    let spawn = || {
        helper_command("fixed")
            .env("KAV_HELPER_PORT", port.to_string())
            .stdout(Stdio::null())
            .spawn()
            .expect("spawn child")
    };
    let mut child = spawn();
    wait_listening(port);
    println!("PORT={port}");
    std::io::stdout().flush().expect("flush");
    loop {
        if STOP.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            std::process::exit(0);
        }
        if child.try_wait().expect("try_wait").is_some() {
            child = spawn();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Run the `kav` binary under test with `args`.
pub fn kav(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kav"))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run kav")
}

/// Entry point for the re-executed helper. Returns immediately unless `KAV_HELPER` is set.
pub fn run_helper_from_env() {
    let Ok(mode) = std::env::var("KAV_HELPER") else {
        return;
    };
    if mode == "supervisor" {
        run_supervisor();
    }
    if mode == "orphaner" {
        run_orphaner();
    }
    let mut held_udp = None;
    let tcp = match mode.as_str() {
        "v6" => TcpListener::bind("[::1]:0"),
        "exposed" => TcpListener::bind("0.0.0.0:0"),
        "fixed" => {
            let port: u16 = std::env::var("KAV_HELPER_PORT")
                .expect("KAV_HELPER_PORT")
                .parse()
                .expect("port");
            TcpListener::bind(("127.0.0.1", port))
        }
        _ => TcpListener::bind("127.0.0.1:0"),
    }
    .expect("bind helper listener");
    let port = tcp.local_addr().expect("local_addr").port();
    match mode.as_str() {
        "tcpudp" => {
            held_udp = Some(UdpSocket::bind(("127.0.0.1", port)).expect("bind helper udp"));
        }
        "busy" => {
            std::thread::spawn(|| {
                let mut x: u64 = 0;
                loop {
                    x = std::hint::black_box(x.wrapping_add(1));
                }
            });
        }
        "noterm" => unsafe {
            libc::signal(libc::SIGTERM, libc::SIG_IGN);
        },
        _ => {}
    }
    println!("PORT={port}");
    std::io::stdout().flush().expect("flush");
    let _keep = (tcp, held_udp);
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}
