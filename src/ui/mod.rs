//! Interactive TUI.

mod app;
mod clipboard;
mod draw;

use std::io::{self, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        cursor,
        event::{self, DisableMouseCapture, EnableMouseCapture, Event},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    Terminal,
};

use crate::docker;
use crate::killer::{self, Action, KillTarget, SendError, Signal};
use crate::probe;
use crate::scanner::{ContainerMode, ScanOptions, Scanner};
use app::{App, Effect};

/// How often the loop wakes up even without input (status expiry, background results).
const TICK: Duration = Duration::from_millis(250);
const DOCKER_STOP_TIMEOUT: Duration = Duration::from_secs(30);

pub struct TuiOptions {
    pub interval: Duration,
    pub mouse: bool,
}

/// Puts the terminal into TUI mode and restores it however the TUI exits —
/// a normal return, an error propagated with `?`, or a panic.
pub struct TerminalGuard<W: Write> {
    out: W,
    mouse: bool,
    active: bool,
}

impl<W: Write> TerminalGuard<W> {
    pub fn enter(out: W, mouse: bool) -> io::Result<Self> {
        let mut guard = TerminalGuard {
            out,
            mouse,
            active: false,
        };
        guard.activate()?;
        Ok(guard)
    }

    fn activate(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        self.active = true; // from here on, Drop restores even if the rest fails
        execute!(self.out, EnterAlternateScreen, cursor::Hide)?;
        if self.mouse {
            execute!(self.out, EnableMouseCapture)?;
        }
        Ok(())
    }

    fn restore(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        if self.mouse {
            let _ = execute!(self.out, DisableMouseCapture);
        }
        let _ = execute!(self.out, LeaveAlternateScreen, cursor::Show);
        let _ = self.out.flush();
        let _ = disable_raw_mode();
    }

    /// Ctrl+Z: give the terminal back, stop until the shell resumes us, re-enter.
    pub fn suspend(&mut self) -> io::Result<()> {
        self.restore();
        // SAFETY: raise(SIGTSTP) stops this process until it receives SIGCONT.
        unsafe {
            libc::raise(libc::SIGTSTP);
        }
        self.activate()
    }
}

impl<W: Write> Drop for TerminalGuard<W> {
    fn drop(&mut self) {
        self.restore();
    }
}

/// Restore the terminal before the default panic message prints.
fn install_panic_hook(mouse: bool) {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut out = io::stdout();
        if mouse {
            let _ = execute!(out, DisableMouseCapture);
        }
        let _ = execute!(out, LeaveAlternateScreen, cursor::Show);
        let _ = disable_raw_mode();
        original(info);
    }));
}

/// Results from work done off the UI thread.
enum Background {
    ContainerStopped {
        name: String,
        result: std::result::Result<(), String>,
    },
    Probe {
        port: u16,
        health: probe::Health,
    },
}

fn scan(app: &mut App, scanner: &mut Scanner, sample_cpu: bool) {
    let opts = ScanOptions {
        tcp: app.show_tcp,
        udp: app.show_udp,
        sample_cpu,
        containers: ContainerMode::Background,
    };
    match scanner.scan(&opts) {
        Ok(snapshot) => app.apply_snapshot(snapshot, Instant::now()),
        Err(e) => app.set_status(format!("Scan error: {e}"), true),
    }
}

fn kill(
    app: &mut App,
    scanner: &mut Scanner,
    targets: Vec<KillTarget>,
    signal: Signal,
    tx: &Sender<Background>,
) {
    for target in targets {
        match target.action {
            Action::Signal { pid, start_time } => {
                if !scanner.process_alive(pid, start_time) {
                    app.set_status(format!("{} (PID {pid}) already exited", target.name), false);
                    continue;
                }
                match killer::send(pid, signal) {
                    Ok(()) | Err(SendError::NoSuchProcess) => {
                        app.set_status(
                            format!("Sent {signal} to {} (PID {pid})", target.name),
                            false,
                        );
                        app.kill_sent(&target, signal, Instant::now());
                    }
                    Err(e) => app.set_status(format!("Kill failed: {e}"), true),
                }
            }
            Action::DockerStop(container) => {
                app.set_status(format!("Stopping container {}…", container.name), false);
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let result =
                        docker::stop(&container, DOCKER_STOP_TIMEOUT).map_err(|e| format!("{e:#}"));
                    let _ = tx.send(Background::ContainerStopped {
                        name: container.name,
                        result,
                    });
                });
            }
            Action::Refuse(reason) => app.set_status(format!("Not killing: {reason}"), true),
        }
    }
}

fn on_background(app: &mut App, msg: Background) {
    match msg {
        Background::ContainerStopped { name, result } => match result {
            Ok(()) => app.set_status(format!("Stopped container {name}"), false),
            Err(e) => app.set_status(format!("docker stop {name} failed: {e}"), true),
        },
        Background::Probe { port, health } => {
            app.set_status(format!("Port {port}: {}", probe::describe(&health)), false);
            app.probe_done(port, health, Instant::now());
        }
    }
}

pub fn run(opts: TuiOptions) -> Result<()> {
    install_panic_hook(opts.mouse);
    let mut guard = TerminalGuard::enter(io::stdout(), opts.mouse)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut app = App::new();
    let mut scanner = Scanner::new();
    let (tx, rx): (Sender<Background>, Receiver<Background>) = mpsc::channel();

    // Draw the loading frame before the first (CPU-sampling) scan.
    terminal.draw(|f| draw::draw(f, &mut app))?;
    scan(&mut app, &mut scanner, true);
    let mut last_scan = Instant::now();

    loop {
        terminal.draw(|f| draw::draw(f, &mut app))?;

        let until_scan = opts.interval.saturating_sub(last_scan.elapsed());
        if event::poll(until_scan.min(TICK))? {
            let effects = match event::read()? {
                Event::Key(key) => app.handle_key(key),
                Event::Mouse(mouse) => {
                    let area = app.table_area;
                    app.handle_mouse(mouse, area)
                }
                _ => Vec::new(),
            };
            for effect in effects {
                match effect {
                    Effect::Quit => return Ok(()),
                    Effect::Refresh => {
                        scan(&mut app, &mut scanner, false);
                        last_scan = Instant::now();
                    }
                    Effect::Copy { text, what } => match clipboard::copy(&text) {
                        Ok(how) => app.set_status(format!("Copied {what} via {how}"), false),
                        Err(e) => app.set_status(e, true),
                    },
                    Effect::Open(url) => {
                        if clipboard::open_url(&url) {
                            app.set_status(format!("Opening {url}"), false);
                        } else {
                            app.set_status("No browser opener available", true);
                        }
                    }
                    Effect::Kill { targets, signal } => {
                        kill(&mut app, &mut scanner, targets, signal, &tx);
                        scan(&mut app, &mut scanner, false);
                        last_scan = Instant::now();
                    }
                    Effect::Probe { ip, port, http } => {
                        let tx = tx.clone();
                        std::thread::spawn(move || {
                            let health = probe::health(ip, port, http, Duration::from_secs(2));
                            let _ = tx.send(Background::Probe { port, health });
                        });
                    }
                    Effect::Suspend => {
                        guard.suspend()?;
                        // A fresh Terminal repaints everything on the next draw.
                        // (Terminal::clear would query the cursor position, which
                        // some terminals never answer.)
                        terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
                    }
                }
            }
        }

        while let Ok(msg) = rx.try_recv() {
            on_background(&mut app, msg);
        }

        if last_scan.elapsed() >= opts.interval {
            scan(&mut app, &mut scanner, false);
            last_scan = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A writer that records everything for inspection.
    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<u8>>>);

    impl Write for Recorder {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn guard_restores_on_drop() {
        let recorder = Recorder::default();
        {
            // An active guard, as if `enter` had succeeded (no real TTY in tests).
            let _guard = TerminalGuard {
                out: recorder.clone(),
                mouse: true,
                active: true,
            };
        }
        let written = String::from_utf8(recorder.0.lock().unwrap().clone()).unwrap();
        assert!(
            written.contains("\x1b[?1000l"),
            "mouse capture disabled: {written:?}"
        );
        assert!(
            written.contains("\x1b[?1049l"),
            "alternate screen left: {written:?}"
        );
        assert!(written.contains("\x1b[?25h"), "cursor shown: {written:?}");
    }

    #[test]
    fn inactive_guard_writes_nothing() {
        let recorder = Recorder::default();
        drop(TerminalGuard {
            out: recorder.clone(),
            mouse: true,
            active: false,
        });
        assert!(recorder.0.lock().unwrap().is_empty());
    }
}
