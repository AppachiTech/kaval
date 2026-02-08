# Kaval — Architecture Notes

## Overview
Kaval (காவல், Tamil for "Guard/Watch") is a port/process manager TUI built in Rust.
Binary: `kav`. Tagline: "Guard your ports."

## Module Map
- **main.rs** — CLI entry point, dispatches to TUI or one-shot commands (list, check, kill)
- **cli.rs** — Clap derive structs for all subcommands
- **scanner.rs** — Core engine: scans listening sockets (netstat2) and maps to process info (sysinfo)
- **models.rs** — Data types: PortEntry, Protocol, ServiceCategory, SortField
- **ui.rs** — Interactive TUI: table view, filter, sort, kill confirmation, detail pane, auto-refresh
- **theme.rs** — Appachi Tech dark theme (matches Suvadu's color palette)
- **util.rs** — Known service detection by port number and process name

## Key Dependencies
- `netstat2` — Cross-platform socket enumeration (macOS: proc_pidfdinfo, Linux: procfs)
- `sysinfo` — Process details (name, CPU, memory, uptime)
- `ratatui` + `crossterm` — TUI rendering
- `clap` — CLI parsing
- `anyhow` — Error handling

## Design Decisions
- No database: real-time tool, not historical
- No config file: minimal surface for v0.1
- Known service detection combines port mapping AND process name detection
- Kill uses system `kill` command rather than libc signals for simplicity
- TUI refreshes every 2 seconds automatically

## Build & Run
```sh
cargo build --release
./target/release/kav         # TUI
./target/release/kav list    # One-shot table
./target/release/kav check 3000
./target/release/kav kill 3000
```
