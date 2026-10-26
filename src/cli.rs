use std::io::Write;
use std::time::Duration;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};

use crate::killer::Signal;
pub use crate::models::SortField;
use crate::output::ColorChoice;

#[derive(Parser)]
#[command(
    name = "kav",
    about = "Kaval — Guard your ports. A developer-focused port and process manager TUI.",
    version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("BUILD_DATE"), ")"),
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// When to use colours
    #[arg(long, global = true, value_enum, value_name = "WHEN", default_value_t = ColorChoice::Auto)]
    pub color: ColorChoice,
}

fn parse_signal(s: &str) -> Result<Signal, String> {
    s.parse()
}

/// Parse a timeout in seconds (fractions allowed, 0 to 3600).
fn parse_timeout(s: &str) -> Result<Duration, String> {
    let secs: f64 = s
        .parse()
        .map_err(|_| format!("'{s}' is not a number of seconds"))?;
    if !secs.is_finite() || !(0.0..=3600.0).contains(&secs) {
        return Err("timeout must be between 0 and 3600 seconds".to_string());
    }
    Ok(Duration::from_secs_f64(secs))
}

/// An inclusive port range for `kav pick`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortRange {
    pub start: u16,
    pub end: u16,
}

/// Parse `3000-3999` (or a single port) into a range.
fn parse_range(s: &str) -> Result<PortRange, String> {
    let (start, end) = s.split_once('-').unwrap_or((s, s));
    let port = |p: &str| {
        p.trim()
            .parse::<u16>()
            .map_err(|_| format!("'{p}' is not a port number (1-65535)"))
    };
    let (start, end) = (port(start)?, port(end)?);
    if start == 0 || start > end {
        return Err(format!(
            "invalid range '{s}' (use START-END with 1 <= START <= END)"
        ));
    }
    Ok(PortRange { start, end })
}

/// Parse a refresh interval in seconds (fractions allowed, at least 0.1).
pub fn parse_interval(s: &str) -> Result<Duration, String> {
    let secs: f64 = s
        .parse()
        .map_err(|_| format!("'{s}' is not a number of seconds"))?;
    if !secs.is_finite() || secs < 0.1 {
        return Err("interval must be at least 0.1 seconds".to_string());
    }
    Ok(Duration::from_secs_f64(secs))
}

#[derive(Subcommand)]
pub enum Command {
    /// List all listening ports (one-shot table output)
    #[command(long_about = "List all listening ports.

Prints a table of listening TCP and UDP ports with bind address, process, service,
project, PID, CPU, memory and uptime. A trailing `?` marks a service guessed from the
port number alone; BIND `*` means reachable from other machines.

Exit codes:
  0  Success (even if no ports are listening)
  2  Error")]
    List {
        /// Output as JSON (schema v2); with --watch, one document per line (NDJSON)
        #[arg(long)]
        json: bool,

        /// Sort by
        #[arg(long, value_enum, default_value_t = SortField::Port)]
        sort: SortField,

        /// Reverse the sort order
        #[arg(long, short = 'r')]
        reverse: bool,

        /// Filter: exact port or PID, or text in process, service, command, project, container or user
        #[arg(long, short = 'f')]
        filter: Option<String>,

        /// Refresh continuously; Ctrl+C to exit
        #[arg(long, short = 'w')]
        watch: bool,

        /// Seconds between refreshes with --watch
        #[arg(long, value_name = "SECS", default_value = "2", value_parser = parse_interval)]
        interval: Duration,

        /// Show only TCP ports
        #[arg(long)]
        tcp: bool,

        /// Show only UDP ports
        #[arg(long)]
        udp: bool,

        /// Show only listeners reachable from other machines (bound to * or a LAN address)
        #[arg(long)]
        exposed: bool,
    },

    /// Check what's running on specific port(s)
    #[command(long_about = "Check what's running on specific port(s).

Shows each listener with its command line, directory, parent and connections. Without
root, ports held by other users' processes are still detected (via a loopback connect)
and reported as in use.

Exit codes:
  0  At least one port is in use (with --free: every port is free)
  1  No port is in use (with --free: at least one port is busy)
  2  Error")]
    Check {
        /// Port number(s) to check
        #[arg(required = true)]
        ports: Vec<u16>,

        /// Output as JSON (schema v2)
        #[arg(long)]
        json: bool,

        /// Succeed only if every port is free (a pre-flight check before starting servers)
        #[arg(long)]
        free: bool,

        /// Print nothing; report only through the exit code
        #[arg(long, short = 'q')]
        quiet: bool,

        /// Probe busy ports on this machine (TCP connect; HTTP HEAD for web ports)
        #[arg(long)]
        probe: bool,
    },

    /// Kill the process(es) listening on port(s)
    #[command(
        long_about = "Stop the process(es) or container(s) listening on port(s).

Sends SIGTERM and waits (up to --timeout) until each process has exited, so \"Stopped\"
means the port is really free. Ports published by a Docker container are freed with
`docker stop`; container runtimes, init and system services are never killed unless
--allow-protected is given. Asks for confirmation in a terminal unless --yes.

Exit codes:
  0  The ports were freed (ports with nothing listening are ignored)
  1  Nothing was listening, or every prompt was declined
  2  A kill failed or was refused (permission denied, protected process)
  3  A process was still running after --timeout, or the port was taken again"
    )]
    Kill {
        /// Port number(s) whose processes to stop
        #[arg(required = true)]
        ports: Vec<u16>,

        /// Signal to send (TERM, KILL, INT, HUP, QUIT, USR1, USR2 or a number)
        #[arg(long, short = 's', value_name = "SIG", default_value = "TERM", value_parser = parse_signal)]
        signal: Signal,

        /// Shorthand for --signal KILL (still asks for confirmation; add --yes to skip it)
        #[arg(long, short = 'f', conflicts_with = "signal")]
        force: bool,

        /// Don't ask for confirmation
        #[arg(long, short = 'y')]
        yes: bool,

        /// Seconds to wait for each process to exit
        #[arg(long, value_name = "SECS", default_value = "5", value_parser = parse_timeout)]
        timeout: Duration,

        /// Send the signal and return without waiting for the process to exit
        #[arg(long)]
        no_wait: bool,

        /// Send SIGKILL if a process is still running after --timeout
        #[arg(long)]
        escalate: bool,

        /// Stop the listener's parent instead (e.g. nodemon or a reloader that would restart it)
        #[arg(long)]
        parent: bool,

        /// Allow stopping protected processes (container runtimes, system services)
        #[arg(long)]
        allow_protected: bool,

        /// Report results as JSON (schema v2); never prompts
        #[arg(long)]
        json: bool,
    },

    /// Wait until port(s) are listening, or free with --free (for scripts and CI)
    #[command(
        long_about = "Wait until every given port is listening (or, with --free, until every port is free).

Polls every 250 ms without binding the ports, so it never races a server that is
starting up. Handy before running tests against a dev server, or after `kav kill`.

Exit codes:
  0  The condition was met
  1  Timed out
  2  Error"
    )]
    Wait {
        /// Port number(s) to wait for
        #[arg(required = true)]
        ports: Vec<u16>,

        /// Wait until the ports are free instead
        #[arg(long)]
        free: bool,

        /// Give up after this many seconds (0 waits forever)
        #[arg(long, value_name = "SECS", default_value = "30", value_parser = parse_timeout)]
        timeout: Duration,

        /// Wait for UDP ports instead of TCP
        #[arg(long)]
        udp: bool,

        /// Print nothing; report only through the exit code
        #[arg(long, short = 'q')]
        quiet: bool,
    },

    /// Print free port(s) you can bind to
    #[command(
        long_about = "Print free port(s) in a range, one per line, for scripts:

  PORT=$(kav pick --range 3000-3999) npm run dev

Exit codes:
  0  Enough free ports were found
  1  Not enough free ports in the range
  2  Error"
    )]
    Pick {
        /// Range to search, e.g. 3000-3999 (or a single port)
        #[arg(long, value_name = "START-END", default_value = "3000-9999", value_parser = parse_range)]
        range: PortRange,

        /// How many ports to print
        #[arg(long, short = 'n', default_value_t = 1)]
        count: usize,

        /// Find free UDP ports instead of TCP
        #[arg(long)]
        udp: bool,
    },

    /// Print shell completion script for the given shell
    ///
    /// Pipe the output to your shell's completion directory.
    /// Exits 0 always.
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Print a man page for kav to stdout
    ///
    /// Pipe to a file or directly to man: kav man | man -l -
    /// Exits 0 always.
    Man,
}

/// Generate and print shell completions to stdout.
pub fn print_completions(shell: Shell) {
    generate(shell, &mut Cli::command(), "kav", &mut std::io::stdout());
}

/// Generate and print a man page to stdout.
pub fn print_man_page() -> Result<()> {
    let cmd = Cli::command();
    let man = clap_mangen::Man::new(cmd);
    let mut buf = vec![];
    man.render(&mut buf)?;
    std::io::stdout().write_all(&buf)?;
    Ok(())
}
