# Kaval — Guard your ports.

[![CI](https://github.com/AppachiTech/kaval/actions/workflows/ci.yml/badge.svg)](https://github.com/AppachiTech/kaval/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/kaval)](https://crates.io/crates/kaval)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

**Kaval** (காவல், Tamil for "Guard/Watch") is a developer-focused port and process manager TUI built in Rust.

Stop running `lsof -i :3000 | grep LISTEN`. Kaval shows all listening ports, maps them to processes, identifies known dev tools, and lets you kill with a keystroke.

<p align="center">
  <img src="demo/demo.gif" alt="Kaval demo" width="100%">
</p>

## Features

- **See everything at a glance** — all listening ports with process name, PID, CPU, memory, uptime
- **Smart service detection** — recognizes Vite, Next.js, PostgreSQL, Redis, Django, Docker, Elasticsearch, and 56+ more
- **Interactive TUI** — filter, sort, navigate, kill processes — all from one screen
- **One-shot commands** — `kav list`, `kav check 3000`, `kav kill 3000` for scripting
- **JSON output** — `kav list --json` for piping to other tools
- **Sort by any column** — `kav list --sort cpu` to find what's eating resources
- **Multi-port operations** — `kav check 3000 5432 8080` or `kav kill 3000 5432`
- **Color-coded** — green for dev servers, yellow for databases, purple for caches
- **Safe terminal handling** — panic hook ensures your terminal is never left broken

## Install

**Homebrew (recommended):**

```sh
brew update && brew tap AppachiTech/kaval && brew install kaval
```

**Manual (macOS):**

```sh
curl -sL https://downloads.appachi.tech/macos/kav-macos-latest.tar.gz | tar xz
sudo mv kav /usr/local/bin/
```

**Manual (Linux):**

```sh
curl -sL https://downloads.appachi.tech/linux/kav-linux-latest.tar.gz | tar xz
sudo mv kav /usr/local/bin/
```

**Cargo (crates.io):**

```sh
cargo install kaval
```

**Build from source:**

```sh
cargo build --release
sudo cp target/release/kav /usr/local/bin/
```

## Update

```sh
brew upgrade kaval                # Homebrew
```

Manual install users can re-run the install command — the URL always points to the latest release.

## Usage

```sh
kav                       # Launch interactive TUI
kav list                  # Print all listening ports
kav list --json           # JSON output
kav list --sort cpu       # Sort by CPU usage (port, name, cpu, mem)
kav list --watch          # Refresh in-place every 2s (Ctrl+C to exit)
kav list -w -f postgres   # Watch filtered to postgres only
kav check 3000            # What's on port 3000?
kav check 3000 5432 8080  # Check multiple ports
kav kill 3000             # Kill process on port 3000
kav kill 3000 5432 -f     # Force kill on multiple ports
kav man | man -l -        # View man page inline
```

## Exit Codes

| Command | 0 | 1 | other |
|---------|---|---|-------|
| `kav list` | always | — | — |
| `kav check` | ≥1 port active | no ports active | — |
| `kav kill` | ≥1 process killed | nothing listening | kill failed (e.g. permission denied) |
| `kav completions` | always | — | — |
| `kav man` | always | — | — |

Designed for scripting: `kav check 3000 && echo "server is up"` or `kav kill 3000 || echo "nothing to kill"`.

## JSON Output Schema

`kav list --json` outputs a JSON array — one object per listening port:

```json
[
  {
    "port": 5432,
    "protocol": "TCP",
    "process": "postmaster",
    "service": "PostgreSQL",
    "pid": 1234,
    "cpu": 0.5,
    "memory_mb": 42.0,
    "uptime_secs": 3600
  }
]
```

| Field | Type | Description |
|-------|------|-------------|
| `port` | integer | Port number |
| `protocol` | `"TCP"` \| `"UDP"` | Socket protocol |
| `process` | string | Process name as reported by the OS |
| `service` | string \| `null` | Identified service name (`null` if unknown) |
| `pid` | integer | Process ID |
| `cpu` | float | CPU usage % (sampled over ~200 ms at startup) |
| `memory_mb` | float | Resident memory in megabytes |
| `uptime_secs` | integer | Process uptime in seconds |

**Example usage with `jq`:**

```sh
kav list --json | jq '.[] | select(.service == "PostgreSQL")'
kav list --json | jq 'map(select(.cpu > 10)) | sort_by(.cpu) | reverse'
kav list --json | jq '[.[].port]'
```

## TUI Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `↑/↓` or `j/k` | Navigate |
| `Home` / `End` | Jump to top / bottom |
| `/` | Filter by port, name, or service |
| `Ctrl+X` | Kill selected process (with confirmation) |
| `Ctrl+K` | Force kill (SIGKILL, with confirmation) |
| `Ctrl+D` | Toggle detail pane |
| `Ctrl+S` | Cycle sort (Port → Name → CPU → Mem) |
| `Ctrl+T` | Cycle TCP/UDP filter |
| `Ctrl+R` | Force refresh |
| `PgUp/PgDn` | Scroll detail pane |
| `Ctrl+Q` / `Esc` | Quit |
| `?` | Show keyboard shortcut help |
| `y` | Yank port number to clipboard |
| `o` | Open `http://localhost:<port>` in browser |

## Privacy

- **Zero storage:** Kaval writes nothing to disk. No config, no logs, no database.
- **Zero network:** Kaval makes no network connections of any kind.
- **Zero telemetry:** No analytics, no crash reports, no data collection.

## Links

- [Website](https://www.appachi.tech/kaval/)
- [FAQ](https://www.appachi.tech/kaval/faq)
- [Privacy Policy](https://www.appachi.tech/kaval/privacy/)
- [Contributing](CONTRIBUTING.md)
- [Changelog](CHANGELOG.md)
- [Security Policy](SECURITY.md)

## License

MIT — [Madhubalan Appachi](https://www.linkedin.com/in/madhuappachi/)
