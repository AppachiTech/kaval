# Changelog

All notable changes to Kaval will be documented in this file.


## [Unreleased]

## [0.2.0] - 2026-10-27

Kaval now shows who owns each port, which project and container it belongs to, and whether other machines can reach it. Kills are verified, scripts get versioned JSON plus `kav wait` and `kav pick`, and release downloads cover Intel Macs and Linux on arm64.

### Breaking
- **JSON schema 2.** `kav list --json` prints an object — `{"schema": 2, "version", "complete", "ports": [...]}` — instead of a bare array, so `jq '.[]'` becomes `jq '.ports[]'`. Entries gain bind addresses, exposure, command line, directory, project, user, parent, container, service source and connection count; `process` is `null` when the owner is hidden. `--watch --json` streams one document per line (NDJSON) instead of failing.
- **TCP and UDP listeners on the same port are separate rows** (and separate JSON entries); they used to be merged.
- **`kav kill` confirms and verifies.** In a terminal it asks before each kill (`-y/--yes` skips the question; scripts and `--json` never prompt), then waits up to `--timeout` (5 s) for the process to exit, and says "Stopped" only once the port is free. `-f/--force` is now shorthand for `--signal KILL` and can't be combined with `--signal`.
- **`kav kill` exit codes:** 0 freed, 1 nothing listening (or every prompt declined), 2 failed or refused, 3 still running after `--timeout` or the port was taken again.
- **Errors exit with 2** for every command; they exited 1, which `kav check` also uses for "not in use". A closed pipe (`kav list | head`) now exits 0 quietly.
- **Protected processes.** `kav kill` refuses container runtimes and their port forwarders, critical system processes and, over SSH, `sshd`, unless `--allow-protected` is given; init and kav itself are always refused. Ports published by a Docker container are freed with `docker stop` instead of killing the runtime.
- **TUI keys.** `Esc` no longer quits: it clears the filter, then the marks (quit with `q`, `Ctrl+Q` or `Ctrl+C`). `y` opens a copy menu (`yy` copies the port as before). `PgUp`/`PgDn` page the list; the detail pane scrolls with `[`/`]` or `←`/`→`.
- **Service labels.** OpenSearch is reported as OpenSearch (it was labelled Elasticsearch); Kafka, RabbitMQ and other brokers are in a new `queue` category (they were `cache`); services guessed from the port number alone end with `?`.
- **Building from source needs Rust 1.95** or newer.

### Added
- `kav wait <ports>` blocks until the ports are listening, or free with `--free` (`--timeout`, `--udp`, `--quiet`).
- `kav pick` prints free ports from a range (`--range`, `-n/--count`, `--udp`).
- `kav list`: a BIND column and `--exposed` (only listeners other machines can reach); a PROJECT column (the git repository the process runs in); `-r/--reverse`; `--interval` for `--watch`; `--filter` also matches command lines, projects, containers and users; columns fit the terminal width.
- `kav check` shows each listener's command line, directory, parent, connections, container and notes; reports ports held by other users' processes as in use even without root; and adds `--free` (a pre-flight check), `--json`, `-q/--quiet` and `--probe` (TCP connect, HTTP status for web ports).
- `kav kill` adds `-s/--signal`, `--timeout`, `--no-wait`, `--escalate` (SIGKILL after the timeout), `--parent` (stop a supervisor such as nodemon), `--allow-protected` and `--json`, and notices when a supervisor restarts what it stopped.
- Ports published by Docker containers show the container's name, image and compose project.
- Service detection from the command line of generic runtimes (node, python, java, ruby, php, bun, deno, …) recognises Next.js, Vite, Django, Uvicorn, Rails, Tomcat, Kafka and more: 140+ services in all. Worker processes inherit their parent's service.
- Themes: `--theme auto|dark|light` (or `KAV_THEME`) with automatic detection of light terminals (#1); the nearest 256-colour palette on terminals without true colour; `--color auto|always|never`, `NO_COLOR` and `CLICOLOR_FORCE`.
- TUI:
  - `e` shows only exposed listeners; new listeners are marked `+` for ten seconds, and new exposed ones are announced.
  - `Space` marks rows for a multi-row kill; `Enter` expands a browser's group of ports.
  - `p` probes the selected listener; `o` opens https for TLS ports and the LAN address for network-bound listeners.
  - `y` opens a copy menu: port, PID, URL, command line or working directory.
  - The kill dialog shows PID, port and signal, and offers `p` to stop the supervising parent instead. After a kill, the status line reports when the port is free or that the process is still running.
  - The detail pane shows exposure, user, project, directory, parent, container, connections, notes and probe results, and moves below the table on narrow terminals.
  - `s`, `S` (reverse sort), `t`, `d` and `r` work without Ctrl; the mouse selects and scrolls (`--no-mouse` turns that off); `--interval` sets the refresh; `Ctrl+Z` suspends.
  - A note on the table says when other users' processes are hidden (run with sudo to see them).
  - Running `kav` without a terminal prints the list instead.
- Release downloads for Intel Macs (one universal macOS binary) and Linux arm64, and static Linux binaries. Every tarball includes the man page and `uninstall.sh`, with a `.sha256` beside it.
- The Homebrew formula also works on Linux, and installs the man page and bash, zsh and fish completions.

### Changed
- Signals are sent directly rather than through the system `kill` command.
- The TUI selection follows its row across refreshes and re-sorts; the help overlay scrolls and fits an 80×24 terminal; the header counts ports rather than rows.
- Release builds use LTO and are stripped, and their build dates are reproducible (`SOURCE_DATE_EPOCH`).

### Fixed
- CPU usage read near 0% (about 2% on macOS and 0% on Linux for a process using a full core); it is now sampled over a real interval.
- Command lines were missing from the detail pane and `kav check`.
- Ports held by other users' processes were silently missing; they are now reported with a hint to use sudo, and JSON says `"complete": false`.
- `kav kill` said "Killed" while the process was still running.
- macOS release binaries ran only on Apple Silicon, and Linux binaries needed glibc 2.39 (so they failed on Ubuntu 22.04, Debian 12 and RHEL 9).
- The Homebrew formula's test could never pass.
- The terminal is restored on every exit path, including errors and panics.
- Application-support and hidden directories (such as Docker Desktop's data directory) are no longer shown as projects.

### Security
- Process names, command lines and other text read from the system are sanitised before printing, so a process can't inject terminal escape sequences.
- `kav kill` re-checks a process's start time before signalling it, so a reused PID is never hit.
- Dependencies refreshed to clear RUSTSEC-2026-0204 and newly reported unsound advisories.
- Release workflows pin every action to a commit and use read-only tokens; downloads come with checksums, also attached to each GitHub release.

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
