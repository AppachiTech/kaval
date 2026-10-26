//! `kav kill`: free ports by stopping the processes (or containers) behind them.

use std::collections::HashSet;
use std::io::{self, BufRead, IsTerminal, Write};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::Result;

use crate::cmd::check::{dedup_ports, hidden_in_use};
use crate::docker;
use crate::killer::{self, Action, KillEnv, KillOptions, KillTarget, SendError, Signal};
use crate::models::PortEntry;
use crate::output::json;
use crate::scanner::{ContainerMode, ScanOptions, Scanner};

const DOCKER_STOP_TIMEOUT: Duration = Duration::from_secs(30);
/// Extra wait after escalating to SIGKILL.
const ESCALATION_GRACE: Duration = Duration::from_secs(2);
/// Pause before checking whether a supervisor re-bound a freed port.
const REBIND_GRACE: Duration = Duration::from_millis(300);

pub struct KillArgs {
    pub ports: Vec<u16>,
    pub signal: Signal,
    pub yes: bool,
    pub timeout: Duration,
    pub no_wait: bool,
    pub escalate: bool,
    pub parent: bool,
    pub allow_protected: bool,
    pub json: bool,
}

/// What happened to one target (or one idle port).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Exited,
    SignalSent,
    Stopped,
    StillRunning,
    Respawned,
    Failed,
    Refused,
    Declined,
    NotFound,
}

impl Outcome {
    fn name(self) -> &'static str {
        match self {
            Outcome::Exited => "exited",
            Outcome::SignalSent => "signal_sent",
            Outcome::Stopped => "stopped",
            Outcome::StillRunning => "still_running",
            Outcome::Respawned => "respawned",
            Outcome::Failed => "failed",
            Outcome::Refused => "refused",
            Outcome::Declined => "declined",
            Outcome::NotFound => "not_found",
        }
    }
}

/// Exit status: 2 if anything failed or was refused, 3 if a port is still
/// busy (timed out or respawned), 0 if something was freed, otherwise 1.
pub(crate) fn exit_code(outcomes: &[Outcome]) -> u8 {
    use Outcome::*;
    if outcomes.iter().any(|o| matches!(o, Failed | Refused)) {
        2
    } else if outcomes
        .iter()
        .any(|o| matches!(o, StillRunning | Respawned))
    {
        3
    } else if outcomes
        .iter()
        .any(|o| matches!(o, Exited | Stopped | SignalSent))
    {
        0
    } else {
        1
    }
}

pub(crate) fn is_yes(answer: &str) -> bool {
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// Ask on stderr (so stdout stays clean for pipes) and read the answer.
fn confirm(question: &str) -> io::Result<bool> {
    eprint!("{question} [y/N] ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    Ok(is_yes(&answer))
}

struct Report {
    ports: Vec<u16>,
    pid: Option<u32>,
    name: Option<String>,
    action: &'static str,
    signal: Option<Signal>,
    outcome: Outcome,
    message: Option<String>,
}

impl Report {
    fn port_only(port: u16, outcome: Outcome, message: Option<String>) -> Self {
        Report {
            ports: vec![port],
            pid: None,
            name: None,
            action: "none",
            signal: None,
            outcome,
            message,
        }
    }

    fn for_target(
        t: &KillTarget,
        action: &'static str,
        signal: Option<Signal>,
        outcome: Outcome,
    ) -> Self {
        Report {
            ports: t.ports.clone(),
            pid: t.pid,
            name: Some(t.name.clone()),
            action,
            signal,
            outcome,
            message: None,
        }
    }

    fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    fn who(&self) -> String {
        match (&self.name, self.pid) {
            (Some(name), Some(pid)) if self.action != "docker_stop" => {
                format!("{name} (PID {pid})")
            }
            (Some(name), _) => name.clone(),
            (None, _) => "the owner".to_string(),
        }
    }

    fn ports_text(&self) -> String {
        let ports: Vec<String> = self.ports.iter().map(u16::to_string).collect();
        ports.join(", ")
    }
}

fn ports_text(ports: &[u16]) -> String {
    let ports: Vec<String> = ports.iter().map(u16::to_string).collect();
    ports.join(", ")
}

pub fn run(args: &KillArgs) -> Result<ExitCode> {
    let ports = dedup_ports(&args.ports);
    let mut scanner = Scanner::new();
    let snapshot = scanner.scan(&ScanOptions {
        tcp: true,
        udp: true,
        sample_cpu: false,
        containers: ContainerMode::Blocking,
    })?;
    let env = KillEnv::current();
    let options = KillOptions {
        target_parent: args.parent,
        allow_protected: args.allow_protected,
    };
    let interactive = !args.yes && !args.json && io::stdin().is_terminal();
    let mut reports: Vec<Report> = Vec::new();

    let mut rows: Vec<&PortEntry> = Vec::new();
    for &port in &ports {
        let matched: Vec<&PortEntry> = snapshot.entries.iter().filter(|e| e.port == port).collect();
        if matched.is_empty() {
            reports.push(if hidden_in_use(port) {
                Report::port_only(
                    port,
                    Outcome::Refused,
                    Some("in use by a process kav can't see (another user's) — try sudo".into()),
                )
            } else {
                Report::port_only(port, Outcome::NotFound, None)
            });
        }
        rows.extend(matched);
    }
    // Processes that held the ports before we acted; anything else listening
    // there afterwards was started (or restarted) in the meantime.
    let original_pids: HashSet<u32> = rows.iter().filter_map(|e| e.pid).collect();

    let mut waiting: Vec<(usize, (u32, u64))> = Vec::new();
    for target in killer::plan(&rows, &options, &env) {
        if let Some(warning) = target.warning {
            eprintln!("Note: {warning}");
        }
        let ports = ports_text(&target.ports);
        match &target.action {
            Action::Refuse(reason) => {
                let overridable = !args.allow_protected
                    && !args.parent
                    && target.pid.is_some_and(|p| p > 1 && p != env.own_pid);
                let hint = if overridable {
                    " (--allow-protected overrides)"
                } else {
                    ""
                };
                reports.push(
                    Report::for_target(&target, "none", None, Outcome::Refused)
                        .with_message(format!("{reason}{hint}")),
                );
            }
            Action::DockerStop(container) => {
                let question = format!(
                    "Stop container {} ({}) that publishes port {ports}?",
                    container.name, container.image
                );
                if interactive && !confirm(&question)? {
                    reports.push(Report::for_target(
                        &target,
                        "docker_stop",
                        None,
                        Outcome::Declined,
                    ));
                    continue;
                }
                let report = Report::for_target(&target, "docker_stop", None, Outcome::Stopped);
                reports.push(match docker::stop(container, DOCKER_STOP_TIMEOUT) {
                    Ok(()) => report,
                    Err(e) => Report {
                        outcome: Outcome::Failed,
                        ..report
                    }
                    .with_message(format!("{e:#}")),
                });
            }
            Action::Signal { pid, start_time } => {
                let question = match &target.via_parent {
                    Some(_) => format!(
                        "Send {} to {} (PID {pid}), the parent of the listener on port {ports}?",
                        args.signal, target.name
                    ),
                    None => format!(
                        "Send {} to {} (PID {pid}) on port {ports}?",
                        args.signal, target.name
                    ),
                };
                if interactive && !confirm(&question)? {
                    reports.push(Report::for_target(
                        &target,
                        "signal",
                        Some(args.signal),
                        Outcome::Declined,
                    ));
                    continue;
                }
                let report =
                    Report::for_target(&target, "signal", Some(args.signal), Outcome::Exited);
                if !scanner.process_alive(*pid, *start_time) {
                    reports.push(report.with_message("it had already exited"));
                    continue;
                }
                match killer::send(*pid, args.signal) {
                    Ok(()) if args.no_wait => reports.push(Report {
                        outcome: Outcome::SignalSent,
                        ..report
                    }),
                    Ok(()) => {
                        waiting.push((reports.len(), (*pid, *start_time)));
                        reports.push(report);
                    }
                    Err(SendError::NoSuchProcess) => {
                        reports.push(report.with_message("it had already exited"))
                    }
                    Err(e) => reports.push(
                        Report {
                            outcome: Outcome::Failed,
                            ..report
                        }
                        .with_message(e.to_string()),
                    ),
                }
            }
        }
    }

    if !waiting.is_empty() {
        let targets: Vec<(u32, u64)> = waiting.iter().map(|(_, t)| *t).collect();
        let mut exited = killer::wait_for_exit(&mut scanner, &targets, args.timeout);
        if args.escalate && args.signal != Signal::Kill {
            let stubborn: Vec<usize> = (0..targets.len()).filter(|&i| !exited[i]).collect();
            for &i in &stubborn {
                let _ = killer::send(targets[i].0, Signal::Kill);
                reports[waiting[i].0].message = Some(format!(
                    "escalated to SIGKILL after {}s",
                    args.timeout.as_secs_f64()
                ));
            }
            let again = killer::wait_for_exit(
                &mut scanner,
                &stubborn.iter().map(|&i| targets[i]).collect::<Vec<_>>(),
                ESCALATION_GRACE,
            );
            for (&i, done) in stubborn.iter().zip(again) {
                exited[i] = done;
            }
        }
        for (i, (idx, _)) in waiting.iter().enumerate() {
            if !exited[i] {
                let r = &mut reports[*idx];
                r.outcome = Outcome::StillRunning;
                r.message = Some(format!(
                    "still running {}s after {} — retry with --force or --escalate",
                    args.timeout.as_secs_f64(),
                    r.signal.unwrap_or(args.signal)
                ));
            }
        }
        detect_respawns(&mut scanner, &mut reports, &waiting, &original_pids);
    }

    print_reports(&reports, args.json)?;
    let outcomes: Vec<Outcome> = reports.iter().map(|r| r.outcome).collect();
    Ok(ExitCode::from(exit_code(&outcomes)))
}

/// After the targets exit, make sure their ports really are free: a
/// supervisor may have restarted the listener (respawned), or the original
/// listener may have outlived the process we stopped (e.g. `--parent` on a
/// parent that does not stop its children).
fn detect_respawns(
    scanner: &mut Scanner,
    reports: &mut [Report],
    waiting: &[(usize, (u32, u64))],
    original_pids: &HashSet<u32>,
) {
    let freed: Vec<usize> = waiting
        .iter()
        .map(|(idx, _)| *idx)
        .filter(|&idx| reports[idx].outcome == Outcome::Exited)
        .collect();
    if freed.is_empty() {
        return;
    }
    // Targets still running already say so in their own report.
    let still_running: HashSet<u32> = waiting
        .iter()
        .filter(|(idx, _)| reports[*idx].outcome == Outcome::StillRunning)
        .map(|(_, (pid, _))| *pid)
        .collect();
    std::thread::sleep(REBIND_GRACE);
    let Ok(after) = scanner.scan(&ScanOptions {
        sample_cpu: false,
        containers: ContainerMode::Off,
        ..ScanOptions::default()
    }) else {
        return;
    };
    for idx in freed {
        let r = &mut reports[idx];
        let holder = after.entries.iter().find(|e| {
            r.ports.contains(&e.port)
                && e.pid
                    .is_none_or(|p| Some(p) != r.pid && !still_running.contains(&p))
        });
        let Some(e) = holder else { continue };
        let pid = e.pid.map_or_else(|| "?".to_string(), |p| p.to_string());
        if e.pid.is_some_and(|p| original_pids.contains(&p)) {
            r.outcome = Outcome::StillRunning;
            r.message = Some(format!(
                "port {} is still held by {} (PID {pid}), which outlived it",
                e.port, e.process_name
            ));
        } else {
            let parent = e
                .parent
                .as_ref()
                .map(|p| format!(", started by {} (PID {})", p.name, p.pid))
                .unwrap_or_default();
            r.outcome = Outcome::Respawned;
            r.message = Some(format!(
                "port {} was taken again by {} (PID {pid}){parent} — stop the parent with --parent",
                e.port, e.process_name
            ));
        }
    }
}

fn print_reports(reports: &[Report], as_json: bool) -> Result<()> {
    if as_json {
        let doc = json::KillDoc::new(
            reports
                .iter()
                .map(|r| json::KillResultJson {
                    ports: r.ports.clone(),
                    pid: r.pid,
                    process: r.name.clone(),
                    action: r.action,
                    signal: r.signal.map(|s| s.to_string()),
                    outcome: r.outcome.name(),
                    message: r.message.clone(),
                })
                .collect(),
        );
        return json::write_pretty(&doc, &mut io::stdout().lock());
    }
    let mut out = io::stdout().lock();
    for r in reports {
        let (who, ports) = (r.who(), r.ports_text());
        let extra = r
            .message
            .as_deref()
            .map(|m| format!(" ({m})"))
            .unwrap_or_default();
        match r.outcome {
            Outcome::NotFound => writeln!(out, "Nothing listening on port {ports}")?,
            Outcome::Exited => writeln!(out, "Stopped {who} — port {ports} is free{extra}")?,
            Outcome::SignalSent => writeln!(
                out,
                "Sent {} to {who} on port {ports}",
                r.signal.unwrap_or(Signal::Term)
            )?,
            Outcome::Stopped => writeln!(out, "Stopped container {who} — port {ports} is free")?,
            Outcome::Declined => writeln!(out, "Skipped {who}")?,
            Outcome::StillRunning => match r.message.as_deref() {
                Some(m) if m.starts_with("still running") => eprintln!("{who} is {m}"),
                Some(m) => eprintln!("Stopped {who}, but {m}"),
                None => eprintln!("{who} is still running"),
            },
            Outcome::Respawned => eprintln!(
                "Stopped {who}, but {}",
                r.message.as_deref().unwrap_or("the port is busy again")
            ),
            Outcome::Failed => eprintln!("Failed to stop {who} on port {ports}{extra}"),
            Outcome::Refused => eprintln!("Not stopping {who} on port {ports}{extra}"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_rules() {
        use Outcome::*;
        assert_eq!(
            exit_code(&[Exited, NotFound]),
            0,
            "a freed port wins over idle ones"
        );
        assert_eq!(exit_code(&[Stopped]), 0);
        assert_eq!(exit_code(&[SignalSent]), 0);
        assert_eq!(exit_code(&[NotFound, NotFound]), 1);
        assert_eq!(exit_code(&[Declined]), 1);
        assert_eq!(exit_code(&[]), 1);
        assert_eq!(exit_code(&[Exited, Failed]), 2);
        assert_eq!(
            exit_code(&[Refused, StillRunning]),
            2,
            "failure beats timeout"
        );
        assert_eq!(exit_code(&[Exited, StillRunning]), 3);
        assert_eq!(exit_code(&[Respawned]), 3);
    }

    #[test]
    fn yes_answers() {
        for yes in ["y", "Y", "yes", "YES\n", " y \n"] {
            assert!(is_yes(yes), "{yes:?}");
        }
        for no in ["", "n", "no", "\n", "yep"] {
            assert!(!is_yes(no), "{no:?}");
        }
    }
}
