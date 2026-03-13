use std::io::Write;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};

pub use crate::models::SortField;

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
}

#[derive(Subcommand)]
pub enum Command {
    /// List all listening ports (one-shot table output)
    ///
    /// Prints a table of all listening TCP and UDP ports with process name,
    /// PID, CPU, memory, and uptime. Exits 0 always.
    #[command(long_about = "List all listening ports (one-shot table output).

Prints a table of all listening TCP and UDP ports with process name, PID, CPU, memory,
and uptime. Combine with --sort and --filter for scripting.

Exit codes:
  0  Always (even if no ports are listening)")]
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Sort output (port, name, cpu, mem)
        #[arg(long, value_enum)]
        sort: Option<SortField>,

        /// Filter by port number, process name, or service (case-insensitive substring)
        #[arg(long, short = 'f')]
        filter: Option<String>,
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
