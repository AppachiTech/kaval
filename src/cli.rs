use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "kav",
    about = "Kaval — Guard your ports. A developer-focused port and process manager TUI.",
    version,
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
    },

    /// Check what's running on a specific port
    Check {
        /// Port number to check
        port: u16,
    },

    /// Kill the process listening on a port
    Kill {
        /// Port number whose process to kill
        port: u16,

        /// Force kill (SIGKILL) without confirmation
        #[arg(short, long)]
        force: bool,
    },
}
