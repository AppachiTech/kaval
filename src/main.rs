mod cli;
mod cmd;
mod docker;
mod killer;
mod models;
mod output;
mod probe;
mod project;
mod scanner;
mod services;
mod theme;
mod ui;

use std::collections::HashSet;
use std::io::{self, BufRead, IsTerminal, Write};
use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};
use scanner::{kill_process, ContainerMode, ScanOptions, Scanner};

/// True when an error chain ends in a closed pipe (e.g. `kav list | head`).
fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.chain().any(|e| {
        e.downcast_ref::<io::Error>()
            .is_some_and(|io| io.kind() == io::ErrorKind::BrokenPipe)
            || e.downcast_ref::<serde_json::Error>()
                .and_then(|j| j.io_error_kind())
                == Some(io::ErrorKind::BrokenPipe)
    })
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(e) if is_broken_pipe(&e) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("kav: error: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        None => {
            // Default: launch TUI
            ui::run_tui()?;
        }

        Some(Command::List {
            json,
            sort,
            reverse,
            filter,
            watch,
            interval,
            tcp,
            udp,
            exposed,
        }) => {
            // Neither --tcp nor --udp means both
            let (tcp, udp) = match (tcp, udp) {
                (false, false) => (true, true),
                other => other,
            };
            let args = cmd::list::ListArgs {
                json,
                sort,
                reverse,
                filter,
                watch,
                interval,
                tcp,
                udp,
                exposed,
            };
            cmd::list::run(&args, cli.color)?;
        }

        Some(Command::Check {
            ports,
            json,
            free,
            quiet,
            probe,
        }) => {
            let args = cmd::check::CheckArgs {
                ports,
                json,
                free,
                quiet,
                probe,
            };
            return cmd::check::run(&args, cli.color);
        }

        Some(Command::Kill { ports, force, yes }) => {
            let mut scanner = Scanner::new();
            let opts = ScanOptions {
                containers: ContainerMode::Blocking,
                ..ScanOptions::default()
            };
            let entries = scanner.scan(&opts)?.entries;
            let mut killed_pids: HashSet<u32> = HashSet::new();
            let mut any_found = false;
            let interactive = !yes && io::stdin().is_terminal();
            for port in &ports {
                let matches: Vec<_> = entries.iter().filter(|e| e.port == *port).collect();
                if matches.is_empty() {
                    println!("Nothing listening on port {}", port);
                } else {
                    any_found = true;
                    for entry in &matches {
                        let Some(pid) = entry.pid else {
                            println!(
                                "Port {} is held by a process kav can't see{} — try sudo",
                                entry.port,
                                entry
                                    .user
                                    .as_deref()
                                    .map(|u| format!(" (owned by {u})"))
                                    .unwrap_or_default()
                            );
                            continue;
                        };
                        if !killed_pids.insert(pid) {
                            continue;
                        }
                        // Prompt if running in a TTY and --yes / --force not passed
                        if interactive && !force {
                            print!(
                                "Kill {} (PID {}) on port {}? [y/N] ",
                                entry.process_name, pid, entry.port
                            );
                            io::stdout().flush()?;
                            let mut input = String::new();
                            io::stdin().lock().read_line(&mut input)?;
                            if !input.trim().eq_ignore_ascii_case("y") {
                                println!("Skipped.");
                                continue;
                            }
                        } else if !force {
                            println!(
                                "Killing {} (PID {}) on port {}...",
                                entry.process_name, pid, entry.port
                            );
                        }
                        if let Err(e) = kill_process(scanner.system(), pid, force) {
                            eprintln!("Error: {e}");
                            std::process::exit(2);
                        }
                        println!(
                            "{}Killed {} (PID {})",
                            if force { "Force " } else { "" },
                            entry.process_name,
                            pid
                        );
                    }
                }
            }
            if !any_found {
                std::process::exit(1);
            }
        }

        Some(Command::Completions { shell }) => {
            cli::print_completions(shell);
        }

        Some(Command::Man) => {
            cli::print_man_page()?;
        }
    }

    Ok(ExitCode::SUCCESS)
}
