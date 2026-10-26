mod cli;
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

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};
use models::{filter_entries, sort_entries, SortField};
use scanner::{kill_process, ContainerMode, ScanOptions, Scanner};

/// Refresh `kav list` output in-place on the same screen.
/// Clears the terminal and redraws every 2 seconds until Ctrl+C (SIGINT) terminates the process.
fn run_watch(
    sort: Option<SortField>,
    filter: Option<&str>,
    show_tcp: bool,
    show_udp: bool,
) -> Result<()> {
    use ratatui::crossterm::{cursor, execute, terminal};

    let color = io::stdout().is_terminal();
    let mut scanner = Scanner::new();
    let mut opts = ScanOptions {
        tcp: show_tcp,
        udp: show_udp,
        sample_cpu: true,
        containers: ContainerMode::Blocking,
    };

    loop {
        execute!(
            io::stdout(),
            terminal::Clear(terminal::ClearType::All),
            cursor::MoveTo(0, 0)
        )?;
        println!("kav watch  —  Ctrl+C to exit  —  refreshing every 2s\n");

        let mut entries = scanner.scan(&opts)?.entries;
        opts.sample_cpu = false; // the persistent scanner measures between refreshes
        sort_entries(&mut entries, sort.unwrap_or(SortField::Port), false);
        if let Some(pattern) = filter {
            filter_entries(&mut entries, pattern);
        }
        output::print_table(&entries, color);

        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            // Default: launch TUI
            ui::run_tui()?;
        }

        Some(Command::List {
            json,
            sort,
            filter,
            watch,
            tcp,
            udp,
        }) => {
            // If neither --tcp nor --udp specified, show both (default)
            let (show_tcp, show_udp) = match (tcp, udp) {
                (false, false) => (true, true),
                other => other,
            };

            if watch {
                if json {
                    anyhow::bail!("--watch and --json are mutually exclusive");
                }
                return run_watch(sort, filter.as_deref(), show_tcp, show_udp);
            }
            let opts = ScanOptions {
                tcp: show_tcp,
                udp: show_udp,
                sample_cpu: true,
                containers: ContainerMode::Blocking,
            };
            let mut entries = Scanner::new().scan(&opts)?.entries;
            sort_entries(&mut entries, sort.unwrap_or(SortField::Port), false);
            if let Some(ref pattern) = filter {
                filter_entries(&mut entries, pattern);
            }
            if json {
                output::print_json(&entries)?;
            } else {
                output::print_table(&entries, io::stdout().is_terminal());
            }
        }

        Some(Command::Check { ports }) => {
            let opts = ScanOptions {
                sample_cpu: true,
                containers: ContainerMode::Blocking,
                ..ScanOptions::default()
            };
            let entries = Scanner::new().scan(&opts)?.entries;
            let color = io::stdout().is_terminal();
            let matches: Vec<_> = entries.iter().filter(|e| ports.contains(&e.port)).collect();
            let missing: Vec<_> = ports
                .iter()
                .filter(|p| !entries.iter().any(|e| e.port == **p))
                .collect();
            if !matches.is_empty() {
                output::print_check_table(&matches, color);
            }
            for port in &missing {
                println!("Nothing listening on port {}", port);
            }
            if ports.len() > 1 {
                let active = ports.len() - missing.len();
                println!("\n{} of {} ports active", active, ports.len());
            }
            if matches.is_empty() {
                std::process::exit(1);
            }
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

    Ok(())
}
