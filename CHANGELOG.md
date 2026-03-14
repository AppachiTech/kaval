# Changelog

All notable changes to Kaval will be documented in this file.


## [Unreleased]

## [0.1.0] - 2026-03-14

### Added
- `kav list --sort <port|name|cpu|mem>` — sort one-shot output by any column
- `kav check <port> [port ...]` — check multiple ports, with table output and "N of M ports active" summary
- `kav kill <port> [port ...]` — kill processes on multiple ports in one command
- `kav completions <bash|zsh|fish|...>` — generate shell completion scripts
- PgUp/PgDn scrolling for detail pane (Left/Right as aliases)
- Persistent scroll hint in detail pane title bar
- Panic hook: terminal always restored to normal state on crash
- Integration tests in `tests/cli.rs` running the actual binary
- Linux added to CI matrix (previously macOS only)
- Build date shown in `--version` output
- Service detection expanded to 56+ (was 30+)
- `cargo audit` step in CI for security scanning
- Demo GIF and `.tape` file

### Changed
- Browser entries grouped by (PID, service) in both TUI and CLI — shows port count instead of repeating rows
- `kav check` exits code 1 when no ports match (scriptable)
- `kav kill` exits code 1 when no ports match (consistent with `kav check`)
- `kav list --json` now correctly escapes backslashes, newlines, tabs, and control characters
- CPU warning threshold unified between CLI and TUI: >20% = yellow, >50% = red
- UDP count in summary footer now counted directly (was incorrectly derived by subtraction when browser entries were collapsed)
- Kill confirmation dialog stores PID directly — no longer at risk from auto-refresh mid-confirmation
- Service detection switched to data-driven `PROCESS_RULES` / `PORT_RULES` const tables
- Rendering code extracted to `output.rs`; `main.rs` is now pure CLI dispatch
- `SortField` enum unified — single definition in `models.rs` used by both CLI and TUI; `SortBy` removed
- Theme struct cleaned up — removed unused fields (`primary_dim`, `bg_elevated`, `border_focus`)
- Release pipeline consolidated into single `release.yml` — no more race condition with duplicate GitHub releases
- `ryand56/r2-upload-action` pinned to `v1.4.1` (was `@latest`)
- `bump-version` in Makefile uses `perl -i` instead of `sed -i ''` (cross-platform)
- ANSI colors stripped when output is piped (TTY detection)

### Fixed
- CPU always showing 0% in TUI — fixed by persisting `sysinfo::System` across refreshes
- `truncate()` panic on non-ASCII process names — now uses `chars()` not byte slicing
- `partial_cmp` unwrap on NaN float values — uses `unwrap_or(Ordering::Equal)`
- `print_json` panic on empty entry list
- Filter `Escape` now clears text and exits filter mode

## [0.0.6] - 2026-03-01

### Changed
- Kill shortcuts changed to `Ctrl+X` (SIGTERM) and `Ctrl+K` (SIGKILL) to require explicit modifier key, preventing accidental kills

### Fixed
- `Ctrl+K` force kill now shows confirmation dialog before killing (previously killed immediately)

## [0.0.5] - 2026-02-20

### Fixed
- Clippy warnings resolved
- Added crates.io metadata (description, repository, homepage, keywords, categories)

## [0.0.4] - 2026-02-18

### Fixed
- Release flow regenerates `Cargo.lock` before commit
- Added `cargo publish` to release flow
- Ran `cargo fmt` to normalize formatting

## [0.0.3] - 2026-02-15

### Added
- Improved service detection for more dev tools
- Colored terminal output
- Community templates (issue templates, contributing guide, security policy)

## [0.0.2] - 2026-02-10

### Added
- Interactive TUI for viewing all listening TCP and UDP ports
- Smart service detection for 30+ dev tools (PostgreSQL, Redis, Next.js, Vite, Docker, Nginx, etc.)
- Kill with SIGTERM (`x`) and SIGKILL (`K`), with confirmation prompt
- Detail pane showing full process info (command line, PID, CPU, memory, uptime)
- Filter mode to search by port, process name, or service
- Sort cycling: Port → Name → CPU → Memory
- TCP/UDP protocol filter toggle (`Ctrl+T`)
- Auto-refresh every 2 seconds
- CLI commands: `kav list`, `kav check <port>`, `kav kill <port>`
- JSON output: `kav list --json`
- Color-coded categories: green (dev servers), yellow (databases), purple (caches), blue (containers), orange (browsers), gray (system)
- macOS and Linux support
- Homebrew tap installation
