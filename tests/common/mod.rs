//! Helper listener processes for end-to-end tests.
//!
//! The integration-test binary re-executes itself with `--exact helper_process` and
//! `KAV_HELPER=<mode>`. The `helper_process` test (in tests/cli.rs) calls
//! [`run_helper_from_env`], which opens the requested socket(s), prints `PORT=<n>`
//! and sleeps until it is killed. Running a separate process means `kav` sees a real
//! listener with its own PID, command line and CPU usage.

#![allow(dead_code)] // each test binary uses a different subset of these helpers

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, UdpSocket};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

/// A running helper process that listens on `port`. Killed on drop.
pub struct Helper {
    pub pid: u32,
    pub port: u16,
    child: Child,
}

impl Helper {
    /// Spawn a helper. Modes: `tcp`, `tcpudp`, `busy`, `noterm`, `v6`, `exposed`.
    pub fn spawn(mode: &str) -> Helper {
        let mut child = Command::new(std::env::current_exe().expect("current_exe"))
            .args([
                "--exact",
                "helper_process",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("KAV_HELPER", mode)
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
                if let Some(port) = line.split("PORT=").nth(1) {
                    let _ = tx.send(port.trim().parse::<u16>().ok());
                    return;
                }
            }
            let _ = tx.send(None);
        });
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Some(port)) => Helper {
                pid: child.id(),
                port,
                child,
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
        let _ = self.child.kill();
        let _ = self.child.wait();
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
    let mut held_udp = None;
    let tcp = match mode.as_str() {
        "v6" => TcpListener::bind("[::1]:0"),
        "exposed" => TcpListener::bind("0.0.0.0:0"),
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
