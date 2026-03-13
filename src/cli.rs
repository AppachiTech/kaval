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
    Check {
        /// Port number(s) to check
        #[arg(required = true)]
        ports: Vec<u16>,
    },

    /// Kill the process(es) listening on port(s)
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
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Print a man page for kav to stdout
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
