mod cli;
mod models;
mod scanner;
mod theme;
mod ui;
mod util;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Command};
use scanner::{kill_process, scan_ports};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            // Default: launch TUI
            ui::run_tui()?;
        }

        Some(Command::List { json }) => {
            let entries = scan_ports(true, true)?;
            if json {
                print_json(&entries)?;
            } else {
                print_table(&entries);
            }
        }

        Some(Command::Check { port }) => {
            let entries = scan_ports(true, true)?;
            let matches: Vec<_> = entries.iter().filter(|e| e.port == port).collect();
            if matches.is_empty() {
                println!("Nothing listening on port {}", port);
            } else {
                for entry in &matches {
                    println!(
                        "Port {} ({}) — {} (PID {}){}",
                        entry.port,
                        entry.protocol,
                        entry.process_name,
                        entry.pid,
                        entry
                            .known_service
                            .map(|s| format!(" [{}]", s))
                            .unwrap_or_default(),
                    );
                    if !entry.process_cmd.is_empty() {
                        println!("  Command: {}", entry.process_cmd);
                    }
                    println!(
                        "  CPU: {:.1}%  Memory: {}  Uptime: {}",
                        entry.cpu_percent,
                        entry.memory_display(),
                        entry.uptime_display()
                    );
                }
            }
        }

        Some(Command::Kill { port, force }) => {
            let entries = scan_ports(true, true)?;
            let matches: Vec<_> = entries.iter().filter(|e| e.port == port).collect();
            if matches.is_empty() {
                println!("Nothing listening on port {}", port);
            } else {
                for entry in &matches {
                    if !force {
                        println!(
                            "Killing {} (PID {}) on port {}...",
                            entry.process_name, entry.pid, entry.port
                        );
                    }
                    kill_process(entry.pid, force)?;
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

    Ok(())
}

fn print_table(entries: &[models::PortEntry]) {
    if entries.is_empty() {
        println!("No listening ports found.");
        return;
    }

    println!(
        "{:<7} {:<5} {:<15} {:<18} {:<7} {:<7} {:<9} {}",
        "PORT", "PROTO", "PROCESS", "SERVICE", "PID", "CPU", "MEM", "UPTIME"
    );
    println!("{}", "─".repeat(80));

    for e in entries {
        println!(
            "{:<7} {:<5} {:<15} {:<18} {:<7} {:<7} {:<9} {}",
            e.port,
            e.protocol,
            truncate(&e.process_name, 15),
            e.known_service.unwrap_or("—"),
            e.pid,
            format!("{:.1}%", e.cpu_percent),
            e.memory_display(),
            e.uptime_display(),
        );
    }
}

fn print_json(entries: &[models::PortEntry]) -> Result<()> {
    // Manual JSON to avoid serde dependency
    println!("[");
    for (i, e) in entries.iter().enumerate() {
        let comma = if i < entries.len() - 1 { "," } else { "" };
        println!(
            r#"  {{"port":{},"protocol":"{}","process":"{}","service":{},"pid":{},"cpu":{:.1},"memory_mb":{:.1},"uptime_secs":{}}}{}"#,
            e.port,
            e.protocol,
            e.process_name.replace('"', "\\\""),
            e.known_service
                .map(|s| format!("\"{}\"", s))
                .unwrap_or_else(|| "null".to_string()),
            e.pid,
            e.cpu_percent,
            e.memory_mb,
            e.uptime.as_secs(),
            comma,
        );
    }
    println!("]");
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}
