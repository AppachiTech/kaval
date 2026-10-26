//! Drives the real TUI through a pseudo-terminal: drawing, key handling,
//! suspend/resume and — above all — that the terminal is restored on exit.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::FromRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Pty {
    master: File,
    child: Child,
    output: Arc<Mutex<Vec<u8>>>,
}

impl Pty {
    /// Start `kav args…` on a fresh `cols`×`rows` pseudo-terminal.
    fn spawn(args: &[&str], cols: u16, rows: u16) -> Pty {
        let (mut master, mut slave) = (0, 0);
        let mut size = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: openpty fills in two fresh descriptors we take ownership of.
        let rc = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        assert_eq!(rc, 0, "openpty failed");
        let slave = unsafe { File::from_raw_fd(slave) };
        let master = unsafe { File::from_raw_fd(master) };

        let mut cmd = Command::new(env!("CARGO_BIN_EXE_kav"));
        cmd.args(args)
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave))
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env_remove("NO_COLOR");
        // A new session: Ctrl+Z's SIGTSTP is discarded for an orphaned process
        // group instead of stopping kav (and this test) for good.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let child = cmd.spawn().expect("spawn kav");
        drop(cmd); // close our copies of the slave so reads end when kav exits

        let output = Arc::new(Mutex::new(Vec::new()));
        let mut reader = master.try_clone().unwrap();
        let sink = Arc::clone(&output);
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                sink.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });
        Pty {
            master,
            child,
            output,
        }
    }

    fn output(&self) -> Vec<u8> {
        self.output.lock().unwrap().clone()
    }

    fn contains(&self, needle: &[u8]) -> bool {
        self.output().windows(needle.len()).any(|w| w == needle)
    }

    /// Wait until the output contains `needle`.
    fn wait_for(&self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !self.contains(needle.as_bytes()) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {needle:?}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.master.write_all(bytes).unwrap();
        std::thread::sleep(Duration::from_millis(400));
    }

    /// Wait for kav to exit; returns its exit code and everything it printed.
    fn finish(mut self) -> (Option<i32>, Vec<u8>) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                std::thread::sleep(Duration::from_millis(200)); // let the reader drain
                return (status.code(), self.output());
            }
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                panic!("kav did not exit");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

const ENTER_ALT: &[u8] = b"\x1b[?1049h";
const LEAVE_ALT: &[u8] = b"\x1b[?1049l";

#[test]
fn tui_draws_help_and_restores_the_terminal() {
    let mut tui = Pty::spawn(&[], 120, 40);
    tui.wait_for("Guard your ports");
    tui.send(b"?");
    tui.wait_for("Keyboard Shortcuts");
    tui.send(b"x"); // any key dismisses help
    tui.send(b"/zz");
    tui.send(b"\x1b"); // Esc leaves the filter without quitting
    tui.send(b"q");
    let (code, out) = tui.finish();
    assert_eq!(code, Some(0));
    assert!(count(&out, b"\x1b[?1000h") >= 1, "mouse captured");
    assert!(count(&out, b"\x1b[?1000l") >= 1, "mouse released");
    let last_enter = out.windows(ENTER_ALT.len()).rposition(|w| w == ENTER_ALT);
    let last_leave = out.windows(LEAVE_ALT.len()).rposition(|w| w == LEAVE_ALT);
    assert!(last_leave > last_enter, "alternate screen left at exit");
}

#[test]
fn tui_suspend_resumes_and_redraws() {
    let mut tui = Pty::spawn(&[], 100, 30);
    tui.wait_for("Guard your ports");
    tui.send(b"\x1a"); // Ctrl+Z
    std::thread::sleep(Duration::from_millis(600));
    tui.send(b"q");
    let (code, out) = tui.finish();
    assert_eq!(code, Some(0), "{}", String::from_utf8_lossy(&out));
    assert_eq!(count(&out, ENTER_ALT), 2, "re-entered after resume");
    assert_eq!(count(&out, LEAVE_ALT), 2, "left on suspend and on quit");
    assert!(!String::from_utf8_lossy(&out).contains("kav: error"));
}

#[test]
fn tui_no_mouse_leaves_mouse_alone() {
    let mut tui = Pty::spawn(&["--no-mouse"], 100, 30);
    tui.wait_for("Guard your ports");
    tui.send(b"\x03"); // Ctrl+C quits
    let (code, out) = tui.finish();
    assert_eq!(code, Some(0));
    assert_eq!(
        count(&out, b"\x1b[?1000h"),
        0,
        "no mouse capture with --no-mouse"
    );
}
