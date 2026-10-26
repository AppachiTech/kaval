use std::io::Write;
use std::time::Duration;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};

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
    ///
    /// Exits 0 if at least one port is active, 1 if none are active.
    #[command(long_about = "Check what's running on specific port(s).

Prints a table of matching ports. When checking multiple ports, prints a summary
line ('N of M ports active').

Exit codes:
  0  At least one requested port is active
  1  No requested ports are active (useful for scripting)")]
    Check {
        /// Port number(s) to check
        #[arg(required = true)]
        ports: Vec<u16>,
    },

    /// Kill the process(es) listening on port(s)
    ///
    /// Exits 0 if at least one process was killed, 1 if nothing was listening.
    #[command(long_about = "Kill the process(es) listening on port(s).

Sends SIGTERM by default. Use --force for SIGKILL. Prompts for confirmation
when running in an interactive terminal unless --yes or --force is passed.

Exit codes:
  0  At least one process was killed
  1  Nothing was listening on any of the requested ports
  2  Kill failed (e.g. permission denied — try sudo)")]
    Kill {
        /// Port number(s) whose processes to kill
        #[arg(required = true)]
        ports: Vec<u16>,

        /// Force kill (SIGKILL) without confirmation
        #[arg(short, long)]
        force: bool,

        /// Skip interactive confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
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
