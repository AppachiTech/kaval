use clap::{Parser, Subcommand, ValueEnum};

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
        sort: Option<SortBy>,
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
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SortBy {
    Port,
    Name,
    Cpu,
    Mem,
}
