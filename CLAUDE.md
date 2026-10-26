# Kaval — Architecture Notes

## Overview
Kaval (காவல், Tamil for "Guard/Watch") is a port/process manager for macOS and Linux, built in Rust:
an interactive TUI plus scriptable one-shot commands. Binary: `kav`. Tagline: "Guard your ports."
Windows is out of scope.

## Module Map
- **main.rs** — Entry point: parses the CLI, initialises the theme, dispatches. Errors exit 2; a closed
  pipe exits 0 quietly. With no subcommand: TUI if stdout is a terminal, otherwise `kav list`.
- **cli.rs** — Clap derive structs for every subcommand, value parsers (signals, timeouts, ranges).
- **cmd/** — One file per subcommand: `list.rs` (table/JSON/`--watch`), `check.rs`, `kill.rs`, `wait.rs`, `pick.rs`.
- **scanner.rs** — Core engine: listening sockets (netstat2) → `PortEntry` rows with process details
  (sysinfo), parents, connections, projects and containers. `Snapshot { entries, visibility }`.
- **models.rs** — Data types: `PortEntry`, `Protocol`, `Service`/`ServiceSource`/`ServiceCategory`,
  `Exposure`, `EntryKey`, sorting and filtering.
- **services.rs** — Known-service detection: `COMMAND_RULES` (generic runtimes only) → `PROCESS_RULES` →
  `PORT_RULES` (a guess, shown with `?`). Also container-proxy detection and web/TLS port lists.
- **project.rs** — Git project of a working directory (cached; stops at `$HOME`; ignores `~/Library` and dot-dirs).
- **docker.rs** — `docker ps` port mappings (2 s timeout, 30 s back-off on failure, cached) and `docker stop`.
- **probe.rs** — Loopback TCP connect, bind check (used by `pick`), and the health probe (`HEAD /`).
- **killer.rs** — Kill planning and execution: protection rules, `--parent` targeting, `docker stop` for
  container ports, signals via `libc::kill`, PID-reuse guard (start time), waiting for exit.
- **output/** — `mod.rs` (colour choice, `sanitize`, terminal width), `table.rs` (width-aware tables),
  `json.rs` (schema v2 documents).
- **theme.rs** — Dark, light and mono palettes; auto detection (COLORFGBG, then an OSC 11 query); 256-colour fallback.
- **ui/** — Interactive TUI: `mod.rs` (event loop, `TerminalGuard`, background work), `app.rs` (state and
  input → `Effect`s), `draw.rs` (rendering), `clipboard.rs` (copy, open in browser).

## Key Dependencies
- `netstat2` — Cross-platform socket enumeration (macOS: proc_pidfdinfo, Linux: netlink sock_diag).
  **Low-activity upstream.** This is the most likely breakage point on OS upgrades.
  Migration path: direct `/proc/net/tcp` parsing on Linux (no crate); `proc-info` on macOS.
  (The `listeners` crate has the same macOS blind spot for other users' sockets.)
- `sysinfo` — Process details (name, command line, cwd, user, CPU, memory, start time); default features off.
- `ratatui` — TUI; crossterm is used through `ratatui::crossterm` so only one version is built.
- `clap` (+ `clap_complete`, `clap_mangen`) — CLI, completions, man page.
- `serde`/`serde_json` — JSON output. `socket2` — bind checks. `libc` — signals, `geteuid`.
- `terminal-colorsaurus` — terminal background query. `anyhow` — error handling.

## Design Decisions
- No database and no config file: a real-time tool configured by flags and env (`KAV_THEME`, `NO_COLOR`, `CLICOLOR_FORCE`).
- No MCP server: `--json` (schema 2) and documented exit codes already serve scripts and agents.
- JSON field names are a contract: add fields freely; renaming or removing one bumps `SCHEMA` in `output/json.rs`.
- Network use is limited to this machine: loopback/own-address connects (hidden-owner check, probes),
  brief binds in `pick`, and the `docker` CLI. The hidden-owner fallback only connects over TCP; it never binds.
- Kills use `libc::kill` directly and are verified: kav waits for the process to exit and reports respawns.
  Before signalling it re-checks the start time so a reused PID is never hit. PID 1 and kav itself are
  always refused; container runtimes, critical system processes and `sshd` (over SSH) need `--allow-protected`.
- Ports published by containers are freed with `docker stop`, never by killing the runtime's port forwarder.
- Text from the system (process names, command lines, paths) goes through `output::sanitize` before printing.
- The TUI refreshes every 2 s by default (`--interval`). Input handling is pure: `App::handle_key`/`handle_mouse`
  return `Effect`s that `ui::run` executes, so key handling is unit-tested without a terminal.
- `TerminalGuard` restores raw mode, the alternate screen, mouse capture and the cursor on every exit path;
  a panic hook does the same. After Ctrl+Z, a fresh `Terminal` is created (`Terminal::clear` would query
  the cursor position, which some terminals never answer).

## Gotchas
- sysinfo CPU needs two refreshes and a sleep of `MINIMUM_CPU_UPDATE_INTERVAL` before the measuring refresh
  of a newly seen process; `refresh_processes()` alone does not collect cmd/cwd/user — use
  `refresh_processes_specifics` with `UpdateKind::OnlyIfNotSet`.
- Without root, macOS hides other users' sockets entirely and Linux hides their PIDs; `Visibility` reports it.
- macOS std sets CLOEXEC on sockets non-atomically: a child spawned by a concurrent test can inherit a
  listener. Tests that assert a port was freed poll instead of checking once.

## Testing
- Unit tests live beside the code; `tests/cli.rs` runs the real binary against helper listener processes
  (`tests/common/mod.rs`); `tests/tui.rs` drives the TUI through a pseudo-terminal.
- Free ports in tests come from `kav pick` with a range per test.
- CI runs fmt, `clippy --all-targets -D warnings` and tests on macOS (arm64, Intel) and Linux (x86_64, arm64),
  plus cargo-audit, an MSRV (1.95) check and a static musl build.

## Build & Run
```sh
cargo build --release
./target/release/kav            # TUI
./target/release/kav list       # One-shot table
./target/release/kav check 3000
./target/release/kav kill 3000
make lint && make test          # what CI checks
make package                    # release-style tarball for this machine in target/dist
```

## Releasing
Pushing a `v*` tag runs `.github/workflows/release.yml`: the CI checks, a tag/Cargo.toml/CHANGELOG
consistency check, a universal macOS binary, static Linux x86_64/arm64 binaries (`scripts/package.sh`),
upload to downloads.appachi.tech (R2), the Homebrew tap (`scripts/homebrew-formula.sh`) and a GitHub release.
`make release-current` tags the version already in Cargo.toml and publishes the crate.
