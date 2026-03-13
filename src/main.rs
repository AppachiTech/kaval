mod cli;
mod models;
mod output;
mod scanner;
mod theme;
mod ui;
mod util;

use std::collections::HashSet;
use std::io::{self, IsTerminal};

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command, SortBy};
use scanner::{kill_process, new_system_with_cpu, scan_ports};

fn sort_entries(entries: &mut [models::PortEntry], sort: Option<SortBy>) {
    match sort {
        None | Some(SortBy::Port) => entries.sort_by_key(|e| e.port),
        Some(SortBy::Name) => entries.sort_by(|a, b| {
            a.process_name
                .to_lowercase()
                .cmp(&b.process_name.to_lowercase())
        }),
        Some(SortBy::Cpu) => entries.sort_by(|a, b| {
            b.cpu_percent
                .partial_cmp(&a.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        Some(SortBy::Mem) => entries.sort_by(|a, b| {
            b.memory_mb
                .partial_cmp(&a.memory_mb)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            // Default: launch TUI
            ui::run_tui()?;
        }

        Some(Command::List { json, sort }) => {
            let mut sys = new_system_with_cpu();
            let mut entries = scan_ports(&mut sys, true, true)?;
            sort_entries(&mut entries, sort);
            if json {
                output::print_json(&entries)?;
            } else {
                output::print_table(&entries, io::stdout().is_terminal());
            }
        }

        Some(Command::Check { ports }) => {
            let mut sys = new_system_with_cpu();
            let entries = scan_ports(&mut sys, true, true)?;
            let color = io::stdout().is_terminal();
            let matches: Vec<_> = entries
                .iter()
                .filter(|e| ports.contains(&e.port))
                .collect();
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
            // Summary when checking multiple ports
            if ports.len() > 1 {
                let active = ports.len() - missing.len();
                println!(
                    "\n{} of {} ports active",
                    active,
                    ports.len()
                );
            }
            if matches.is_empty() {
                std::process::exit(1);
            }
        }

        Some(Command::Kill { ports, force }) => {
            let mut sys = new_system_with_cpu();
            let entries = scan_ports(&mut sys, true, true)?;
            let mut killed_pids: HashSet<u32> = HashSet::new();
            for port in &ports {
                let matches: Vec<_> = entries.iter().filter(|e| e.port == *port).collect();
                if matches.is_empty() {
                    println!("Nothing listening on port {}", port);
                } else {
                    for entry in &matches {
                        // Skip if we already killed this PID (same process on multiple ports)
                        if !killed_pids.insert(entry.pid) {
                            continue;
                        }
                        if !force {
                            println!(
                                "Killing {} (PID {}) on port {}...",
                                entry.process_name, entry.pid, entry.port
                            );
                        }
                        kill_process(&sys, entry.pid, force)?;
                        println!(
                            "{}Killed {} (PID {})",
                            if force { "Force " } else { "" },
                            entry.process_name,
                            entry.pid
                        );
                    }
                }
            }
        }
    }

    Ok(())
}
